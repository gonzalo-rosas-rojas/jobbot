//! `jobbot today`: what Gonza has to do, as plain Spanish text. Read-only. It lists his open tasks
//! (soonest due first), the applications that wait for his decision (the same list and reason the
//! digest and the panel show) and the answers of the last days. Times are Santiago's, because
//! that is where he reads them.

use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};
use chrono_tz::America::Santiago;

use crate::outbound::plain_line;
use crate::tracker::{EmailRow, State, TaskRow, Tracker};

/// How far back "recent answers" looks.
pub const RECENT_DAYS: i64 = 7;
/// Lines shown of the decision list and of the recent answers; the rest is counted.
pub const LIST_CAP: usize = 50;

fn santiago(at: DateTime<Utc>) -> String {
    at.with_timezone(&Santiago)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

/// "vence en 5 h" up to two days away, "vence en 3 días" beyond, "VENCIDA" once it has passed; the
/// Santiago time follows.
fn due_phrase(task: &TaskRow, due: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let when = santiago(due);
    if task.is_overdue(now) {
        return format!("VENCIDA (vencía el {when})");
    }
    let hours = (due - now).num_hours();
    let left = match hours {
        0 => "vence en menos de 1 h".to_string(),
        1..48 => format!("vence en {hours} h"),
        _ => format!("vence en {} días", hours / 24),
    };
    format!("{left} ({when})")
}

fn task_line(task: &TaskRow, now: DateTime<Utc>) -> String {
    let key = task
        .job_key
        .as_deref()
        .map(|key| format!(" [{}]", plain_line(key)))
        .unwrap_or_default();
    let link = task
        .link
        .as_deref()
        .map(|link| format!(" · {}", plain_line(link)))
        .unwrap_or_default();
    let due = match task.due_at {
        Some(due) => due_phrase(task, due, now),
        None => "sin fecha".to_string(),
    };
    format!(
        "  #{} {}{link}{key} — {due}",
        task.id,
        plain_line(&task.text)
    )
}

fn email_line(email: &EmailRow) -> String {
    let who = email.company.as_deref().unwrap_or(&email.from_addr);
    let subject = match plain_line(&email.subject) {
        subject if subject.is_empty() => "(sin asunto)".to_string(),
        subject => subject,
    };
    let summary = match plain_line(&email.summary) {
        summary if summary.is_empty() => String::new(),
        summary => format!(" — {summary}"),
    };
    format!(
        "  {} · {} · {} · «{subject}»{summary}",
        santiago(email.received_at),
        plain_line(who),
        email.kind.label()
    )
}

/// A heading with the total, the rows (cut at `cap`, with the count of the rest) or the sentence
/// for "nothing", and a blank line.
fn section(
    lines: &mut Vec<String>,
    heading: &str,
    mut rows: Vec<String>,
    cap: usize,
    when_empty: &str,
) {
    lines.push(format!("{heading} ({})", rows.len()));
    if rows.is_empty() {
        lines.push(format!("  {when_empty}"));
    } else if rows.len() > cap {
        let rest = rows.len() - cap;
        rows.truncate(cap);
        lines.extend(rows);
        lines.push(format!("  … y {rest} más."));
    } else {
        lines.extend(rows);
    }
    lines.push(String::new());
}

/// The text of `jobbot today`. Everything that came from an email goes through `plain_line` again
/// here, because this text is printed on a terminal.
pub fn build_today(tracker: &Tracker, now: DateTime<Utc>) -> Result<String> {
    let day = now.with_timezone(&Santiago).format("%Y-%m-%d");
    let mut lines = vec![
        format!("Tu lista de hoy — {day} (hora de Santiago)"),
        String::new(),
    ];

    // The tasks are never cut: they are the list.
    let tasks: Vec<String> = tracker
        .open_tasks()?
        .iter()
        .map(|task| task_line(task, now))
        .collect();
    section(
        &mut lines,
        "Tareas abiertas",
        tasks,
        usize::MAX,
        "Sin tareas abiertas.",
    );

    let waiting: Vec<String> = tracker
        .list_by_state(State::NeedsGonza)?
        .iter()
        .map(|row| {
            format!(
                "  {} — {} · {}: {}",
                plain_line(&row.company),
                plain_line(&row.title),
                plain_line(&row.job_key),
                plain_line(row.waiting_reason().unwrap_or("sin motivo"))
            )
        })
        .collect();
    section(
        &mut lines,
        "Esperan tu decisión",
        waiting,
        LIST_CAP,
        "Nada pendiente.",
    );

    let recent: Vec<String> = tracker
        .recent_emails(now - TimeDelta::days(RECENT_DAYS))?
        .iter()
        .map(email_line)
        .collect();
    section(
        &mut lines,
        &format!("Respuestas recientes, últimos {RECENT_DAYS} días"),
        recent,
        LIST_CAP,
        "Sin respuestas nuevas.",
    );

    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    Ok(lines.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::TimeDelta;

    use super::*;
    use crate::testing::{NOW, make_job, make_job_with};
    use crate::tracker::{EmailKind, NewEmail, NewTask, State};

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
                && text
                    .contains("Respuestas recientes, últimos 7 días (0)\n  Sin respuestas nuevas."),
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
            text: "Llamar a Globex".into(),
            due_at: Some(*NOW + TimeDelta::hours(5)),
            source_message_id: None,
            link: Some("https://example.com/oferta".into()),
        };
        t.add_manual_task(&manual, *NOW).unwrap();

        let text = build_today(&t, *NOW).unwrap();

        assert!(
            text.contains(
                "  #1 Llamar a Globex · https://example.com/oferta [test:1] — vence en 5 h (2026-10-01 14:00)"
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
}
