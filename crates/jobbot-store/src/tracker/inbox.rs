//! The mail jobbot imported and the tasks it leaves for Gonza (`emails`, `gonza_tasks`).

use chrono::{DateTime, Utc};
use jobbot_domain::privacy::Pii;
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

use super::{
    EmailAdded, EmailRow, NewEmail, NewTask, Result, TaskAdded, TaskCompletion, TaskRow, Tracker,
    email_kind_from_text, from_text, optional_from_text, to_text,
};

const EMAIL_SELECT: &str = "SELECT e.message_id, e.thread_id, e.from_addr, e.subject, e.received_at, \
     e.job_key, e.kind, e.summary, e.imported_at, j.company \
     FROM emails e LEFT JOIN jobs j ON j.key = e.job_key";

fn email_from_row(row: &Row<'_>) -> rusqlite::Result<EmailRow> {
    let received: String = row.get(4)?;
    let kind: String = row.get(6)?;
    let imported: String = row.get(8)?;
    Ok(EmailRow {
        message_id: row.get(0)?,
        thread_id: row.get(1)?,
        from_addr: Pii::new(row.get(2)?),
        subject: row.get(3)?,
        received_at: from_text(4, &received)?,
        job_key: row.get(5)?,
        kind: email_kind_from_text(6, &kind)?,
        summary: row.get(7)?,
        imported_at: from_text(8, &imported)?,
        company: row.get(9)?,
    })
}

const TASK_SELECT: &str = "SELECT id, job_key, kind, text, due_at, status, created_at, \
     source_message_id, link FROM gonza_tasks";

fn task_from_row(row: &Row<'_>) -> rusqlite::Result<TaskRow> {
    let due: Option<String> = row.get(4)?;
    let created: String = row.get(6)?;
    Ok(TaskRow {
        id: row.get(0)?,
        job_key: row.get(1)?,
        kind: row.get(2)?,
        text: row.get(3)?,
        due_at: optional_from_text(4, due)?,
        status: row.get(5)?,
        created_at: from_text(6, &created)?,
        source_message_id: row.get(7)?,
        link: row.get(8)?,
    })
}

/// `INSERT OR IGNORE` of a task: false when `(source_message_id, kind)` already has one.
fn insert_task(conn: &Connection, task: &NewTask, now: DateTime<Utc>) -> rusqlite::Result<bool> {
    let changed = conn.execute(
        "INSERT OR IGNORE INTO gonza_tasks \
         (job_key, kind, text, due_at, created_at, source_message_id, link) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            task.job_key,
            task.kind,
            task.text,
            task.due_at.map(to_text),
            to_text(now),
            task.source_message_id,
            task.link
        ],
    )?;
    Ok(changed > 0)
}

impl Tracker {
    pub fn email_exists(&self, message_id: &str) -> Result<bool> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM emails WHERE message_id = ?1)",
            [message_id],
            |row| row.get(0),
        )?)
    }

    /// Stores an email and its task in one transaction. A message id that is already stored writes
    /// nothing at all (not even the task), so importing the same file twice changes nothing.
    pub fn add_email(
        &self,
        email: &NewEmail,
        task: Option<&NewTask>,
        now: DateTime<Utc>,
    ) -> Result<EmailAdded> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stored = tx.execute(
            "INSERT OR IGNORE INTO emails (message_id, thread_id, from_addr, subject, received_at, \
             job_key, kind, summary, imported_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                email.message_id,
                email.thread_id,
                email.from_addr.expose(),
                email.subject,
                to_text(email.received_at),
                email.job_key,
                email.kind.as_str(),
                email.summary,
                to_text(now)
            ],
        )?;
        if stored == 0 {
            return Ok(EmailAdded::AlreadyKnown);
        }
        let task_created = match task {
            Some(task) => insert_task(&tx, task, now)?,
            None => false,
        };
        tx.commit()?;
        Ok(EmailAdded::Stored { task_created })
    }

    pub fn get_email(&self, message_id: &str) -> Result<Option<EmailRow>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!("{EMAIL_SELECT} WHERE e.message_id = ?1"),
                [message_id],
                email_from_row,
            )
            .optional()?)
    }

    /// Sets the posting (`job_key`) of a stored email that had none, and of the task that email
    /// made. False when the message is unknown or already has a posting: a link is made once.
    pub fn link_email(&self, message_id: &str, job_key: &str) -> Result<bool> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let linked = tx.execute(
            "UPDATE emails SET job_key = ?2 WHERE message_id = ?1 AND job_key IS NULL",
            params![message_id, job_key],
        )?;
        if linked > 0 {
            tx.execute(
                "UPDATE gonza_tasks SET job_key = ?2 WHERE source_message_id = ?1 AND job_key IS NULL",
                params![message_id, job_key],
            )?;
        }
        tx.commit()?;
        Ok(linked > 0)
    }

    /// Adds a task for Gonza; false when that message already produced one of this kind.
    pub fn add_task(&self, task: &NewTask, now: DateTime<Utc>) -> Result<bool> {
        let conn = self.lock();
        Ok(insert_task(&conn, task, now)?)
    }

    /// Adds a task that came from no email (`jobbot task add`). The same text for the same
    /// posting is not added again while a task like it, from any source, is still open.
    pub fn add_manual_task(&self, task: &NewTask, now: DateTime<Utc>) -> Result<TaskAdded> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let open: Option<i64> = tx
            .query_row(
                "SELECT id FROM gonza_tasks WHERE status = 'open' AND text = ?1 AND job_key IS ?2 \
                 ORDER BY id LIMIT 1",
                params![task.text, task.job_key],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(id) = open {
            return Ok(TaskAdded::AlreadyOpen(id));
        }
        insert_task(&tx, task, now)?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(TaskAdded::Created(id))
    }

    pub fn get_task(&self, id: i64) -> Result<Option<TaskRow>> {
        let conn = self.lock();
        Ok(conn
            .query_row(&format!("{TASK_SELECT} WHERE id = ?1"), [id], task_from_row)
            .optional()?)
    }

    /// The tasks still to do: soonest due first, the ones without a date last.
    pub fn open_tasks(&self) -> Result<Vec<TaskRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "{TASK_SELECT} WHERE status = 'open' ORDER BY due_at IS NULL, due_at, id"
        ))?;
        let rows = stmt.query_map([], task_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Closes an open task. A task that is already closed, or does not exist, is reported, not changed.
    pub fn complete_task(&self, id: i64) -> Result<TaskCompletion> {
        let conn = self.lock();
        let changed = conn.execute(
            "UPDATE gonza_tasks SET status = 'done' WHERE id = ?1 AND status = 'open'",
            [id],
        )?;
        if changed > 0 {
            return Ok(TaskCompletion::Done);
        }
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM gonza_tasks WHERE id = ?1)",
            [id],
            |row| row.get(0),
        )?;
        Ok(if exists {
            TaskCompletion::AlreadyDone
        } else {
            TaskCompletion::Unknown
        })
    }

    /// Emails received at or after `since` that are not acknowledgements, newest first.
    pub fn recent_emails(&self, since: DateTime<Utc>) -> Result<Vec<EmailRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "{EMAIL_SELECT} WHERE e.received_at >= ?1 AND e.kind <> 'ack' \
             ORDER BY e.received_at DESC, e.message_id"
        ))?;
        let rows = stmt.query_map([to_text(since)], email_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::TimeDelta;
    use jobbot_domain::fixtures::{NOW, make_job};

    use crate::tracker::test_support::{new_email, new_task};
    use crate::tracker::{EmailAdded, EmailKind, NewTask, TaskAdded, TaskCompletion, Tracker};

    #[test]
    fn a_task_is_overdue_only_when_its_due_time_has_passed() {
        let t = Tracker::in_memory().unwrap();
        for due in [
            Some(*NOW - TimeDelta::minutes(1)),
            Some(*NOW),
            Some(*NOW + TimeDelta::minutes(1)),
            None,
        ] {
            t.add_task(&new_task("x", due, None), *NOW).unwrap();
        }
        let flags: Vec<bool> = t
            .open_tasks()
            .unwrap()
            .iter()
            .map(|task| task.is_overdue(*NOW))
            .collect();
        assert_eq!(flags, [true, false, false, false]);
    }

    #[test]
    fn an_email_is_stored_once_and_its_task_with_it() {
        let t = Tracker::in_memory().unwrap();
        let email = new_email("m1", EmailKind::ActionRequired, *NOW);
        let task = new_task(
            "Enviar el video",
            Some(*NOW + TimeDelta::hours(5)),
            Some("m1"),
        );

        assert!(!t.email_exists("m1").unwrap());
        let first = t.add_email(&email, Some(&task), *NOW).unwrap();
        assert_eq!(first, EmailAdded::Stored { task_created: true });
        assert!(t.email_exists("m1").unwrap());

        // The same message again changes nothing, not even its task.
        let again = t.add_email(&email, Some(&task), *NOW).unwrap();
        assert_eq!(again, EmailAdded::AlreadyKnown);
        assert_eq!(t.open_tasks().unwrap().len(), 1);
        assert_eq!(t.recent_emails(*NOW - TimeDelta::days(1)).unwrap().len(), 1);

        // A task the unique key would not catch (it names no message) is not written either: a
        // known message returns before it writes anything.
        let loose = new_task("otra tarea", None, None);
        assert_eq!(
            t.add_email(&email, Some(&loose), *NOW).unwrap(),
            EmailAdded::AlreadyKnown
        );
        assert_eq!(t.open_tasks().unwrap().len(), 1);
    }

    #[test]
    fn an_email_stored_without_a_posting_can_be_linked_once_and_takes_its_task_along() {
        let t = Tracker::in_memory().unwrap();
        let email = new_email("m1", EmailKind::InterviewInvite, *NOW);
        let task = NewTask {
            kind: "interview_invite".into(),
            ..new_task("Responder", None, Some("m1"))
        };
        t.add_email(&email, Some(&task), *NOW).unwrap();
        assert_eq!(t.get_email("m1").unwrap().unwrap().job_key, None);
        assert!(t.get_email("nope").unwrap().is_none());

        assert!(t.link_email("m1", "test:1").unwrap());

        assert_eq!(
            t.get_email("m1").unwrap().unwrap().job_key.as_deref(),
            Some("test:1")
        );
        assert_eq!(
            t.open_tasks().unwrap()[0].job_key.as_deref(),
            Some("test:1")
        );
        // A linked email keeps its link: the second call changes nothing.
        assert!(!t.link_email("m1", "test:2").unwrap());
        assert_eq!(
            t.get_email("m1").unwrap().unwrap().job_key.as_deref(),
            Some("test:1")
        );
        assert!(!t.link_email("nope", "test:1").unwrap());
    }

    #[test]
    fn a_manual_task_names_no_message_and_is_added_once_while_it_is_open() {
        let t = Tracker::in_memory().unwrap();
        let task = NewTask {
            kind: "manual".into(),
            link: Some("https://example.com/oferta".into()),
            ..new_task("Llamar a Toku", None, None)
        };

        let TaskAdded::Created(id) = t.add_manual_task(&task, *NOW).unwrap() else {
            panic!("the first one is created");
        };
        assert_eq!(
            t.add_manual_task(&task, *NOW).unwrap(),
            TaskAdded::AlreadyOpen(id)
        );
        let rows = t.open_tasks().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (
                rows[0].id,
                rows[0].kind.as_str(),
                rows[0].source_message_id.clone()
            ),
            (id, "manual", None)
        );
        assert_eq!(rows[0].link.as_deref(), Some("https://example.com/oferta"));

        // The same words for another posting are another task.
        let keyed = NewTask {
            job_key: Some("test:1".into()),
            ..task.clone()
        };
        assert!(matches!(
            t.add_manual_task(&keyed, *NOW).unwrap(),
            TaskAdded::Created(_)
        ));
        // A task that came from an email counts too: the list would show the same line twice.
        t.add_task(&new_task("Responder", None, Some("m1")), *NOW)
            .unwrap();
        let same_text = NewTask {
            text: "Responder".into(),
            ..task.clone()
        };
        assert!(matches!(
            t.add_manual_task(&same_text, *NOW).unwrap(),
            TaskAdded::AlreadyOpen(_)
        ));
        // Once it is done, the same task may be added again.
        t.complete_task(id).unwrap();
        assert!(matches!(
            t.add_manual_task(&task, *NOW).unwrap(),
            TaskAdded::Created(_)
        ));
        assert_eq!(t.open_tasks().unwrap().len(), 3);
    }

    #[test]
    fn a_task_is_unique_per_message_and_kind() {
        let t = Tracker::in_memory().unwrap();
        let task = new_task("Responder", None, Some("m1"));
        assert!(t.add_task(&task, *NOW).unwrap());
        assert!(!t.add_task(&task, *NOW).unwrap(), "same message and kind");
        let other_kind = NewTask {
            kind: "interview_invite".into(),
            ..task.clone()
        };
        assert!(t.add_task(&other_kind, *NOW).unwrap());
        // Tasks that come from no message are never merged.
        let loose = new_task("Llamar a la empresa", None, None);
        assert!(t.add_task(&loose, *NOW).unwrap());
        assert!(t.add_task(&loose, *NOW).unwrap());
        assert_eq!(t.open_tasks().unwrap().len(), 4);
    }

    #[test]
    fn open_tasks_come_soonest_due_first_and_undated_last() {
        let t = Tracker::in_memory().unwrap();
        t.add_task(&new_task("sin fecha", None, None), *NOW)
            .unwrap();
        t.add_task(
            &new_task("tarde", Some(*NOW + TimeDelta::days(3)), None),
            *NOW,
        )
        .unwrap();
        t.add_task(
            &new_task("vencida", Some(*NOW - TimeDelta::hours(2)), None),
            *NOW,
        )
        .unwrap();
        t.add_task(
            &new_task("pronto", Some(*NOW + TimeDelta::hours(4)), None),
            *NOW,
        )
        .unwrap();

        let texts: Vec<String> = t
            .open_tasks()
            .unwrap()
            .into_iter()
            .map(|r| r.text)
            .collect();

        assert_eq!(texts, ["vencida", "pronto", "tarde", "sin fecha"]);
    }

    #[test]
    fn completing_a_task_closes_it_once() {
        let t = Tracker::in_memory().unwrap();
        t.add_task(&new_task("a", None, None), *NOW).unwrap();
        let id = t.open_tasks().unwrap()[0].id;

        assert_eq!(t.complete_task(id).unwrap(), TaskCompletion::Done);
        assert!(t.open_tasks().unwrap().is_empty());
        assert_eq!(t.complete_task(id).unwrap(), TaskCompletion::AlreadyDone);
        assert_eq!(t.complete_task(id + 100).unwrap(), TaskCompletion::Unknown);
    }

    #[test]
    fn recent_emails_skip_acks_and_old_mail_and_come_newest_first() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        let day = TimeDelta::days(1);
        let mut linked = new_email("old-ish", EmailKind::Recruiter, *NOW - day * 3);
        linked.job_key = Some(job.key());
        for email in [
            linked,
            new_email("ack", EmailKind::Ack, *NOW - day),
            new_email("newest", EmailKind::Rejection, *NOW - TimeDelta::hours(1)),
            new_email("ancient", EmailKind::Recruiter, *NOW - day * 8),
        ] {
            t.add_email(&email, None, *NOW).unwrap();
        }

        let rows = t.recent_emails(*NOW - day * 7).unwrap();

        let ids: Vec<&str> = rows.iter().map(|r| r.message_id.as_str()).collect();
        assert_eq!(ids, ["newest", "old-ish"]);
        assert_eq!(rows[0].kind, EmailKind::Rejection);
        assert_eq!(rows[0].company, None);
        assert_eq!(rows[1].company.as_deref(), Some("Acme AI"));
        assert_eq!(rows[1].received_at, *NOW - day * 3);
        assert_eq!(rows[1].imported_at, *NOW);
    }
}
