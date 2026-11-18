//! Interview dossiers: the `dossier` command, and the step of `run-daily` that writes one for
//! each application with an answer that has none yet.

use std::io::Write;

use anyhow::Result;
use chrono::{DateTime, Utc};

use super::claude::{ClaudeSetup, print_account_warnings, read_cv_master};
use super::prep::open_prep_store;
use super::run_guard::WindowWatch;
use super::{Deps, EXIT_ERROR, EXIT_OK, open_tracker};
use crate::claude::{is_budget_error, load_claude_config};
use crate::dossier::{build_dossier, dossier_path};
use crate::paths::Paths;
use crate::rules::load_exclusions;
use crate::tracker::State;

pub(super) async fn cmd_dossier(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    keys: &[String],
    out: &mut dyn Write,
) -> Result<i32> {
    let setup = ClaudeSetup::open(paths, deps)?;
    let tracker = &setup.tracker;
    let store = open_prep_store(paths)?;
    let cv_md = read_cv_master(paths)?;
    let exclusions = load_exclusions(&paths.profile_dir.join("exclusions.toml"))?;
    let gate = setup.gate();
    // Before any prompt: a wrong login stops the whole command, not one dossier. A managed
    // policy on the account is shown, as in `run-daily`.
    let seen = gate.check_account(now).await?;
    print_account_warnings(&seen, out)?;
    let dir = paths.data_dir.join("dossiers");
    let mut code = EXIT_OK;
    for key in keys {
        let Some(job) = tracker.get_job(key)? else {
            writeln!(out, "Aviso desconocido: {key}")?;
            code = EXIT_ERROR;
            continue;
        };
        match build_dossier(&gate, &store, &job, &cv_md, &exclusions, &dir, now).await {
            Ok(outcome) => writeln!(out, "{}", outcome.describe())?,
            Err(err) => {
                writeln!(out, "Error en {key}: {err:#}")?;
                code = EXIT_ERROR;
                if is_budget_error(&err) {
                    break; // every further call would be refused the same way
                }
            }
        }
    }
    Ok(code)
}

/// The last step of `run-daily`: the dossier of every application that has an answer (an
/// interview first, then a test, then a reply) and has none yet, at most `max_dossiers_per_run`.
/// It looks at STOP and at the window before every call, and stops at the first failure: the next
/// hourly run tries again, and a Claude problem must not cost the run its digest.
pub(super) async fn run_dossier_step(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    watch: &WindowWatch,
    out: &mut dyn Write,
) -> Result<usize> {
    let tracker = open_tracker(paths)?;
    let dir = paths.data_dir.join("dossiers");
    let mut due: Vec<String> = Vec::new();
    for state in [State::Interview, State::TechnicalTest, State::Replied] {
        for row in tracker.list_by_state(state)? {
            if !dossier_path(&dir, &row.job_key).exists() {
                due.push(row.job_key);
            }
        }
    }
    if due.is_empty() {
        return Ok(0); // nothing to do: not even claude.toml or claude.exe is looked at
    }
    let config = load_claude_config(&paths.profile_dir.join("claude.toml"))?;
    due.truncate(config.max_dossiers_per_run);
    let cv_md = read_cv_master(paths)?;
    let setup = ClaudeSetup::with(paths, deps, config, tracker)?;
    let tracker = &setup.tracker;
    let store = open_prep_store(paths)?;
    let exclusions = load_exclusions(&paths.profile_dir.join("exclusions.toml"))?;
    let gate = setup.gate();
    // A wrong login is an account error: `run_daily_steps` turns it into exit 1.
    let seen = gate.check_account(now).await?;
    print_account_warnings(&seen, out)?;
    let mut written = 0;
    for key in due {
        if paths.stop_file.exists() || !watch.is_open() {
            writeln!(
                out,
                "Dossiers: se detiene (archivo STOP o fin de la ventana)."
            )?;
            break;
        }
        let Some(job) = tracker.get_job(&key)? else {
            continue;
        };
        match build_dossier(&gate, &store, &job, &cv_md, &exclusions, &dir, now).await {
            Ok(outcome) => {
                writeln!(out, "{}", outcome.describe())?;
                written += 1;
            }
            Err(err) => {
                writeln!(out, "Dossier de {key}: {err:#}")?;
                break;
            }
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{TimeDelta, TimeZone, Utc};

    use super::*;
    use crate::claude::{ClaudeError, ClaudeReply, ClaudeRequest, ClaudeRunner};
    use crate::cli::test_support::{
        busy_now, cli, dossier_file, fake_deps, open_now, plain, seed, seed_prepared, seed_replied,
        temp_root, tracker_of, with_claude, write_claude_toml, write_cv_master,
    };
    use crate::models::Job;
    use crate::testing::{FakeClaude, SAMPLE_CV, make_job, make_job_with, sample_dossier_json};

    #[tokio::test]
    async fn dossier_stops_before_any_prompt_when_the_login_is_not_private() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed_replied(root.path(), &make_job());
        let fake = Arc::new(
            FakeClaude::new(vec![FakeClaude::structured(sample_dossier_json(), 0.05)])
                .refusing_account("la suscripción activa es team"),
        );
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["dossier", "test:1"]).await;
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("es team"), "{out}");
        assert!(fake.requests().is_empty());
    }

    #[tokio::test]
    async fn dossier_shows_the_managed_policy_warning_of_the_account() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed_replied(root.path(), &make_job());
        let note = "hay una política administrada de prueba";
        let fake = Arc::new(
            FakeClaude::new(vec![FakeClaude::structured(sample_dossier_json(), 0.05)])
                .warning_about(note),
        );
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["dossier", "test:1"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains(&format!("Aviso: {note}")), "{out}");
    }

    /// A Claude that takes its time, so a run lasts long enough to cross the edge of the window.
    struct SlowClaude {
        delay: std::time::Duration,
        inner: Arc<FakeClaude>,
    }

    #[async_trait::async_trait]
    impl ClaudeRunner for SlowClaude {
        async fn run(&self, req: &ClaudeRequest) -> Result<ClaudeReply, ClaudeError> {
            tokio::time::sleep(self.delay).await;
            self.inner.run(req).await
        }

        async fn preflight(&self) -> Result<crate::claude::AccountSeen, ClaudeError> {
            self.inner.preflight().await
        }
    }

    fn dossier_replies(n: usize) -> Arc<FakeClaude> {
        Arc::new(FakeClaude::new(
            (0..n)
                .map(|_| FakeClaude::structured(sample_dossier_json(), 0.05))
                .collect(),
        ))
    }

    fn job_n(n: &str) -> Job {
        make_job_with(|j| j.external_id = n.into())
    }

    #[tokio::test]
    async fn the_dossier_command_writes_the_file_and_the_drill_cards() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed(root.path(), &make_job(), &[]);
        let fake = dossier_replies(1);
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["dossier", "test:1"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Dossier listo"), "{out}");
        assert!(dossier_file(root.path(), "test:1").exists());
        let (_, card) = plain(
            root.path(),
            open_now(),
            &["prep", "next", "--topic", "kubernetes-operators"],
        )
        .await;
        assert!(
            card.contains(&format!("gap-{}-1", crate::outbound::key_slug("test:1"))),
            "{card}"
        );
    }

    #[tokio::test]
    async fn the_dossier_command_reports_unknown_keys_and_keeps_going() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed(root.path(), &make_job(), &[]);
        let fake = dossier_replies(1);
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(
            root.path(),
            open_now(),
            deps,
            &["dossier", "nope:1", "test:1"],
        )
        .await;
        assert_eq!(code, 1);
        assert!(
            out.contains("Aviso desconocido: nope:1") && out.contains("Dossier listo"),
            "{out}"
        );
    }

    #[tokio::test]
    async fn the_dossier_command_respects_stop_the_window_and_the_budget() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed(root.path(), &make_job(), &[]);
        let fake = dossier_replies(1);
        let go = || with_claude(fake_deps(vec![]).0, &fake);
        assert_eq!(
            cli(root.path(), busy_now(), go(), &["dossier", "test:1"])
                .await
                .0,
            4
        );
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        assert_eq!(
            cli(root.path(), open_now(), go(), &["dossier", "test:1"])
                .await
                .0,
            3
        );
        std::fs::remove_file(root.path().join("STOP")).unwrap();
        write_claude_toml(root.path(), "daily_budget_usd = 0.01\n");
        tracker_of(root.path())
            .record_claude_call("dossier", open_now(), 0.02, 1, 1, true)
            .unwrap();
        let (code, out) = cli(root.path(), open_now(), go(), &["dossier", "test:1"]).await;
        assert_eq!(code, 1);
        assert!(out.contains("presupuesto diario"), "{out}");
        assert!(
            fake.requests().is_empty(),
            "no call was made in any of the three cases"
        );
    }

    #[tokio::test]
    async fn run_daily_writes_the_dossier_of_a_replied_application() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed_replied(root.path(), &make_job());
        let fake = dossier_replies(1);
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Dossier listo"), "{out}");
        assert!(dossier_file(root.path(), "test:1").exists());
        assert_eq!(fake.requests().len(), 1);
    }

    #[tokio::test]
    async fn run_daily_writes_at_most_the_configured_number_of_dossiers() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        for n in ["1", "2", "3"] {
            seed_replied(root.path(), &job_n(n));
        }
        write_claude_toml(root.path(), "max_dossiers_per_run = 2\n");
        let fake = dossier_replies(3);
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        assert_eq!(fake.requests().len(), 2, "{out}");
        // The next run does the third one: the first two are not asked again.
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        cli(
            root.path(),
            open_now() + TimeDelta::hours(1),
            deps,
            &["run-daily"],
        )
        .await;
        assert_eq!(fake.requests().len(), 3);
    }

    #[tokio::test]
    async fn run_daily_skips_dossiers_when_the_budget_is_spent_and_still_exits_zero() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed_replied(root.path(), &make_job());
        tracker_of(root.path())
            .record_claude_call("dossier", open_now(), 5.0, 1, 1, true)
            .unwrap();
        let fake = dossier_replies(1);
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("presupuesto diario"), "{out}");
        assert!(fake.requests().is_empty());
        assert!(!dossier_file(root.path(), "test:1").exists());
        assert!(
            root.path()
                .join("data")
                .join("digest-2026-10-14.md")
                .exists()
        );
    }

    #[tokio::test]
    async fn run_daily_does_not_fail_when_claude_is_missing() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed_replied(root.path(), &make_job());
        let nowhere = root.path().join("nowhere").join("claude.exe");
        write_claude_toml(
            root.path(),
            &format!("program = {:?}\n", nowhere.display().to_string()),
        );
        let (code, out) = cli(root.path(), open_now(), fake_deps(vec![]).0, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Dossiers omitidos") && out.contains("no se encontró claude"),
            "{out}"
        );
        assert!(
            root.path()
                .join("data")
                .join("digest-2026-10-14.md")
                .exists()
        );
    }

    #[tokio::test]
    async fn run_daily_never_touches_claude_when_nobody_has_replied() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed_prepared(root.path(), &make_job());
        let nowhere = root.path().join("nowhere").join("claude.exe");
        write_claude_toml(
            root.path(),
            &format!("program = {:?}\n", nowhere.display().to_string()),
        );
        let (code, out) = cli(root.path(), open_now(), fake_deps(vec![]).0, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            !out.contains("Dossiers omitidos"),
            "no replied application, no reason to look for claude: {out}"
        );
    }

    #[tokio::test]
    async fn a_broken_claude_toml_is_not_reported_when_nobody_has_replied() {
        let root = temp_root();
        // With a CV master the earlier steps pass and `run-daily` reaches the dossier step.
        write_cv_master(root.path(), SAMPLE_CV);
        write_claude_toml(root.path(), "this is = not [valid toml\n");
        let (_, out) = cli(root.path(), open_now(), fake_deps(vec![]).0, &["run-daily"]).await;
        assert!(
            !out.contains("Dossiers omitidos"),
            "nothing was due, so claude.toml should not even be read: {out}"
        );
    }

    #[tokio::test]
    async fn run_daily_stops_the_dossiers_when_the_window_closes_in_the_middle() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed_replied(root.path(), &job_n("1"));
        seed_replied(root.path(), &job_n("2"));
        // 1.5 s before Santiago's busy block starts (12:00 UTC); each Claude call takes 2 s. The
        // margin is generous on purpose: the steps before the dossiers must finish inside it.
        let start =
            Utc.with_ymd_and_hms(2026, 10, 14, 11, 59, 58).unwrap() + TimeDelta::milliseconds(500);
        let fake = dossier_replies(2);
        let slow = SlowClaude {
            delay: std::time::Duration::from_secs(2),
            inner: fake.clone(),
        };
        let mut deps = fake_deps(vec![]).0;
        let runner: Arc<dyn ClaudeRunner> = Arc::new(slow);
        deps.claude = Some(runner);
        let (code, out) = cli(root.path(), start, deps, &["run-daily"]).await;
        assert_eq!(code, 0, "{out}");
        assert_eq!(
            fake.requests().len(),
            1,
            "the second dossier waits for the next window: {out}"
        );
        assert!(out.contains("se detiene"), "{out}");
    }
}
