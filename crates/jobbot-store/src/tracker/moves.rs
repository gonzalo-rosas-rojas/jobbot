//! Changing an application's state, and taking a mark made by hand back. They share one
//! transaction shape and the `events.undo` column, which is why they live together.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::{
    Result, State, Tracker, TrackerError, UndoRefusal, UndoableMark, from_text, state_from_text,
    to_text,
};

/// What a move overwrites in its application row, as one JSON object: a hand mark keeps it in
/// `events.undo` and `undo_mark` writes it back.
const ROW_BEFORE_MOVE: &str = "json_object('note', note, 'needs_reason', needs_reason, \
     'snoozed_until', snoozed_until, 'updated_at', updated_at, 'submitted_at', submitted_at)";

/// Writes a `ROW_BEFORE_MOVE` object (`?2`) back into the application row (`?3`) and puts the state
/// back (`?1`). It must restore every column `ROW_BEFORE_MOVE` saves; a test compares the two.
const RESTORE_ROW_BEFORE_MOVE: &str = "UPDATE applications SET state = ?1, \
     note = json_extract(?2, '$.note'), \
     needs_reason = json_extract(?2, '$.needs_reason'), \
     snoozed_until = json_extract(?2, '$.snoozed_until'), \
     updated_at = json_extract(?2, '$.updated_at'), \
     submitted_at = json_extract(?2, '$.submitted_at') \
     WHERE job_key = ?3";

/// The newest event of an application, whatever kind it is.
fn newest_event(conn: &Connection, job_key: &str) -> Result<Option<StoredEvent>> {
    Ok(conn
        .query_row(
            "SELECT id, at, from_state, to_state, undo FROM events WHERE job_key = ?1 \
             ORDER BY id DESC LIMIT 1",
            [job_key],
            |row| {
                Ok(StoredEvent {
                    id: row.get(0)?,
                    at: row.get(1)?,
                    from: row.get(2)?,
                    to: row.get(3)?,
                    undo: row.get(4)?,
                })
            },
        )
        .optional()?)
}

/// Who made a move: the program (`transition`) or Gonza by hand (`mark_transition`). Only a move
/// by hand keeps what it overwrites, so only it can be taken back.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MoveOrigin {
    Automatic,
    ByHand,
}

/// An `events` row as stored, as `undo_mark` reads it.
struct StoredEvent {
    id: i64,
    at: String,
    from: Option<String>,
    to: String,
    undo: Option<String>,
}

impl Tracker {
    /// Moves an application along the state machine and records the event. Moving to
    /// `NeedsGonza` stores `note` as the reason; leaving it clears the reason; moving to
    /// `Submitted` stamps `submitted_at`. Any move ends a snooze: what Gonza put off was the
    /// application as it stood.
    pub fn transition(
        &self,
        job_key: &str,
        new_state: State,
        note: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.move_application(job_key, new_state, note, now, MoveOrigin::Automatic)
    }

    /// `transition` for a move Gonza recorded by hand. Its event also keeps the columns the move
    /// overwrites (`events.undo`), so `undo_mark` can put them back.
    pub fn mark_transition(
        &self,
        job_key: &str,
        new_state: State,
        note: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.move_application(job_key, new_state, note, now, MoveOrigin::ByHand)
    }

    fn move_application(
        &self,
        job_key: &str,
        new_state: State,
        note: Option<&str>,
        now: DateTime<Utc>,
        origin: MoveOrigin,
    ) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: Option<(String, String)> = tx
            .query_row(
                &format!("SELECT state, {ROW_BEFORE_MOVE} FROM applications WHERE job_key = ?1"),
                [job_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((current_text, before)) = current else {
            return Err(TrackerError::UnknownKey(job_key.to_string()));
        };
        let current = state_from_text(0, &current_text)?;
        if !current.allowed_next().contains(&new_state) {
            return Err(TrackerError::IllegalTransition {
                from: current.to_string(),
                to: new_state.to_string(),
            });
        }
        let at = to_text(now);
        // Notes carry text from web pages (form labels, validation messages) and are printed raw.
        let note = note.map(jobbot_domain::outbound::plain_line);
        let note = note.as_deref();
        tx.execute(
            "UPDATE applications SET state = ?1, updated_at = ?2, note = ?3, \
             needs_reason = CASE WHEN ?1 = ?5 THEN ?3 \
                                 WHEN state = ?5 THEN NULL \
                                 ELSE needs_reason END, \
             submitted_at = CASE WHEN ?1 = ?6 THEN ?2 ELSE submitted_at END, \
             snoozed_until = NULL \
             WHERE job_key = ?4",
            params![
                new_state.as_str(),
                at,
                note,
                job_key,
                State::NeedsGonza.as_str(),
                State::Submitted.as_str()
            ],
        )?;
        tx.execute(
            "INSERT INTO events (job_key, at, from_state, to_state, note, undo) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                job_key,
                at,
                current.as_str(),
                new_state.as_str(),
                note,
                (origin == MoveOrigin::ByHand).then_some(before)
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Takes back the newest move of an application when it was a mark (`mark_transition`) made
    /// after `since` and the row still shows it: same state, nothing written over it since. The
    /// application gets back the columns the mark overwrote and the mark's event is deleted, as if
    /// it had never been made. Anything else is a refusal and changes nothing.
    pub fn undo_mark(
        &self,
        job_key: &str,
        since: DateTime<Utc>,
    ) -> Result<std::result::Result<UndoableMark, UndoRefusal>> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row: Option<(String, String, Option<String>)> = tx
            .query_row(
                "SELECT state, updated_at, snoozed_until FROM applications WHERE job_key = ?1",
                [job_key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((state, updated_at, snoozed_until)) = row else {
            return Ok(Err(UndoRefusal::UnknownKey));
        };
        let since = to_text(since);
        let Some(StoredEvent {
            id,
            at,
            from: Some(from),
            to,
            undo: Some(before),
        }) = newest_event(&tx, job_key)?
        else {
            let marked_since = tx
                .prepare(
                    "SELECT 1 FROM events WHERE job_key = ?1 AND undo IS NOT NULL AND at > ?2",
                )?
                .exists(params![job_key, since])?;
            return Ok(Err(if marked_since {
                UndoRefusal::ChangedSince
            } else {
                UndoRefusal::NoRecentMark
            }));
        };
        if at <= since {
            return Ok(Err(UndoRefusal::TooOld));
        }
        if state != to || updated_at != at || snoozed_until.is_some() {
            return Ok(Err(UndoRefusal::ChangedSince));
        }
        let mark = UndoableMark {
            from: state_from_text(2, &from)?,
            to: state_from_text(3, &to)?,
            at: from_text(1, &at)?,
        };
        tx.execute(RESTORE_ROW_BEFORE_MOVE, params![from, before, job_key])?;
        tx.execute("DELETE FROM events WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(Ok(mark))
    }

    /// The applications whose newest move is a mark made after `since` that `undo_mark` would
    /// still take back, by key.
    pub fn undoable_marks(&self, since: DateTime<Utc>) -> Result<BTreeMap<String, UndoableMark>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT e.job_key, e.from_state, e.to_state, e.at FROM events e \
             JOIN applications a ON a.job_key = e.job_key \
             WHERE e.undo IS NOT NULL AND e.at > ?1 \
               AND e.id = (SELECT MAX(id) FROM events WHERE job_key = e.job_key) \
               AND a.state = e.to_state AND a.updated_at = e.at AND a.snoozed_until IS NULL",
        )?;
        let rows = stmt.query_map([to_text(since)], |row| {
            let from: String = row.get(1)?;
            let to: String = row.get(2)?;
            let at: String = row.get(3)?;
            Ok((
                row.get::<_, String>(0)?,
                UndoableMark {
                    from: state_from_text(1, &from)?,
                    to: state_from_text(2, &to)?,
                    at: from_text(3, &at)?,
                },
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::TimeDelta;
    use jobbot_domain::fixtures::{NOW, make_job, make_job_with};

    use crate::tracker::test_support::{state_of, submit, tracker_with_prepared};

    use super::*;

    #[test]
    fn a_note_from_a_web_page_is_stored_without_control_characters() {
        // Form labels and validation messages reach the notes; a page can put an escape sequence or a
        // right-to-left override in them, and the notes are printed raw by `submit`, `top`, `results`.
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        t.transition(&job.key(), State::Prepared, None, *NOW)
            .unwrap();
        t.transition(
            &job.key(),
            State::NeedsGonza,
            Some("faltan respuestas: \u{1b}[2JEmail\u{202e}x\n*\u{7}"),
            *NOW,
        )
        .unwrap();

        let app = t.get_application(&job.key()).unwrap().unwrap();

        assert_eq!(
            app.needs_reason.as_deref(),
            Some("faltan respuestas: [2JEmailx *")
        );
        assert_eq!(app.note, app.needs_reason);
    }

    #[test]
    fn illegal_transition_rejected_and_nothing_changes() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        let err = t
            .transition(&job.key(), State::Submitted, None, *NOW)
            .unwrap_err();
        assert!(matches!(
            err,
            TrackerError::IllegalTransition { ref from, ref to } if from == "scored" && to == "submitted"
        ));
        assert_eq!(err.to_string(), "illegal transition scored -> submitted");
        assert_eq!(state_of(&t, &job.key()), State::Scored);
        assert_eq!(
            t.count_events(&[State::Submitted], *NOW - TimeDelta::days(1))
                .unwrap(),
            0
        );
    }

    #[test]
    fn unknown_keys_are_reported() {
        let t = Tracker::in_memory().unwrap();
        let err = t
            .transition("nope:1", State::Prepared, None, *NOW)
            .unwrap_err();
        assert!(matches!(err, TrackerError::UnknownKey(ref key) if key == "nope:1"));
        let err = t.create_application("nope:1", *NOW).unwrap_err();
        assert!(matches!(err, TrackerError::UnknownKey(_)));
        assert!(t.approve("nope:1").is_err());
        assert!(t.set_cv_path("nope:1", Path::new("cv.pdf")).is_err());
        assert!(t.set_screenshot("nope:1", Path::new("shot.png")).is_err());
        assert!(t.get_application("nope:1").unwrap().is_none());
    }

    #[test]
    fn needs_gonza_keeps_reason_and_clears_it_on_leave() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        t.transition(&job.key(), State::NeedsGonza, Some("falta email"), *NOW)
            .unwrap();
        let app = t.get_application(&job.key()).unwrap().unwrap();
        assert_eq!(app.needs_reason.as_deref(), Some("falta email"));
        t.transition(&job.key(), State::Prepared, None, *NOW)
            .unwrap();
        let app = t.get_application(&job.key()).unwrap().unwrap();
        assert_eq!(app.needs_reason, None);
        assert_eq!(app.state, State::Prepared);
    }

    #[test]
    fn technical_test_path_ends_in_accepted_and_refuses_withdrawal() {
        let t = Tracker::in_memory().unwrap();
        let first = make_job_with(|j| j.external_id = "1".into());
        submit(&t, &first);
        for state in [
            State::TechnicalTest,
            State::Interview,
            State::Interview,
            State::Offer,
            State::Accepted,
        ] {
            t.transition(&first.key(), state, None, *NOW).unwrap();
        }
        assert!(
            t.transition(&first.key(), State::Withdrawn, None, *NOW)
                .is_err()
        );
    }

    #[test]
    fn ghosted_path_can_end_in_rejected() {
        let t = Tracker::in_memory().unwrap();
        let second = make_job_with(|j| {
            j.external_id = "2".into();
            j.company = "Other Co".into();
        });
        submit(&t, &second);
        for state in [
            State::Ghosted,
            State::Replied,
            State::TechnicalTest,
            State::Rejected,
        ] {
            t.transition(&second.key(), state, None, *NOW).unwrap();
        }
        assert_eq!(state_of(&t, &second.key()), State::Rejected);
    }

    #[test]
    fn a_late_answer_after_silence_moves_a_ghosted_application() {
        for late in [
            State::Replied,
            State::TechnicalTest,
            State::Interview,
            State::Offer,
            State::Rejected,
        ] {
            let t = Tracker::in_memory().unwrap();
            let job = make_job();
            submit(&t, &job);
            t.transition(&job.key(), State::Ghosted, None, *NOW)
                .unwrap();
            t.transition(&job.key(), late, None, *NOW).unwrap();
            assert_eq!(state_of(&t, &job.key()), late);
        }
        // What was never answered cannot jump to the end of the process.
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        submit(&t, &job);
        t.transition(&job.key(), State::Ghosted, None, *NOW)
            .unwrap();
        for never in [State::Accepted, State::Withdrawn, State::Submitted] {
            assert!(t.transition(&job.key(), never, None, *NOW).is_err());
        }
    }

    #[test]
    fn needs_gonza_can_be_confirmed_as_submitted() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        t.transition(&job.key(), State::Prepared, None, *NOW)
            .unwrap();
        t.transition(
            &job.key(),
            State::NeedsGonza,
            Some("unconfirmed_submit: sin confirmación visible"),
            *NOW,
        )
        .unwrap();
        t.transition(
            &job.key(),
            State::Submitted,
            Some("confirmada por Gonza"),
            *NOW,
        )
        .unwrap();
        let row = t.get_application(&job.key()).unwrap().unwrap();
        assert_eq!(row.state, State::Submitted);
        assert_eq!(row.submitted_at, Some(*NOW));
        assert_eq!(
            row.needs_reason, None,
            "leaving needs_gonza clears the reason"
        );
    }

    #[test]
    fn transition_waits_for_a_competing_writer_instead_of_failing_with_busy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobbot.db");
        let t = Tracker::open(&path).unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        // Another process (for example `discover` of the hourly task) is in the middle of a write.
        let other = Connection::open(&path).unwrap();
        other.execute_batch("BEGIN IMMEDIATE").unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(400));
            other.execute_batch("COMMIT").unwrap();
        });
        // A deferred transaction that reads first and writes later is refused with SQLITE_BUSY at once
        // (SQLite never calls the busy handler for a read-to-write upgrade); one that takes the write
        // lock up front waits for the other writer.
        t.transition(&job.key(), State::Prepared, None, *NOW)
            .expect("the transition waits for the other writer");
        releaser.join().unwrap();
        assert_eq!(
            t.get_application(&job.key()).unwrap().unwrap().state,
            State::Prepared
        );
    }

    #[test]
    fn a_state_change_forgets_the_snooze() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        let until = *NOW + TimeDelta::days(1);
        // What Gonza put off was the application as it stood: once it moves, the snooze is over.
        t.set_snoozed_until(&job.key(), Some(until)).unwrap();
        t.transition(&job.key(), State::Prepared, None, *NOW)
            .unwrap();
        assert_eq!(
            t.get_application(&job.key())
                .unwrap()
                .unwrap()
                .snoozed_until,
            None
        );
    }

    /// A submitted application with a note and a snooze, the way a card sits before Gonza marks it.
    fn submitted_with_snooze(id: &str) -> (Tracker, String) {
        let tracker = tracker_with_prepared(&[id]);
        let key = format!("test:{id}");
        tracker
            .transition(&key, State::Submitted, Some("enviada"), *NOW)
            .unwrap();
        tracker
            .set_snoozed_until(&key, Some(*NOW + TimeDelta::days(3)))
            .unwrap();
        (tracker, key)
    }

    #[test]
    fn a_mark_undone_in_time_puts_the_application_back_as_it_was() {
        let (t, key) = submitted_with_snooze("1");
        let before = t.get_application(&key).unwrap().unwrap();
        let marked_at = *NOW + TimeDelta::minutes(1);
        t.mark_transition(&key, State::Rejected, Some("no siguen"), marked_at)
            .unwrap();
        assert_eq!(state_of(&t, &key), State::Rejected);

        let undone = t.undo_mark(&key, *NOW).unwrap();
        assert_eq!(
            undone,
            Ok(UndoableMark {
                from: State::Submitted,
                to: State::Rejected,
                at: marked_at,
            })
        );
        let after = t.get_application(&key).unwrap().unwrap();
        assert_eq!(after.state, State::Submitted);
        assert_eq!(after.note, before.note);
        assert_eq!(after.needs_reason, before.needs_reason);
        assert_eq!(after.snoozed_until, before.snoozed_until);
        assert_eq!(after.updated_at, before.updated_at);
        assert_eq!(after.submitted_at, before.submitted_at);
        let last = t.events_for(&key).unwrap().pop().unwrap();
        assert_eq!(last.to_state, State::Submitted, "the mark leaves no trace");
    }

    #[test]
    fn only_a_move_marked_by_hand_can_be_undone() {
        let (t, key) = submitted_with_snooze("1");
        t.transition(&key, State::Rejected, None, *NOW + TimeDelta::minutes(1))
            .unwrap();
        assert_eq!(
            t.undo_mark(&key, *NOW).unwrap(),
            Err(UndoRefusal::NoRecentMark)
        );
        assert_eq!(state_of(&t, &key), State::Rejected);
    }

    #[test]
    fn a_mark_followed_by_another_change_cannot_be_undone() {
        let (t, key) = submitted_with_snooze("1");
        t.mark_transition(&key, State::Replied, None, *NOW + TimeDelta::minutes(1))
            .unwrap();
        t.transition(&key, State::Interview, None, *NOW + TimeDelta::minutes(2))
            .unwrap();
        assert_eq!(
            t.undo_mark(&key, *NOW).unwrap(),
            Err(UndoRefusal::ChangedSince)
        );
        assert_eq!(state_of(&t, &key), State::Interview);

        let (t, key) = submitted_with_snooze("2");
        t.mark_transition(&key, State::Replied, None, *NOW + TimeDelta::minutes(1))
            .unwrap();
        t.set_snoozed_until(&key, Some(*NOW + TimeDelta::days(1)))
            .unwrap();
        assert_eq!(
            t.undo_mark(&key, *NOW).unwrap(),
            Err(UndoRefusal::ChangedSince),
            "a snooze set after the mark would be lost"
        );
        assert_eq!(state_of(&t, &key), State::Replied);
    }

    #[test]
    fn a_mark_older_than_the_window_cannot_be_undone() {
        let (t, key) = submitted_with_snooze("1");
        let marked_at = *NOW + TimeDelta::minutes(1);
        t.mark_transition(&key, State::Ghosted, None, marked_at)
            .unwrap();
        // The window opens a minute after the mark was made.
        assert_eq!(
            t.undo_mark(&key, marked_at + TimeDelta::minutes(1))
                .unwrap(),
            Err(UndoRefusal::TooOld)
        );
        assert_eq!(state_of(&t, &key), State::Ghosted);
    }

    #[test]
    fn a_mark_made_at_the_instant_the_window_opens_cannot_be_undone() {
        let (t, key) = submitted_with_snooze("1");
        t.mark_transition(&key, State::Ghosted, None, *NOW).unwrap();
        assert_eq!(t.undo_mark(&key, *NOW).unwrap(), Err(UndoRefusal::TooOld));
        assert_eq!(state_of(&t, &key), State::Ghosted);
    }

    /// An application in `Prepared` whose five saved columns all hold a value other than the
    /// default, written straight into the row.
    fn prepared_with_every_saved_column_set(t: &Tracker) -> String {
        let key = "test:1".to_string();
        let long_ago = to_text(*NOW - TimeDelta::days(9));
        t.lock()
            .execute(
                "UPDATE applications SET note = 'a note', needs_reason = 'a reason', \
                 snoozed_until = ?1, updated_at = ?2, submitted_at = ?3 WHERE job_key = ?4",
                params![to_text(*NOW + TimeDelta::days(5)), long_ago, long_ago, key],
            )
            .unwrap();
        key
    }

    #[test]
    fn undo_restores_every_column_a_move_saves() {
        // Submitted rewrites `submitted_at`, needs_gonza rewrites `needs_reason`; every move
        // rewrites the note, the date of the change and the snooze.
        for target in [State::Submitted, State::NeedsGonza] {
            let t = tracker_with_prepared(&["1"]);
            let key = prepared_with_every_saved_column_set(&t);
            let before = t.get_application(&key).unwrap().unwrap();
            let marked_at = *NOW + TimeDelta::minutes(1);
            t.mark_transition(&key, target, Some("moved"), marked_at)
                .unwrap();
            assert_ne!(
                t.get_application(&key).unwrap().unwrap().note,
                before.note,
                "the mark did overwrite the row"
            );

            assert!(t.undo_mark(&key, *NOW).unwrap().is_ok(), "{target:?}");

            let after = t.get_application(&key).unwrap().unwrap();
            assert_eq!(after.state, State::Prepared, "{target:?}");
            assert_eq!(after.note, before.note, "{target:?}: note");
            assert_eq!(
                after.needs_reason, before.needs_reason,
                "{target:?}: needs_reason"
            );
            assert_eq!(
                after.snoozed_until, before.snoozed_until,
                "{target:?}: snoozed_until"
            );
            assert_eq!(
                after.updated_at, before.updated_at,
                "{target:?}: updated_at"
            );
            assert_eq!(
                after.submitted_at, before.submitted_at,
                "{target:?}: submitted_at"
            );
        }
    }

    #[test]
    fn a_move_saves_exactly_the_five_columns_the_undo_test_sets() {
        // What `ROW_BEFORE_MOVE` produces, not what it spells: a column added to it must also be
        // added to the restore and to `undo_restores_every_column_a_move_saves`.
        let t = tracker_with_prepared(&["1"]);
        let mut keys: Vec<String> = {
            let conn = t.lock();
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT key FROM json_each((SELECT {ROW_BEFORE_MOVE} FROM applications LIMIT 1))"
                ))
                .unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        keys.sort();
        assert_eq!(
            keys,
            [
                "needs_reason",
                "note",
                "snoozed_until",
                "submitted_at",
                "updated_at"
            ]
        );
    }

    #[test]
    fn undoing_a_mark_of_an_unknown_application_is_refused() {
        let t = Tracker::in_memory().unwrap();
        assert_eq!(
            t.undo_mark("test:nope", *NOW).unwrap(),
            Err(UndoRefusal::UnknownKey)
        );
    }

    #[test]
    fn two_marks_are_undone_newest_first() {
        let (t, key) = submitted_with_snooze("1");
        t.mark_transition(&key, State::Replied, None, *NOW + TimeDelta::minutes(1))
            .unwrap();
        t.mark_transition(&key, State::Interview, None, *NOW + TimeDelta::minutes(2))
            .unwrap();
        let first = t.undo_mark(&key, *NOW).unwrap().unwrap();
        assert_eq!((first.from, first.to), (State::Replied, State::Interview));
        let second = t.undo_mark(&key, *NOW).unwrap().unwrap();
        assert_eq!((second.from, second.to), (State::Submitted, State::Replied));
        assert_eq!(
            t.undo_mark(&key, *NOW).unwrap(),
            Err(UndoRefusal::NoRecentMark)
        );
        assert_eq!(state_of(&t, &key), State::Submitted);
    }

    /// `undoable_marks` (SQL) and `undo_mark` (Rust) each spell the rule "the row still shows the
    /// mark"; this fails when they stop agreeing, so the panel never offers an undo that is refused.
    #[test]
    fn undoable_marks_lists_exactly_what_undo_mark_accepts() {
        let t = tracker_with_prepared(&["1", "2", "3", "4", "5", "6"]);
        for id in ["1", "2", "3", "4", "5", "6"] {
            t.transition(&format!("test:{id}"), State::Submitted, None, *NOW)
                .unwrap();
        }
        let since = *NOW + TimeDelta::minutes(1);
        let later = *NOW + TimeDelta::minutes(2);
        t.mark_transition("test:1", State::Replied, None, later)
            .unwrap();
        t.mark_transition("test:2", State::Replied, None, *NOW)
            .unwrap();
        t.mark_transition("test:3", State::Replied, None, later)
            .unwrap();
        t.transition("test:3", State::Interview, None, later)
            .unwrap();
        t.mark_transition("test:4", State::Replied, None, later)
            .unwrap();
        t.set_snoozed_until("test:4", Some(later + TimeDelta::days(1)))
            .unwrap();
        t.transition("test:5", State::Replied, None, later).unwrap();
        t.mark_transition("test:6", State::Replied, None, later)
            .unwrap();
        t.mark_transition(
            "test:6",
            State::Interview,
            None,
            later + TimeDelta::minutes(1),
        )
        .unwrap();

        let listed = t.undoable_marks(since).unwrap();
        let mut offered = 0;
        for id in ["1", "2", "3", "4", "5", "6"] {
            let key = format!("test:{id}");
            let accepted = t.undo_mark(&key, since).unwrap().is_ok();
            assert_eq!(listed.contains_key(&key), accepted, "{key}");
            offered += usize::from(accepted);
        }
        assert!(
            (2..6).contains(&offered),
            "the scenarios must include both answers"
        );
    }

    #[test]
    fn undoable_marks_are_the_recent_marks_an_undo_would_take_back() {
        let t = tracker_with_prepared(&["1", "2", "3", "4", "5"]);
        for id in ["1", "2", "3", "4", "5"] {
            t.transition(&format!("test:{id}"), State::Submitted, None, *NOW)
                .unwrap();
        }
        let since = *NOW + TimeDelta::minutes(1);
        let later = *NOW + TimeDelta::minutes(2);
        // 1: marked within the window. 2: marked before it. 3: moved on after the mark.
        // 4: snoozed after the mark. 5: moved, not marked.
        t.mark_transition("test:1", State::Replied, None, later)
            .unwrap();
        t.mark_transition("test:2", State::Replied, None, *NOW)
            .unwrap();
        t.mark_transition("test:3", State::Replied, None, later)
            .unwrap();
        t.transition("test:3", State::Interview, None, later)
            .unwrap();
        t.mark_transition("test:4", State::Replied, None, later)
            .unwrap();
        t.set_snoozed_until("test:4", Some(later + TimeDelta::days(1)))
            .unwrap();
        t.transition("test:5", State::Replied, None, later).unwrap();

        let marks = t.undoable_marks(since).unwrap();
        assert_eq!(
            marks.into_iter().collect::<Vec<_>>(),
            vec![(
                "test:1".to_string(),
                UndoableMark {
                    from: State::Submitted,
                    to: State::Replied,
                    at: later,
                }
            )]
        );
    }
}
