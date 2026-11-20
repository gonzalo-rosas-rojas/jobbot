//! The `submit` command: which browsers a run needs, the filler and inspector it starts, the
//! open-check of each posting, and what it tells Gonza about the outcomes.

use std::io::Write;
use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Utc};

use super::run_guard::{WINDOW_CLOSED_MESSAGE, WindowWatch};
use super::{Deps, EXIT_OK, open_tracker};
use crate::answers::load_answers;
use crate::form::{BrowserFiller, FillMode, FormFiller, SubmitMode, SubmitResult};
use crate::handoff::{BrowserInspector, FormInspector, is_handoff_job, is_handoff_reason};
use crate::models::Job;
use crate::open_check::{NetworkOpenChecker, OpenChecker};
use crate::paths::Paths;
use crate::pipeline::submit_all_while;
use crate::rules::{load_criteria, load_exclusions};
use crate::tracker::{ApplicationRow, State, Tracker};

/// Whether `submit_all` would call the filler at least once, so Chromium is started only when
/// there is a form to fill. The gate may still refuse every row (cooldowns, caps); that is fine.
fn has_work(prepared: &[ApplicationRow], mode: SubmitMode, in_review_phase: bool) -> bool {
    match mode {
        SubmitMode::Assist => !prepared.is_empty(),
        SubmitMode::DryRun => prepared.iter().any(|r| r.screenshot_path.is_none()),
        SubmitMode::Live => prepared
            .iter()
            .any(|r| r.approved || !in_review_phase || r.screenshot_path.is_none()),
    }
}

/// Which browsers a `submit` run has work for: the main one (persistent profile, fills and sends
/// the ordinary forms) and the inspector (headless, opens the handoff forms Gonza sends himself).
struct SubmitNeeds {
    filler: bool,
    inspector: bool,
}

/// Sorts the prepared applications into handoff forms (`handoff::is_handoff_job`) and ordinary
/// ones, so each browser is started only when there is a form for it.
fn submit_needs(
    tracker: &Tracker,
    prepared: &[ApplicationRow],
    mode: SubmitMode,
    in_review_phase: bool,
) -> Result<SubmitNeeds> {
    let mut ordinary = Vec::new();
    let mut inspector = false;
    for row in prepared {
        let handoff = tracker
            .get_job(&row.job_key)?
            .is_some_and(|job| is_handoff_job(&job));
        if handoff {
            inspector = true;
        } else {
            ordinary.push(row.clone());
        }
    }
    Ok(SubmitNeeds {
        filler: has_work(&ordinary, mode, in_review_phase),
        inspector,
    })
}

/// The filler of a run that has handoff forms only: they go through the inspector, so this is
/// never asked to open anything.
struct NoBrowser;

#[async_trait::async_trait]
impl FormFiller for NoBrowser {
    async fn submit(
        &self,
        job: &Job,
        _cv_pdf: &Path,
        _bank: &crate::answers::AnswerBank,
        _mode: FillMode,
        _shots_dir: &Path,
    ) -> Result<SubmitResult> {
        anyhow::bail!(
            "el navegador de envío no se inició (solo había formularios de handoff) y se pidió abrir {}",
            job.key()
        )
    }
}

pub(super) async fn cmd_submit(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    headed: bool,
    mode: SubmitMode,
    watch: &WindowWatch,
    out: &mut dyn Write,
) -> Result<i32> {
    let criteria = load_criteria(&paths.profile_dir.join("criteria.toml"))?;
    let exclusions = load_exclusions(&paths.profile_dir.join("exclusions.toml"))?;
    let bank = load_answers(&paths.profile_dir.join("answers.toml"))?;
    let tracker = open_tracker(paths)?;
    let prepared = tracker.list_by_state(State::Prepared)?;
    let in_review_phase =
        tracker.approved_submitted_total()? < i64::from(criteria.first_n_reviewed);
    let needs = submit_needs(&tracker, &prepared, mode, in_review_phase)?;
    if !needs.filler && !needs.inspector {
        // Nothing the filler would touch: do not start a browser just to find that out.
        writeln!(out, "Nada que enviar.")?;
        return Ok(EXIT_OK);
    }
    let launched;
    let filler: &dyn FormFiller = match &deps.filler {
        Some(injected) => injected.as_ref(),
        None if needs.filler => {
            launched =
                BrowserFiller::launch(&paths.data_dir.join("browser-profile"), headed).await?;
            &launched
        }
        None => &NoBrowser,
    };
    let launched_inspector;
    let inspector: Option<&dyn FormInspector> = match (&deps.inspector, &deps.filler) {
        (Some(injected), _) => Some(injected.as_ref()),
        // An injected filler stands for the whole browser side of a test: no real Chromium.
        (None, Some(_)) => None,
        (None, None) if needs.inspector => {
            launched_inspector = BrowserInspector::launch().await?;
            Some(&launched_inspector)
        }
        (None, None) => None,
    };
    let network_checker;
    let open_checker: Option<&dyn OpenChecker> = match (&deps.open_checker, &deps.filler) {
        (Some(injected), _) => Some(injected.as_ref()),
        // An injected filler stands for the whole outside world of a test: no network either.
        (None, Some(_)) => None,
        (None, None) => {
            // A host discovery left resting (a 429) is not asked whether its postings are open.
            let resting = tracker
                .resting_hosts(now)?
                .into_iter()
                .map(|(host, _)| host);
            network_checker = NetworkOpenChecker::new(
                reqwest::Client::builder()
                    .user_agent("jobbot/0.1")
                    .build()?,
            )
            .with_rests(resting);
            Some(&network_checker)
        }
    };
    match mode {
        SubmitMode::DryRun => writeln!(
            out,
            "Modo prueba en seco: se llena el formulario y se guarda una captura, no se envía. Si faltan respuestas, se llena el resto y la captura marca en rojo lo que falta. Usa --live para enviar o --assist para completar y enviar tú."
        )?,
        SubmitMode::Assist => writeln!(
            out,
            "Modo asistido: se llena cada formulario en el navegador; lo que no se sabe responder queda marcado en rojo para que lo completes tú, y el clic en enviar también lo das tú (hasta 10 minutos por formulario; cierra la pestaña para saltarte uno). Lo que envíes queda aprobado."
        )?,
        SubmitMode::Live => {}
    }
    let outcomes = submit_all_while(
        &tracker,
        &criteria,
        &exclusions,
        &bank,
        filler,
        inspector,
        open_checker,
        mode,
        &paths.data_dir,
        &paths.stop_file,
        now,
        &|| watch.is_open(),
    )
    .await?;
    for o in &outcomes {
        writeln!(
            out,
            "{:12} {} {}",
            o.status,
            o.job_key,
            o.reason.as_deref().unwrap_or("")
        )?;
    }
    if outcomes
        .iter()
        .any(|o| o.reason.as_deref().is_some_and(is_handoff_reason))
    {
        writeln!(
            out,
            "Handoff: esos formularios los envías tú desde tu navegador normal, copiando de la hoja de respuestas; después `jobbot confirm KEY` los marca como enviados."
        )?;
    }
    if outcomes.iter().any(|o| {
        o.status == "needs_gonza"
            && o.reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with("faltan respuestas"))
    }) {
        writeln!(
            out,
            "Para responder tú lo que falta: `jobbot resolve KEY` devuelve la postulación a la cola y `jobbot submit --assist` la llena en un navegador visible y te deja lo demás."
        )?;
    }
    if !watch.is_open() {
        writeln!(
            out,
            "{WINDOW_CLOSED_MESSAGE}: el envío se detuvo y lo que falta sigue en la cola."
        )?;
    } else if outcomes.is_empty() {
        writeln!(out, "Nada que enviar.")?;
    }
    Ok(EXIT_OK)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use super::*;
    use crate::answers::AnswerBank;
    use crate::cli::test_support::{
        acme, ashby_job, cli, fake_deps, open_now, plain, seed_prepared, temp_root, tracker_of,
    };
    use crate::form::{FillMode, FormFiller, SubmitMode, SubmitResult, SubmitStatus};
    use crate::models::Job;
    use crate::open_check::OpenStatus;
    use crate::testing::{make_job, make_job_with};
    use crate::tracker::{State, Tracker};

    #[tokio::test]
    async fn submit_is_dry_run_unless_live() {
        let root = temp_root();
        let job = make_job();
        seed_prepared(root.path(), &job);
        // Approved, so the review gate would let a live submission through.
        tracker_of(root.path()).approve(&job.key()).unwrap();

        let (deps, calls) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["submit"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("prueba en seco"), "{out}");
        assert_eq!(*calls.lock().unwrap(), vec![SubmitMode::DryRun]);

        let (deps, calls) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["submit", "--live"]).await;
        assert_eq!(code, 0, "{out}");
        assert_eq!(*calls.lock().unwrap(), vec![SubmitMode::Live]);
        assert_eq!(
            tracker_of(root.path())
                .get_application("test:1")
                .unwrap()
                .unwrap()
                .state,
            State::Submitted
        );
    }

    /// Says the same thing about every posting.
    struct FixedChecker(OpenStatus);

    #[async_trait::async_trait]
    impl OpenChecker for FixedChecker {
        async fn check(&self, _job: &Job) -> OpenStatus {
            self.0
        }
    }

    #[tokio::test]
    async fn submit_withdraws_a_posting_its_source_no_longer_lists() {
        let root = temp_root();
        let job = make_job();
        seed_prepared(root.path(), &job);

        let (mut deps, calls) = fake_deps(vec![]);
        deps.open_checker = Some(Box::new(FixedChecker(OpenStatus::Closed)));
        let (code, out) = cli(root.path(), open_now(), deps, &["submit"]).await;

        assert_eq!(code, 0, "{out}");
        assert!(out.contains("posting_closed"), "{out}");
        assert!(calls.lock().unwrap().is_empty(), "no form was opened");
        assert_eq!(
            tracker_of(root.path())
                .get_application("test:1")
                .unwrap()
                .unwrap()
                .state,
            State::Withdrawn
        );
    }

    #[tokio::test]
    async fn submit_goes_on_when_the_source_cannot_say() {
        let root = temp_root();
        let job = make_job();
        seed_prepared(root.path(), &job);

        let (mut deps, calls) = fake_deps(vec![]);
        deps.open_checker = Some(Box::new(FixedChecker(OpenStatus::Unknown)));
        let (code, out) = cli(root.path(), open_now(), deps, &["submit"]).await;

        assert_eq!(code, 0, "{out}");
        assert_eq!(*calls.lock().unwrap(), vec![SubmitMode::DryRun]);
    }

    #[tokio::test]
    async fn submit_assist_uses_assist_mode_and_approves() {
        let root = temp_root();
        let job = make_job();
        seed_prepared(root.path(), &job); // not approved: in Live mode this would be a dry run
        let (deps, calls) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["submit", "--assist"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Modo asistido"), "{out}");
        assert_eq!(*calls.lock().unwrap(), vec![SubmitMode::Assist]);
        let row = tracker_of(root.path())
            .get_application("test:1")
            .unwrap()
            .unwrap();
        assert_eq!(row.state, State::Submitted);
        assert!(row.approved);
        // --live and --assist exclude each other.
        assert_eq!(
            plain(root.path(), open_now(), &["submit", "--live", "--assist"])
                .await
                .0,
            1
        );
    }

    /// A filler whose dry run always stops for a question the bank cannot answer.
    struct LacksAnswersFiller;

    #[async_trait::async_trait]
    impl FormFiller for LacksAnswersFiller {
        async fn submit(
            &self,
            _job: &Job,
            _cv: &Path,
            _bank: &AnswerBank,
            _mode: FillMode,
            _shots: &Path,
        ) -> anyhow::Result<SubmitResult> {
            Ok(SubmitResult {
                status: SubmitStatus::NeedsGonza,
                reason: Some("faltan respuestas: Why are you interested in this role? *".into()),
                screenshot: None,
            })
        }
    }

    #[tokio::test]
    async fn submit_points_to_assist_when_a_form_lacks_answers() {
        let root = temp_root();
        seed_prepared(root.path(), &make_job());
        let (mut deps, _calls) = fake_deps(vec![]);
        deps.filler = Some(Box::new(LacksAnswersFiller));
        let (code, out) = cli(root.path(), open_now(), deps, &["submit"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("faltan respuestas: Why are you interested in this role? *"),
            "{out}"
        );
        // The way to answer them yourself: back to the queue, then the assisted mode.
        assert!(
            out.contains("jobbot resolve KEY") && out.contains("jobbot submit --assist"),
            "{out}"
        );
    }

    #[tokio::test]
    async fn submit_does_not_hint_assist_without_missing_answers() {
        let root = temp_root();
        seed_prepared(root.path(), &make_job());
        let (deps, _calls) = fake_deps(vec![]);
        let (_, out) = cli(root.path(), open_now(), deps, &["submit"]).await;
        assert!(!out.contains("jobbot submit --assist"), "{out}");
    }

    // ---- handoff (Ashby): the bot fills, Gonza sends ----

    use crate::handoff::{FieldReport, FormInspection, FormInspector, SheetField};

    #[tokio::test]
    async fn submit_hands_an_ashby_form_off_in_every_mode() {
        for flags in [
            &["submit"][..],
            &["submit", "--live"][..],
            &["submit", "--assist"][..],
        ] {
            let root = temp_root();
            let job = ashby_job();
            seed_prepared(root.path(), &job);
            // Approved, so even Live would click on any other form.
            tracker_of(root.path()).approve(&job.key()).unwrap();
            let (deps, calls) = fake_deps(vec![]);
            let (code, out) = cli(root.path(), open_now(), deps, flags).await;
            assert_eq!(code, 0, "{flags:?}: {out}");
            assert_eq!(
                *calls.lock().unwrap(),
                vec![SubmitMode::DryRun],
                "{flags:?}"
            );
            let row = tracker_of(root.path())
                .get_application("test:1")
                .unwrap()
                .unwrap();
            assert_eq!(row.state, State::NeedsGonza, "{flags:?}");
            let sheet = root.path().join("data").join("handoff").join("test_1.md");
            assert!(sheet.is_file(), "{flags:?}");
            assert!(out.contains("handoff:"), "{out}");
            assert!(out.contains("jobbot confirm KEY"), "{out}");
        }
    }

    #[tokio::test]
    async fn a_run_with_nothing_but_a_handed_off_form_is_not_nothing_to_send() {
        let root = temp_root();
        seed_prepared(root.path(), &ashby_job());
        let (deps, _) = fake_deps(vec![]);
        let (_, out) = cli(root.path(), open_now(), deps, &["submit"]).await;
        assert!(!out.contains("Nada que enviar"), "{out}");
    }

    /// An inspector that sees one required question the bank cannot answer.
    struct OneQuestionInspector;

    #[async_trait::async_trait]
    impl FormInspector for OneQuestionInspector {
        async fn inspect(
            &self,
            _job: &Job,
            _cv_pdf: &Path,
            _bank: &AnswerBank,
            _shots_dir: &Path,
        ) -> anyhow::Result<FormInspection> {
            Ok(FormInspection {
                result: SubmitResult {
                    status: SubmitStatus::NeedsGonza,
                    reason: Some("faltan respuestas: Why this role?".into()),
                    screenshot: None,
                },
                report: FieldReport::Full(vec![SheetField::missing("Why this role?")]),
            })
        }
    }

    #[tokio::test]
    async fn an_injected_inspector_does_the_dry_run_of_a_handoff_form() {
        let root = temp_root();
        seed_prepared(root.path(), &ashby_job());
        let (mut deps, calls) = fake_deps(vec![]);
        deps.inspector = Some(Box::new(OneQuestionInspector));
        let (code, out) = cli(root.path(), open_now(), deps, &["submit", "--live"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            calls.lock().unwrap().is_empty(),
            "the filler stayed out: {out}"
        );
        let sheet =
            std::fs::read_to_string(root.path().join("data").join("handoff").join("test_1.md"))
                .unwrap();
        assert!(
            sheet.contains("### 1. Why this role? (obligatoria)"),
            "{sheet}"
        );
    }

    #[test]
    fn a_run_of_only_handoff_forms_needs_the_inspector_and_not_the_main_browser() {
        let tracker = Tracker::in_memory().unwrap();
        let ashby = ashby_job();
        tracker
            .add_job(&ashby, 80.0, &BTreeMap::new(), open_now())
            .unwrap();
        tracker
            .start_cold_for_tests(&ashby.key(), open_now())
            .unwrap();
        tracker
            .transition(&ashby.key(), State::Prepared, None, open_now())
            .unwrap();
        let rows = tracker.list_by_state(State::Prepared).unwrap();
        let needs = submit_needs(&tracker, &rows, SubmitMode::Live, true).unwrap();
        assert!(!needs.filler && needs.inspector);
        // An ordinary form brings the main browser back.
        let other = make_job_with(|j| {
            j.external_id = "2".into();
            j.company = "Other Co".into();
        });
        tracker
            .add_job(&other, 70.0, &BTreeMap::new(), open_now())
            .unwrap();
        tracker
            .start_cold_for_tests(&other.key(), open_now())
            .unwrap();
        tracker
            .transition(&other.key(), State::Prepared, None, open_now())
            .unwrap();
        let rows = tracker.list_by_state(State::Prepared).unwrap();
        let needs = submit_needs(&tracker, &rows, SubmitMode::Live, true).unwrap();
        assert!(needs.filler && needs.inspector);
    }

    #[tokio::test]
    async fn submit_reports_the_application_the_company_cap_left_alone() {
        let root = temp_root();
        for n in 1..=3 {
            seed_prepared(root.path(), &acme(n));
        }
        let (deps, calls) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["submit"]).await;
        assert_eq!(code, 0, "{out}");
        assert_eq!(
            *calls.lock().unwrap(),
            vec![SubmitMode::DryRun, SubmitMode::DryRun],
            "{out}"
        );
        assert!(out.contains("tope por empresa"), "{out}");
        assert!(out.contains("test:3"), "{out}");
        assert_eq!(
            tracker_of(root.path())
                .get_application("test:3")
                .unwrap()
                .unwrap()
                .state,
            State::Prepared
        );
    }
}
