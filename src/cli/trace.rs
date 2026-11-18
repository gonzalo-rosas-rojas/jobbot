//! The trace of a run and the writers its output goes through: the span writer on stderr, the
//! privacy filter in front of a log, and the plain-text writer every command prints through.

use std::io::Write;
use std::path::Path;

use jobbot_app::privacy::{LineFilter, UserPaths};

use crate::outbound;
use crate::paths::Paths;

/// Turns on the run's trace: each use case's span is written to stderr when it closes, with its
/// counts and outcome and how long it was busy and idle. Unset, empty or `0`, nothing new is
/// printed.
pub const TRACE_VAR: &str = "JOBBOT_TRACE";

fn trace_requested(value: Option<&str>) -> bool {
    value.is_some_and(|value| !matches!(value.trim(), "" | "0"))
}

/// Installs the trace writer on stderr when `JOBBOT_TRACE` asks for it, behind the one privacy
/// filter (the profile under `root` names Gonza's own addresses). `main` calls it once.
pub fn init_trace(root: &Path) {
    if trace_requested(std::env::var(TRACE_VAR).ok().as_deref()) {
        let user = UserPaths::for_profile(&Paths::new(root).profile_dir);
        // Only fails when a subscriber is already installed, and then that one stays.
        let _ = tracing::subscriber::set_global_default(filtered_trace(std::io::stderr, user));
    }
}

/// The trace writer with every line through `UserPaths::redact`: a trace is a log.
fn filtered_trace<W>(
    make: impl Fn() -> W + Send + Sync + 'static,
    user: UserPaths,
) -> impl tracing::Subscriber + Send + Sync
where
    W: Write + 'static,
{
    trace_subscriber(move || LineFilter::new(make(), user.clone()))
}

/// Where a run writes: a terminal as it is (Gonza can open the paths it names); anything else, the
/// scheduled task's log or a pipe, through the one privacy filter. `user` is only read for a log.
pub(super) fn log_output<'a, W: Write + 'a>(
    stdout: W,
    terminal: bool,
    user: impl FnOnce() -> UserPaths,
) -> Box<dyn Write + 'a> {
    if terminal {
        Box::new(stdout)
    } else {
        Box::new(LineFilter::new(stdout, user()))
    }
}

/// The plain text writer: one line per span as it closes, at INFO (the use cases' level; the HTTP
/// libraries write below it), without the module path.
fn trace_subscriber<W>(writer: W) -> impl tracing::Subscriber + Send + Sync
where
    W: for<'w> tracing_subscriber::fmt::MakeWriter<'w> + Send + Sync + 'static,
{
    use tracing_subscriber::layer::SubscriberExt;
    // Only jobbot's own spans, whose fields are counts and outcomes: a dependency's span (a page
    // the browser opens, a request) may carry a link with a token in it.
    let own =
        tracing_subscriber::filter::Targets::new().with_target("jobbot", tracing::Level::INFO);
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_max_level(tracing::Level::INFO)
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
        .with_target(false)
        .finish()
        .with(own)
}

/// What a run prints, cleaned line by line before it reaches the terminal or the log: postings,
/// e-mails and notes are strangers' text, and an escape sequence, a bidirectional control or a
/// hidden character in them must not move the cursor, recolour or hide what Gonza reads. Every
/// control character but the newline becomes a space; the invisible format characters go.
pub(super) struct PlainText<'a> {
    inner: &'a mut dyn Write,
    held: Vec<u8>,
}

impl<'a> PlainText<'a> {
    pub(super) fn new(inner: &'a mut dyn Write) -> Self {
        PlainText {
            inner,
            held: Vec::new(),
        }
    }

    fn write_clean(&mut self, line: &[u8]) -> std::io::Result<()> {
        let text = String::from_utf8_lossy(line);
        let clean: String = outbound::strip_invisible(&text)
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        self.inner.write_all(clean.as_bytes())
    }
}

impl Write for PlainText<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.held.extend_from_slice(buf);
        while let Some(end) = self.held.iter().position(|&byte| byte == b'\n') {
            let line: Vec<u8> = self.held.drain(..=end).collect();
            self.write_clean(&line[..end])?;
            self.inner.write_all(b"\n")?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let rest = std::mem::take(&mut self.held);
        self.write_clean(&rest)?;
        self.inner.flush()
    }
}

impl Drop for PlainText<'_> {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::cli::test_support::{fake_deps, open_now, plain, temp_root, tracker_of};
    use crate::cli::{EXIT_ERROR, run_with};
    use crate::tracker::{State, Tracker};

    /// The hostile corpus in every state a list shows, with hostile notes and hostile e-mails: what
    /// the terminal prints holds no escape sequence, control or invisible character.
    #[tokio::test]
    async fn hostile_postings_and_emails_print_as_plain_text() {
        use crate::testing::hostile::{MARK, emails, hidden_chars, postings};

        let root = temp_root();
        let t = tracker_of(root.path());
        let walks: [&[State]; 4] = [
            &[],
            &[State::Prepared],
            &[State::NeedsGonza],
            &[State::Prepared, State::Submitted],
        ];
        for (i, (_, job)) in postings().into_iter().enumerate() {
            t.add_job(&job, 80.0, &BTreeMap::new(), open_now()).unwrap();
            t.create_application(&job.key(), open_now()).unwrap();
            for state in walks[i % walks.len()] {
                t.transition(&job.key(), *state, Some(&job.title), open_now())
                    .unwrap();
            }
        }
        let items: Vec<serde_json::Value> = emails(open_now())
            .into_iter()
            .map(|(_, item)| item)
            .collect();
        let file = root.path().join("emails.json");
        std::fs::write(&file, serde_json::to_string(&items).unwrap()).unwrap();

        let mut printed = String::new();
        for command in [
            vec!["inbox", "import", file.to_str().unwrap()],
            vec!["top", "--n", "50"],
            vec!["digest"],
            vec!["status"],
            vec!["today"],
        ] {
            let (code, out) = plain(root.path(), open_now(), &command).await;
            assert_eq!(code, 0, "{command:?}: {out}");
            let hidden = hidden_chars(&out);
            assert!(hidden.is_empty(), "{command:?} printed {hidden:?}");
            printed.push_str(&out);
        }
        assert!(printed.contains(MARK), "the lists showed the corpus");
        let digest =
            std::fs::read_to_string(root.path().join("data").join("digest-2026-10-14.md")).unwrap();
        let hidden = hidden_chars(&digest);
        assert!(hidden.is_empty(), "the digest file holds {hidden:?}");
    }

    #[test]
    fn the_trace_is_asked_for_by_any_value_but_empty_or_zero() {
        for (value, asked) in [
            (None, false),
            (Some(""), false),
            (Some("  "), false),
            (Some("0"), false),
            (Some("1"), true),
            (Some("yes"), true),
        ] {
            assert_eq!(trace_requested(value), asked, "{value:?}");
        }
    }

    /// What the trace writer wrote, shared with the test.
    #[derive(Clone, Default)]
    struct Written(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for Written {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn the_trace_writes_each_span_when_it_closes_with_its_fields_and_time() {
        jobbot_testkit::spans::quiet_default();
        let written = Written::default();
        let sink = written.clone();
        let tracker = Tracker::in_memory().unwrap();
        tracing::subscriber::with_default(trace_subscriber(move || sink.clone()), || {
            jobbot_app::actions::approve(&tracker, "nope:1").unwrap();
        });
        let text = String::from_utf8(written.0.lock().unwrap().clone()).unwrap();
        assert!(
            text.contains("approve") && text.contains("ok=false") && text.contains("time.busy"),
            "{text}"
        );
        assert!(!text.contains("nope:1"), "the key stays out: {text}");
    }

    fn ana() -> UserPaths {
        UserPaths::new(Some(r"C:\Users\ana"), None, None)
    }

    /// True when `text` still has a path part that starts like Ana's folder, after either slash; the trace
    /// doubles the backslash, and `\\ana` holds `\ana`. A bare "ana" would also be found in
    /// "ventana" or "semana".
    fn names_the_user_folder(text: &str) -> bool {
        [r"\ana", "/ana"].iter().any(|part| text.contains(part))
    }

    fn text_of(written: &Written) -> String {
        String::from_utf8(written.0.lock().unwrap().clone()).unwrap()
    }

    #[test]
    fn a_run_writes_to_a_log_through_the_privacy_filter_and_to_a_terminal_as_it_is() {
        for (terminal, expected) in [
            (false, "%USERPROFILE%\\x.json\n"),
            (true, "C:\\Users\\ana\\x.json\n"),
        ] {
            let written = Written::default();
            {
                let mut out = log_output(written.clone(), terminal, ana);
                writeln!(out, r"C:\Users\ana\x.json").unwrap();
            }
            assert_eq!(text_of(&written), expected, "terminal: {terminal}");
        }
    }

    #[tokio::test]
    async fn the_error_of_a_logged_run_loses_the_user_folder() {
        let root = temp_root();
        let written = Written::default();
        let code = {
            let mut log = log_output(written.clone(), false, ana);
            let args = [
                "jobbot",
                "inbox",
                "import",
                r"C:\Users\ana\Desktop\correos.json",
            ]
            .map(String::from)
            .to_vec();
            run_with(args, root.path(), open_now(), fake_deps(vec![]).0, &mut log).await
        };
        let text = text_of(&written);
        assert_eq!(code, EXIT_ERROR, "{text}");
        assert!(
            text.contains(r"%USERPROFILE%\Desktop\correos.json") && !names_the_user_folder(&text),
            "{text}"
        );
    }

    #[test]
    fn the_trace_goes_through_the_privacy_filter() {
        jobbot_testkit::spans::quiet_default();
        let written = Written::default();
        let sink = written.clone();
        tracing::subscriber::with_default(filtered_trace(move || sink.clone(), ana()), || {
            let span = tracing::info_span!("probe", file = r"C:\Users\ana\x.json");
            drop(span.enter());
        });
        // The trace prints a text field escaped (`C:\\Users\\...`); the filter knows that form too.
        let text = text_of(&written);
        assert!(
            text.contains("%USERPROFILE%") && !names_the_user_folder(&text),
            "{text}"
        );
    }

    #[test]
    fn the_trace_shows_only_jobbots_own_spans() {
        jobbot_testkit::spans::quiet_default();
        let written = Written::default();
        let sink = written.clone();
        tracing::subscriber::with_default(filtered_trace(move || sink.clone(), ana()), || {
            // A dependency's span may carry a link with a token in it.
            let url = "https://jobs.example/apply?token=abc";
            drop(tracing::info_span!(target: "playwright_rs::page", "goto", url).entered());
            drop(
                tracing::info_span!(target: "jobbot_app::discovery", "discover", sources = 1)
                    .entered(),
            );
        });
        let text = text_of(&written);
        assert!(
            text.contains("discover") && !text.contains("goto"),
            "{text}"
        );
    }
}
