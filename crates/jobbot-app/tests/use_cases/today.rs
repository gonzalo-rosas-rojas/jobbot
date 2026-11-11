//! The `today` use cases against the real SQLite stores, the fake Claude and the shipped
//! profile (moved from the root crate's `src/today.rs`).

use std::collections::BTreeMap;

use chrono::TimeDelta;
use chrono::{DateTime, Utc};

use jobbot_app::ports::store::{EmailKind, NewEmail, NewTask};
use jobbot_app::reporting::today::*;
use jobbot_domain::state::State;
use jobbot_testkit::Tracker;
use jobbot_testkit::{NOW, make_job, make_job_with};

fn task(t: &Tracker, text: &str, due: Option<DateTime<Utc>>, job_key: Option<&str>) {
    t.add_task(
        &NewTask {
            job_key: job_key.map(str::to_string),
            kind: "action_required".into(),
            text: text.into(),
            due_at: due,
            source_message_id: None,
            link: None,
        },
        *NOW,
    )
    .unwrap();
}
fn email(t: &Tracker, id: &str, kind: EmailKind, at: DateTime<Utc>, job_key: Option<&str>) {
    t.add_email(
        &NewEmail {
            message_id: id.into(),
            thread_id: format!("th-{id}"),
            from_addr: "Ana <ana@acme.example>".into(),
            subject: format!("Asunto {id}"),
            received_at: at,
            job_key: job_key.map(str::to_string),
            kind,
            summary: format!("Resumen {id}"),
        },
        None,
        *NOW,
    )
    .unwrap();
}
#[test]
fn an_empty_tracker_says_so_in_each_section() {
    let text = build_today(&Tracker::in_memory().unwrap(), *NOW).unwrap();
    assert!(text.contains("Tu lista de hoy — 2026-10-01"), "{text}");
    assert!(
        text.contains("Tareas abiertas (0)\n  Sin tareas abiertas.")
            && text.contains("Esperan tu decisión (0)\n  Nada pendiente.")
            && text.contains("Respuestas recientes, últimos 7 días (0)\n  Sin respuestas nuevas."),
        "{text}"
    );
}
#[test]
fn tasks_come_soonest_due_first_with_their_deadline_in_santiago_time() {
    let t = Tracker::in_memory().unwrap();
    task(&t, "sin fecha", None, None);
    task(
        &t,
        "para el lunes",
        Some(*NOW + TimeDelta::days(3)),
        Some("test:1"),
    );
    task(&t, "vencida ayer", Some(*NOW - TimeDelta::hours(27)), None);
    task(
        &t,
        "hoy mismo",
        Some(*NOW + TimeDelta::hours(5) + TimeDelta::minutes(30)),
        None,
    );
    task(&t, "ya casi", Some(*NOW + TimeDelta::minutes(20)), None);

    let text = build_today(&t, *NOW).unwrap();

    let section: Vec<&str> = text
        .lines()
        .skip_while(|l| !l.starts_with("Tareas abiertas"))
        .take_while(|l| !l.is_empty())
        .collect();
    assert_eq!(section[0], "Tareas abiertas (5)");
    // 12:00 UTC is 09:00 in Santiago in October.
    assert_eq!(
        section[1],
        "  #3 vencida ayer — VENCIDA (vencía el 2026-09-30 06:00)"
    );
    assert_eq!(
        section[2],
        "  #5 ya casi — vence en menos de 1 h (2026-10-01 09:20)"
    );
    assert_eq!(
        section[3],
        "  #4 hoy mismo — vence en 5 h (2026-10-01 14:30)"
    );
    assert_eq!(
        section[4],
        "  #2 para el lunes [test:1] — vence en 3 días (2026-10-04 09:00)"
    );
    assert_eq!(section[5], "  #1 sin fecha — sin fecha");
}
#[test]
fn a_task_with_a_link_shows_it_after_the_text() {
    let t = Tracker::in_memory().unwrap();
    let manual = NewTask {
        job_key: Some("test:1".into()),
        kind: "manual".into(),
        text: "Llamar a Toku".into(),
        due_at: Some(*NOW + TimeDelta::hours(5)),
        source_message_id: None,
        link: Some("https://example.com/oferta".into()),
    };
    t.add_manual_task(&manual, *NOW).unwrap();

    let text = build_today(&t, *NOW).unwrap();

    assert!(
            text.contains(
                "  #1 Llamar a Toku · https://example.com/oferta [test:1] — vence en 5 h (2026-10-01 14:00)"
            ),
            "{text}"
        );
}
#[test]
fn what_waits_for_his_decision_is_the_needs_gonza_list_with_its_reason() {
    let t = Tracker::in_memory().unwrap();
    let job = make_job();
    t.add_job(&job, 80.0, &BTreeMap::new(), *NOW).unwrap();
    t.create_application(&job.key(), *NOW).unwrap();
    t.transition(
        &job.key(),
        State::NeedsGonza,
        Some("faltan respuestas: LinkedIn"),
        *NOW,
    )
    .unwrap();
    // Not waiting for him: a sent application.
    let other = make_job_with(|j| j.external_id = "2".into());
    t.add_job(&other, 80.0, &BTreeMap::new(), *NOW).unwrap();
    t.create_application(&other.key(), *NOW).unwrap();

    let text = build_today(&t, *NOW).unwrap();

    assert!(
            text.contains(
                "Esperan tu decisión (1)\n  Acme AI — Senior AI Engineer · test:1: faltan respuestas: LinkedIn"
            ),
            "{text}"
        );
}
#[test]
fn recent_answers_skip_acks_and_old_mail_and_show_the_company_or_the_sender() {
    let t = Tracker::in_memory().unwrap();
    let job = make_job();
    t.add_job(&job, 80.0, &BTreeMap::new(), *NOW).unwrap();
    let day = TimeDelta::days(1);
    email(
        &t,
        "old",
        EmailKind::Rejection,
        *NOW - day * 2,
        Some(&job.key()),
    );
    email(
        &t,
        "new",
        EmailKind::InterviewInvite,
        *NOW - TimeDelta::hours(1),
        None,
    );
    email(&t, "ack", EmailKind::Ack, *NOW - TimeDelta::hours(2), None);
    email(&t, "ancient", EmailKind::Recruiter, *NOW - day * 9, None);

    let text = build_today(&t, *NOW).unwrap();

    let section: Vec<&str> = text
        .lines()
        .skip_while(|l| !l.starts_with("Respuestas recientes"))
        .take_while(|l| !l.is_empty())
        .collect();
    assert_eq!(
        section,
        [
            "Respuestas recientes, últimos 7 días (2)",
            "  2026-10-01 08:00 · Ana <ana@acme.example> · invitación a entrevista · «Asunto new» — Resumen new",
            "  2026-09-29 09:00 · Acme AI · rechazo · «Asunto old» — Resumen old",
        ]
    );
}
#[test]
fn text_from_an_email_never_reaches_the_terminal_with_control_characters() {
    let t = Tracker::in_memory().unwrap();
    t.add_email(
        &NewEmail {
            message_id: "m1".into(),
            thread_id: "t1".into(),
            from_addr: "x\u{1b}[2J@evil.example".into(),
            subject: "Hola\u{202e}\nmundo".into(),
            received_at: *NOW,
            job_key: None,
            kind: EmailKind::Other,
            summary: "\u{7}beep".into(),
        },
        Some(&NewTask {
            job_key: None,
            kind: "other".into(),
            text: "tarea\u{1b}[31m roja".into(),
            due_at: None,
            source_message_id: Some("m1".into()),
            link: None,
        }),
        *NOW,
    )
    .unwrap();

    let text = build_today(&t, *NOW).unwrap();

    assert!(
        !text.chars().any(|c| c.is_control() && c != '\n'),
        "{text:?}"
    );
    assert!(
        text.contains("Hola mundo") && text.contains("beep"),
        "{text}"
    );
}
#[test]
fn the_recent_answers_are_cut_with_a_count_of_the_rest() {
    let t = Tracker::in_memory().unwrap();
    for n in 0..(LIST_CAP + 3) {
        email(
            &t,
            &format!("m{n}"),
            EmailKind::Recruiter,
            *NOW - TimeDelta::minutes(i64::try_from(n).unwrap()),
            None,
        );
    }

    let text = build_today(&t, *NOW).unwrap();

    assert!(
        text.contains(&format!("({})", LIST_CAP + 3)) && text.contains("y 3 más"),
        "{text}"
    );
    assert_eq!(text.matches("· reclutador ·").count(), LIST_CAP);
}
#[test]
fn the_decision_list_is_cut_too_but_the_tasks_never_are() {
    let t = Tracker::in_memory().unwrap();
    for n in 0..(LIST_CAP + 2) {
        let job = make_job_with(|j| j.external_id = n.to_string());
        t.add_job(&job, 80.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        t.transition(&job.key(), State::NeedsGonza, Some("falta email"), *NOW)
            .unwrap();
        task(&t, &format!("tarea {n}"), None, None);
    }

    let text = build_today(&t, *NOW).unwrap();

    assert!(
        text.contains(&format!("Esperan tu decisión ({})", LIST_CAP + 2))
            && text.contains("y 2 más"),
        "{text}"
    );
    assert_eq!(text.matches(": falta email").count(), LIST_CAP);
    assert_eq!(text.matches("— sin fecha").count(), LIST_CAP + 2);
    assert_eq!(
        text.matches("y 2 más").count(),
        1,
        "only the decision list was cut"
    );
}
