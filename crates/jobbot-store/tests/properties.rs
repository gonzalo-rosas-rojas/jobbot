//! The state machine as the database keeps it, over generated sequences of moves: whatever is
//! asked, an application only ever takes the moves the state table allows, and its history says
//! exactly which ones it took.

use std::collections::BTreeMap;

use chrono::{DateTime, TimeDelta, Utc};
use jobbot_domain::fixtures::{NOW, make_job};
use jobbot_domain::state::State;
use jobbot_store::tracker::{Tracker, TrackerError};
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::select;

/// One attempt: the state asked for, whether it carries a note, and whether Gonza put the
/// application off just before.
type Attempt = (State, bool, bool);

/// What a move may change in the application's row.
type Row = (
    State,
    Option<String>,
    Option<String>,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

fn row(tracker: &Tracker, key: &str) -> Row {
    let app = tracker.get_application(key).unwrap().unwrap();
    (
        app.state,
        app.note,
        app.needs_reason,
        app.submitted_at,
        app.updated_at,
        app.snoozed_until,
    )
}

fn attempts() -> impl Strategy<Value = Vec<Attempt>> {
    vec(
        (select(State::ALL.to_vec()), any::<bool>(), any::<bool>()),
        0..32,
    )
}

proptest! {
    #[test]
    fn an_application_takes_exactly_the_moves_the_state_table_allows(attempts in attempts()) {
        let tracker = Tracker::in_memory().unwrap();
        let job = make_job();
        let key = job.key();
        tracker.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        tracker.create_application(&key, *NOW).unwrap();

        let mut state = State::Scored;
        let mut history = vec![(None, State::Scored)];
        let mut reason = None;
        let mut submitted_at = None;
        for (step, (target, with_note, snooze)) in attempts.into_iter().enumerate() {
            let at = *NOW + TimeDelta::minutes(step as i64 + 1);
            if snooze {
                tracker
                    .set_snoozed_until(&key, Some(at + TimeDelta::days(3)))
                    .unwrap();
            }
            let note = with_note.then(|| format!("note {step}"));
            let before = row(&tracker, &key);

            let result = tracker.transition(&key, target, note.as_deref(), at);
            let after = row(&tracker, &key);

            if state.allowed_next().contains(&target) {
                prop_assert!(result.is_ok(), "{} -> {}: {:?}", state, target, result);
                history.push((Some(state), target));
                if target == State::NeedsGonza {
                    reason = note.clone();
                } else {
                    reason = None;
                }
                if target == State::Submitted {
                    submitted_at = Some(at);
                }
                state = target;
                prop_assert_eq!(&after.1, &note, "the move's note");
                prop_assert_eq!(after.4, at, "a move updates the row");
                prop_assert_eq!(after.5, None, "a move ends a snooze");
            } else {
                prop_assert!(
                    matches!(result, Err(TrackerError::IllegalTransition { .. })),
                    "{} -> {}: {:?}", state, target, result
                );
                prop_assert_eq!(&after, &before, "a refused move changes nothing");
            }
            prop_assert_eq!(after.0, state);
            prop_assert_eq!(&after.2, &reason, "the reason belongs to needs_gonza only");
            prop_assert_eq!(after.3, submitted_at);
        }

        let events = tracker.events_for(&key).unwrap();
        let recorded: Vec<_> = events.iter().map(|event| (event.from_state, event.to_state)).collect();
        prop_assert_eq!(recorded, history);
        prop_assert!(events.windows(2).all(|pair| pair[0].at <= pair[1].at));
    }
}
