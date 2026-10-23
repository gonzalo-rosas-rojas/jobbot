//! Whether the Windows scheduled task that runs the engine exists, and when it fires next.
//!
//! The panel starts nothing by design, and this is the one exception: a read-only question to the
//! Task Scheduler, asked in a child process that gets a deadline and is killed when it passes it.
//! It never runs on the request path. `TaskWatch` keeps the last answer for a few minutes and asks
//! again in a thread of its own, so a snapshot returns at once whatever Windows is doing; until
//! there is an answer, or when there is none (the child failed, timed out, or said something this
//! file does not understand), the snapshot says `null`, "no data". Only an answer from Windows
//! that the task is not registered becomes `exists: false`.

use std::io::Read;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

/// The task `scripts/schedule.ps1` registers by default: `\jobbot\run-daily`, named after the program
/// so it is easy to find and remove (keep the two in step).
pub const TASK_FOLDER: &str = "\\jobbot";
pub const TASK_NAME: &str = "run-daily";
/// The most one question may take. A cold PowerShell answers in about 0.2 s; an unfiltered
/// `schtasks /query` took more than two minutes on this PC, which is why the question names the
/// task and why there is a deadline at all.
pub const QUERY_DEADLINE: Duration = Duration::from_secs(3);
/// How long an answer is believed, and how long to wait before asking again after no answer.
const ANSWER_LASTS: Duration = Duration::from_secs(300);
const NO_ANSWER_LASTS: Duration = Duration::from_secs(60);

/// What Windows said.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskAnswer {
    /// Windows answered and the task is not registered (or its folder is not).
    Missing,
    /// Registered. `next_run_at` is `None` when the task is disabled or has nothing left to fire.
    Registered { next_run_at: Option<DateTime<Utc>> },
}

/// Something that can ask the Task Scheduler. Injected, so no test ever touches the real one.
pub trait TaskQuery: Send + Sync {
    /// `None` when there is no answer: it failed, took too long or was not understood.
    fn ask(&self) -> Option<TaskAnswer>;
}

/// The question as one line of PowerShell, read-only: it connects, opens the folder and gets the
/// task. Folder and name arrive in the environment, never in the text. It prints `MISSING`,
/// `EXISTS -` or `EXISTS <UTC moment>`; an error that is not "not found" exits with 3. A task that
/// is not there is `0x80070002` (file not found) or `0x80070003` (path not found).
const PROBE: &str = "$ErrorActionPreference='Stop'; try { $s = New-Object -ComObject Schedule.Service; $s.Connect(); $t = $s.GetFolder($env:JOBBOT_TASK_FOLDER).GetTask($env:JOBBOT_TASK_NAME); $n = '-'; if ($t.Enabled -and $t.NextRunTime.Year -gt 2000) { $n = $t.NextRunTime.ToUniversalTime().ToString('s') + 'Z' }; 'EXISTS ' + $n } catch { if ($_.Exception.HResult -eq -2147024894 -or $_.Exception.HResult -eq -2147024893) { 'MISSING' } else { exit 3 } }";

/// The real Task Scheduler, asked through a short PowerShell child.
#[derive(Debug, Clone)]
pub struct WindowsTasks {
    folder: String,
    name: String,
    deadline: Duration,
}

impl WindowsTasks {
    /// `\jobbot\run-daily`, three seconds.
    pub fn default_task() -> WindowsTasks {
        WindowsTasks {
            folder: TASK_FOLDER.to_string(),
            name: TASK_NAME.to_string(),
            deadline: QUERY_DEADLINE,
        }
    }

    /// The child, ready to spawn: the PowerShell of the system folder (not whatever `PATH` finds
    /// first), without profile or prompts, and with no window.
    fn command(&self) -> Command {
        let program = std::env::var_os("SystemRoot")
            .map(|root| {
                PathBuf::from(root)
                    .join("System32")
                    .join("WindowsPowerShell")
                    .join("v1.0")
                    .join("powershell.exe")
            })
            .filter(|path| path.is_file())
            .unwrap_or_else(|| PathBuf::from("powershell.exe"));
        let mut command = Command::new(program);
        command
            .args(["-NoProfile", "-NonInteractive", "-Command", PROBE])
            .env("JOBBOT_TASK_FOLDER", &self.folder)
            .env("JOBBOT_TASK_NAME", &self.name);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: the child never opens a console of its own.
            command.creation_flags(0x0800_0000);
        }
        command
    }
}

impl TaskQuery for WindowsTasks {
    fn ask(&self) -> Option<TaskAnswer> {
        let text = run_within(self.command(), self.deadline)?;
        parse_answer(&text)
    }
}

/// What the child printed, as an answer; `None` for anything else.
fn parse_answer(text: &str) -> Option<TaskAnswer> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    if line == "MISSING" {
        return Some(TaskAnswer::Missing);
    }
    let rest = line.strip_prefix("EXISTS ")?;
    if rest == "-" {
        return Some(TaskAnswer::Registered { next_run_at: None });
    }
    let at = DateTime::parse_from_rfc3339(rest).ok()?;
    Some(TaskAnswer::Registered {
        next_run_at: Some(at.with_timezone(&Utc)),
    })
}

/// Runs `command` and returns what it printed if it exited successfully within `deadline`;
/// kills it and returns `None` otherwise. Its output is read after it ends, which is fine for the
/// few bytes it is meant to print.
fn run_within(mut command: Command, deadline: Duration) -> Option<String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                let mut text = String::new();
                child.stdout.take()?.read_to_string(&mut text).ok()?;
                return Some(text);
            }
            Ok(Some(_)) => return None,
            Ok(None) if started.elapsed() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

struct Checked {
    at: Instant,
    answer: Option<TaskAnswer>,
}

struct Inner {
    query: Option<Box<dyn TaskQuery>>,
    state: Mutex<State>,
    idle: Condvar,
    answer_lasts: Duration,
    no_answer_lasts: Duration,
}

#[derive(Default)]
struct State {
    last: Option<Checked>,
    asking: bool,
}

/// The last answer about the task, kept for a few minutes and renewed off the request path.
#[derive(Clone)]
pub struct TaskWatch {
    inner: Arc<Inner>,
}

impl TaskWatch {
    /// Never asks anything: every answer is "no data". What a panel starts with.
    pub fn off() -> TaskWatch {
        TaskWatch::build(None, ANSWER_LASTS, NO_ANSWER_LASTS)
    }

    /// Asks `query` once now, in a thread of its own, and again whenever the answer is older than a
    /// few minutes (or its next run has passed) and somebody looks.
    pub fn asking(query: impl TaskQuery + 'static) -> TaskWatch {
        let watch = TaskWatch::build(Some(Box::new(query)), ANSWER_LASTS, NO_ANSWER_LASTS);
        watch.renew();
        watch
    }

    fn build(
        query: Option<Box<dyn TaskQuery>>,
        answer_lasts: Duration,
        no_answer_lasts: Duration,
    ) -> TaskWatch {
        TaskWatch {
            inner: Arc::new(Inner {
                query,
                state: Mutex::new(State::default()),
                idle: Condvar::new(),
                answer_lasts,
                no_answer_lasts,
            }),
        }
    }

    /// The believed answer, at once. It starts a renewal in the background when there is none yet
    /// or it is old; it never waits for it.
    pub fn current(&self, now: DateTime<Utc>) -> Option<TaskAnswer> {
        self.inner.query.as_ref()?;
        let (given, fresh) = {
            let state = self.lock();
            match &state.last {
                None => (None, false),
                Some(checked) => {
                    // A next run that is already behind us is not a next run: do not show it, and ask
                    // again for the one after.
                    let passed = matches!(
                        &checked.answer,
                        Some(TaskAnswer::Registered { next_run_at: Some(at) }) if *at <= now
                    );
                    let lasts = if checked.answer.is_some() {
                        self.inner.answer_lasts
                    } else {
                        self.inner.no_answer_lasts
                    };
                    (
                        if passed { None } else { checked.answer.clone() },
                        !passed && checked.at.elapsed() < lasts,
                    )
                }
            }
        };
        if !fresh {
            self.renew();
        }
        given
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Starts a question in a new thread unless one is already running.
    fn renew(&self) {
        if self.inner.query.is_none() {
            return;
        }
        {
            let mut state = self.lock();
            if state.asking {
                return;
            }
            state.asking = true;
        }
        let inner = Arc::clone(&self.inner);
        let started = std::thread::Builder::new()
            .name("panel-task-query".into())
            .spawn(move || {
                // A question that panics is no answer, and must not leave `asking` set for good.
                let answer = inner.query.as_ref().and_then(|query| {
                    std::panic::catch_unwind(AssertUnwindSafe(|| query.ask())).unwrap_or(None)
                });
                let mut state = inner
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state.last = Some(Checked {
                    at: Instant::now(),
                    answer,
                });
                state.asking = false;
                inner.idle.notify_all();
            });
        if started.is_err() {
            self.lock().asking = false;
            self.inner.idle.notify_all();
        }
    }

    /// Blocks until no question is running (tests only: the panel never waits).
    #[cfg(test)]
    pub(crate) fn settle(&self) {
        let mut state = self.lock();
        while state.asking {
            state = self
                .inner
                .idle
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

/// A scheduler that always gives the same answer, for tests of what is built on top of it.
#[cfg(test)]
pub(crate) struct FixedTask(pub Option<TaskAnswer>);

#[cfg(test)]
impl TaskQuery for FixedTask {
    fn ask(&self) -> Option<TaskAnswer> {
        self.0.clone()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    use chrono::TimeDelta;

    use super::*;
    use crate::testing::NOW;

    /// A scheduler that answers from a script and counts how often it was asked.
    struct Scripted {
        answers: Mutex<Vec<Option<TaskAnswer>>>,
        asked: Arc<AtomicUsize>,
    }

    impl Scripted {
        fn new(answers: Vec<Option<TaskAnswer>>) -> (Scripted, Arc<AtomicUsize>) {
            let asked = Arc::new(AtomicUsize::new(0));
            let scripted = Scripted {
                answers: Mutex::new(answers),
                asked: Arc::clone(&asked),
            };
            (scripted, asked)
        }
    }

    impl TaskQuery for Scripted {
        fn ask(&self) -> Option<TaskAnswer> {
            self.asked.fetch_add(1, Ordering::SeqCst);
            let mut answers = self.answers.lock().unwrap();
            if answers.is_empty() {
                None
            } else {
                answers.remove(0)
            }
        }
    }

    fn registered(next: Option<DateTime<Utc>>) -> Option<TaskAnswer> {
        Some(TaskAnswer::Registered { next_run_at: next })
    }

    // ---- what the child prints

    #[test]
    fn the_child_says_missing_or_exists_and_everything_else_is_no_answer() {
        assert_eq!(parse_answer("MISSING\r\n"), Some(TaskAnswer::Missing));
        assert_eq!(parse_answer("  MISSING "), Some(TaskAnswer::Missing));
        assert_eq!(parse_answer("EXISTS -\r\n"), registered(None));
        assert_eq!(
            parse_answer("EXISTS 2026-10-09T19:00:00Z\r\n"),
            registered(Some(
                DateTime::parse_from_rfc3339("2026-10-09T19:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc)
            ))
        );
        for nothing in [
            "",
            "\r\n",
            "EXISTS",
            "EXISTS mañana",
            "EXISTS 09/10/2026 16:00:00",
            "exists -",
            "ERROR: El sistema no puede encontrar el archivo especificado.",
            "MISSING EXISTS -",
        ] {
            assert_eq!(parse_answer(nothing), None, "{nothing:?} is no answer");
        }
    }

    #[test]
    fn the_question_only_reads_and_names_the_task_by_environment() {
        // The text never carries the name, so nothing typed anywhere can become code, and it has no
        // verb that registers, changes, runs or removes a task.
        for verb in [
            "Register",
            "Delete",
            "Disable",
            "Set-",
            "New-ScheduledTask",
            "Start-",
            "Stop-",
            "Remove",
            ".Run",
            "Invoke",
            "NewTask",
            "Save",
            "Enable-",
            ".Enabled =",
        ] {
            assert!(
                !PROBE.contains(verb),
                "the question must not contain {verb}"
            );
        }
        assert!(PROBE.contains("GetFolder($env:JOBBOT_TASK_FOLDER)"));
        assert!(PROBE.contains("GetTask($env:JOBBOT_TASK_NAME)"));
        assert!(!PROBE.contains(TASK_NAME) && !PROBE.contains(TASK_FOLDER));
        assert!(!PROBE.contains('"'), "one line, no double quotes to escape");
    }

    #[test]
    fn the_child_is_powershell_with_the_folder_and_the_name_in_its_environment() {
        let command = WindowsTasks::default_task().command();
        let env: Vec<(String, String)> = command
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.unwrap_or_default().to_string_lossy().into_owned(),
                )
            })
            .collect();
        assert!(env.contains(&("JOBBOT_TASK_FOLDER".into(), r"\jobbot".into())));
        assert!(env.contains(&("JOBBOT_TASK_NAME".into(), "run-daily".into())));
        let args: Vec<String> = command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(
            command
                .get_program()
                .to_string_lossy()
                .to_lowercase()
                .ends_with("powershell.exe")
        );
        assert_eq!(args[..3], ["-NoProfile", "-NonInteractive", "-Command"]);
        assert_eq!(args[3], PROBE);
    }

    // ---- the deadline

    #[cfg(windows)]
    #[test]
    fn a_child_that_ends_in_time_gives_its_output_and_one_that_fails_gives_none() {
        let mut ok = Command::new("cmd");
        ok.args(["/C", "echo hola"]);
        assert_eq!(
            run_within(ok, Duration::from_secs(10))
                .as_deref()
                .map(str::trim),
            Some("hola")
        );
        let mut failing = Command::new("cmd");
        failing.args(["/C", "echo roto & exit 3"]);
        assert_eq!(run_within(failing, Duration::from_secs(10)), None);
        assert_eq!(
            run_within(
                Command::new("a-program-that-does-not-exist"),
                Duration::from_secs(1)
            ),
            None
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_child_that_does_not_end_is_killed_at_the_deadline() {
        let mut slow = Command::new("cmd");
        slow.args(["/C", "ping -n 30 127.0.0.1 >nul"]);
        let started = Instant::now();
        assert_eq!(run_within(slow, Duration::from_millis(300)), None);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "it came back at the deadline, not when the child ended: {:?}",
            started.elapsed()
        );
    }

    // ---- the watch

    #[test]
    fn a_watch_that_asks_nothing_has_no_data() {
        assert_eq!(TaskWatch::off().current(*NOW), None);
    }

    #[test]
    fn the_first_look_starts_the_question_and_the_answer_waits_in_the_cache() {
        let (query, asked) = Scripted::new(vec![Some(TaskAnswer::Missing)]);
        let watch = TaskWatch::asking(query);
        watch.settle();
        assert_eq!(asked.load(Ordering::SeqCst), 1, "asked once, at the start");
        assert_eq!(watch.current(*NOW), Some(TaskAnswer::Missing));
        assert_eq!(watch.current(*NOW), Some(TaskAnswer::Missing));
        watch.settle();
        assert_eq!(
            asked.load(Ordering::SeqCst),
            1,
            "not again while it is fresh"
        );
    }

    #[test]
    fn a_failed_question_is_no_data_and_not_a_missing_task() {
        let (query, _) = Scripted::new(vec![None]);
        let watch = TaskWatch::asking(query);
        watch.settle();
        assert_eq!(watch.current(*NOW), None);
    }

    #[test]
    fn an_old_answer_is_renewed_and_a_failed_renewal_takes_it_back() {
        let (query, asked) = Scripted::new(vec![registered(None), None]);
        let watch = TaskWatch::build(Some(Box::new(query)), Duration::ZERO, Duration::ZERO);
        watch.renew();
        watch.settle();
        // Believed for no time at all: the old answer is given while the new one is asked for.
        assert_eq!(watch.current(*NOW), registered(None));
        watch.settle();
        assert_eq!(asked.load(Ordering::SeqCst), 2);
        // The renewal failed: no data, not the old answer dressed as current.
        assert_eq!(watch.current(*NOW), None);
    }

    #[test]
    fn a_next_run_that_has_passed_is_not_given_and_is_asked_again() {
        let soon = *NOW + TimeDelta::minutes(5);
        let later = *NOW + TimeDelta::minutes(65);
        let (query, asked) = Scripted::new(vec![registered(Some(soon)), registered(Some(later))]);
        let watch = TaskWatch::asking(query);
        watch.settle();
        assert_eq!(watch.current(*NOW), registered(Some(soon)));
        let after = *NOW + TimeDelta::minutes(6);
        assert_eq!(
            watch.current(after),
            None,
            "a moment in the past is not the next run"
        );
        watch.settle();
        assert_eq!(asked.load(Ordering::SeqCst), 2);
        assert_eq!(watch.current(after), registered(Some(later)));
    }

    #[test]
    fn a_question_that_never_ends_does_not_hold_up_the_one_who_looks() {
        struct Stuck(Mutex<mpsc::Receiver<()>>);
        impl TaskQuery for Stuck {
            fn ask(&self) -> Option<TaskAnswer> {
                let _ = self.0.lock().unwrap().recv();
                Some(TaskAnswer::Missing)
            }
        }
        let (release, wait) = mpsc::channel();
        let watch = TaskWatch::asking(Stuck(Mutex::new(wait)));
        let started = Instant::now();
        for _ in 0..50 {
            assert_eq!(watch.current(*NOW), None);
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "looking never waits for Windows"
        );
        release.send(()).unwrap();
        watch.settle();
        assert_eq!(watch.current(*NOW), Some(TaskAnswer::Missing));
    }

    #[test]
    fn a_question_that_panics_does_not_leave_the_watch_waiting_forever() {
        struct Broken;
        impl TaskQuery for Broken {
            fn ask(&self) -> Option<TaskAnswer> {
                panic!("the scheduler blew up");
            }
        }
        let watch = TaskWatch::asking(Broken);
        watch.settle();
        assert_eq!(watch.current(*NOW), None);
    }

    /// Asks the real Task Scheduler, read-only, and prints how long it took. Not part of the normal
    /// run: `cargo test real_scheduler -- --ignored --nocapture`.
    #[test]
    #[ignore = "asks the real Windows Task Scheduler (read-only)"]
    fn the_real_scheduler_answers_within_the_deadline() {
        let query = WindowsTasks::default_task();
        for round in 1..=3 {
            let started = Instant::now();
            let answer = query.ask();
            let took = started.elapsed();
            println!("round {round}: {answer:?} in {took:?}");
            assert!(answer.is_some(), "Windows answered");
            assert!(took < QUERY_DEADLINE, "inside the deadline, took {took:?}");
        }
    }
}
