//! "Tu lista de hoy": the open tasks and the recent emails the page lists first.

use std::collections::BTreeSet;

use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};
use serde::Serialize;

use super::{QUEUE_CAP, count_of, iso};
use crate::Panel;
use jobbot_app::inbox::{INBOX_IMPORTED_AT, LIVE_STATES};
use jobbot_app::ports::store::TaskRow;
use jobbot_app::reporting::today::RECENT_DAYS;

/// One of Gonza's open tasks, as "Tu lista de hoy" shows it.
#[derive(Debug, Serialize)]
pub struct TodayTask {
    pub id: i64,
    pub text: String,
    pub job_key: Option<String>,
    pub due_at: Option<String>,
    pub overdue: bool,
    /// An http(s) address that came with a task added by hand; the page shows it as a link.
    pub link: Option<String>,
    /// "manual" for one added by hand, else the kind of the email it came from.
    pub kind: String,
    pub created_at: String,
}

/// A recent email that is not an acknowledgement. The text came from someone else's email: the
/// page draws it with `textContent`.
#[derive(Debug, Serialize)]
pub struct TodayEmail {
    pub received_at: String,
    pub from: String,
    /// The company of the posting the email is linked to, when it is linked.
    pub company: Option<String>,
    pub job_key: Option<String>,
    pub kind: &'static str,
    pub subject: String,
    pub summary: String,
}

/// "Tu lista de hoy": the open tasks (soonest due first) and the answers of the last days. The
/// lists are cut at `QUEUE_CAP`; the totals say how many there are.
#[derive(Debug, Serialize)]
pub struct Today {
    pub tasks_total: usize,
    pub tasks: Vec<TodayTask>,
    pub emails_total: usize,
    pub emails: Vec<TodayEmail>,
    pub inbox: TodayInbox,
}

/// Whether the mailbox is being read: the page warns when applications wait for an answer and
/// the last import is old or missing.
#[derive(Debug, Serialize)]
pub struct TodayInbox {
    /// When the last `inbox import` ran (RFC 3339), `null` if it never did.
    pub imported_at: Option<String>,
    /// Applications in a state in which a company may still answer.
    pub waiting: usize,
}

/// `snoozed` are the keys of the applications put off: a task about one of them waits with it.
pub(super) fn today(
    panel: &Panel,
    now: DateTime<Utc>,
    snoozed: &BTreeSet<String>,
) -> Result<Today> {
    let tasks: Vec<TaskRow> = panel
        .tracker
        .open_tasks()?
        .into_iter()
        .filter(|task| {
            !task
                .job_key
                .as_ref()
                .is_some_and(|key| snoozed.contains(key))
        })
        .collect();
    let emails = panel
        .tracker
        .recent_emails(now - TimeDelta::days(RECENT_DAYS))?;
    let counts = panel.tracker.counts()?;
    let waiting = LIVE_STATES
        .iter()
        .map(|state| count_of(&counts, *state))
        .sum();
    Ok(Today {
        inbox: TodayInbox {
            imported_at: panel.tracker.get_meta(INBOX_IMPORTED_AT)?,
            waiting,
        },
        tasks_total: tasks.len(),
        tasks: tasks
            .iter()
            .take(QUEUE_CAP)
            .map(|task| TodayTask {
                id: task.id,
                text: task.text.clone(),
                job_key: task.job_key.clone(),
                due_at: task.due_at.map(iso),
                overdue: task.is_overdue(now),
                link: task.link.clone(),
                kind: task.kind.clone(),
                created_at: iso(task.created_at),
            })
            .collect(),
        emails_total: emails.len(),
        emails: emails
            .into_iter()
            .take(QUEUE_CAP)
            .map(|email| TodayEmail {
                received_at: iso(email.received_at),
                from: email.from_addr.into_inner(),
                company: email.company,
                job_key: email.job_key,
                kind: email.kind.as_str(),
                subject: email.subject,
                summary: email.summary,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;
    use crate::fixtures::{add, panel};
    use crate::snapshot::build_snapshot;
    use crate::snapshot::support_tests::{email, task};
    use jobbot_app::ports::store::{EmailKind, NewEmail, NewTask};
    use jobbot_domain::state::State;
    use jobbot_testkit::NOW;

    #[test]
    fn today_lists_open_tasks_soonest_first_with_an_overdue_flag() {
        let (panel, _dir) = panel();
        task(&panel, "sin fecha", None, None);
        task(
            &panel,
            "pronto",
            Some(*NOW + TimeDelta::hours(3)),
            Some("test:1"),
        );
        task(&panel, "vencida", Some(*NOW - TimeDelta::hours(3)), None);
        task(&panel, "hecha", Some(*NOW), None);
        let done = panel
            .tracker
            .open_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.text == "hecha")
            .unwrap()
            .id;
        panel.tracker.complete_task(done).unwrap();

        let today = serde_json::to_value(build_snapshot(&panel).unwrap().today).unwrap();

        assert_eq!(today["tasks_total"], 3);
        let tasks = today["tasks"].as_array().unwrap();
        let texts: Vec<&str> = tasks.iter().map(|t| t["text"].as_str().unwrap()).collect();
        assert_eq!(texts, ["vencida", "pronto", "sin fecha"]);
        assert_eq!(tasks[0]["overdue"], true);
        assert_eq!(tasks[1]["overdue"], false);
        assert_eq!(tasks[2]["overdue"], false, "no date, never overdue");
        assert_eq!(tasks[1]["job_key"], "test:1");
        assert_eq!(tasks[1]["due_at"], "2026-10-01T15:00:00Z");
        assert!(tasks[2]["due_at"].is_null() && tasks[2]["job_key"].is_null());
        assert!(tasks[1]["id"].as_i64().unwrap() > 0);
        assert_eq!(tasks[0]["kind"], "action_required");
        assert_eq!(tasks[0]["created_at"], "2026-10-01T12:00:00Z");
    }

    #[test]
    fn today_shows_a_manual_task_with_its_link() {
        let (panel, _dir) = panel();
        panel
            .tracker
            .add_manual_task(
                &NewTask {
                    job_key: None,
                    kind: "manual".into(),
                    text: "Llamar a Acme".into(),
                    due_at: Some(*NOW + TimeDelta::hours(2)),
                    source_message_id: None,
                    link: Some("https://example.com/oferta".into()),
                },
                *NOW,
            )
            .unwrap();
        task(&panel, "sin enlace", None, None);

        let today = serde_json::to_value(build_snapshot(&panel).unwrap().today).unwrap();

        assert_eq!(today["tasks_total"], 2);
        let tasks = today["tasks"].as_array().unwrap();
        assert_eq!(tasks[0]["text"], "Llamar a Acme");
        assert_eq!(tasks[0]["link"], "https://example.com/oferta");
        assert!(tasks[1]["link"].is_null(), "a task without a link has none");
    }

    #[test]
    fn today_lists_the_recent_emails_that_are_not_acks_newest_first() {
        let (panel, _dir) = panel();
        let day = TimeDelta::days(1);
        email(
            &panel,
            "old",
            EmailKind::Rejection,
            *NOW - day * 2,
            "Hace dos días",
        );
        email(
            &panel,
            "new",
            EmailKind::InterviewInvite,
            *NOW - TimeDelta::hours(1),
            "Entrevista",
        );
        email(
            &panel,
            "ack",
            EmailKind::Ack,
            *NOW - TimeDelta::hours(2),
            "Recibimos tu postulación",
        );
        email(
            &panel,
            "ancient",
            EmailKind::Recruiter,
            *NOW - day * 9,
            "Muy viejo",
        );

        let today = serde_json::to_value(build_snapshot(&panel).unwrap().today).unwrap();

        assert_eq!(today["emails_total"], 2);
        let emails = today["emails"].as_array().unwrap();
        let subjects: Vec<&str> = emails
            .iter()
            .map(|e| e["subject"].as_str().unwrap())
            .collect();
        assert_eq!(subjects, ["Entrevista", "Hace dos días"]);
        assert_eq!(emails[0]["kind"], "interview_invite");
        assert_eq!(emails[0]["from"], "Ana <ana@acme.example>");
        assert_eq!(emails[0]["summary"], "Resumen new");
        assert_eq!(emails[0]["received_at"], "2026-10-01T11:00:00Z");
        assert!(emails[0]["company"].is_null());
    }

    #[test]
    fn today_names_the_company_of_the_linked_posting() {
        let (panel, _dir) = panel();
        let key = add(&panel, "1", 80.0, &[]);
        panel
            .tracker
            .add_email(
                &NewEmail {
                    message_id: "m1".into(),
                    thread_id: "t1".into(),
                    from_addr: "rrhh@co1.example".into(),
                    subject: "Hola".into(),
                    received_at: *NOW,
                    job_key: Some(key.clone()),
                    kind: EmailKind::Recruiter,
                    summary: "x".into(),
                },
                None,
                *NOW,
            )
            .unwrap();

        let today = build_snapshot(&panel).unwrap().today;

        assert_eq!(today.emails[0].company.as_deref(), Some("Co 1"));
        assert_eq!(today.emails[0].job_key.as_deref(), Some(key.as_str()));
    }

    #[test]
    fn today_says_when_the_mailbox_was_last_read_and_how_many_applications_wait() {
        let (panel, _dir) = panel();
        let json =
            |panel: &Panel| serde_json::to_value(build_snapshot(panel).unwrap().today).unwrap();
        let today = json(&panel);
        assert!(
            today["inbox"]["imported_at"].is_null(),
            "never imported: null"
        );
        assert_eq!(today["inbox"]["waiting"], 0);

        add(&panel, "1", 80.0, &[State::Prepared, State::Submitted]);
        add(
            &panel,
            "2",
            80.0,
            &[State::Prepared, State::Submitted, State::Rejected],
        );
        add(&panel, "3", 80.0, &[State::Prepared]);
        jobbot_app::inbox::import(&*panel.tracker, &[], *NOW).unwrap();

        let today = json(&panel);
        assert_eq!(
            today["inbox"]["imported_at"].as_str(),
            Some(NOW.to_rfc3339().as_str())
        );
        assert_eq!(
            today["inbox"]["waiting"], 1,
            "a sent one waits; a rejected and a merely prepared one do not"
        );
    }

    #[test]
    fn a_long_today_is_capped_but_counts_everything() {
        let (panel, _dir) = panel();
        for n in 0..(QUEUE_CAP + 5) {
            task(&panel, &format!("tarea {n}"), None, None);
            email(
                &panel,
                &format!("m{n}"),
                EmailKind::Recruiter,
                *NOW - TimeDelta::minutes(i64::try_from(n).unwrap()),
                "x",
            );
        }

        let today = build_snapshot(&panel).unwrap().today;

        assert_eq!(
            (today.tasks_total, today.emails_total),
            (QUEUE_CAP + 5, QUEUE_CAP + 5)
        );
        assert_eq!(
            (today.tasks.len(), today.emails.len()),
            (QUEUE_CAP, QUEUE_CAP)
        );
    }

    #[test]
    fn hostile_text_in_an_email_or_a_task_stays_plain_data() {
        let (panel, _dir) = panel();
        let hostile = "</script><img src=x onerror=alert(1)>";
        task(&panel, hostile, None, None);
        email(&panel, "m1", EmailKind::Other, *NOW, hostile);

        let today = build_snapshot(&panel).unwrap().today;

        assert_eq!(today.tasks[0].text, hostile);
        assert_eq!(today.emails[0].subject, hostile);
    }
}
