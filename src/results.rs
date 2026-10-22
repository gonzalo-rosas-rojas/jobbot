//! The results log: one line of JSON per submit attempt in `data/results.jsonl`.
//!
//! An attempt is a form the bot opened: a dry run, an assisted form, a live send or a handoff.
//! What the gate refused before a form was opened is not an attempt and leaves no line. The log
//! only grows; `jobbot results` prints its tail. It is not part of the SQLite schema.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::{DateTime, SecondsFormat, Utc};
use chrono_tz::America::Santiago;
use serde::{Deserialize, Serialize};

use crate::handoff::FormTally;
use crate::models::Job;
use crate::tracker::State;

/// The log's file name, under the data directory.
pub const RESULTS_FILE: &str = "results.jsonl";

/// Where the log of a project lives: `<data_dir>/results.jsonl`.
pub fn results_path(data_dir: &Path) -> PathBuf {
    data_dir.join(RESULTS_FILE)
}

/// What the bot did with the form: the mode that actually ran, which is not always the one asked
/// for (an unreviewed application is dry-run under `--live`, and a handoff form never goes past a
/// dry run).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptMode {
    DryRun,
    Assist,
    Live,
    Handoff,
}

/// One line of the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultLine {
    /// UTC, ISO-8601 to the second.
    pub timestamp: String,
    /// The application key (`source:id`).
    pub key: String,
    pub source: String,
    pub company: String,
    pub title: String,
    pub mode: AttemptMode,
    /// The state the application was in once the attempt was recorded.
    pub state: String,
    /// Questions the bank answered; `null` when the filler cannot count them (everything but a
    /// handoff, whose inspector lists the form).
    pub fields_filled: Option<usize>,
    /// Labels of the required questions left unanswered.
    pub fields_left: Vec<String>,
    pub screenshot: Option<String>,
    pub handoff_sheet: Option<String>,
}

impl ResultLine {
    pub fn new(
        job: &Job,
        mode: AttemptMode,
        state: State,
        tally: FormTally,
        screenshot: Option<&Path>,
        handoff_sheet: Option<&Path>,
        at: DateTime<Utc>,
    ) -> ResultLine {
        let shown = |path: &Path| path.to_string_lossy().into_owned();
        ResultLine {
            timestamp: at.to_rfc3339_opts(SecondsFormat::Secs, true),
            key: job.key(),
            source: job.source.clone(),
            company: job.company.clone(),
            title: job.title.clone(),
            mode,
            state: state.as_str().to_string(),
            fields_filled: tally.filled,
            fields_left: tally.left,
            screenshot: screenshot.map(shown),
            handoff_sheet: handoff_sheet.map(shown),
        }
    }
}

/// Adds `line` to the log at `path`, creating the file and its folder when missing. A last line
/// that a crash left without its newline is not glued to the new one.
pub fn append_result(path: &Path, line: &ResultLine) -> anyhow::Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)
            .with_context(|| format!("creating {}", folder.display()))?;
    }
    let mut text = serde_json::to_string(line)?;
    text.push('\n');
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    if file.metadata()?.len() > 0 {
        let mut last = [0u8; 1];
        file.seek(SeekFrom::End(-1))?;
        file.read_exact(&mut last)?;
        if last[0] != b'\n' {
            text.insert(0, '\n');
        }
    }
    file.write_all(text.as_bytes())
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// The tail of the log and how long the whole log is.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LastResults {
    /// The last lines, oldest first.
    pub lines: Vec<ResultLine>,
    /// Readable lines in the whole log.
    pub total: usize,
}

/// The last `last` lines of the log, oldest first. A missing log is an empty one, and a line that
/// does not parse (a write cut short) is skipped instead of hiding the rest.
pub fn read_last(path: &Path, last: usize) -> anyhow::Result<LastResults> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LastResults::default());
        }
        Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
    };
    let all: Vec<ResultLine> = text
        .trim_start_matches('\u{feff}') // an editor may have added a byte order mark
        .lines()
        .filter_map(|line| serde_json::from_str(line.trim()).ok())
        .collect();
    let total = all.len();
    let lines = all.into_iter().skip(total.saturating_sub(last)).collect();
    Ok(LastResults { lines, total })
}

/// `text` cut to `max` characters, with an ellipsis when something was left out.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let kept: String = text.chars().take(max.saturating_sub(1)).collect();
        format!("{kept}…")
    }
}

/// A short table in Spanish for `jobbot results`: when (Santiago time), mode, state, key, who and
/// what, how many questions were answered and which required ones were left.
pub fn format_table(results: &LastResults) -> String {
    if results.lines.is_empty() {
        return format!(
            "Sin resultados todavía: data/{RESULTS_FILE} se llena con cada envío o prueba en seco."
        );
    }
    let header = [
        "FECHA",
        "MODO",
        "ESTADO",
        "CLAVE",
        "EMPRESA — CARGO",
        "LLENOS",
        "FALTAN",
    ];
    let rows: Vec<[String; 7]> = results
        .lines
        .iter()
        .map(|line| {
            let when = DateTime::parse_from_rfc3339(&line.timestamp)
                .map(|at| {
                    at.with_timezone(&Santiago)
                        .format("%Y-%m-%d %H:%M")
                        .to_string()
                })
                .unwrap_or_else(|_| line.timestamp.clone());
            let mode = serde_json::to_value(line.mode)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_default();
            let filled = line
                .fields_filled
                .map_or("-".to_string(), |count| count.to_string());
            let left = if line.fields_left.is_empty() {
                "-".to_string()
            } else {
                cut(&line.fields_left.join("; "), 60)
            };
            [
                when,
                mode,
                line.state.clone(),
                line.key.clone(),
                cut(&format!("{} — {}", line.company, line.title), 40),
                filled,
                left,
            ]
        })
        .collect();
    let mut widths: [usize; 7] = std::array::from_fn(|i| header[i].chars().count());
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let render = |cells: [&str; 7]| {
        let mut out = String::new();
        for (i, cell) in cells.iter().enumerate() {
            if i == 6 {
                out.push_str(cell);
            } else {
                out.push_str(&format!("{cell:<width$}  ", width = widths[i]));
            }
        }
        out.trim_end().to_string()
    };
    let mut text = format!(
        "Últimos {} resultados (de {}), hora de Santiago:\n{}",
        results.lines.len(),
        results.total,
        render(header)
    );
    for row in &rows {
        text.push('\n');
        text.push_str(&render(std::array::from_fn(|i| row[i].as_str())));
    }
    text
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::testing::{NOW, make_job_with};

    fn line(n: u32, mode: AttemptMode, state: State) -> ResultLine {
        let job = make_job_with(|j| {
            j.external_id = n.to_string();
            j.company = format!("Co{n}");
            j.title = format!("Role {n}");
        });
        ResultLine::new(
            &job,
            mode,
            state,
            FormTally {
                filled: Some(7),
                left: vec!["Email *".into(), "Why this role? *".into()],
            },
            Some(Path::new("data/screenshots/test_1.png")),
            None,
            *NOW,
        )
    }

    #[test]
    fn a_line_carries_everything_the_log_promises() {
        let value: serde_json::Value =
            serde_json::to_value(line(1, AttemptMode::DryRun, State::NeedsGonza)).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "timestamp": "2026-10-01T12:00:00Z",
                "key": "test:1",
                "source": "test",
                "company": "Co1",
                "title": "Role 1",
                "mode": "dry_run",
                "state": "needs_gonza",
                "fields_filled": 7,
                "fields_left": ["Email *", "Why this role? *"],
                "screenshot": "data/screenshots/test_1.png",
                "handoff_sheet": null
            })
        );
    }

    #[test]
    fn a_count_the_filler_could_not_give_is_null() {
        let job = make_job_with(|_| {});
        let unknown = ResultLine::new(
            &job,
            AttemptMode::Live,
            State::Submitted,
            FormTally {
                filled: None,
                left: vec![],
            },
            None,
            None,
            *NOW,
        );
        let value = serde_json::to_value(unknown).unwrap();
        assert_eq!(value["fields_filled"], serde_json::Value::Null);
        assert_eq!(value["screenshot"], serde_json::Value::Null);
        assert_eq!(value["fields_left"], serde_json::json!([]));
        assert_eq!(value["mode"], "live");
        assert_eq!(value["state"], "submitted");
    }

    #[test]
    fn every_mode_has_its_own_name() {
        let names: Vec<String> = [
            AttemptMode::DryRun,
            AttemptMode::Assist,
            AttemptMode::Live,
            AttemptMode::Handoff,
        ]
        .iter()
        .map(|mode| {
            serde_json::to_value(mode)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
        assert_eq!(names, ["dry_run", "assist", "live", "handoff"]);
    }

    #[test]
    fn append_creates_the_folder_and_adds_one_json_line_per_call() {
        let dir = tempfile::tempdir().unwrap();
        let path = results_path(&dir.path().join("data"));
        assert_eq!(path, dir.path().join("data").join("results.jsonl"));
        append_result(&path, &line(1, AttemptMode::DryRun, State::Prepared)).unwrap();
        append_result(&path, &line(2, AttemptMode::Live, State::Submitted)).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        for (text, key) in lines.iter().zip(["test:1", "test:2"]) {
            let value: serde_json::Value = serde_json::from_str(text).unwrap();
            assert_eq!(value["key"], key);
            assert!(!text.contains('\n'));
        }
    }

    #[test]
    fn read_last_returns_the_latest_lines_oldest_first() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("results.jsonl");
        for n in 1..=5 {
            append_result(&path, &line(n, AttemptMode::DryRun, State::Prepared)).unwrap();
        }
        let keys = |last: usize| -> Vec<String> {
            read_last(&path, last)
                .unwrap()
                .lines
                .into_iter()
                .map(|l| l.key)
                .collect()
        };
        assert_eq!(keys(2), ["test:4", "test:5"]);
        assert_eq!(keys(99).len(), 5);
        assert!(keys(0).is_empty());
        // The total is the whole log, not just what is shown.
        assert_eq!(read_last(&path, 2).unwrap().total, 5);
    }

    #[test]
    fn read_last_survives_a_missing_file_and_damaged_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("results.jsonl");
        let none = read_last(&path, 5).unwrap();
        assert!(none.lines.is_empty() && none.total == 0);
        append_result(&path, &line(1, AttemptMode::DryRun, State::Prepared)).unwrap();
        // A blank line and a half-written one without its newline (a crash mid-write) must not
        // hide the good ones, nor swallow the next line appended after them.
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("\n{\"timestamp\": \"2026-10");
        std::fs::write(&path, text).unwrap();
        append_result(&path, &line(2, AttemptMode::DryRun, State::Prepared)).unwrap();
        let latest = read_last(&path, 10).unwrap();
        let keys: Vec<&str> = latest.lines.iter().map(|l| l.key.as_str()).collect();
        assert_eq!(keys, ["test:1", "test:2"]);
        assert_eq!(latest.total, 2);
    }

    #[test]
    fn a_byte_order_mark_from_an_editor_does_not_hide_the_first_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("results.jsonl");
        append_result(&path, &line(1, AttemptMode::DryRun, State::Prepared)).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, format!("\u{feff}{text}")).unwrap();
        assert_eq!(read_last(&path, 5).unwrap().total, 1);
    }

    #[test]
    fn the_table_is_short_and_in_spanish() {
        let mut handoff = line(2, AttemptMode::Handoff, State::NeedsGonza);
        handoff.handoff_sheet = Some("data/handoff/test_2.md".into());
        let unknown = ResultLine {
            fields_filled: None,
            fields_left: vec![],
            ..line(3, AttemptMode::Live, State::Submitted)
        };
        let table = format_table(&LastResults {
            lines: vec![
                line(1, AttemptMode::DryRun, State::Prepared),
                handoff,
                unknown,
            ],
            total: 9,
        });
        let rows: Vec<&str> = table.lines().collect();
        assert_eq!(rows[0], "Últimos 3 resultados (de 9), hora de Santiago:");
        for header in [
            "FECHA",
            "MODO",
            "ESTADO",
            "CLAVE",
            "EMPRESA — CARGO",
            "LLENOS",
            "FALTAN",
        ] {
            assert!(rows[1].contains(header), "{header} in {}", rows[1]);
        }
        // 2026-10-01 12:00 UTC is 09:00 in Santiago (UTC-3 in October).
        assert!(rows[2].contains("2026-10-01 09:00"), "{}", rows[2]);
        assert!(rows[2].contains("dry_run") && rows[2].contains("prepared"));
        assert!(rows[2].contains("Co1 — Role 1") && rows[2].contains("test:1"));
        assert!(rows[2].contains(" 7 "), "{}", rows[2]);
        assert!(rows[2].contains("Email *; Why this role? *"), "{}", rows[2]);
        assert!(rows[3].contains("handoff") && rows[3].contains("needs_gonza"));
        // What the filler could not count shows as a dash, and nothing left as a dash too.
        assert!(rows[4].contains("submitted"));
        assert!(rows[4].trim_end().ends_with('-'), "{}", rows[4]);
        assert_eq!(rows.len(), 5);
    }

    #[test]
    fn an_empty_log_says_so() {
        assert_eq!(
            format_table(&LastResults {
                lines: vec![],
                total: 0
            }),
            "Sin resultados todavía: data/results.jsonl se llena con cada envío o prueba en seco."
        );
    }
}
