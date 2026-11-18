//! What Gonza reads: the daily digest, the best scores, the counts per state, the last results
//! and the list for today. They only read (the digest also writes its file).

use std::io::Write;

use anyhow::Result;
use chrono::{DateTime, Utc};
use chrono_tz::America::Santiago;
use jobbot_app::reporting::status::{
    describe_auto_approve, describe_last_account, describe_last_run,
};

use super::claude::unanchored_note;
use super::run_guard::STOP_MESSAGE;
use super::{EXIT_OK, open_tracker};
use crate::claude::{ClaudeConfig, load_claude_config};
use crate::digest::build_digest;
use crate::handoff::is_handoff_reason;
use crate::paths::Paths;
use crate::results::{format_table, read_last, results_path};
use crate::today::build_today;
use crate::tracker::{ApplicationRow, State};

/// How many of the best scored postings the daily digest lists.
const DIGEST_ITEMS: usize = 5;

/// Read-only, like `top` and `status`: it works under STOP and outside the window.
pub(super) fn cmd_today(paths: &Paths, now: DateTime<Utc>, out: &mut dyn Write) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    write!(out, "{}", build_today(&tracker, now)?)?;
    Ok(EXIT_OK)
}

pub(super) fn cmd_digest(paths: &Paths, now: DateTime<Utc>, out: &mut dyn Write) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    let text = build_digest(&tracker, now, DIGEST_ITEMS)?;
    // Named after Gonza's day, like the title inside it.
    let file = paths.data_dir.join(format!(
        "digest-{}.md",
        now.with_timezone(&Santiago).format("%Y-%m-%d")
    ));
    std::fs::write(&file, &text)?;
    writeln!(out, "{text}")?;
    Ok(EXIT_OK)
}

pub(super) fn cmd_top(paths: &Paths, n: usize, out: &mut dyn Write) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    for r in tracker.top_scored(n)? {
        writeln!(
            out,
            "{:5.1}  {} — {}  [{}] {}",
            r.score,
            r.company,
            r.title,
            r.state.as_str(),
            r.url
        )?;
    }
    // The forms the bot filled and left for Gonza's own send are not in the queue above anymore.
    let handoff: Vec<ApplicationRow> = tracker
        .list_by_state(State::NeedsGonza)?
        .into_iter()
        .filter(|r| r.needs_reason.as_deref().is_some_and(is_handoff_reason))
        .collect();
    if !handoff.is_empty() {
        writeln!(out, "\nPara enviar tú desde tu navegador normal (handoff):")?;
        for r in &handoff {
            writeln!(
                out,
                "{:5.1}  {} — {}  [{}]\n       {}\n       cuando lo envíes: jobbot confirm {}",
                r.score,
                r.company,
                r.title,
                r.state.as_str(),
                r.needs_reason.as_deref().unwrap_or(""),
                r.job_key
            )?;
        }
    }
    Ok(EXIT_OK)
}

/// Read-only: the applications per state, then the three things that say whether the hourly task
/// is doing its job (when `run-daily` last ended and how, which Claude login was last verified,
/// whether STOP is on). The last run is only what ran: a task turned away by the window or STOP
/// leaves it alone, so an evening `status` still shows the last real run.
pub(super) fn cmd_status(paths: &Paths, now: DateTime<Utc>, out: &mut dyn Write) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    let counts = tracker.counts()?;
    if counts.is_empty() {
        writeln!(out, "Estado: sin postulaciones todavía.")?;
    }
    for (state, n) in counts {
        writeln!(out, "{state:12} {n}")?;
    }
    writeln!(out)?;
    if paths.stop_file.exists() {
        writeln!(out, "{STOP_MESSAGE}")?;
    }
    writeln!(out, "{}", describe_last_run(&tracker, now)?)?;
    writeln!(out, "{}", describe_last_account(&tracker, now)?)?;
    writeln!(out, "{}", describe_auto_approve(&tracker)?)?;
    // Without the keys that pin the account, "verificada" only means "a Pro or Max login": say it.
    // A `claude.toml` that exists and cannot be used is its own warning: reading it as "no
    // config" would blame the missing anchor and hide the real mistake.
    let claude_toml = paths.profile_dir.join("claude.toml");
    match load_claude_config(&claude_toml) {
        Ok(config) => {
            if let Some(note) = unanchored_note(&config) {
                writeln!(out, "{note}")?;
            }
        }
        Err(_) if matches!(claude_toml.try_exists(), Ok(false)) => {
            if let Some(note) = unanchored_note(&ClaudeConfig::default()) {
                writeln!(out, "{note}")?;
            }
        }
        Err(err) => writeln!(
            out,
            "Aviso: claude.toml no se pudo leer: {:#}",
            anyhow::Error::new(err)
        )?,
    }
    Ok(EXIT_OK)
}

/// Read-only, like `top` and `status`: it works under STOP and outside the window.
pub(super) fn cmd_results(paths: &Paths, last: u64, out: &mut dyn Write) -> Result<i32> {
    let last = usize::try_from(last).unwrap_or(usize::MAX);
    let results = read_last(&results_path(&paths.data_dir), last)?;
    writeln!(out, "{}", format_table(&results))?;
    Ok(EXIT_OK)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use chrono::TimeDelta;

    use crate::cli::test_support::{
        ashby_job, busy_now, cli, fake_deps, inbox_msg, open_now, plain, seed, seed_prepared,
        strong_job, temp_root, tracker_of, with_claude, write_cv_master, write_inbox_file,
    };
    use crate::testing::{FakeClaude, make_job, make_job_with};
    use crate::tracker::State;

    #[tokio::test]
    async fn digest_on_empty_database_writes_a_file() {
        let root = temp_root();
        let (code, out) = plain(root.path(), open_now(), &["digest"]).await;
        assert_eq!(code, 0);
        assert!(out.contains("Resumen diario"), "{out}");
        assert!(
            root.path()
                .join("data")
                .join("digest-2026-10-14.md")
                .exists()
        );
    }

    #[tokio::test]
    async fn the_digest_file_is_named_after_the_day_in_santiago_not_in_utc() {
        use chrono::{TimeZone, Utc};
        for (now, name) in [
            // Summer, UTC-3: the day changes at 03:00 UTC.
            (
                Utc.with_ymd_and_hms(2026, 1, 15, 2, 59, 59).unwrap(),
                "digest-2026-01-14.md",
            ),
            (
                Utc.with_ymd_and_hms(2026, 1, 15, 3, 0, 0).unwrap(),
                "digest-2026-01-15.md",
            ),
            // Winter, UTC-4: at 04:00 UTC.
            (
                Utc.with_ymd_and_hms(2026, 7, 15, 3, 59, 59).unwrap(),
                "digest-2026-07-14.md",
            ),
            (
                Utc.with_ymd_and_hms(2026, 7, 15, 4, 0, 0).unwrap(),
                "digest-2026-07-15.md",
            ),
            // The night the clocks go back (2025-04-06 03:00 UTC) the day is still the 5th.
            (
                Utc.with_ymd_and_hms(2025, 4, 6, 3, 30, 0).unwrap(),
                "digest-2025-04-05.md",
            ),
            (
                Utc.with_ymd_and_hms(2025, 4, 6, 4, 0, 0).unwrap(),
                "digest-2025-04-06.md",
            ),
        ] {
            let root = temp_root();
            plain(root.path(), now, &["digest"]).await;
            let mut files: Vec<String> = std::fs::read_dir(root.path().join("data"))
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|file| file.starts_with("digest-"))
                .collect();
            files.sort();
            assert_eq!(files, [name], "{now:?}");
        }
    }

    #[tokio::test]
    async fn status_on_empty_database() {
        let root = temp_root();
        let (code, out) = plain(root.path(), open_now(), &["status"]).await;
        assert_eq!(code, 0);
        assert!(out.contains("sin postulaciones"), "{out}");
    }

    #[tokio::test]
    async fn top_lists_scored_jobs() {
        let root = temp_root();
        seed(root.path(), &make_job(), &[]);
        let (code, out) = plain(root.path(), open_now(), &["top", "--n", "3"]).await;
        assert_eq!(code, 0);
        assert!(out.contains("Acme AI — Senior AI Engineer"), "{out}");
    }

    #[tokio::test]
    async fn status_names_a_claude_toml_it_cannot_read_instead_of_calling_it_unanchored() {
        let root = temp_root();
        std::fs::write(
            root.path().join("profile").join("claude.toml"),
            "expected_mail = \"typo@example.com\"\n",
        )
        .unwrap();
        let (code, status) = plain(root.path(), open_now(), &["status"]).await;
        assert_eq!(code, 0, "{status}");
        assert!(
            status.contains("Aviso: claude.toml no se pudo leer:")
                && status.contains("expected_mail"),
            "{status}"
        );
        assert!(!status.contains("sin anclar"), "{status}");
    }

    #[tokio::test]
    async fn today_lists_tasks_decisions_and_answers_and_needs_neither_stop_nor_the_window() {
        let root = temp_root();
        seed(root.path(), &make_job(), &[State::NeedsGonza]);
        let mut reply = inbox_msg("m1", "recruiter");
        reply["summary"] = "Quiere hablar de sueldo".into();
        let mut video = inbox_msg("m2", "action_required");
        video["task"] = serde_json::json!({
            "text": "Grabar el video de Acme",
            "due_at": "2026-10-16T15:00:00Z"
        });
        let file = write_inbox_file(root.path(), &serde_json::json!([reply, video]));
        let (code, out) = plain(
            root.path(),
            open_now(),
            &["inbox", "import", file.to_str().unwrap()],
        )
        .await;
        assert_eq!(code, 0, "{out}");
        std::fs::write(root.path().join("STOP"), "x").unwrap();

        let (code, out) = plain(root.path(), busy_now(), &["today"]).await;

        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Tu lista de hoy — 2026-10-14")
                && out.contains("Tareas abiertas (1)")
                && out.contains("Grabar el video de Acme — vence en")
                && out.contains("Esperan tu decisión (1)")
                && out.contains("Acme AI — Senior AI Engineer · test:1: falta email")
                && out.contains("Respuestas recientes, últimos 7 días (2)")
                && out.contains("reclutador · «Asunto m1» — Quiere hablar de sueldo"),
            "{out}"
        );
        // Reading it changed nothing.
        assert_eq!(tracker_of(root.path()).open_tasks().unwrap().len(), 1);
    }

    // ---- jobbot results ----

    #[tokio::test]
    async fn results_shows_what_submit_just_did() {
        let root = temp_root();
        seed_prepared(root.path(), &make_job());
        let (deps, _) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["submit"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(root.path().join("data").join("results.jsonl").is_file());
        let (code, out) = plain(root.path(), open_now(), &["results"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Últimos 1 resultados (de 1)"), "{out}");
        for expected in [
            "dry_run",
            "prepared",
            "test:1",
            "Acme AI — Senior AI Engineer",
        ] {
            assert!(out.contains(expected), "{expected} in {out}");
        }
    }

    fn log_line(root: &Path, n: u32) {
        let job = make_job_with(|j| j.external_id = n.to_string());
        let line = crate::results::ResultLine::new(
            &job,
            crate::results::AttemptMode::DryRun,
            State::Prepared,
            crate::handoff::FormTally {
                filled: Some(n as usize),
                left: vec![],
            },
            None,
            None,
            open_now(),
        );
        crate::results::append_result(&crate::results::results_path(&root.join("data")), &line)
            .unwrap();
    }

    #[tokio::test]
    async fn results_last_n_prints_only_the_latest_lines() {
        let root = temp_root();
        for n in 1..=3 {
            log_line(root.path(), n);
        }
        let (code, out) = plain(root.path(), open_now(), &["results", "--last", "2"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Últimos 2 resultados (de 3)"), "{out}");
        assert!(out.contains("test:2") && out.contains("test:3"), "{out}");
        assert!(!out.contains("test:1"), "{out}");
        // The default is the last 20: all three fit.
        let (_, out) = plain(root.path(), open_now(), &["results"]).await;
        assert!(out.contains("Últimos 3 resultados (de 3)"), "{out}");
    }

    #[tokio::test]
    async fn results_without_a_log_says_so_and_never_needs_the_window_or_stop() {
        let root = temp_root();
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        let (code, out) = plain(root.path(), busy_now(), &["results"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Sin resultados todavía"), "{out}");
        // A count of zero is not a request: it is a usage error.
        assert_eq!(
            plain(root.path(), open_now(), &["results", "--last", "0"])
                .await
                .0,
            1
        );
    }

    #[tokio::test]
    async fn top_lists_the_forms_waiting_for_gonzas_own_send() {
        let root = temp_root();
        let job = ashby_job();
        seed(root.path(), &job, &[]);
        let tracker = tracker_of(root.path());
        tracker
            .transition(
                &job.key(),
                State::NeedsGonza,
                Some("handoff: envía tú. Hoja de respuestas: data/handoff/test_1.md"),
                open_now(),
            )
            .unwrap();
        drop(tracker);
        let (code, out) = plain(root.path(), open_now(), &["top"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Para enviar tú"), "{out}");
        assert!(out.contains("Acme AI — Senior AI Engineer"), "{out}");
        assert!(out.contains("data/handoff/test_1.md"), "{out}");
        assert!(out.contains("jobbot confirm test:1"), "{out}");
        // A needs_gonza for any other reason is not listed there.
        let other = make_job_with(|j| j.external_id = "2".into());
        seed(root.path(), &other, &[State::NeedsGonza]);
        let (_, out) = plain(root.path(), open_now(), &["top"]).await;
        assert!(!out.contains("jobbot confirm test:2"), "{out}");
    }

    #[tokio::test]
    async fn status_says_nothing_has_run_yet_and_the_login_is_unchecked() {
        let root = temp_root();
        let (code, out) = plain(root.path(), open_now(), &["status"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Última corrida: ninguna todavía"), "{out}");
        assert!(out.contains("Cuenta de Claude: sin verificar"), "{out}");
    }

    #[tokio::test]
    async fn status_shows_the_last_run_its_steps_and_the_verified_account() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        cli(
            root.path(),
            open_now(),
            fake_deps(vec![strong_job()]).0,
            &["run-daily"],
        )
        .await;
        let fake = Arc::new(FakeClaude::new(vec![FakeClaude::text("OK", 0.0)]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        cli(root.path(), open_now(), deps, &["claude-check"]).await;

        // Two hours later (and a little: the run ended a moment after it began), in Santiago's evening.
        let later = open_now() + TimeDelta::hours(2) + TimeDelta::seconds(30);
        let (code, out) = plain(root.path(), later, &["status"]).await;

        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Última corrida: 2026-10-14 19:00 (hora de Santiago, hace 2 h): ok"),
            "{out}"
        );
        assert!(
            out.contains("descubrir: 1 nuevas")
                && out.contains("preparar: 1 preparadas")
                && out.contains("enviar: 0 enviadas, 0 para ti")
                && out.contains("dossiers: 0 escritos"),
            "{out}"
        );
        assert!(
            out.contains(
                "Cuenta de Claude: verificada 2026-10-14 19:00 (hace 2 h) · método claude.ai"
            ) && out.contains("suscripción max · correo t***@example.com"),
            "{out}"
        );
        // The counts per state are still there.
        assert!(out.contains("prepared"), "{out}");
    }

    #[tokio::test]
    async fn status_says_why_the_last_login_check_failed_and_that_stop_is_on() {
        let root = temp_root();
        let refused = Arc::new(FakeClaude::new(vec![]).refusing_account("cuenta equivocada"));
        cli(
            root.path(),
            open_now(),
            with_claude(fake_deps(vec![]).0, &refused),
            &["claude-check"],
        )
        .await;
        std::fs::write(root.path().join("STOP"), "x").unwrap();

        let (_, out) = plain(root.path(), open_now(), &["status"]).await;

        assert!(
            out.contains("Cuenta de Claude: la última verificación falló")
                && out.contains("cuenta equivocada"),
            "{out}"
        );
        assert!(out.contains("STOP activo"), "{out}");
    }

    #[tokio::test]
    async fn status_shows_an_error_run_and_a_skipped_step_in_plain_spanish() {
        let root = temp_root();
        write_cv_master(root.path(), "⟦ pendiente ⟧");
        seed(root.path(), &make_job(), &[]);
        plain(root.path(), open_now(), &["run-daily"]).await;

        let (_, out) = plain(root.path(), open_now(), &["status"]).await;

        assert!(
            out.contains("hace un momento): error: prepare terminó con código 2"),
            "{out}"
        );
        assert!(out.contains("enviar: omitido (tras un error)"), "{out}");
    }
}
