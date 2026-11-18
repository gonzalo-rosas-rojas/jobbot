//! The scheduled job: `run-daily` runs discover, prepare, submit and the dossiers in order, keeps
//! the record of the run for `status` and the panel, and always writes the digest.

use std::io::Write;

use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};
use jobbot_app::reporting::status::{DISCOVER_COOLDOWN_HOURS, RunEnd, StepLog, record_run};

use super::dossier::run_dossier_step;
use super::pipeline::{cmd_discover, cmd_prepare};
use super::reports::cmd_digest;
use super::run_guard::{WindowWatch, stopped_or_closed};
use super::submit::cmd_submit;
use super::{Deps, EXIT_ERROR, EXIT_OK, open_tracker};
use crate::claude::is_account_error;
use crate::form::SubmitMode;
use crate::paths::Paths;
use crate::tracker::State;

/// The scheduled job. The steps run in order and the first one that fails skips the rest, but
/// the digest is always written. Submission is `SubmitMode::Live`: the gate inside `submit_all`
/// still turns every application that Gonza has not approved yet into a dry run.
pub(super) async fn cmd_run_daily(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    watch: &WindowWatch,
    out: &mut dyn Write,
) -> Result<i32> {
    writeln!(out, "== run-daily {} ==", now.format("%Y-%m-%d %H:%M UTC"))?;
    let mut log = StepLog::default();
    // A step that fails outright (a broken profile file, a database error) is shown and costs the
    // other steps, but never the digest: it is what Gonza reads to know what happened.
    let (steps, end) = match run_daily_steps(paths, deps, now, watch, &mut log, out).await {
        Ok(done) => done,
        Err(err) => {
            writeln!(out, "Error en un paso de run-daily: {err:#}")?;
            (EXIT_ERROR, RunEnd::Failed(format!("{err:#}")))
        }
    };
    // The record is a convenience for `status` and the panel: failing to write it must not cost
    // the run its digest or its exit code.
    let recorded =
        open_tracker(paths).and_then(|tracker| record_run(&tracker, watch.now(), &end, &mut log));
    if let Err(err) = recorded {
        writeln!(out, "No pude anotar la corrida: {err:#}")?;
    }
    let digest_code = cmd_digest(paths, now, out)?;
    Ok(if steps != EXIT_OK { steps } else { digest_code })
}

/// Transitions into `states` ever recorded; what a step moved is the difference across it.
fn transitions_into(paths: &Paths, states: &[State]) -> Result<i64> {
    Ok(open_tracker(paths)?.count_events(states, DateTime::UNIX_EPOCH)?)
}

/// The steps in order. `log` collects what each did while it runs, so that a step that fails
/// outright (`Err`) still leaves the record of the ones before it. Returns the exit code and how
/// the run ended.
async fn run_daily_steps(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    watch: &WindowWatch,
    log: &mut StepLog,
    out: &mut dyn Write,
) -> Result<(i32, RunEnd)> {
    // The task fires hourly; the boards do not change that fast. Open the tracker only to read
    // the stamp, so the discover step gets its own connection.
    let last_discover = open_tracker(paths)?
        .get_meta("last_discover_at")?
        .and_then(|text| DateTime::parse_from_rfc3339(&text).ok())
        .map(|t| t.with_timezone(&Utc));
    let last_discover = match last_discover {
        // A stamp from the future (the clock went back, or it came from another date) says
        // nothing about when discovery last ran: it does not hold the next run back.
        Some(last) if last > now => {
            tracing::warn!(stamp = %last, %now, "last_discover_at is in the future; discovery runs");
            None
        }
        known => known,
    };
    match last_discover {
        Some(last) if now - last < TimeDelta::hours(DISCOVER_COOLDOWN_HOURS) => {
            writeln!(
                out,
                "Descubrimiento omitido: el último fue hace {} min (se repite cada {} h).",
                (now - last).num_minutes(),
                DISCOVER_COOLDOWN_HOURS
            )?;
            log.skipped("discover", "cooldown");
        }
        _ => {
            let before = transitions_into(paths, &[State::Scored])?;
            // Until the step answers it counts as failed: a step that errors out (`?`) leaves this.
            log.skipped("discover", "error");
            let code = cmd_discover(paths, deps, now, false, out).await?;
            let new = transitions_into(paths, &[State::Scored])? - before;
            log.ran("discover", serde_json::json!({"new": new}));
            if code != EXIT_OK {
                let why = format!("descubrir terminó con código {code}");
                return Ok((code, RunEnd::Failed(why)));
            }
        }
    }
    // Gonza may create STOP while a long run is in progress: look again between steps.
    if let Some(end) = stopped_or_closed(
        paths,
        watch,
        "la preparación de CVs y el envío",
        EXIT_OK,
        out,
    )? {
        return Ok(end);
    }
    let before = transitions_into(paths, &[State::Prepared])?;
    log.skipped("prepare", "error");
    let code = cmd_prepare(paths, deps, now, out).await?;
    let prepared = transitions_into(paths, &[State::Prepared])? - before;
    if code != EXIT_OK {
        let why = format!("prepare terminó con código {code}");
        writeln!(out, "{why}; se omite el envío.")?;
        log.ran(
            "prepare",
            serde_json::json!({"prepared": prepared, "code": code}),
        );
        return Ok((code, RunEnd::Failed(why)));
    }
    log.ran("prepare", serde_json::json!({"prepared": prepared}));
    if let Some(end) = stopped_or_closed(paths, watch, "el envío", EXIT_OK, out)? {
        return Ok(end);
    }
    let sent = transitions_into(paths, &[State::Submitted])?;
    let handed = transitions_into(paths, &[State::NeedsGonza])?;
    log.skipped("submit", "error");
    let code = cmd_submit(paths, deps, now, false, SubmitMode::Live, watch, out).await?;
    log.ran(
        "submit",
        serde_json::json!({
            "submitted": transitions_into(paths, &[State::Submitted])? - sent,
            "needs_gonza": transitions_into(paths, &[State::NeedsGonza])? - handed,
        }),
    );
    // The dossiers come last: they need no browser and a problem with them changes nothing above.
    // A closed window here still ends the run with the submit step's own code.
    if let Some(end) = stopped_or_closed(paths, watch, "los dossiers", code, out)? {
        return Ok(end);
    }
    match run_dossier_step(paths, deps, now, watch, out).await {
        Ok(written) => log.ran("dossiers", serde_json::json!({"written": written})),
        // A missing program or a spent budget only costs the dossiers. A login that is not the
        // private subscription is different: nothing was asked, and the run fails so the
        // scheduled task's log and the digest show it before anything else is tried.
        Err(err) if is_account_error(&err) => {
            writeln!(out, "Claude no se usó: {err:#}")?;
            log.skipped("dossiers", "error");
            let why = format!("Claude no se usó: {err:#}");
            return Ok((EXIT_ERROR, RunEnd::Failed(why)));
        }
        Err(err) => {
            writeln!(out, "Dossiers omitidos: {err:#}")?;
            log.skipped("dossiers", "error");
        }
    }
    Ok((code, RunEnd::Done))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use chrono::{DateTime, TimeDelta, TimeZone, Utc};

    use super::*;
    use crate::cli::test_support::{
        FakeFiller, FakeRenderer, busy_now, cli, fake_deps, open_now, plain, seed, seed_prepared,
        seed_replied, strong_job, temp_root, tracker_of, with_claude, write_cv_master,
    };
    use crate::cli::{Deps, EXIT_CV_NOT_READY, EXIT_ERROR, EXIT_STOP};
    use crate::form::SubmitMode;
    use crate::models::FetchResult;
    use crate::sources::Source;
    use crate::testing::{FakeClaude, SAMPLE_CV, make_job, make_job_with};

    #[tokio::test]
    async fn run_daily_writes_the_digest_even_when_a_step_fails() {
        let root = temp_root();
        // A step that cannot even start (a broken profile file) must not cost Gonza the digest.
        std::fs::write(
            root.path().join("profile").join("criteria.toml"),
            "this is = not [valid",
        )
        .unwrap();

        let (code, out) = plain(root.path(), open_now(), &["run-daily"]).await;

        assert_eq!(code, EXIT_ERROR, "{out}");
        assert!(out.contains("criteria"), "the failure is shown: {out}");
        let digests = std::fs::read_dir(root.path().join("data"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("digest-"))
            .count();
        assert_eq!(digests, 1, "the digest was written: {out}");
    }

    #[tokio::test]
    async fn run_daily_keeps_unreviewed_applications_in_dry_run() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        // Prepared but NOT approved: even though run-daily submits live, the review gate must
        // turn this into a dry run.
        seed_prepared(root.path(), &make_job_with(|j| j.external_id = "9".into()));
        let (deps, calls) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        assert_eq!(*calls.lock().unwrap(), vec![SubmitMode::DryRun], "{out}");
        assert!(
            root.path()
                .join("data")
                .join("digest-2026-10-14.md")
                .exists()
        );
    }

    #[tokio::test]
    async fn run_daily_with_placeholder_cv_skips_submit_but_writes_the_digest() {
        let root = temp_root();
        write_cv_master(root.path(), "⟦ pendiente ⟧");
        seed(root.path(), &make_job(), &[]);
        let (deps, calls) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;
        assert_eq!(code, 2, "{out}");
        assert!(calls.lock().unwrap().is_empty());
        assert!(out.contains("se omite el envío"), "{out}");
        assert!(
            root.path()
                .join("data")
                .join("digest-2026-10-14.md")
                .exists()
        );
    }

    #[tokio::test]
    async fn run_daily_skips_discovery_within_the_cooldown() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        let (deps, _) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Descubrimiento: 0 avisos"), "{out}");
        // The same hour again (the task fires hourly): the sources are not fetched a second time...
        let (deps, _) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Descubrimiento omitido") && !out.contains("Descubrimiento: 0 avisos"),
            "{out}"
        );
        // ...but an explicit discover always runs.
        let (deps, _) = fake_deps(vec![]);
        let (_, out) = cli(root.path(), open_now(), deps, &["discover"]).await;
        assert!(out.contains("Descubrimiento: 0 avisos"), "{out}");
    }

    /// What `run-daily` says about discovery when the stamp of the last one reads `stamp` and the
    /// clock stands at `open_now()` (2026-10-14T22:00:00Z).
    async fn discovery_line_with_stamp(stamp: &str) -> String {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        tracker_of(root.path())
            .set_meta("last_discover_at", stamp)
            .unwrap();
        let (deps, _) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        out.lines()
            .find(|line| line.starts_with("Descubrimiento"))
            .unwrap_or_else(|| panic!("no line about discovery: {out}"))
            .to_string()
    }

    #[tokio::test]
    async fn the_discovery_stamp_is_read_in_any_rfc_3339_shape_and_the_cooldown_is_six_hours() {
        let skipped = "Descubrimiento omitido: el último fue hace 30 min (se repite cada 6 h).";
        for stamp in [
            "2026-10-14T21:30:00+00:00",
            "2026-10-14T21:30:00Z",
            "2026-10-14T21:30:00.000000000+00:00",
            "2026-10-14T18:30:00-03:00",
            "2026-10-15T01:30:00+04:00",
        ] {
            assert_eq!(discovery_line_with_stamp(stamp).await, skipped, "{stamp}");
        }
        // One second short of six hours it is still skipped; at six hours it runs.
        assert_eq!(
            discovery_line_with_stamp("2026-10-14T16:00:01+00:00").await,
            "Descubrimiento omitido: el último fue hace 359 min (se repite cada 6 h)."
        );
        assert!(
            discovery_line_with_stamp("2026-10-14T16:00:00+00:00")
                .await
                .starts_with("Descubrimiento: 0 avisos")
        );
    }

    #[tokio::test]
    async fn a_discovery_stamp_that_is_not_a_date_does_not_hold_discovery_back() {
        for stamp in [
            "ayer",
            "2026-10-14",
            "2026-10-14T21:30:00",
            "1791000000",
            "",
        ] {
            assert!(
                discovery_line_with_stamp(stamp)
                    .await
                    .starts_with("Descubrimiento: 0 avisos"),
                "{stamp:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_discovery_stamp_in_the_future_does_not_count_as_a_recent_run() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        // The clock went back, or the stamp came from another date: it is not "just ran".
        tracker_of(root.path())
            .set_meta(
                "last_discover_at",
                &(open_now() + TimeDelta::hours(2)).to_rfc3339(),
            )
            .unwrap();
        let (deps, _) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Descubrimiento: 0 avisos") && !out.contains("Descubrimiento omitido"),
            "{out}"
        );
    }

    /// A board that is slow to answer, so a run lasts long enough to cross the edge of the window.
    struct SlowSource(std::time::Duration);

    #[async_trait::async_trait]
    impl Source for SlowSource {
        fn name(&self) -> &str {
            "slow"
        }
        async fn fetch(&self) -> FetchResult {
            tokio::time::sleep(self.0).await;
            FetchResult {
                jobs: vec![],
                errors: vec![],
            }
        }
    }

    #[tokio::test]
    async fn run_daily_stops_when_the_window_closes_during_the_run() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        // Prepared and not approved: if the run reached `submit` it would dry-run this one.
        seed_prepared(root.path(), &make_job());
        // 0.4 s before Santiago's busy block starts (09:00 CLST = 12:00 UTC); discovery takes 1.2 s.
        let start =
            Utc.with_ymd_and_hms(2026, 10, 14, 11, 59, 59).unwrap() + TimeDelta::milliseconds(600);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let deps = Deps {
            sources: Some(vec![Box::new(SlowSource(
                std::time::Duration::from_millis(1200),
            ))]),
            renderer: Box::new(FakeRenderer),
            filler: Some(Box::new(FakeFiller(calls.clone()))),
            inspector: None,
            open_checker: None,
            claude: None,
        };
        let (code, out) = cli(root.path(), start, deps, &["run-daily"]).await;
        // Not an error and not exit 4: run-daily.cmd would throw the log of exit 4 away.
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("La ventana de ejecución se cerró"), "{out}");
        assert!(
            calls.lock().unwrap().is_empty(),
            "nothing is submitted once the window has closed: {out}"
        );
        // The digest is still written.
        assert!(
            root.path()
                .join("data")
                .join("digest-2026-10-14.md")
                .exists()
        );
    }

    // ---- the record of the last run (`meta.last_run_*`), which `status` and the panel read

    /// The three keys `run-daily` leaves behind: (when, result, steps), `None` before any run.
    fn last_run(root: &Path) -> Option<(DateTime<Utc>, String, serde_json::Value)> {
        let tracker = tracker_of(root);
        let at = tracker.get_meta("last_run_at").unwrap()?;
        let result = tracker.get_meta("last_run_result").unwrap().unwrap();
        let steps = tracker.get_meta("last_run_steps").unwrap().unwrap();
        Some((
            DateTime::parse_from_rfc3339(&at)
                .unwrap()
                .with_timezone(&Utc),
            result,
            serde_json::from_str(&steps).unwrap(),
        ))
    }

    /// A board that, while it is being read, gets a STOP file from Gonza.
    struct StopWhileFetching(PathBuf);

    #[async_trait::async_trait]
    impl Source for StopWhileFetching {
        fn name(&self) -> &str {
            "stopper"
        }
        async fn fetch(&self) -> FetchResult {
            std::fs::write(&self.0, "x").unwrap();
            FetchResult {
                jobs: vec![],
                errors: vec![],
            }
        }
    }

    #[tokio::test]
    async fn run_daily_leaves_when_how_and_what_it_did_in_meta() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        let (deps, _) = fake_deps(vec![strong_job()]);

        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;

        assert_eq!(code, 0, "{out}");
        let (at, result, steps) = last_run(root.path()).expect("the run was recorded");
        assert!(
            at >= open_now() && at < open_now() + TimeDelta::minutes(1),
            "the end of the run, {at}"
        );
        assert_eq!(result, "ok");
        assert_eq!(
            steps,
            serde_json::json!({
                "discover": {"ran": true, "new": 1},
                "prepare": {"ran": true, "prepared": 1},
                "submit": {"ran": true, "submitted": 0, "needs_gonza": 0},
                "dossiers": {"ran": true, "written": 0},
            }),
            "{out}"
        );

        // The panel reads back exactly what the run wrote, key for key.
        let panel = crate::panel::open(Paths::new(root.path()), Box::new(open_now)).unwrap();
        let controls = crate::panel::snapshot::build_snapshot(&panel)
            .map(|snapshot| serde_json::to_value(snapshot.controls).unwrap())
            .unwrap();
        assert_eq!(controls["last_run_result"], "ok");
        assert_eq!(controls["last_run_steps"], steps);
        assert_eq!(
            controls["last_run_at"].as_str(),
            Some(
                at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                    .as_str()
            )
        );
        assert_eq!(controls["engine"], "recent");
    }

    #[tokio::test]
    async fn run_daily_counts_only_what_this_run_moved() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        // Two applications already prepared by earlier runs, at the very same instant on purpose.
        seed_prepared(root.path(), &make_job_with(|j| j.external_id = "8".into()));
        seed_prepared(root.path(), &make_job_with(|j| j.external_id = "9".into()));
        let (deps, _) = fake_deps(vec![]);

        cli(root.path(), open_now(), deps, &["run-daily"]).await;

        let (_, _, steps) = last_run(root.path()).unwrap();
        assert_eq!(
            steps["prepare"],
            serde_json::json!({"ran": true, "prepared": 0})
        );
    }

    #[tokio::test]
    async fn the_next_hourly_run_notes_the_discovery_it_skipped_and_replaces_the_record() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        let first = open_now();
        let second = first + TimeDelta::hours(1);
        cli(root.path(), first, fake_deps(vec![]).0, &["run-daily"]).await;
        let (before, _, _) = last_run(root.path()).unwrap();

        let (code, out) = cli(root.path(), second, fake_deps(vec![]).0, &["run-daily"]).await;

        assert_eq!(code, 0, "{out}");
        let (at, result, steps) = last_run(root.path()).unwrap();
        assert!(at > before, "the record is of the latest run");
        assert_eq!(result, "ok");
        assert_eq!(
            steps["discover"],
            serde_json::json!({"ran": false, "skipped": "cooldown"})
        );
    }

    #[tokio::test]
    async fn a_stop_created_mid_run_is_recorded_as_stopped_with_the_steps_it_cost() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        let (mut deps, _) = fake_deps(vec![]);
        deps.sources = Some(vec![Box::new(StopWhileFetching(root.path().join("STOP")))]);

        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;

        assert_eq!(code, EXIT_STOP, "{out}");
        let (_, result, steps) = last_run(root.path()).expect("a stopped run is recorded too");
        assert_eq!(result, "stopped");
        assert_eq!(steps["discover"]["ran"], serde_json::json!(true));
        for step in ["prepare", "submit", "dossiers"] {
            assert_eq!(
                steps[step],
                serde_json::json!({"ran": false, "skipped": "stop"}),
                "{step}"
            );
        }
    }

    #[tokio::test]
    async fn a_window_that_closes_mid_run_is_recorded_as_window_closed() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        // Same edge as `run_daily_stops_when_the_window_closes_during_the_run`.
        let start =
            Utc.with_ymd_and_hms(2026, 10, 14, 11, 59, 59).unwrap() + TimeDelta::milliseconds(600);
        let (mut deps, _) = fake_deps(vec![]);
        deps.sources = Some(vec![Box::new(SlowSource(
            std::time::Duration::from_millis(1200),
        ))]);

        let (code, out) = cli(root.path(), start, deps, &["run-daily"]).await;

        assert_eq!(code, 0, "{out}");
        let (at, result, steps) = last_run(root.path()).unwrap();
        assert_eq!(result, "window_closed");
        assert!(
            at >= start + TimeDelta::seconds(1),
            "the end, not the start: {at}"
        );
        assert_eq!(
            steps["prepare"],
            serde_json::json!({"ran": false, "skipped": "window_closed"})
        );
    }

    #[tokio::test]
    async fn a_failed_step_is_recorded_as_one_line_of_error() {
        let root = temp_root();
        std::fs::write(
            root.path().join("profile").join("criteria.toml"),
            "this is = not [valid",
        )
        .unwrap();

        let (code, _) = plain(root.path(), open_now(), &["run-daily"]).await;

        assert_eq!(code, EXIT_ERROR);
        let (_, result, steps) = last_run(root.path()).unwrap();
        assert!(
            result.starts_with("error: ") && result.contains("criteria") && !result.contains('\n'),
            "{result}"
        );
        // The result keeps 300 characters of the error after the "error: " prefix.
        assert!(
            result.chars().count() <= "error: ".len() + 300,
            "short enough to show: {result}"
        );
        assert_eq!(
            steps["discover"],
            serde_json::json!({"ran": false, "skipped": "error"})
        );
    }

    #[tokio::test]
    async fn a_cv_that_is_not_ready_is_an_error_that_skips_the_rest() {
        let root = temp_root();
        write_cv_master(root.path(), "⟦ pendiente ⟧");
        seed(root.path(), &make_job(), &[]);

        let (code, _) = plain(root.path(), open_now(), &["run-daily"]).await;

        assert_eq!(code, EXIT_CV_NOT_READY);
        let (_, result, steps) = last_run(root.path()).unwrap();
        assert!(
            result.starts_with("error: prepare terminó con código 2"),
            "{result}"
        );
        assert_eq!(steps["prepare"]["code"], serde_json::json!(2));
        assert_eq!(
            steps["submit"],
            serde_json::json!({"ran": false, "skipped": "after_error"})
        );
    }

    #[tokio::test]
    async fn a_login_that_is_not_private_is_recorded_as_an_error_of_the_run() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed_replied(root.path(), &make_job());
        let fake = Arc::new(
            FakeClaude::new(vec![]).refusing_account("la cuenta privada de jobbot no tiene sesión"),
        );
        let deps = with_claude(fake_deps(vec![]).0, &fake);

        let (code, _) = cli(root.path(), open_now(), deps, &["run-daily"]).await;

        assert_eq!(code, EXIT_ERROR);
        let (_, result, _) = last_run(root.path()).unwrap();
        assert!(
            result.starts_with("error: Claude no se usó") && result.contains("no tiene sesión"),
            "{result}"
        );
    }

    #[tokio::test]
    async fn a_run_turned_away_at_the_door_leaves_the_last_real_run_alone() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        cli(root.path(), open_now(), fake_deps(vec![]).0, &["run-daily"]).await;
        let before = last_run(root.path()).unwrap();

        // The hourly task fires during working hours, and again with a STOP: both exit at once.
        assert_eq!(plain(root.path(), busy_now(), &["run-daily"]).await.0, 4);
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        assert_eq!(plain(root.path(), open_now(), &["run-daily"]).await.0, 3);

        assert_eq!(
            last_run(root.path()).unwrap(),
            before,
            "nothing ran, so nothing is stamped (and the task's log stays empty)"
        );
    }
}
