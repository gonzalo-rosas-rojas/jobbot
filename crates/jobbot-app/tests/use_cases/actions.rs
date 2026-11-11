//! The `actions` use cases against the real SQLite stores, the fake Claude and the shipped
//! profile (moved from the root crate's `src/actions.rs`).

use std::collections::BTreeMap;

use chrono::{DateTime, TimeDelta, TimeZone, Utc};

use jobbot_app::actions::*;
use jobbot_domain::prep::Question;
use jobbot_domain::state::State;
use jobbot_testkit::PrepStore;
use jobbot_testkit::Tracker;
use jobbot_testkit::{NOW, make_job};

/// A tracker holding one application of `make_job()` walked through `states`.
fn tracker_with(states: &[State]) -> (Tracker, String) {
    let t = Tracker::in_memory().unwrap();
    let job = make_job();
    t.add_job(&job, 80.0, &BTreeMap::new(), *NOW).unwrap();
    t.create_application(&job.key(), *NOW).unwrap();
    for state in states {
        t.transition(&job.key(), *state, Some("nota"), *NOW)
            .unwrap();
    }
    (t, job.key())
}
fn bank() -> (PrepStore, Vec<Question>) {
    let store = PrepStore::in_memory().unwrap();
    let questions = store.all_questions().unwrap();
    (store, questions)
}
fn tracker_with_task(text: &str) -> (Tracker, i64) {
    let t = Tracker::in_memory().unwrap();
    t.add_task(
        &jobbot_app::ports::store::NewTask {
            job_key: None,
            kind: "action_required".into(),
            text: text.into(),
            due_at: None,
            source_message_id: None,
            link: None,
        },
        *NOW,
    )
    .unwrap();
    let id = t.open_tasks().unwrap()[0].id;
    (t, id)
}
fn spec(text: &str) -> TaskSpec {
    TaskSpec {
        text: text.into(),
        ..TaskSpec::default()
    }
}
#[test]
fn task_add_puts_a_manual_task_on_the_list_and_says_its_number() {
    let t = Tracker::in_memory().unwrap();
    let outcome = task_add(&t, &spec("Llamar a Toku"), *NOW).unwrap();
    assert_eq!(
        outcome,
        Outcome {
            ok: true,
            message: "Tarea agregada: #1 Llamar a Toku".to_string()
        }
    );
    let row = &t.open_tasks().unwrap()[0];
    assert_eq!(row.kind, "manual");
    assert_eq!(row.source_message_id, None);
    assert_eq!(
        (row.job_key.clone(), row.due_at, row.link.clone()),
        (None, None, None)
    );
    assert_eq!(row.created_at, *NOW);
}
#[test]
fn task_add_cleans_the_text_and_cuts_it_at_300() {
    let t = Tracker::in_memory().unwrap();
    let hostile = format!("Hacer\u{1b}[2J\u{202e} algo\n{}", "x".repeat(400));
    let outcome = task_add(&t, &spec(&hostile), *NOW).unwrap();
    assert!(outcome.ok, "{outcome:?}");
    let text = t.open_tasks().unwrap()[0].text.clone();
    assert!(
        text.starts_with("Hacer [2J algo xxx") && text.ends_with('…'),
        "{text}"
    );
    assert_eq!(text.chars().count(), 301);
    assert!(!text.chars().any(|c| c.is_control()));
    assert!(!outcome.message.chars().any(|c| c.is_control()));

    let empty = task_add(&t, &spec(" \u{200b}\n "), *NOW).unwrap();
    assert!(!empty.ok);
    assert!(empty.message.contains("vacía"), "{empty:?}");
    assert_eq!(t.open_tasks().unwrap().len(), 1);
}
#[test]
fn task_add_checks_the_deadline_with_the_bounds_of_the_importer() {
    let t = Tracker::in_memory().unwrap();
    let with_due = |due: DateTime<Utc>| TaskSpec {
        due_at: Some(due),
        ..spec("Con fecha")
    };
    let beyond = latest_due(*NOW) + chrono::TimeDelta::seconds(1);
    let before = earliest_date() - chrono::TimeDelta::seconds(1);
    for bad in [
        beyond,
        before,
        Utc.with_ymd_and_hms(9999, 12, 31, 23, 59, 59).unwrap(),
    ] {
        let outcome = task_add(&t, &with_due(bad), *NOW).unwrap();
        assert!(!outcome.ok, "{bad}");
        assert!(outcome.message.contains("fuera de rango"), "{outcome:?}");
    }
    assert!(t.open_tasks().unwrap().is_empty());
    // The limits themselves are inside.
    assert!(task_add(&t, &with_due(latest_due(*NOW)), *NOW).unwrap().ok);
    let other = TaskSpec {
        due_at: Some(earliest_date()),
        ..spec("Otra con fecha")
    };
    assert!(task_add(&t, &other, *NOW).unwrap().ok);
    assert_eq!(
        t.open_tasks().unwrap()[0].due_at,
        Some(earliest_date()),
        "soonest due first"
    );
}
#[test]
fn task_add_needs_the_posting_to_exist() {
    let (t, key) = tracker_with(&[]);
    let named = |key: &str| TaskSpec {
        job_key: Some(key.into()),
        ..spec("Preparar la entrevista")
    };

    let unknown = task_add(&t, &named("nope:1"), *NOW).unwrap();
    assert!(!unknown.ok);
    assert!(unknown.message.contains("nope:1"), "{unknown:?}");
    assert!(t.open_tasks().unwrap().is_empty());

    assert!(task_add(&t, &named(&key), *NOW).unwrap().ok);
    assert_eq!(
        t.open_tasks().unwrap()[0].job_key.as_deref(),
        Some(key.as_str())
    );
}
#[test]
fn task_add_keeps_only_a_web_link_that_is_short_and_clean() {
    let t = Tracker::in_memory().unwrap();
    let with_link = |link: &str| TaskSpec {
        link: Some(link.into()),
        ..spec("Con enlace")
    };
    let too_long = format!("https://example.com/{}", "a".repeat(290));
    for bad in [
        "javascript:alert(1)",
        "ftp://example.com/x",
        "file:///C:/secreto.txt",
        "example.com/sin-esquema",
        "https://",
        "https://ejemplo.com/con espacio",
        too_long.as_str(),
    ] {
        let outcome = task_add(&t, &with_link(bad), *NOW).unwrap();
        assert!(!outcome.ok, "{bad}");
        assert!(outcome.message.contains("enlace"), "{outcome:?}");
    }
    assert!(t.open_tasks().unwrap().is_empty());

    let ok = task_add(
        &t,
        &with_link("  HTTPS://Example.com/oferta?x=1\u{1b}  "),
        *NOW,
    )
    .unwrap();
    assert!(ok.ok, "{ok:?}");
    assert_eq!(
        t.open_tasks().unwrap()[0].link.as_deref(),
        Some("HTTPS://Example.com/oferta?x=1")
    );
    let plain = TaskSpec {
        link: Some("   ".into()),
        ..spec("Sin enlace de verdad")
    };
    assert!(task_add(&t, &plain, *NOW).unwrap().ok);
}
#[test]
fn task_add_twice_while_the_first_is_open_is_refused_and_changes_nothing() {
    let t = Tracker::in_memory().unwrap();
    task_add(&t, &spec("Llamar a Toku"), *NOW).unwrap();

    let again = task_add(&t, &spec("  Llamar   a Toku "), *NOW).unwrap();

    assert!(!again.ok);
    assert_eq!(
        again.message,
        "Ya tienes esa tarea abierta (#1): Llamar a Toku"
    );
    assert_eq!(t.open_tasks().unwrap().len(), 1);
    // Closed, it can be added again.
    task_done(&t, 1).unwrap();
    assert!(task_add(&t, &spec("Llamar a Toku"), *NOW).unwrap().ok);
}
#[test]
fn task_done_closes_an_open_task_and_says_which() {
    let (t, id) = tracker_with_task("Entregar prueba técnica de Acme");
    let outcome = task_done(&t, id).unwrap();
    assert_eq!(
        outcome,
        Outcome {
            ok: true,
            message: format!("Hecha: #{id} Entregar prueba técnica de Acme")
        }
    );
    assert!(t.open_tasks().unwrap().is_empty());
}
#[test]
fn task_done_prints_the_text_without_control_characters() {
    // The importer cleans it, but the message goes to a terminal: it is cleaned again.
    let (t, id) = tracker_with_task("Hacer\u{1b}[2J algo\u{202e}\nya");
    let outcome = task_done(&t, id).unwrap();
    assert_eq!(outcome.message, format!("Hecha: #{id} Hacer [2J algo ya"));
}
#[test]
fn task_done_refuses_a_task_that_is_already_done_or_unknown() {
    let (t, id) = tracker_with_task("Responder a Toku");
    task_done(&t, id).unwrap();

    let again = task_done(&t, id).unwrap();
    assert!(!again.ok);
    assert_eq!(again.message, format!("La tarea #{id} ya estaba hecha"));

    let unknown = task_done(&t, id + 50).unwrap();
    assert!(!unknown.ok);
    assert_eq!(unknown.message, format!("No existe la tarea #{}", id + 50));
}
#[test]
fn approve_marks_the_application_and_can_be_repeated() {
    let (t, key) = tracker_with(&[State::Prepared]);
    t.set_screenshot(&key, std::path::Path::new("shots/a.png"))
        .unwrap();
    let first = approve(&t, &key).unwrap();
    assert!(first.ok, "{first:?}");
    assert_eq!(first.message, format!("Aprobada: {key}"));
    assert!(t.get_application(&key).unwrap().unwrap().approved);
    assert!(approve(&t, &key).unwrap().ok, "approving twice is harmless");
}
#[test]
fn approve_refuses_a_prepared_application_that_has_no_capture_to_review() {
    let (t, key) = tracker_with(&[State::Prepared]);
    let refused = approve(&t, &key).unwrap();
    assert!(!refused.ok, "{refused:?}");
    assert!(
        refused.message.starts_with("No hay captura que revisar"),
        "{}",
        refused.message
    );
    assert!(!t.get_application(&key).unwrap().unwrap().approved);
    // Once the dry run has saved its capture there is something to approve.
    t.set_screenshot(&key, std::path::Path::new("shots/a.png"))
        .unwrap();
    assert!(approve(&t, &key).unwrap().ok);
}
#[test]
fn approve_of_an_unknown_key_is_a_refusal_not_an_error() {
    let t = Tracker::in_memory().unwrap();
    assert_eq!(
        approve(&t, "nope:1").unwrap(),
        Outcome {
            ok: false,
            message: "No existe: nope:1".to_string()
        }
    );
}
#[test]
fn resolve_returns_a_prepared_application_to_the_queue_and_forgets_the_attempt() {
    let (t, key) = tracker_with(&[State::Prepared, State::NeedsGonza]);
    t.set_cv_path(&key, std::path::Path::new("cv.pdf")).unwrap();
    t.record_attempt(&key, *NOW).unwrap();
    let outcome = resolve(&t, &key, *NOW).unwrap();
    assert!(outcome.ok, "{outcome:?}");
    assert_eq!(outcome.message, format!("De vuelta a la cola: {key}"));
    let app = t.get_application(&key).unwrap().unwrap();
    assert_eq!(app.state, State::Prepared);
    assert!(
        !t.company_in_cooldown(&app.company_norm, 90, *NOW).unwrap(),
        "the unconfirmed click no longer holds the company"
    );
}
#[test]
fn resolve_without_a_cv_goes_back_to_scored_and_a_replay_is_refused_in_spanish() {
    let (t, key) = tracker_with(&[State::NeedsGonza]);
    let first = resolve(&t, &key, *NOW).unwrap();
    assert!(
        first.message.starts_with("De vuelta a puntuadas"),
        "{first:?}"
    );
    assert_eq!(
        t.get_application(&key).unwrap().unwrap().state,
        State::Scored
    );
    let again = resolve(&t, &key, *NOW).unwrap();
    assert!(!again.ok);
    assert!(
        again
            .message
            .starts_with(&format!("No se pudo resolver {key}"))
            && again
                .message
                .contains("no se puede pasar de scored a scored"),
        "{again:?}"
    );
}
#[test]
fn confirm_marks_submitted_and_approved_and_refuses_a_replay() {
    let (t, key) = tracker_with(&[State::Prepared, State::NeedsGonza]);
    let first = confirm(&t, &key, *NOW).unwrap();
    assert!(first.ok, "{first:?}");
    assert_eq!(first.message, format!("Confirmada como enviada: {key}"));
    let app = t.get_application(&key).unwrap().unwrap();
    assert_eq!(app.state, State::Submitted);
    assert!(app.approved);
    let again = confirm(&t, &key, *NOW).unwrap();
    assert!(!again.ok);
    assert!(
        again
            .message
            .starts_with(&format!("No se pudo confirmar {key}")),
        "{again:?}"
    );
}
#[test]
fn confirming_a_form_the_bot_never_filled_does_not_count_as_a_review() {
    // The review phase exists to look at forms the BOT filled. A form Gonza sent by hand (an
    // Ashby handoff, a listing-only or manual-only posting) says nothing about the filler, so
    // confirming five of them must not end the phase.
    for reason in [
        "handoff: envía tú este formulario. Hoja de respuestas: data/handoff/x.md",
        "listing_only: postular a mano desde el enlace del aviso",
        "manual_only_platform: postular a mano desde el enlace del aviso",
    ] {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 80.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        t.transition(&job.key(), State::Prepared, None, *NOW)
            .unwrap();
        t.transition(&job.key(), State::NeedsGonza, Some(reason), *NOW)
            .unwrap();

        let outcome = confirm(&t, &job.key(), *NOW).unwrap();

        assert!(outcome.ok, "{reason}: {outcome:?}");
        let app = t.get_application(&job.key()).unwrap().unwrap();
        assert_eq!(app.state, State::Submitted, "{reason}");
        assert!(!app.approved, "{reason}: sent by hand is not a bot review");
        assert_eq!(t.approved_submitted_total().unwrap(), 0, "{reason}");
    }
}
#[test]
fn confirming_a_bot_click_gonza_checked_still_counts_as_reviewed() {
    let t = Tracker::in_memory().unwrap();
    let job = make_job();
    t.add_job(&job, 80.0, &BTreeMap::new(), *NOW).unwrap();
    t.create_application(&job.key(), *NOW).unwrap();
    t.transition(&job.key(), State::Prepared, None, *NOW)
        .unwrap();
    t.transition(
        &job.key(),
        State::NeedsGonza,
        Some("unconfirmed_submit: Phone number is invalid."),
        *NOW,
    )
    .unwrap();

    confirm(&t, &job.key(), *NOW).unwrap();

    assert!(t.get_application(&job.key()).unwrap().unwrap().approved);
    assert_eq!(t.approved_submitted_total().unwrap(), 1);
}
#[test]
fn mark_records_a_legal_move_with_a_clean_note() {
    let (t, key) = tracker_with(&[State::Prepared, State::Submitted]);
    let outcome = mark(&t, &key, "interview", Some("  martes 19:00\u{7}  "), *NOW).unwrap();
    assert!(outcome.ok, "{outcome:?}");
    assert_eq!(outcome.message, format!("{key} → interview"));
    let app = t.get_application(&key).unwrap().unwrap();
    assert_eq!(app.state, State::Interview);
    assert_eq!(app.note.as_deref(), Some("martes 19:00"));
}
#[test]
fn an_empty_note_is_stored_as_no_note() {
    let (t, key) = tracker_with(&[State::Prepared, State::Submitted]);
    mark(&t, &key, "replied", Some("   "), *NOW).unwrap();
    assert_eq!(t.get_application(&key).unwrap().unwrap().note, None);
}
#[test]
fn mark_explains_an_illegal_move_in_spanish() {
    let (t, key) = tracker_with(&[]);
    let outcome = mark(&t, &key, "interview", None, *NOW).unwrap();
    assert!(!outcome.ok);
    assert!(
        outcome
            .message
            .contains("no se puede pasar de scored a interview"),
        "{outcome:?}"
    );
    assert!(!outcome.message.contains("illegal transition"));
}
#[test]
fn mark_from_needs_gonza_points_to_confirm_and_resolve() {
    let (t, key) = tracker_with(&[State::Prepared, State::NeedsGonza]);
    let outcome = mark(&t, &key, "replied", None, *NOW).unwrap();
    assert!(!outcome.ok);
    assert!(
        outcome
            .message
            .contains("no se puede pasar de needs_gonza a replied")
            && outcome.message.contains("confirm")
            && outcome.message.contains("resolve"),
        "{outcome:?}"
    );
}
#[test]
fn mark_refuses_a_state_the_engine_owns_and_lists_the_valid_ones() {
    let (t, key) = tracker_with(&[]);
    let outcome = mark(&t, &key, "submitted", None, *NOW).unwrap();
    assert!(!outcome.ok);
    assert!(
        outcome.message.starts_with("Estado no válido: submitted.")
            && outcome.message.contains("replied"),
        "{outcome:?}"
    );
}
#[test]
fn mark_of_an_unknown_key_says_so_in_spanish() {
    let t = Tracker::in_memory().unwrap();
    let outcome = mark(&t, "x:1", "replied", None, *NOW).unwrap();
    assert!(!outcome.ok);
    assert!(
        outcome.message.contains("no existe la postulación x:1"),
        "{outcome:?}"
    );
}
#[test]
fn a_mark_is_undone_within_the_window_and_the_application_goes_back() {
    let (t, key) = tracker_with(&[State::Prepared, State::Submitted]);
    mark(&t, &key, "interview", None, *NOW).unwrap();
    let outcome = unmark(&t, &key, *NOW + UNDO_WINDOW - TimeDelta::seconds(1)).unwrap();
    assert_eq!(
        outcome,
        Outcome {
            ok: true,
            message: format!("Deshecho: {key} vuelve a submitted")
        }
    );
    let app = t.get_application(&key).unwrap().unwrap();
    assert_eq!(app.state, State::Submitted);
    assert_eq!(app.note.as_deref(), Some("nota"));
}
#[test]
fn unmark_explains_each_refusal_in_spanish() {
    let (t, key) = tracker_with(&[State::Prepared, State::Submitted]);
    assert_eq!(
        unmark(&t, &key, *NOW).unwrap(),
        Outcome {
            ok: false,
            message: "No hay una marca reciente que deshacer".to_string()
        }
    );

    mark(&t, &key, "replied", None, *NOW).unwrap();
    assert_eq!(
        unmark(&t, &key, *NOW + UNDO_WINDOW).unwrap(),
        Outcome {
            ok: false,
            message: "Ya pasaron más de 10 minutos: ya no se puede deshacer".to_string()
        }
    );

    t.transition(&key, State::Interview, None, *NOW).unwrap();
    assert_eq!(
        unmark(&t, &key, *NOW).unwrap(),
        Outcome {
            ok: false,
            message: "Hubo otro cambio después: ya no se puede deshacer".to_string()
        }
    );

    assert_eq!(
        unmark(&t, "x:1", *NOW).unwrap(),
        Outcome {
            ok: false,
            message: "No existe la postulación x:1".to_string()
        }
    );
}
fn record_spec(company: &str, title: &str, url: &str) -> RecordSpec {
    RecordSpec {
        company: company.to_string(),
        title: title.to_string(),
        url: url.to_string(),
        ..RecordSpec::default()
    }
}
#[test]
fn record_refuses_an_address_that_is_not_a_plain_http_or_https_one() {
    let t = Tracker::in_memory().unwrap();
    let too_long = format!("https://example.com/{}", "a".repeat(RECORD_LINK_MAX_CHARS));
    for bad in [
        "javascript:alert(1)",
        "file:///C:/Windows/win.ini",
        "data:text/html,x",
        "https://",
        "jobs/42",
        "https://ejemplo.com/con espacio",
        too_long.as_str(),
    ] {
        let outcome = record(&t, &record_spec("Acme", "Cargo", bad), *NOW).unwrap();
        assert!(!outcome.ok, "{bad:?} was accepted");
        assert!(
            outcome.message.starts_with("El enlace no sirve"),
            "{bad:?}: {}",
            outcome.message
        );
    }
    assert!(
        t.counts().unwrap().is_empty(),
        "a refusal stores nothing, not even the posting"
    );
    // A long tracking query is fine up to the limit.
    let long_but_fine = format!("https://example.com/job?{}", "q=1&".repeat(100));
    assert!(
        record(&t, &record_spec("Acme", "Cargo", &long_but_fine), *NOW)
            .unwrap()
            .ok
    );
}
#[test]
fn record_stores_the_text_it_is_given_as_one_clean_line_and_cut() {
    let t = Tracker::in_memory().unwrap();
    let spec = RecordSpec {
        location: format!("Remoto\n{}", "x".repeat(400)),
        ..record_spec(
            "  Acme\u{7}\n Labs ",
            &format!("Backend\r\nEngineer {}", "y".repeat(400)),
            "https://jobs.example.com/acme/7",
        )
    };

    let outcome = record(&t, &spec, *NOW).unwrap();

    assert!(outcome.ok, "{outcome:?}");
    let key = outcome
        .message
        .rsplit('(')
        .next()
        .unwrap()
        .trim_end_matches(')');
    let job = t.get_job(key).unwrap().unwrap();
    assert_eq!(job.company, "Acme Labs");
    assert!(job.title.starts_with("Backend Engineer y") && job.title.chars().count() <= 301);
    assert!(job.location.starts_with("Remoto x") && job.location.chars().count() <= 201);
    assert!(
        ![&job.company, &job.title, &job.location]
            .iter()
            .any(|text| text.contains(['\n', '\r', '\u{7}']))
    );
}
#[test]
fn record_of_a_new_posting_creates_it_and_stamps_the_given_time() {
    let t = Tracker::in_memory().unwrap();
    let spec = RecordSpec {
        location: "Santiago, Chile".to_string(),
        cv_path: Some(std::path::PathBuf::from("cv.pdf")),
        note: Some("  enviada con Playwright\u{7} ".to_string()),
        salary_min_monthly_usd: Some(5000.0),
        ..record_spec(
            "Toku",
            "Senior Software Engineer",
            "https://jobs.ashbyhq.com/toku/abc/application?utm_source=x",
        )
    };

    let outcome = record(&t, &spec, *NOW).unwrap();

    assert!(outcome.ok, "{outcome:?}");
    let key = "manual:jobs.ashbyhq.com_toku_abc";
    assert_eq!(
        outcome.message,
        format!("Registrada como enviada: Toku — Senior Software Engineer ({key})")
    );
    let app = t.get_application(key).unwrap().unwrap();
    assert_eq!(app.state, State::Submitted);
    assert_eq!(app.submitted_at, Some(*NOW));
    assert!(
        !app.approved,
        "a hand-sent form is not a review of the filler"
    );
    assert_eq!(app.cv_path.as_deref(), Some("cv.pdf"));
    assert_eq!(app.note.as_deref(), Some("enviada con Playwright"));
    let job = t.get_job(key).unwrap().unwrap();
    assert_eq!(job.source, "manual");
    assert_eq!(job.company, "Toku");
    assert_eq!(job.location, "Santiago, Chile");
    assert_eq!(job.salary_min_monthly_usd, Some(5000.0));
    let trail: Vec<State> = t
        .events_for(key)
        .unwrap()
        .iter()
        .map(|e| e.to_state)
        .collect();
    assert_eq!(
        trail,
        vec![State::Scored, State::NeedsGonza, State::Submitted]
    );
    assert_eq!(t.approved_submitted_total().unwrap(), 0);
    assert!(
        t.company_in_cooldown("toku", 90, *NOW).unwrap(),
        "the per-company wait must know about it"
    );
}
#[test]
fn record_reuses_a_stored_posting_found_by_its_link() {
    let (t, key) = tracker_with(&[]);
    let job = t.get_job(&key).unwrap().unwrap();
    let spec = record_spec(
        "Otro nombre",
        "Otro cargo",
        &format!("{}?ref=linkedin#form", job.apply_url),
    );

    let outcome = record(&t, &spec, *NOW).unwrap();

    assert!(outcome.ok, "{outcome:?}");
    assert!(outcome.message.contains(&key), "{outcome:?}");
    assert_eq!(
        t.get_application(&key).unwrap().unwrap().state,
        State::Submitted
    );
    assert_eq!(t.counts().unwrap().get("submitted"), Some(&1));
    assert_eq!(
        t.counts().unwrap().len(),
        1,
        "no second posting was created"
    );
}
#[test]
fn record_reuses_a_stored_posting_with_the_same_company_and_title() {
    let (t, key) = tracker_with(&[]);
    let spec = record_spec(
        "ACME AI, Inc.",
        " senior ai engineer ",
        "https://other.example/x",
    );

    let outcome = record(&t, &spec, *NOW).unwrap();

    assert!(outcome.ok && outcome.message.contains(&key), "{outcome:?}");
    assert_eq!(t.counts().unwrap().len(), 1);
}
#[test]
fn record_with_a_key_uses_that_posting_and_an_unknown_key_is_refused() {
    let (t, key) = tracker_with(&[State::Prepared]);
    let spec = RecordSpec {
        key: Some(key.clone()),
        ..RecordSpec::default()
    };
    let outcome = record(&t, &spec, *NOW).unwrap();
    assert!(outcome.ok, "{outcome:?}");
    let trail: Vec<State> = t
        .events_for(&key)
        .unwrap()
        .iter()
        .map(|e| e.to_state)
        .collect();
    assert_eq!(
        trail,
        vec![State::Scored, State::Prepared, State::Submitted],
        "a prepared form goes straight to submitted"
    );

    let unknown = RecordSpec {
        key: Some("nope:1".to_string()),
        ..RecordSpec::default()
    };
    assert_eq!(
        record(&t, &unknown, *NOW).unwrap(),
        Outcome {
            ok: false,
            message: "No existe: nope:1".to_string()
        }
    );
}
#[test]
fn record_from_needs_gonza_marks_it_submitted_without_approving_it() {
    let (t, key) = tracker_with(&[State::Prepared, State::NeedsGonza]);
    let spec = RecordSpec {
        key: Some(key.clone()),
        ..RecordSpec::default()
    };

    assert!(record(&t, &spec, *NOW).unwrap().ok);

    let app = t.get_application(&key).unwrap().unwrap();
    assert_eq!(app.state, State::Submitted);
    assert!(!app.approved);
    assert_eq!(app.needs_reason, None);
}
#[test]
fn recording_the_same_application_twice_is_refused_and_changes_nothing() {
    let (t, key) = tracker_with(&[]);
    let spec = RecordSpec {
        key: Some(key.clone()),
        ..RecordSpec::default()
    };
    record(&t, &spec, *NOW).unwrap();
    let later = *NOW + chrono::TimeDelta::hours(3);

    let again = record(&t, &spec, later).unwrap();

    assert!(!again.ok);
    assert!(
        again.message.contains("ya está registrada") && again.message.contains("submitted"),
        "{again:?}"
    );
    let app = t.get_application(&key).unwrap().unwrap();
    assert_eq!(app.submitted_at, Some(*NOW), "the first time stands");
    assert_eq!(t.events_for(&key).unwrap().len(), 3);
}
#[test]
fn record_refuses_a_withdrawn_application() {
    let (t, key) = tracker_with(&[State::Withdrawn]);
    let spec = RecordSpec {
        key: Some(key.clone()),
        ..RecordSpec::default()
    };

    let outcome = record(&t, &spec, *NOW).unwrap();

    assert!(!outcome.ok);
    assert!(outcome.message.contains("withdrawn"), "{outcome:?}");
    assert_eq!(
        t.get_application(&key).unwrap().unwrap().state,
        State::Withdrawn
    );
}
#[test]
fn record_without_a_key_needs_company_title_and_link() {
    let t = Tracker::in_memory().unwrap();
    for spec in [
        record_spec("", "Cargo", "https://x.example/1"),
        record_spec("Empresa", "  ", "https://x.example/1"),
        record_spec("Empresa", "Cargo", ""),
    ] {
        let outcome = record(&t, &spec, *NOW).unwrap();
        assert!(!outcome.ok, "{spec:?}");
        assert!(
            outcome.message.contains("--company")
                && outcome.message.contains("--title")
                && outcome.message.contains("--url"),
            "{outcome:?}"
        );
    }
    assert!(t.counts().unwrap().is_empty(), "nothing was stored");
}
#[test]
fn grade_card_records_the_review() {
    let (store, questions) = bank();
    let id = questions[0].id.clone();
    let outcome = grade_card(&store, &questions, &id, 4, *NOW).unwrap();
    assert!(outcome.ok, "{outcome:?}");
    assert_eq!(outcome.message, format!("{id}: nota 4 registrada."));
    assert_eq!(store.stats(&questions, *NOW).unwrap().seen, 1);
}
#[test]
fn grade_card_refuses_an_unknown_card_and_a_grade_above_five() {
    let (store, questions) = bank();
    let id = questions[0].id.clone();
    assert_eq!(
        grade_card(&store, &questions, "nope", 3, *NOW).unwrap(),
        Outcome {
            ok: false,
            message: "Pregunta desconocida: nope".to_string()
        }
    );
    let high = grade_card(&store, &questions, &id, 6, *NOW).unwrap();
    assert!(!high.ok && high.message.contains("0 a 5"), "{high:?}");
    assert_eq!(
        store.stats(&questions, *NOW).unwrap().seen,
        0,
        "a refused grade changes nothing"
    );
}
#[test]
fn snooze_puts_an_application_off_and_unsnooze_brings_it_back() {
    let (t, key) = tracker_with(&[State::Prepared, State::NeedsGonza]);
    let done = snooze(&t, &key, "3d", *NOW).unwrap();
    assert_eq!(
        done,
        Outcome {
            ok: true,
            message: format!("Pospuesta hasta 2026-10-04 09:00: {key}")
        }
    );
    let row = t.get_application(&key).unwrap().unwrap();
    assert_eq!(row.snoozed_until, Some(*NOW + TimeDelta::days(3)));
    assert_eq!(row.state, State::NeedsGonza, "the state does not move");

    let back = unsnooze(&t, &key).unwrap();
    assert!(back.ok && back.message.starts_with("De vuelta"), "{back:?}");
    assert_eq!(
        t.get_application(&key).unwrap().unwrap().snoozed_until,
        None
    );
    let twice = unsnooze(&t, &key).unwrap();
    assert!(!twice.ok && twice.message.starts_with("No estaba pospuesta"));
    assert!(!unsnooze(&t, "nope:1").unwrap().ok);
}
#[test]
fn snooze_refuses_the_past_a_year_away_nonsense_and_an_unknown_key() {
    let (t, key) = tracker_with(&[]);
    let refused = |until: &str| {
        let outcome = snooze(&t, &key, until, *NOW).unwrap();
        assert!(!outcome.ok, "{until:?} was accepted");
        outcome.message
    };
    assert!(refused("2026-10-01T12:00:00Z").starts_with("La fecha ya pasó"));
    assert!(refused("2020-01-01T00:00:00Z").starts_with("La fecha ya pasó"));
    assert!(refused("2027-10-02T12:00:00Z").starts_with("Hasta un año"));
    let nonsense = refused("pasado mañana\u{0}");
    assert!(
        nonsense.starts_with("No entiendo hasta cuándo: pasado mañana."),
        "{nonsense}"
    );
    assert!(
        snooze(&t, "nope:1", "1d", *NOW)
            .unwrap()
            .message
            .starts_with("No existe")
    );
    assert_eq!(
        t.get_application(&key).unwrap().unwrap().snoozed_until,
        None,
        "a refusal changes nothing"
    );
}
#[test]
fn the_auto_approval_threshold_is_kept_within_what_the_queue_itself_accepts() {
    let t = Tracker::in_memory().unwrap();
    assert_eq!(auto_approve_min(&t).unwrap(), None, "off until set");

    // The apply threshold of the profile is 60: nothing below it could even be queued.
    let set = |value, threshold| set_auto_approve_min(&t, value, threshold).unwrap();
    for (value, ok) in [
        (59, false),
        (60, true),
        (85, true),
        (100, true),
        (101, false),
        (0, false),
        (-5, false),
        (i64::MAX, false),
    ] {
        let outcome = set(Some(value), Some(60.0));
        assert_eq!(outcome.ok, ok, "{value}: {}", outcome.message);
        if !ok {
            assert!(
                outcome.message.contains("60") && outcome.message.contains("100"),
                "the refusal says the range: {}",
                outcome.message
            );
        }
    }
    assert_eq!(
        auto_approve_min(&t).unwrap(),
        Some(100),
        "a refusal changes nothing"
    );

    // A threshold that is not a whole number rounds up: 62.5 queues from 63.
    assert!(!set(Some(62), Some(62.5)).ok);
    assert!(set(Some(63), Some(62.5)).ok);
    // Without the profile there is nothing to check a number against: it is not kept.
    let unreadable = set(Some(90), None);
    assert!(!unreadable.ok && unreadable.message.contains("criteria.toml"));
    assert_eq!(auto_approve_min(&t).unwrap(), Some(63));

    // Turning it off needs no profile and is fine twice.
    assert!(set(None, None).ok);
    assert_eq!(auto_approve_min(&t).unwrap(), None);
    assert!(set(None, None).ok);
}
#[test]
fn a_stored_threshold_that_makes_no_sense_reads_as_off() {
    let t = Tracker::in_memory().unwrap();
    for stored in ["", "abc", "90.5", "-3", "101", "9999999999999999999999"] {
        t.set_meta(AUTO_APPROVE_KEY, stored).unwrap();
        assert_eq!(auto_approve_min(&t).unwrap(), None, "{stored:?}");
    }
    t.set_meta(AUTO_APPROVE_KEY, " 88 ").unwrap();
    assert_eq!(auto_approve_min(&t).unwrap(), Some(88));
}
