//! The first half of the pipeline: discovering postings, preparing their CVs, and the two
//! commands that set up the rest (the auto-approval threshold and the browser install).

use std::io::Write;
use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use jobbot_app::reporting::status::describe_auto_approve;

use super::{Deps, EXIT_CV_NOT_READY, EXIT_ERROR, EXIT_OK, open_tracker};
use crate::actions;
use crate::cv::CvError;
use crate::discovery::{DiscoveryReport, discover, rescore};
use crate::http::Http;
use crate::paths::Paths;
use crate::pipeline::{REPLACED_CV_DIR, Relocation, prepare_report_relocating, refresh_cvs};
use crate::rules::{load_criteria, load_exclusions};
use crate::sources::{Source, build_sources, load_sources_config};

/// Discovery's time for the job boards: past it no new request leaves, and the run goes on with
/// what came back. A run reads every board in a few minutes; this only stops a host that answers
/// slowly request after request from holding the hourly task.
const BOARDS_DEADLINE: std::time::Duration = std::time::Duration::from_secs(20 * 60);

fn format_report(r: &DiscoveryReport) -> String {
    let mut text = format!(
        "Descubrimiento: {} avisos · {} en cola · {} bajo el piso (excepción) · {} bajo el umbral · \
         {} excluidos · {} fuera de región · {} vencidos · {} ya vistos · {} actualizados · \
         podados: {} vencidos, {} fuera de región",
        r.fetched,
        r.queued,
        r.below_floor_flagged,
        r.below_threshold,
        r.excluded,
        r.region_skipped,
        r.stale,
        r.already_seen,
        r.refreshed,
        r.pruned_stale,
        r.pruned_region
    );
    if r.rate_limited > 0 {
        text.push_str(&format!(
            "\n  {} consultas en pausa: el sitio pidió esperar y se retoman en otra corrida",
            r.rate_limited
        ));
    }
    for error in &r.errors {
        text.push_str(&format!("\n  ERROR {error}"));
    }
    text
}

pub(super) async fn cmd_discover(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    rescore_first: bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let criteria = load_criteria(&paths.profile_dir.join("criteria.toml"))?;
    let exclusions = load_exclusions(&paths.profile_dir.join("exclusions.toml"))?;
    // Shared with the job boards' GETs, which keep their rests and pages in it for the next run.
    let tracker = Arc::new(open_tracker(paths)?);
    if rescore_first {
        // After a change to criteria.toml: stored jobs without an application get today's score.
        let r = rescore(&criteria, &exclusions, &*tracker, now)?;
        writeln!(
            out,
            "Repuntuación: {} avisos revisados · {} nuevos en cola · {} bajo el piso (excepción)",
            r.rescored, r.newly_queued, r.newly_flagged
        )?;
    }
    // Injected sources (tests) or the real ones built from profile/sources.toml.
    let built;
    let sources: &[Box<dyn Source>] = match &deps.sources {
        Some(injected) => injected,
        None => {
            let mut config = load_sources_config(&paths.profile_dir.join("sources.toml"))?;
            config.relevant_title_words = criteria.extra_open_title_words.clone();
            let client = reqwest::Client::builder()
                .user_agent("jobbot/0.1")
                .build()?;
            // One client, one board of resting hosts and one deadline for the whole discovery.
            let deadline = std::time::Instant::now() + BOARDS_DEADLINE;
            let http = Http::remembering(client, deadline, tracker.clone());
            built = build_sources(&config, &http);
            &built
        }
    };
    let report = discover(sources, &criteria, &exclusions, &*tracker, now).await?;
    writeln!(out, "{}", format_report(&report))?;
    // run-daily reads this to skip discovery while the last one is recent. A run where every
    // source failed fetched nothing and is not "done": the next hourly run must try again.
    if report.fetched > 0 || report.errors.is_empty() {
        tracker.set_meta("last_discover_at", &now.to_rfc3339())?;
    }
    Ok(EXIT_OK)
}

/// What `prepare` and `prepare --refresh` say, and the exit code they end with, when the master CV
/// still has marks to fill.
fn cv_not_ready(out: &mut dyn Write) -> Result<i32> {
    writeln!(
        out,
        "CV no listo: cv/cv-master.md todavía tiene marcas ⟦ ⟧ sin completar."
    )?;
    Ok(EXIT_CV_NOT_READY)
}

/// What a CV command got: `None` when the master CV still has marks to fill (the command says so
/// with `cv_not_ready`), the value when it worked, an error for anything else.
fn cv_ready<T>(result: Result<T, CvError>) -> Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(CvError::NotReady) => Ok(None),
        Err(other) => Err(other.into()),
    }
}

pub(super) async fn cmd_prepare(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let criteria = load_criteria(&paths.profile_dir.join("criteria.toml"))?;
    let tracker = open_tracker(paths)?;
    let cv_dir = paths.data_dir.join("cv");
    let result = prepare_report_relocating(
        &tracker,
        &paths.cv_master,
        &cv_dir,
        deps.renderer.as_ref(),
        Relocation::from_criteria(&criteria),
        now,
    )
    .await;
    let Some(report) = cv_ready(result)? else {
        return cv_not_ready(out);
    };
    writeln!(out, "{} postulaciones preparadas", report.prepared.len())?;
    for skip in &report.skipped {
        writeln!(out, "{}", skip.line())?;
    }
    Ok(EXIT_OK)
}

/// `prepare --refresh`: the CVs of the prepared and waiting applications, rendered again from
/// today's master (see `refresh_cvs`).
pub(super) async fn cmd_prepare_refresh(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let criteria = load_criteria(&paths.profile_dir.join("criteria.toml"))?;
    let tracker = open_tracker(paths)?;
    let result = refresh_cvs(
        &tracker,
        &paths.cv_master,
        deps.renderer.as_ref(),
        Relocation::from_criteria(&criteria),
        now,
    )
    .await;
    let Some(keys) = cv_ready(result)? else {
        return cv_not_ready(out);
    };
    writeln!(
        out,
        "{} CV generados de nuevo desde el CV maestro (los anteriores quedan en data/cv/{REPLACED_CV_DIR})",
        keys.len()
    )?;
    Ok(EXIT_OK)
}

/// The auto-approval threshold (see `actions::set_auto_approve_min`): shown with no argument, set
/// with a whole number, turned off with `off`. Like the panel's settings it only writes the tracker.
pub(super) fn cmd_auto_approve(
    paths: &Paths,
    value: Option<&str>,
    out: &mut dyn Write,
) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    let outcome = match value.map(str::trim) {
        None => {
            writeln!(out, "{}", describe_auto_approve(&tracker)?)?;
            return Ok(EXIT_OK);
        }
        Some(text) if text.eq_ignore_ascii_case("off") => {
            actions::set_auto_approve_min(&tracker, None, None)?
        }
        Some(text) => {
            let Ok(number) = text.parse::<i64>() else {
                writeln!(
                    out,
                    "El umbral debe ser un número entero o `off`, no «{}».",
                    crate::outbound::clean(text, 40)
                )?;
                return Ok(EXIT_ERROR);
            };
            let threshold = load_criteria(&paths.profile_dir.join("criteria.toml"))
                .ok()
                .map(|criteria| criteria.apply_threshold);
            actions::set_auto_approve_min(&tracker, Some(number), threshold)?
        }
    };
    writeln!(out, "{}", outcome.message)?;
    Ok(if outcome.ok { EXIT_OK } else { EXIT_ERROR })
}

pub(super) async fn cmd_install_browsers(out: &mut dyn Write) -> Result<i32> {
    writeln!(out, "Instalando Chromium (una sola vez, unos 250 MB)...")?;
    playwright_rs::install_browsers(Some(&["chromium"])).await?;
    writeln!(out, "Chromium instalado.")?;
    Ok(EXIT_OK)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::TimeDelta;
    use jobbot_app::reporting::status::DISCOVER_COOLDOWN_HOURS;

    use super::*;
    use crate::cli::test_support::{
        FakeSource, acme, busy_now, cli, fake_deps, open_now, plain, seed, strong_job, temp_root,
        tracker_of, write_cv_master,
    };
    use crate::models::{FetchError, FetchResult};
    use crate::testing::{make_job, make_job_with};
    use crate::tracker::State;

    #[tokio::test]
    async fn discover_prints_the_report_with_fake_sources() {
        let root = temp_root();
        let (deps, _) = fake_deps(vec![make_job()]);
        let (code, out) = cli(root.path(), open_now(), deps, &["discover"]).await;
        assert_eq!(code, 0);
        assert!(out.contains("Descubrimiento: 1 avisos"), "{out}");
        assert!(
            out.contains("podados: 0 vencidos, 0 fuera de región"),
            "{out}"
        );
    }

    #[test]
    fn format_report_keeps_its_counters_and_adds_the_refresh_and_the_prune() {
        let report = DiscoveryReport {
            fetched: 10,
            queued: 2,
            below_floor_flagged: 1,
            below_threshold: 3,
            excluded: 4,
            region_skipped: 5,
            stale: 6,
            already_seen: 7,
            refreshed: 8,
            pruned_stale: 9,
            pruned_region: 11,
            rate_limited: 0,
            errors: vec![FetchError::failed("ashby", "acme", "boom")],
        };
        assert_eq!(
            format_report(&report),
            "Descubrimiento: 10 avisos · 2 en cola · 1 bajo el piso (excepción) · 3 bajo el umbral · \
             4 excluidos · 5 fuera de región · 6 vencidos · 7 ya vistos · 8 actualizados · \
             podados: 9 vencidos, 11 fuera de región\n  ERROR ashby:acme: boom"
        );
    }

    #[test]
    fn format_report_says_how_many_queries_a_host_asked_to_pause() {
        let limited = FetchError::rate_limited(
            "himalayas",
            "rust",
            120,
            "himalayas.app:443 asked us to slow down; not asking again for 120 s",
        );
        let report = DiscoveryReport {
            fetched: 1,
            rate_limited: 2,
            errors: vec![limited.clone(), limited],
            ..DiscoveryReport::default()
        };
        let text = format_report(&report);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[1],
            "  2 consultas en pausa: el sitio pidió esperar y se retoman en otra corrida"
        );
        assert_eq!(lines.len(), 4, "{text}");
    }

    #[tokio::test]
    async fn discover_withdraws_a_queued_posting_that_has_gone_stale_and_says_so() {
        let root = temp_root();
        let stale = make_job_with(|j| {
            j.posted_at = Some(open_now() - TimeDelta::days(135));
        });
        seed(root.path(), &stale, &[]);
        let (deps, _) = fake_deps(vec![]);

        let (code, out) = cli(root.path(), open_now(), deps, &["discover"]).await;

        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("podados: 1 vencidos, 0 fuera de región"),
            "{out}"
        );
        let row = tracker_of(root.path())
            .get_application("test:1")
            .unwrap()
            .unwrap();
        assert_eq!(row.state, State::Withdrawn);
        assert!(
            row.note.as_deref().unwrap().starts_with("stale_posting:"),
            "{row:?}"
        );
    }

    #[tokio::test]
    async fn discover_rescore_prunes_too_because_its_discover_does() {
        let root = temp_root();
        let stale = make_job_with(|j| {
            j.posted_at = Some(open_now() - TimeDelta::days(135));
        });
        seed(root.path(), &stale, &[]);
        let (deps, _) = fake_deps(vec![]);

        let (_, out) = cli(root.path(), open_now(), deps, &["discover", "--rescore"]).await;

        assert!(out.contains("Repuntuación: 0 avisos revisados"), "{out}");
        assert!(
            out.contains("podados: 1 vencidos, 0 fuera de región"),
            "{out}"
        );
    }

    #[tokio::test]
    async fn prepare_with_placeholders_exits_2_and_leaves_state_alone() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\n⟦ completar resumen ⟧\n");
        seed(root.path(), &make_job(), &[]);
        let (code, out) = plain(root.path(), open_now(), &["prepare"]).await;
        assert_eq!(code, 2);
        assert!(out.contains("CV no listo"), "{out}");
        assert_eq!(
            tracker_of(root.path())
                .get_application("test:1")
                .unwrap()
                .unwrap()
                .state,
            State::Scored
        );
    }

    #[tokio::test]
    async fn prepare_renders_with_the_injected_renderer() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        seed(root.path(), &make_job(), &[]);
        let (code, out) = plain(root.path(), open_now(), &["prepare"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("1 postulaciones preparadas"), "{out}");
        assert_eq!(
            tracker_of(root.path())
                .get_application("test:1")
                .unwrap()
                .unwrap()
                .state,
            State::Prepared
        );
    }

    /// A source that is down: no postings, one error.
    struct DownSource;

    #[async_trait::async_trait]
    impl Source for DownSource {
        fn name(&self) -> &str {
            "down"
        }
        async fn fetch(&self) -> FetchResult {
            FetchResult {
                jobs: vec![],
                errors: vec![FetchError::failed("down", "api", "HTTP 503")],
            }
        }
    }

    #[tokio::test]
    async fn a_discover_where_every_source_failed_is_not_counted_as_done() {
        let root = temp_root();
        let (mut deps, _) = fake_deps(vec![]);
        deps.sources = Some(vec![Box::new(DownSource)]);

        let (code, out) = cli(root.path(), open_now(), deps, &["discover"]).await;

        assert_eq!(code, 0, "{out}");
        let stamp = tracker_of(root.path())
            .get_meta("last_discover_at")
            .unwrap();
        assert_eq!(
            stamp, None,
            "run-daily must try again, not wait {DISCOVER_COOLDOWN_HOURS} h"
        );
    }

    #[tokio::test]
    async fn the_discovery_stamp_is_the_clock_with_a_plus_zero_offset_and_the_digits_of_its_fraction()
     {
        for (nanos, expected) in [
            (0, "2026-10-14T22:00:00+00:00"),
            (500_000_000, "2026-10-14T22:00:00.500+00:00"),
            (123_456_000, "2026-10-14T22:00:00.123456+00:00"),
            (123_456_789, "2026-10-14T22:00:00.123456789+00:00"),
        ] {
            let root = temp_root();
            let (deps, _) = fake_deps(vec![make_job()]);
            let now = open_now() + TimeDelta::nanoseconds(nanos);
            cli(root.path(), now, deps, &["discover"]).await;
            assert_eq!(
                tracker_of(root.path())
                    .get_meta("last_discover_at")
                    .unwrap()
                    .as_deref(),
                Some(expected)
            );
        }
    }

    #[tokio::test]
    async fn a_discover_with_one_good_source_is_done_even_if_another_failed() {
        let root = temp_root();
        // A run that fetched something is done even when one source had an error.
        let (mut deps, _) = fake_deps(vec![make_job()]);
        deps.sources = Some(vec![
            Box::new(DownSource),
            Box::new(FakeSource(vec![make_job()])),
        ]);
        cli(root.path(), open_now(), deps, &["discover"]).await;
        let stamp = tracker_of(root.path())
            .get_meta("last_discover_at")
            .unwrap();
        assert!(stamp.is_some());
    }

    #[tokio::test]
    async fn discover_rescore_requeues_stored_jobs() {
        let root = temp_root();
        // A job stored with a low score and no application, as an earlier criteria file left it.
        // `make_job()`'s one-line description scores under the shipped threshold, so this one
        // carries a description that reaches it.
        let strong = strong_job();
        tracker_of(root.path())
            .add_job(&strong, 10.0, &BTreeMap::new(), open_now())
            .unwrap();
        let (deps, _) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["discover", "--rescore"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Repuntuación: 1 avisos revisados · 1 nuevos en cola"),
            "{out}"
        );
        assert_eq!(
            tracker_of(root.path())
                .get_application("test:1")
                .unwrap()
                .unwrap()
                .state,
            State::Scored
        );
        // Plain discover never touches stored scores.
        let (deps, _) = fake_deps(vec![]);
        let (_, out) = cli(root.path(), open_now(), deps, &["discover"]).await;
        assert!(!out.contains("Repuntuación"), "{out}");
    }

    #[tokio::test]
    async fn auto_approve_shows_sets_and_turns_off_the_threshold_and_status_says_it() {
        let root = temp_root();
        let run = |parts: Vec<&'static str>| {
            let path = root.path().to_path_buf();
            async move { plain(&path, open_now(), &parts).await }
        };
        let (code, shown) = run(vec!["auto-approve"]).await;
        assert_eq!(code, 0, "{shown}");
        assert!(shown.contains("apagada"), "{shown}");

        let (code, set) = run(vec!["auto-approve", "90"]).await;
        assert_eq!(code, 0, "{set}");
        assert!(set.contains("desde 90 puntos"), "{set}");
        assert!(
            run(vec!["auto-approve"])
                .await
                .1
                .contains("desde 90 puntos")
        );
        let (_, status) = run(vec!["status"]).await;
        assert!(
            status.contains("Auto-aprobación: desde 90 puntos"),
            "{status}"
        );

        // Out of range or not a number: exit 1, the reason, and the 90 stays.
        for (bad, reason) in [
            ("59", "debe ser un entero de 60"),
            ("101", "debe ser un entero de 60"),
            ("-4", "debe ser un entero de 60"),
            ("nueve", "número entero o `off`"),
            ("85.5", "número entero o `off`"),
            ("", "número entero o `off`"),
        ] {
            let (code, out) = plain(root.path(), open_now(), &["auto-approve", bad]).await;
            assert_eq!(code, 1, "{bad:?}: {out}");
            assert!(out.contains(reason), "{bad:?}: {out}");
        }
        assert!(
            run(vec!["auto-approve"])
                .await
                .1
                .contains("desde 90 puntos")
        );

        // It only writes the tracker: STOP and the execution window do not matter.
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        let (code, out) = cli(
            root.path(),
            busy_now(),
            fake_deps(vec![]).0,
            &["auto-approve", "off"],
        )
        .await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("apagada"), "{out}");
        let (_, status) = run(vec!["status"]).await;
        assert!(status.contains("Auto-aprobación: apagada"), "{status}");
    }

    #[tokio::test]
    async fn prepare_says_which_applications_the_company_cap_left_alone() {
        let root = temp_root();
        write_cv_master(root.path(), "# Gonza\n\nFull-stack engineer.\n");
        for n in 1..=3 {
            seed(root.path(), &acme(n), &[]);
        }
        let (code, out) = plain(root.path(), open_now(), &["prepare"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("2 postulaciones preparadas"), "{out}");
        assert!(out.contains("tope por empresa"), "{out}");
        assert!(out.contains("test:3"), "{out}");
        let tracker = tracker_of(root.path());
        let state = |key: &str| tracker.get_application(key).unwrap().unwrap().state;
        assert_eq!(state("test:1"), State::Prepared);
        assert_eq!(state("test:3"), State::Scored);
    }
}
