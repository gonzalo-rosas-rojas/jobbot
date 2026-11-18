//! The panel's commands that came with an attempt id: a command with an id runs once and a
//! repeat gets the stored reply.

use chrono::{DateTime, TimeDelta, Utc};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use super::{AttemptStart, Result, Tracker, to_text};

/// How long the reply of a command with an attempt id is kept for a repeat of that id.
const ATTEMPT_KEEP: TimeDelta = TimeDelta::days(7);

impl Tracker {
    /// Starts the attempt `id` of `command` with `request` (its body), unless the id is known.
    /// Only the body's SHA-256 is kept: enough to tell a repeat from another request, without
    /// keeping a week of bodies (an e-mail import is up to 2 MB of mail). Attempts begun more than
    /// `ATTEMPT_KEEP` before `now` are dropped first.
    pub fn begin_attempt(
        &self,
        id: &str,
        command: &str,
        request: &[u8],
        now: DateTime<Utc>,
    ) -> Result<AttemptStart> {
        let request = Sha256::digest(request).to_vec();
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM command_attempts WHERE started_at < ?1",
            [to_text(now - ATTEMPT_KEEP)],
        )?;
        let known = tx
            .query_row(
                "SELECT command, request, state, status, reply FROM command_attempts WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<u16>>(3)?,
                        row.get::<_, Option<Vec<u8>>>(4)?,
                    ))
                },
            )
            .optional()?;
        let start = match known {
            None => {
                tx.execute(
                    "INSERT INTO command_attempts (id, command, request, state, started_at) \
                     VALUES (?1, ?2, ?3, 'started', ?4)",
                    params![id, command, request, to_text(now)],
                )?;
                AttemptStart::New
            }
            Some((known_command, known_request, ..))
                if known_command != command || known_request != request =>
            {
                AttemptStart::Mismatch
            }
            Some((_, _, state, Some(status), Some(reply))) if state == "done" => {
                AttemptStart::Done { status, reply }
            }
            Some(_) => AttemptStart::Unfinished,
        };
        tx.commit()?;
        Ok(start)
    }

    /// Keeps the reply of the attempt `id`, whose command has run.
    pub fn finish_attempt(&self, id: &str, status: u16, reply: &[u8]) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE command_attempts SET state = 'done', status = ?2, reply = ?3 WHERE id = ?1",
            params![id, status, reply],
        )?;
        Ok(())
    }

    /// Drops the attempt `id`, whose command failed, so the same id may try again.
    pub fn forget_attempt(&self, id: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute("DELETE FROM command_attempts WHERE id = ?1", [id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use jobbot_domain::fixtures::NOW;
    use sha2::{Digest, Sha256};

    use crate::tracker::{AttemptStart, Tracker};

    #[test]
    fn a_finished_attempt_gives_its_reply_back_and_a_new_id_runs() {
        let tracker = Tracker::in_memory().unwrap();
        let body = br#"{"id":"q1","grade":4}"#;
        assert_eq!(
            tracker
                .begin_attempt("a-1", "learn/grade", body, *NOW)
                .unwrap(),
            AttemptStart::New
        );
        tracker
            .finish_attempt("a-1", 200, br#"{"ok":true}"#)
            .unwrap();
        assert_eq!(
            tracker
                .begin_attempt("a-1", "learn/grade", body, *NOW)
                .unwrap(),
            AttemptStart::Done {
                status: 200,
                reply: br#"{"ok":true}"#.to_vec()
            }
        );
        assert_eq!(
            tracker
                .begin_attempt("a-2", "learn/grade", body, *NOW)
                .unwrap(),
            AttemptStart::New,
            "another id is another attempt"
        );
    }

    #[test]
    fn an_attempt_id_belongs_to_one_command_and_one_body() {
        let tracker = Tracker::in_memory().unwrap();
        tracker
            .begin_attempt("a-1", "approve", br#"{"key":"x:1"}"#, *NOW)
            .unwrap();
        tracker.finish_attempt("a-1", 200, b"{}").unwrap();
        for (command, body) in [
            ("approve", &br#"{"key":"x:2"}"#[..]),
            ("confirm", &br#"{"key":"x:1"}"#[..]),
        ] {
            assert_eq!(
                tracker.begin_attempt("a-1", command, body, *NOW).unwrap(),
                AttemptStart::Mismatch,
                "{command}"
            );
        }
    }

    #[test]
    fn an_attempt_keeps_a_fingerprint_of_its_body_not_the_body() {
        let tracker = Tracker::in_memory().unwrap();
        let body = br#"[{"summary":"the recruiter's whole e-mail"}]"#;
        tracker
            .begin_attempt("a-1", "inbox/import", body, *NOW)
            .unwrap();
        let kept: Vec<u8> = tracker
            .lock()
            .query_row(
                "SELECT request FROM command_attempts WHERE id = 'a-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        // Exactly the SHA-256 of the body: its fingerprint, and so not the text of the body.
        assert_eq!(kept, Sha256::digest(body).to_vec());
    }

    #[test]
    fn an_unfinished_attempt_is_reported_until_it_is_forgotten() {
        let tracker = Tracker::in_memory().unwrap();
        tracker
            .begin_attempt("a-1", "approve", b"{}", *NOW)
            .unwrap();
        assert_eq!(
            tracker
                .begin_attempt("a-1", "approve", b"{}", *NOW)
                .unwrap(),
            AttemptStart::Unfinished
        );
        tracker.forget_attempt("a-1").unwrap();
        assert_eq!(
            tracker
                .begin_attempt("a-1", "approve", b"{}", *NOW)
                .unwrap(),
            AttemptStart::New,
            "a forgotten attempt (its command failed) may run again"
        );
    }

    #[test]
    fn attempts_older_than_a_week_are_dropped_when_a_new_one_begins() {
        let tracker = Tracker::in_memory().unwrap();
        tracker
            .begin_attempt("old", "approve", b"{}", *NOW)
            .unwrap();
        tracker.finish_attempt("old", 200, b"{}").unwrap();
        tracker
            .begin_attempt("new", "approve", b"{}", *NOW + TimeDelta::days(8))
            .unwrap();
        let ids: Vec<String> = {
            let conn = tracker.lock();
            let mut stmt = conn.prepare("SELECT id FROM command_attempts").unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        assert_eq!(ids, ["new"]);
    }
}
