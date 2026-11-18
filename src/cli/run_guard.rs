//! When a run may start and when it must stop: the STOP file, the execution window and the run
//! lock, plus the `stop` and `resume` commands that switch STOP.

use std::io::Write;

use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};
use chrono_tz::America::Santiago;
use jobbot_app::reporting::status::RunEnd;

use super::{EXIT_ERROR, EXIT_OK, EXIT_OUTSIDE_WINDOW, EXIT_STOP};
use crate::actions;
use crate::paths::Paths;
use crate::window::{Schedule, in_run_window, load_schedule, next_window_start};

pub(super) const STOP_MESSAGE: &str =
    "STOP activo: no se ejecuta nada. Usa `jobbot resume` para reanudar.";

/// The two switches in front of every mutating command. STOP is checked first and nothing
/// overrides it; the execution window comes second and `--ignore-window` skips only that.
/// `Err(code)` is the exit code to stop with; `Ok` carries the schedule that was read for the
/// window check (none with `--ignore-window`), so the run watches the same one it was admitted by.
fn blocked(
    paths: &Paths,
    now: DateTime<Utc>,
    ignore_window: bool,
    out: &mut dyn Write,
) -> Result<std::result::Result<Option<Schedule>, i32>> {
    if paths.stop_file.exists() {
        writeln!(out, "{STOP_MESSAGE}")?;
        return Ok(Err(EXIT_STOP));
    }
    if ignore_window {
        return Ok(Ok(None));
    }
    // A missing or broken schedule is an error (exit 1), not a free pass: fail closed.
    let schedule = load_schedule(&paths.profile_dir.join("schedule.toml"))?;
    if !in_run_window(&schedule, now) {
        let next = next_window_start(&schedule, now).with_timezone(&Santiago);
        writeln!(
            out,
            "Fuera de la ventana de ejecución (horario laboral de Gonza): no se ejecuta nada. \
             Próxima ventana: {} (hora de Santiago). Usa --ignore-window para forzar.",
            next.format("%Y-%m-%d %H:%M")
        )?;
        return Ok(Err(EXIT_OUTSIDE_WINDOW));
    }
    Ok(Ok(Some(schedule)))
}

const LOCK_MESSAGE: &str = "Otra ejecución de jobbot está en curso (data/jobbot.lock): no se ejecuta nada. Espera a que termine.";

/// Takes `data/jobbot.lock` for as long as the returned file lives. The lock belongs to the open
/// file, so the system drops it if the program dies and it never goes stale. `None` means another
/// jobbot process holds it.
fn run_lock(paths: &Paths) -> Result<Option<std::fs::File>> {
    std::fs::create_dir_all(&paths.data_dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(paths.data_dir.join("jobbot.lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(e.into()),
    }
}

/// Watches the execution window while a command runs. `blocked` only looks at the moment the
/// command starts; a run that begins in the evening can last into Gonza's working hours, so the
/// long steps ask the watch again. The time is the start time plus the time elapsed since, which
/// keeps tests deterministic. A run started with `--ignore-window` never closes.
pub(super) struct WindowWatch {
    schedule: Option<Schedule>,
    start: DateTime<Utc>,
    clock: std::time::Instant,
}

impl WindowWatch {
    /// The start time plus the time elapsed since: "now" for a run whose clock is injected.
    pub(super) fn now(&self) -> DateTime<Utc> {
        let elapsed =
            TimeDelta::from_std(self.clock.elapsed()).unwrap_or_else(|_| TimeDelta::zero());
        self.start + elapsed
    }

    pub(super) fn is_open(&self) -> bool {
        let Some(schedule) = &self.schedule else {
            return true;
        };
        in_run_window(schedule, self.now())
    }
}

/// What a mutating command holds while it runs: the run lock and the window watch.
pub(super) struct RunGuard {
    _lock: std::fs::File,
    pub(super) watch: WindowWatch,
}

/// The checks in front of every mutating command, in order: STOP, the execution window, then the
/// run lock (two jobbot processes writing the same database and driving the same browser profile
/// could send one form twice). `Err(code)` is the exit code to stop with.
pub(super) fn start_run(
    paths: &Paths,
    now: DateTime<Utc>,
    ignore_window: bool,
    out: &mut dyn Write,
) -> Result<std::result::Result<RunGuard, i32>> {
    let schedule = match blocked(paths, now, ignore_window, out)? {
        Ok(schedule) => schedule,
        Err(code) => return Ok(Err(code)),
    };
    let Some(lock) = run_lock(paths)? else {
        writeln!(out, "{LOCK_MESSAGE}")?;
        return Ok(Err(EXIT_ERROR));
    };
    Ok(Ok(RunGuard {
        _lock: lock,
        watch: WindowWatch {
            schedule,
            start: now,
            clock: std::time::Instant::now(),
        },
    }))
}

pub(super) const WINDOW_CLOSED_MESSAGE: &str =
    "La ventana de ejecución se cerró (empieza el horario laboral de Gonza)";

pub(super) fn cmd_stop(paths: &Paths, out: &mut dyn Write) -> Result<i32> {
    actions::stop(paths)?;
    writeln!(
        out,
        "STOP creado: discover, prepare, submit, run-daily, dossier, freelance y claude-check quedan detenidos."
    )?;
    Ok(EXIT_OK)
}

pub(super) fn cmd_resume(paths: &Paths, out: &mut dyn Write) -> Result<i32> {
    actions::resume(paths)?;
    writeln!(out, "STOP eliminado.")?;
    Ok(EXIT_OK)
}

/// When the window closes in the middle of a run (a slow board, a long queue of CVs) the rest of
/// the steps are skipped. That is not a failure and not exit 4: `run-daily.cmd` discards the log
/// of an exit 4, and this run did real work that belongs in it.
fn window_closed(watch: &WindowWatch, skipped: &str, out: &mut dyn Write) -> Result<bool> {
    if watch.is_open() {
        return Ok(false);
    }
    writeln!(out, "{WINDOW_CLOSED_MESSAGE}: se omite {skipped}.")?;
    Ok(true)
}

/// The check between two steps of `run-daily`: STOP ends the run, and so does a closed execution
/// window (`skipped` names what the window kept from running; `window_code` is the exit code the
/// run ends with in that case). `None` means the next step may start.
pub(super) fn stopped_or_closed(
    paths: &Paths,
    watch: &WindowWatch,
    skipped: &str,
    window_code: i32,
    out: &mut dyn Write,
) -> Result<Option<(i32, RunEnd)>> {
    if paths.stop_file.exists() {
        writeln!(out, "{STOP_MESSAGE}")?;
        return Ok(Some((EXIT_STOP, RunEnd::Stopped)));
    }
    if window_closed(watch, skipped, out)? {
        return Ok(Some((window_code, RunEnd::WindowClosed)));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use crate::cli::test_support::{busy_now, open_now, plain, temp_root};

    const MUTATING: [&str; 4] = ["discover", "prepare", "submit", "run-daily"];

    #[tokio::test]
    async fn stop_blocks_mutating_commands() {
        let root = temp_root();
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        for cmd in MUTATING {
            let (code, out) = plain(root.path(), open_now(), &[cmd]).await;
            assert_eq!(code, 3, "{cmd}");
            assert!(out.contains("STOP activo"), "{cmd}: {out}");
        }
    }

    #[tokio::test]
    async fn outside_window_blocks_mutating_commands() {
        let root = temp_root();
        for cmd in MUTATING {
            let (code, out) = plain(root.path(), busy_now(), &[cmd]).await;
            assert_eq!(code, 4, "{cmd}");
            // The next window opens at 21:00 UTC = 18:00 in Santiago (UTC-3 in October).
            assert!(
                out.contains("Fuera de la ventana")
                    && out.contains("Próxima ventana: 2026-10-14 18:00"),
                "{cmd}: {out}"
            );
        }
        // The read-only commands do not care about the window.
        assert_eq!(plain(root.path(), busy_now(), &["status"]).await.0, 0);
        assert_eq!(plain(root.path(), busy_now(), &["digest"]).await.0, 0);
    }

    #[tokio::test]
    async fn ignore_window_overrides_the_window_not_stop() {
        let root = temp_root();
        // Busy hour, no STOP: --ignore-window lets the mutating commands through.
        let (code, _) = plain(root.path(), busy_now(), &["discover", "--ignore-window"]).await;
        assert_eq!(code, 0);
        let (code, _) = plain(root.path(), busy_now(), &["submit", "--ignore-window"]).await;
        assert_eq!(code, 0);
        // With STOP present the same flag changes nothing, in a busy hour or an open one.
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        for now in [busy_now(), open_now()] {
            for cmd in MUTATING {
                let (code, out) = plain(root.path(), now, &[cmd, "--ignore-window"]).await;
                assert_eq!(code, 3, "{cmd}");
                assert!(out.contains("STOP activo"), "{cmd}: {out}");
            }
        }
        // STOP also wins over the window when the flag is absent.
        assert_eq!(plain(root.path(), busy_now(), &["discover"]).await.0, 3);
    }

    #[tokio::test]
    async fn broken_schedule_fails_closed() {
        let root = temp_root();
        std::fs::write(
            root.path().join("profile").join("schedule.toml"),
            "busy = []",
        )
        .unwrap();
        let (code, out) = plain(root.path(), open_now(), &["discover"]).await;
        assert_eq!(code, 1);
        assert!(
            out.contains("Error:") && out.contains("no [[busy]] blocks"),
            "{out}"
        );
    }

    #[tokio::test]
    async fn stop_and_resume_toggle_the_file() {
        let root = temp_root();
        assert_eq!(plain(root.path(), open_now(), &["stop"]).await.0, 0);
        assert!(root.path().join("STOP").exists());
        assert_eq!(plain(root.path(), open_now(), &["resume"]).await.0, 0);
        assert!(!root.path().join("STOP").exists());
        // Resuming when there is no STOP is not an error.
        assert_eq!(plain(root.path(), open_now(), &["resume"]).await.0, 0);
    }

    #[tokio::test]
    async fn a_second_run_is_refused_while_another_one_holds_the_run_lock() {
        let root = temp_root();
        std::fs::create_dir_all(root.path().join("data")).unwrap();
        // Another jobbot process (an assisted session, the hourly task) holds the lock file.
        let held = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(root.path().join("data").join("jobbot.lock"))
            .unwrap();
        held.lock().unwrap();
        for cmd in MUTATING {
            let (code, out) = plain(root.path(), open_now(), &[cmd]).await;
            assert_eq!(code, 1, "{cmd}: {out}");
            assert!(out.contains("Otra ejecución"), "{cmd}: {out}");
        }
        // Study and bookkeeping commands never take the lock.
        assert_eq!(plain(root.path(), open_now(), &["status"]).await.0, 0);
        drop(held);
        assert_eq!(plain(root.path(), open_now(), &["discover"]).await.0, 0);
    }

    #[tokio::test]
    async fn stop_names_every_command_it_stops() {
        let root = temp_root();
        let (code, out) = plain(root.path(), open_now(), &["stop"]).await;
        assert_eq!(code, 0, "{out}");
        for name in [
            "discover",
            "prepare",
            "submit",
            "run-daily",
            "dossier",
            "freelance",
            "claude-check",
        ] {
            assert!(out.contains(name), "STOP message should name {name}: {out}");
        }
    }
}
