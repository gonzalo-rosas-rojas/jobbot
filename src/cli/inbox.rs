//! The mailbox and Gonza's tasks: importing the e-mails Claude read, and the `task` commands.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use super::{EXIT_ERROR, EXIT_OK, TaskAddArgs, open_tracker};
use crate::actions;
use crate::inbox;
use crate::outbound;
use crate::paths::Paths;

/// Imports the emails that Claude read in Gmail and wrote to `file`. Like `mark` and `record` it
/// writes only the tracker, so it ignores STOP and the execution window: nothing is sent anywhere.
/// A rejected message makes the exit code 1, but the valid ones were imported all the same.
pub(super) fn cmd_inbox_import(
    paths: &Paths,
    file: &Path,
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    // A file that is too big is refused before it is read.
    let size = std::fs::metadata(file)
        .with_context(|| format!("leyendo {}", file.display()))?
        .len();
    inbox::check_size(size).with_context(|| file.display().to_string())?;
    let text =
        std::fs::read_to_string(file).with_context(|| format!("leyendo {}", file.display()))?;
    let items = inbox::parse_items(&text).with_context(|| file.display().to_string())?;
    let tracker = open_tracker(paths)?;
    let report = inbox::import(&tracker, &items, now)?;
    for line in &report.lines {
        writeln!(out, "{line}")?;
    }
    writeln!(out, "{}", report.summary())?;
    Ok(if report.rejected > 0 {
        EXIT_ERROR
    } else {
        EXIT_OK
    })
}

/// Adds a task to Gonza's list. Writes only the tracker: it ignores STOP and the window.
pub(super) fn cmd_task_add(
    paths: &Paths,
    args: TaskAddArgs,
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let due_at = match args.due.as_deref() {
        None => None,
        Some(text) => match DateTime::parse_from_rfc3339(text.trim()) {
            Ok(time) => Some(time.with_timezone(&Utc)),
            Err(_) => {
                writeln!(
                    out,
                    "--due debe ser una fecha RFC 3339, por ejemplo 2026-10-08T15:00:00Z (recibí «{}»)",
                    outbound::plain_line(text)
                        .chars()
                        .take(40)
                        .collect::<String>()
                )?;
                return Ok(EXIT_ERROR);
            }
        },
    };
    let spec = actions::TaskSpec {
        text: args.text,
        due_at,
        job_key: args.job_key,
        link: args.link,
    };
    let tracker = open_tracker(paths)?;
    let outcome = actions::task_add(&tracker, &spec, now)?;
    writeln!(out, "{}", outcome.message)?;
    Ok(if outcome.ok { EXIT_OK } else { EXIT_ERROR })
}

/// Marks one of Gonza's tasks as done. Writes only the tracker: it ignores STOP and the window.
pub(super) fn cmd_task_done(paths: &Paths, id: i64, out: &mut dyn Write) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    let outcome = actions::task_done(&tracker, id)?;
    writeln!(out, "{}", outcome.message)?;
    Ok(if outcome.ok { EXIT_OK } else { EXIT_ERROR })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::{TimeDelta, TimeZone, Utc};

    use crate::cli::test_support::{
        busy_now, inbox_msg, open_now, plain, seed, temp_root, tracker_of, write_inbox_file,
    };
    use crate::testing::make_job;
    use crate::tracker::State;

    #[tokio::test]
    async fn inbox_import_writes_the_tracker_and_ignores_stop_and_the_window() {
        let root = temp_root();
        // Sent three days before the messages arrive.
        let job = make_job();
        let t = tracker_of(root.path());
        let sent = open_now() - TimeDelta::days(3);
        t.add_job(&job, 80.0, &BTreeMap::new(), sent).unwrap();
        t.create_application(&job.key(), sent).unwrap();
        for state in [State::Prepared, State::Submitted] {
            t.transition(&job.key(), state, None, sent).unwrap();
        }
        drop(t);
        let mut rejection = inbox_msg("m1", "rejection");
        rejection["job_key"] = "test:1".into();
        let mut invite = inbox_msg("m2", "interview_invite");
        invite["company"] = "Otra Empresa".into();
        let file = write_inbox_file(root.path(), &serde_json::json!([rejection, invite]));
        let file = file.to_str().unwrap();
        // Busy hour and STOP at once: importing only writes the tracker, like `mark`.
        std::fs::write(root.path().join("STOP"), "x").unwrap();

        let (code, out) = plain(root.path(), busy_now(), &["inbox", "import", file]).await;

        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Importados: 2")
                && out.contains("cambios de estado: 1")
                && out.contains("tareas creadas: 1")
                && out.contains("sin postulación asociada: 1"),
            "{out}"
        );
        let t = tracker_of(root.path());
        assert_eq!(
            t.get_application("test:1").unwrap().unwrap().state,
            State::Rejected
        );
        assert_eq!(
            t.open_tasks().unwrap()[0].text,
            "Responder a Otra Empresa: entrevista"
        );

        // The same file again changes nothing.
        let (code, out) = plain(root.path(), busy_now(), &["inbox", "import", file]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Importados: 0") && out.contains("omitidos (ya conocidos): 2"),
            "{out}"
        );
        assert_eq!(t.open_tasks().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn inbox_import_exits_with_an_error_when_a_message_is_rejected_but_imports_the_rest() {
        let root = temp_root();
        let file = write_inbox_file(
            root.path(),
            &serde_json::json!([inbox_msg("m1", "other"), inbox_msg("m2", "spam")]),
        );

        let (code, out) = plain(
            root.path(),
            open_now(),
            &["inbox", "import", file.to_str().unwrap()],
        )
        .await;

        assert_eq!(code, 1, "{out}");
        assert!(
            out.contains("elemento 2 rechazado: tipo desconocido")
                && out.contains("Importados: 1")
                && out.contains("rechazados: 1"),
            "{out}"
        );
        assert!(tracker_of(root.path()).email_exists("m1").unwrap());
    }

    #[tokio::test]
    async fn inbox_import_says_what_is_wrong_with_a_missing_or_broken_file() {
        let root = temp_root();
        let missing = root.path().join("no-existe.json");
        let (code, out) = plain(
            root.path(),
            open_now(),
            &["inbox", "import", missing.to_str().unwrap()],
        )
        .await;
        assert_eq!(code, 1);
        assert!(
            out.contains("leyendo") && out.contains("no-existe.json"),
            "{out}"
        );

        let broken = root.path().join("roto.json");
        std::fs::write(&broken, "[ {").unwrap();
        let (code, out) = plain(
            root.path(),
            open_now(),
            &["inbox", "import", broken.to_str().unwrap()],
        )
        .await;
        assert_eq!(code, 1);
        assert!(out.contains("no es JSON válido"), "{out}");
    }

    #[tokio::test]
    async fn inbox_import_refuses_a_file_that_is_too_big_or_has_too_many_messages() {
        let root = temp_root();
        // More than 2 MB in one text.
        let big = write_inbox_file(
            root.path(),
            &serde_json::json!(["x".repeat(2 * 1024 * 1024 + 1)]),
        );
        let (code, out) = plain(
            root.path(),
            open_now(),
            &["inbox", "import", big.to_str().unwrap()],
        )
        .await;
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("2 MB"), "{out}");

        // 501 valid messages: one more than the limit, so none is imported.
        let many: Vec<serde_json::Value> = (0..=crate::inbox::MAX_MESSAGES)
            .map(|n| inbox_msg(&format!("m{n}"), "other"))
            .collect();
        let file = write_inbox_file(root.path(), &serde_json::Value::Array(many));
        let (code, out) = plain(
            root.path(),
            open_now(),
            &["inbox", "import", file.to_str().unwrap()],
        )
        .await;
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("500") && out.contains("mensajes"), "{out}");
        assert!(!out.contains("Importados"), "nothing was imported: {out}");
        assert!(!tracker_of(root.path()).email_exists("m0").unwrap());
    }

    #[tokio::test]
    async fn a_broken_or_oversized_inbox_file_leaves_the_date_the_mailbox_was_read_alone() {
        let root = temp_root();
        let date = || {
            tracker_of(root.path())
                .get_meta(crate::inbox::INBOX_IMPORTED_AT)
                .unwrap()
        };
        let broken = root.path().join("roto.json");
        std::fs::write(&broken, "[ {").unwrap();
        let big = root.path().join("grande.json");
        std::fs::write(
            &big,
            serde_json::json!(["x".repeat(crate::inbox::MAX_FILE_BYTES as usize + 1)]).to_string(),
        )
        .unwrap();
        let bad = [broken.to_str().unwrap(), big.to_str().unwrap()];

        // Never imported: a bad file leaves the date absent.
        for file in bad {
            let (code, _) = plain(root.path(), open_now(), &["inbox", "import", file]).await;
            assert_eq!(code, 1);
        }
        assert_eq!(date(), None);

        // After a good import, a bad file leaves the date of the good one.
        let good = write_inbox_file(root.path(), &serde_json::json!([inbox_msg("m1", "other")]));
        let (code, out) = plain(
            root.path(),
            busy_now(),
            &["inbox", "import", good.to_str().unwrap()],
        )
        .await;
        assert_eq!(code, 0, "{out}");
        let after_good = date();
        assert_eq!(after_good.as_deref(), Some("2026-10-14T15:00:00+00:00"));
        for file in bad {
            let (code, _) = plain(root.path(), open_now(), &["inbox", "import", file]).await;
            assert_eq!(code, 1);
        }
        assert_eq!(date(), after_good);
    }

    #[tokio::test]
    async fn task_add_shows_up_in_today_and_task_done_closes_it_under_stop_and_busy_hours() {
        let root = temp_root();
        seed(root.path(), &make_job(), &[]);
        std::fs::write(root.path().join("STOP"), "x").unwrap();

        let (code, out) = plain(
            root.path(),
            busy_now(),
            &[
                "task",
                "add",
                "Llamar a Toku",
                "--due",
                "2026-10-16T12:00:00-03:00",
                "--job-key",
                "test:1",
                "--link",
                "https://example.com/oferta",
            ],
        )
        .await;

        assert_eq!(code, 0, "adding a task only writes the tracker: {out}");
        assert!(out.contains("Tarea agregada: #1 Llamar a Toku"), "{out}");
        let row = tracker_of(root.path()).get_task(1).unwrap().unwrap();
        assert_eq!(
            (
                row.kind.as_str(),
                row.job_key.as_deref(),
                row.link.as_deref()
            ),
            ("manual", Some("test:1"), Some("https://example.com/oferta"))
        );
        assert_eq!(
            row.due_at,
            Some(Utc.with_ymd_and_hms(2026, 10, 16, 15, 0, 0).unwrap())
        );

        let (code, today) = plain(root.path(), busy_now(), &["today"]).await;
        assert_eq!(code, 0, "{today}");
        assert!(
            today.contains("Tareas abiertas (1)")
                && today.contains(
                    "#1 Llamar a Toku · https://example.com/oferta [test:1] — vence en 2 días"
                ),
            "{today}"
        );

        let (code, out) = plain(root.path(), busy_now(), &["task", "done", "1"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Hecha: #1 Llamar a Toku"), "{out}");
        let (_, today) = plain(root.path(), busy_now(), &["today"]).await;
        assert!(today.contains("Tareas abiertas (0)"), "{today}");
    }

    #[tokio::test]
    async fn task_add_refuses_what_is_wrong_with_a_message_and_changes_nothing() {
        let root = temp_root();
        seed(root.path(), &make_job(), &[]);
        let go = |extra: &'static [&'static str]| {
            let mut parts = vec!["task", "add", "Llamar a Toku"];
            parts.extend_from_slice(extra);
            parts
        };
        for (extra, needle) in [
            (
                &["--due", "mañana"][..],
                "--due debe ser una fecha RFC 3339",
            ),
            (&["--due", "1999-12-31T23:59:59Z"][..], "fuera de rango"),
            (&["--due", "9999-12-31T23:59:59Z"][..], "fuera de rango"),
            (&["--job-key", "nope:1"][..], "No existe el aviso: nope:1"),
            (&["--link", "javascript:alert(1)"][..], "enlace"),
        ] {
            let (code, out) = plain(root.path(), open_now(), &go(extra)).await;
            assert_eq!(code, 1, "{extra:?}: {out}");
            assert!(out.contains(needle), "{extra:?}: {out}");
        }
        assert!(tracker_of(root.path()).open_tasks().unwrap().is_empty());

        // Twice while the first one is open: the second is refused and nothing changes.
        assert_eq!(plain(root.path(), open_now(), &go(&[])).await.0, 0);
        let (code, out) = plain(root.path(), open_now(), &go(&[])).await;
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("Ya tienes esa tarea abierta (#1)"), "{out}");
        assert_eq!(tracker_of(root.path()).open_tasks().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn task_done_closes_a_task_and_refuses_unknown_or_repeated_ones() {
        let root = temp_root();
        let mut item = inbox_msg("m1", "action_required");
        item["task"] = serde_json::json!({ "text": "Grabar el video" });
        let file = write_inbox_file(root.path(), &serde_json::json!([item]));
        plain(
            root.path(),
            open_now(),
            &["inbox", "import", file.to_str().unwrap()],
        )
        .await;
        let id = tracker_of(root.path()).open_tasks().unwrap()[0]
            .id
            .to_string();
        std::fs::write(root.path().join("STOP"), "x").unwrap();

        let (code, out) = plain(root.path(), busy_now(), &["task", "done", &id]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Hecha:") && out.contains("Grabar el video"),
            "{out}"
        );
        assert!(tracker_of(root.path()).open_tasks().unwrap().is_empty());

        let (code, out) = plain(root.path(), busy_now(), &["task", "done", &id]).await;
        assert_eq!(code, 1);
        assert!(out.contains("ya estaba hecha"), "{out}");
        let (code, out) = plain(root.path(), busy_now(), &["task", "done", "999"]).await;
        assert_eq!(code, 1);
        assert!(out.contains("No existe la tarea #999"), "{out}");
    }
}
