//! The write-ahead log of the irreversible act (clicking submit): an intent is written before the
//! click and closed after, and an open one at the next start is recovered.

use chrono::{DateTime, Utc};
use rusqlite::params;

use super::{INTENT_SUBMIT, INTERRUPTED_PREFIX, Intent, Result, State, Tracker, to_text};

impl Tracker {
    /// Announces an irreversible act before it happens. `Some(id)` when this caller now owns it;
    /// `None` when another intent of the same kind for the same application is still open, in
    /// which case the caller must not act.
    pub fn begin_intent(
        &self,
        kind: &str,
        job_key: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<i64>> {
        let conn = self.lock();
        let inserted = conn.execute(
            "INSERT INTO intents (kind, job_key, state, started_at) VALUES (?1, ?2, 'started', ?3) \
             ON CONFLICT DO NOTHING",
            params![kind, job_key, to_text(now)],
        )?;
        Ok((inserted == 1).then(|| conn.last_insert_rowid()))
    }

    /// Closes an intent with what came of it (`submitted`, `refused`, `interrupted`, ...).
    pub fn finish_intent(&self, id: i64, outcome: &str, now: DateTime<Utc>) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE intents SET state = 'finished', finished_at = ?2, outcome = ?3 WHERE id = ?1",
            params![id, to_text(now), outcome],
        )?;
        Ok(())
    }

    /// The intents of `kind` that were announced and never closed, oldest first.
    pub fn open_intents(&self, kind: &str) -> Result<Vec<Intent>> {
        let conn = self.lock();
        let mut statement = conn.prepare(
            "SELECT id, kind, job_key, started_at FROM intents \
             WHERE kind = ?1 AND state = 'started' ORDER BY id",
        )?;
        let rows = statement.query_map([kind], |row| {
            Ok(Intent {
                id: row.get(0)?,
                kind: row.get(1)?,
                job_key: row.get(2)?,
                started_at: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Rows of the intents table, closed or not. The tests use it to prove that a path never
    /// wrote one; public because those tests live in the application crate.
    pub fn count_intents(&self) -> Result<i64> {
        Ok(self
            .lock()
            .query_row("SELECT COUNT(*) FROM intents", [], |row| row.get(0))?)
    }

    /// Run at start: every submit intent still open is a click whose outcome nobody recorded.
    /// An application still `Prepared` is stamped as attempted (the click may have gone through,
    /// so the daily cap and the company cooldown count it) and handed to Gonza with a reason that
    /// discovery will not overwrite. An application that already moved on, or that no longer
    /// exists, only has its intent closed. Returns the keys handed to Gonza.
    pub fn recover_open_intents(&self, now: DateTime<Utc>) -> Result<Vec<String>> {
        let mut handed = Vec::new();
        for intent in self.open_intents(INTENT_SUBMIT)? {
            let prepared = self
                .get_application(&intent.job_key)?
                .is_some_and(|row| row.state == State::Prepared);
            if prepared {
                self.record_attempt(&intent.job_key, now)?;
                self.transition(
                    &intent.job_key,
                    State::NeedsGonza,
                    Some(&format!(
                        "{INTERRUPTED_PREFIX}: la corrida se cortó entre el clic y su confirmación; \
                         revisa a mano si se envió (jobbot confirm si salió, jobbot resolve si no)"
                    )),
                    now,
                )?;
                handed.push(intent.job_key.clone());
            }
            self.finish_intent(intent.id, "interrupted", now)?;
        }
        Ok(handed)
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use jobbot_domain::fixtures::NOW;

    use crate::tracker::test_support::{state_of, tracker_with_prepared};
    use crate::tracker::{INTENT_SUBMIT, INTERRUPTED_PREFIX, State, Tracker};

    #[test]
    fn only_one_submit_intent_can_be_open_per_application() {
        let tracker = tracker_with_prepared(&["1"]);
        let first = tracker.begin_intent(INTENT_SUBMIT, "test:1", *NOW).unwrap();
        assert!(first.is_some());
        assert_eq!(
            tracker.begin_intent(INTENT_SUBMIT, "test:1", *NOW).unwrap(),
            None,
            "a second run must not click the same form"
        );
        tracker
            .finish_intent(first.unwrap(), "submitted", *NOW)
            .unwrap();
        assert!(tracker.open_intents(INTENT_SUBMIT).unwrap().is_empty());
        assert!(
            tracker
                .begin_intent(INTENT_SUBMIT, "test:1", *NOW)
                .unwrap()
                .is_some(),
            "once closed, another may open"
        );
    }

    #[test]
    fn recovery_hands_an_interrupted_application_to_gonza_and_counts_the_click() {
        let tracker = tracker_with_prepared(&["1"]);
        tracker
            .begin_intent(INTENT_SUBMIT, "test:1", *NOW)
            .unwrap()
            .unwrap();
        let moved = tracker.recover_open_intents(*NOW).unwrap();
        assert_eq!(moved, vec!["test:1".to_string()]);
        let row = tracker.get_application("test:1").unwrap().unwrap();
        assert_eq!(row.state, State::NeedsGonza);
        assert!(
            row.needs_reason
                .as_deref()
                .unwrap()
                .starts_with(INTERRUPTED_PREFIX),
            "{:?}",
            row.needs_reason
        );
        assert_eq!(
            tracker.clicked_since(*NOW - TimeDelta::hours(1)).unwrap(),
            1,
            "the click may have happened: the daily cap and the company cooldown count it"
        );
        assert!(tracker.open_intents(INTENT_SUBMIT).unwrap().is_empty());
        assert!(
            tracker.recover_open_intents(*NOW).unwrap().is_empty(),
            "a second pass finds nothing"
        );
    }

    #[test]
    fn recovery_only_closes_the_intent_of_an_application_that_already_moved_on() {
        let tracker = tracker_with_prepared(&["1"]);
        tracker
            .begin_intent(INTENT_SUBMIT, "test:1", *NOW)
            .unwrap()
            .unwrap();
        // The process died after the state change and before closing the intent.
        tracker
            .transition("test:1", State::Submitted, None, *NOW)
            .unwrap();
        assert!(tracker.recover_open_intents(*NOW).unwrap().is_empty());
        assert_eq!(state_of(&tracker, "test:1"), State::Submitted);
        assert!(tracker.open_intents(INTENT_SUBMIT).unwrap().is_empty());
    }

    #[test]
    fn recovery_closes_the_intent_of_an_application_that_does_not_exist() {
        let tracker = Tracker::in_memory().unwrap();
        tracker
            .begin_intent(INTENT_SUBMIT, "ghost:9", *NOW)
            .unwrap()
            .unwrap();
        assert!(tracker.recover_open_intents(*NOW).unwrap().is_empty());
        assert!(tracker.open_intents(INTENT_SUBMIT).unwrap().is_empty());
    }
}
