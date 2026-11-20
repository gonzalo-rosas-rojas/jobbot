//! Counting: the funnel, the events and the cohorts the panel and the reports ask for. Read-only.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use jobbot_domain::origin::Origin;
use jobbot_domain::region::region_signal;
use jobbot_domain::state::ANSWERED_STATES;
use rusqlite::{params, params_from_iter};

use super::{
    CohortRow, EventRow, FunnelRow, Funnels, MoveRow, Result, State, Tracker, from_text,
    optional_from_text, origin_from_text, placeholders, state_from_text, to_text,
};

/// The span the state changes of `moves_between` are read from (`?1` from, `?2` until), with the
/// join that leaves out an event whose posting is not stored. Its total, its rows and
/// `move_counts_between` must add up, so the three read this one text.
const MOVES_IN_SPAN: &str =
    "FROM events e JOIN jobs j ON j.key = e.job_key WHERE e.at >= ?1 AND e.at <= ?2";

/// What one application reached, with what the funnels group it by. Private to this file.
struct Reached {
    source: String,
    origin: Origin,
    title: String,
    location: String,
    remote: bool,
    sent: bool,
    answered: bool,
    rejected: bool,
    tests: bool,
    interviews: bool,
    offers: bool,
}

impl Tracker {
    /// Runs a COUNT query that lists states and then a `since` time: `sql` has `{marks}` where the
    /// `?` of the states go and one more `?` after them for `since`. No states, no count: 0.
    fn count_for_states_since(
        &self,
        sql: &str,
        states: &[State],
        since: DateTime<Utc>,
    ) -> Result<i64> {
        if states.is_empty() {
            return Ok(0);
        }
        let sql = sql.replace("{marks}", &placeholders(states.len()));
        let since_text = to_text(since);
        let names = states.iter().map(|state| state.as_str());
        let conn = self.lock();
        let count = conn.query_row(
            &sql,
            params_from_iter(names.chain(std::iter::once(since_text.as_str()))),
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Number of applications per state name.
    pub fn counts(&self) -> Result<BTreeMap<String, i64>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT state, COUNT(*) FROM applications GROUP BY state")?;
        let counts = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
        Ok(counts)
    }

    /// Submissions stamped on this UTC day.
    pub fn submitted_on(&self, day: NaiveDate) -> Result<i64> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM applications WHERE substr(submitted_at, 1, 10) = ?1",
            [day.to_string()],
            |row| row.get(0),
        )?)
    }

    /// Submissions that Gonza approved before they went out.
    pub fn approved_submitted_total(&self) -> Result<i64> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM applications WHERE approved = 1 AND submitted_at IS NOT NULL",
            [],
            |row| row.get(0),
        )?)
    }

    /// Transitions into any of `to_states` recorded at or after `since`.
    pub fn count_events(&self, to_states: &[State], since: DateTime<Utc>) -> Result<i64> {
        self.count_for_states_since(
            "SELECT COUNT(*) FROM events WHERE to_state IN ({marks}) AND at >= ?",
            to_states,
            since,
        )
    }

    /// The history of one application, oldest first. Empty when the key has none.
    pub fn events_for(&self, job_key: &str) -> Result<Vec<EventRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT at, from_state, to_state, note FROM events WHERE job_key = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([job_key], |row| {
            let at: String = row.get(0)?;
            let from: Option<String> = row.get(1)?;
            let to: String = row.get(2)?;
            Ok(EventRow {
                at: from_text(0, &at)?,
                from_state: from.map(|text| state_from_text(1, &text)).transpose()?,
                to_state: state_from_text(2, &to)?,
                note: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// State changes recorded from `from` to `until` (both included), newest first, with the
    /// company and title of the posting each one belongs to (an event whose posting is not stored
    /// is left out). Returns how many there are in that span and the first `limit` of them.
    pub fn moves_between(
        &self,
        from: DateTime<Utc>,
        until: DateTime<Utc>,
        limit: usize,
    ) -> Result<(i64, Vec<MoveRow>)> {
        let (from, until) = (to_text(from), to_text(until));
        let conn = self.lock();
        let total = conn.query_row(
            &format!("SELECT COUNT(*) {MOVES_IN_SPAN}"),
            [&from, &until],
            |row| row.get(0),
        )?;
        let mut stmt = conn.prepare(&format!(
            "SELECT e.at, e.job_key, j.company, j.title, e.to_state {MOVES_IN_SPAN} \
             ORDER BY e.at DESC, e.id DESC LIMIT ?3"
        ))?;
        let rows = stmt.query_map(
            params![from, until, i64::try_from(limit).unwrap_or(i64::MAX)],
            |row| {
                let at: String = row.get(0)?;
                let to: String = row.get(4)?;
                Ok(MoveRow {
                    at: from_text(0, &at)?,
                    job_key: row.get(1)?,
                    company: row.get(2)?,
                    title: row.get(3)?,
                    to_state: state_from_text(4, &to)?,
                })
            },
        )?;
        Ok((total, rows.collect::<rusqlite::Result<Vec<_>>>()?))
    }

    /// How many of the state changes `moves_between` counts went into each state, by state: the
    /// same span and the same join, so the counts add up to its total whatever its limit cuts.
    pub fn move_counts_between(
        &self,
        from: DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> Result<Vec<(State, i64)>> {
        let (from, until) = (to_text(from), to_text(until));
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT e.to_state, COUNT(*) {MOVES_IN_SPAN} GROUP BY e.to_state"
        ))?;
        let rows = stmt.query_map([&from, &until], |row| {
            let to: String = row.get(0)?;
            Ok((state_from_text(0, &to)?, row.get::<_, i64>(1)?))
        })?;
        let mut counts = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        counts.sort_by_key(|(state, _)| *state);
        Ok(counts)
    }

    /// Applications that reached any of `states` at or after `since`, each counted once even if it
    /// went through (or repeated) a state several times; `count_events` counts every event.
    pub fn jobs_reaching(&self, states: &[State], since: DateTime<Utc>) -> Result<i64> {
        self.count_for_states_since(
            "SELECT COUNT(DISTINCT job_key) FROM events WHERE to_state IN ({marks}) AND at >= ?",
            states,
            since,
        )
    }

    /// Applications submitted at or after `since` that have reached any of `states` (at any
    /// time since): the numerator of a response rate whose denominator is `submitted_since`, so
    /// the rate cannot pass 1.
    pub fn submitted_since_reaching(&self, since: DateTime<Utc>, states: &[State]) -> Result<i64> {
        self.count_for_states_since(
            "SELECT COUNT(*) FROM applications a \
             WHERE EXISTS (SELECT 1 FROM events e WHERE e.job_key = a.job_key \
                           AND e.to_state IN ({marks})) \
               AND a.submitted_at IS NOT NULL AND a.submitted_at >= ?",
            states,
            since,
        )
    }

    /// Applications that reached both `first` and `then` at some point: the base of a
    /// stage-to-stage rate ("of those that reached `first`, how many also reached `then`").
    pub fn jobs_reaching_both(&self, first: State, then: State) -> Result<i64> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT COUNT(DISTINCT e1.job_key) FROM events e1 \
             JOIN events e2 ON e2.job_key = e1.job_key \
             WHERE e1.to_state = ?1 AND e2.to_state = ?2",
            [first.as_str(), then.as_str()],
            |row| row.get(0),
        )?)
    }

    /// Applications clicked submit on at or after `since`: confirmed submissions plus attempts the
    /// page did not confirm. The gate's daily cap counts these.
    pub fn clicked_since(&self, since: DateTime<Utc>) -> Result<i64> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM applications WHERE submitted_at >= ?1 OR attempted_at >= ?1",
            [to_text(since)],
            |row| row.get(0),
        )?)
    }

    /// Every application whose posting is stored, with the stages it ever reached. `events` is read once, and
    /// only its moves into an answered state (few, found through the `to_state` index). The `JOIN jobs` stays:
    /// an application whose posting row is missing is in no funnel. `title` and `location` sit before
    /// `description` in `jobs`, so this does not walk the descriptions.
    fn reached_per_application(&self) -> Result<Vec<Reached>> {
        let marks = placeholders(ANSWERED_STATES.len());
        let sql = format!(
            "WITH reached AS ( \
               SELECT job_key, MAX(to_state = ?) AS tests, MAX(to_state = ?) AS interviews, \
                      MAX(to_state = ?) AS offers, MAX(to_state = ?) AS rejected \
               FROM events \
               WHERE to_state IN ({marks}) \
               GROUP BY job_key) \
             SELECT j.source, a.origin, j.title, j.location, j.remote, \
                    a.submitted_at IS NOT NULL, r.job_key IS NOT NULL, \
                    COALESCE(r.rejected, 0), COALESCE(r.tests, 0), COALESCE(r.interviews, 0), \
                    COALESCE(r.offers, 0) \
             FROM applications a JOIN jobs j ON j.key = a.job_key \
             LEFT JOIN reached r ON r.job_key = a.job_key"
        );
        // In the order of the four `?` of the SELECT, then the answered states of the WHERE.
        let stage_columns = [
            State::TechnicalTest,
            State::Interview,
            State::Offer,
            State::Rejected,
        ];
        let conn = self.lock();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params_from_iter(
                stage_columns
                    .iter()
                    .chain(ANSWERED_STATES.iter())
                    .map(|state| state.as_str()),
            ),
            |row| {
                Ok(Reached {
                    source: row.get(0)?,
                    origin: origin_from_text(1, &row.get::<_, String>(1)?)?,
                    title: row.get(2)?,
                    location: row.get(3)?,
                    remote: row.get(4)?,
                    sent: row.get(5)?,
                    answered: row.get(6)?,
                    rejected: row.get(7)?,
                    tests: row.get(8)?,
                    interviews: row.get(9)?,
                    offers: row.get(10)?,
                })
            },
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The funnel of `reached` grouped by `group`, sorted by group name. The stages mean what
    /// `submitted_since` and `jobs_reaching` mean for the whole funnel: `sent` is a submission stamp, the rest
    /// "reached that state at least once"; `n` is every application of the group, sent or not, so the page can
    /// show a base with each count.
    fn grouped(reached: &[Reached], group: impl Fn(&Reached) -> String) -> Vec<FunnelRow> {
        let mut groups: BTreeMap<String, FunnelRow> = BTreeMap::new();
        for reached in reached {
            let name = group(reached);
            let line = groups.entry(name.clone()).or_insert_with(|| FunnelRow {
                group: name,
                n: 0,
                sent: 0,
                answered: 0,
                rejected: 0,
                tests: 0,
                interviews: 0,
                offers: 0,
            });
            line.n += 1;
            line.sent += i64::from(reached.sent);
            line.answered += i64::from(reached.answered);
            line.rejected += i64::from(reached.rejected);
            line.tests += i64::from(reached.tests);
            line.interviews += i64::from(reached.interviews);
            line.offers += i64::from(reached.offers);
        }
        groups.into_values().collect()
    }

    fn by_source_of(reached: &[Reached]) -> Vec<FunnelRow> {
        Self::grouped(reached, |r| r.source.clone())
    }

    fn by_origin_of(reached: &[Reached]) -> Vec<FunnelRow> {
        Self::grouped(reached, |r| r.origin.as_str().to_string())
    }

    fn by_region_of(reached: &[Reached]) -> Vec<FunnelRow> {
        Self::grouped(reached, |r| {
            region_signal(&r.title, &r.location, r.remote)
                .as_str()
                .to_string()
        })
    }

    /// The all-time funnel split by the source the posting came from, sorted by source name.
    pub fn funnel_by_source(&self) -> Result<Vec<FunnelRow>> {
        Ok(Self::by_source_of(&self.reached_per_application()?))
    }

    /// The all-time funnel split by how each application came about (`Origin`), sorted by stored name. Only
    /// the origins that have applications appear.
    pub fn funnel_by_origin(&self) -> Result<Vec<FunnelRow>> {
        Ok(Self::by_origin_of(&self.reached_per_application()?))
    }

    /// The all-time funnel split by `region::region_signal` of the posting's title, location and remote flag.
    /// Nothing is stored: a change to the region rules changes this split for every application at once. A
    /// posting in one of the criteria's extra open places shows as `other`: the store does not know the criteria.
    pub fn funnel_by_region(&self) -> Result<Vec<FunnelRow>> {
        Ok(Self::by_region_of(&self.reached_per_application()?))
    }

    /// The three funnels from one read of the applications: what the panel asks on every poll.
    pub fn funnels(&self) -> Result<Funnels> {
        let reached = self.reached_per_application()?;
        Ok(Funnels {
            by_source: Self::by_source_of(&reached),
            by_origin: Self::by_origin_of(&reached),
            by_region: Self::by_region_of(&reached),
        })
    }

    /// Submissions stamped at or after `since`.
    pub fn submitted_since(&self, since: DateTime<Utc>) -> Result<i64> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM applications WHERE submitted_at IS NOT NULL AND submitted_at >= ?1",
            [to_text(since)],
            |row| row.get(0),
        )?)
    }

    /// Every posting first seen at or after `since`, with its application when it has one
    /// (`jobs LEFT JOIN applications`, one query). Withdrawn applications are in too.
    pub fn cohort_since(&self, since: DateTime<Utc>) -> Result<Vec<CohortRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT j.score, a.state, a.cv_path, a.screenshot_path, a.submitted_at \
             FROM jobs j LEFT JOIN applications a ON a.job_key = j.key \
             WHERE j.first_seen >= ?1 ORDER BY j.first_seen, j.key",
        )?;
        let rows = stmt.query_map([to_text(since)], |row| {
            let state: Option<String> = row.get(1)?;
            let submitted: Option<String> = row.get(4)?;
            Ok(CohortRow {
                score: row.get(0)?,
                state: state.map(|text| state_from_text(1, &text)).transpose()?,
                cv_path: row.get(2)?,
                screenshot_path: row.get(3)?,
                submitted_at: optional_from_text(4, submitted)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use chrono::{DateTime, TimeDelta, Utc};
    use jobbot_domain::fixtures::{NOW, make_job, make_job_with};
    use jobbot_domain::origin::Origin;

    use crate::tracker::test_support::{stored_at, submit, submit_approved, three_scored};
    use crate::tracker::{FunnelRow, State, Tracker, to_text};

    #[test]
    fn approved_submitted_total_counts_only_approved() {
        let t = Tracker::in_memory().unwrap();
        submit_approved(
            &t,
            &make_job_with(|j| {
                j.external_id = "1".into();
                j.company = "A".into();
            }),
        );
        submit(
            &t,
            &make_job_with(|j| {
                j.external_id = "2".into();
                j.company = "B".into();
            }),
        );
        assert_eq!(t.approved_submitted_total().unwrap(), 1);
    }

    #[test]
    fn counts_groups_by_state() {
        let t = Tracker::in_memory().unwrap();
        three_scored(&t, *NOW);
        assert_eq!(
            t.counts().unwrap(),
            BTreeMap::from([("scored".to_string(), 3)])
        );
    }

    #[test]
    fn count_events_counts_inside_the_window() {
        let now = *NOW;
        let t = Tracker::in_memory().unwrap();
        three_scored(&t, now);
        assert_eq!(
            t.count_events(&[State::Scored], now - TimeDelta::days(1))
                .unwrap(),
            3
        );
        assert_eq!(
            t.count_events(&[State::Scored], now + TimeDelta::days(1))
                .unwrap(),
            0
        );
        assert_eq!(t.count_events(&[], now).unwrap(), 0);
    }

    #[test]
    fn submitted_since_is_zero_while_nothing_was_submitted() {
        let t = Tracker::in_memory().unwrap();
        three_scored(&t, *NOW);
        assert_eq!(t.submitted_since(*NOW - TimeDelta::days(7)).unwrap(), 0);
    }

    #[test]
    fn events_for_lists_one_applications_history_oldest_first() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        let other = make_job_with(|j| j.external_id = "2".into());
        for j in [&job, &other] {
            t.add_job(j, 70.0, &BTreeMap::new(), *NOW).unwrap();
            t.create_application(&j.key(), *NOW).unwrap();
        }
        t.transition(&job.key(), State::Prepared, None, *NOW)
            .unwrap();
        t.transition(&other.key(), State::Prepared, None, *NOW)
            .unwrap();
        let later = *NOW + TimeDelta::minutes(5);
        t.transition(&job.key(), State::NeedsGonza, Some("falta email"), later)
            .unwrap();
        let events = t.events_for(&job.key()).unwrap();
        let states: Vec<State> = events.iter().map(|e| e.to_state).collect();
        assert_eq!(
            states,
            vec![State::Scored, State::Prepared, State::NeedsGonza]
        );
        assert_eq!(events[0].from_state, None);
        assert_eq!(events[2].from_state, Some(State::Prepared));
        assert_eq!(events[2].note.as_deref(), Some("falta email"));
        assert_eq!(events[2].at, later);
        assert!(t.events_for("nope:1").unwrap().is_empty());
    }

    #[test]
    fn jobs_reaching_counts_each_application_once() {
        let t = Tracker::in_memory().unwrap();
        let walk = |id: &str, states: &[State]| {
            let job = make_job_with(|j| j.external_id = id.into());
            t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
            t.create_application(&job.key(), *NOW).unwrap();
            for state in states {
                t.transition(&job.key(), *state, None, *NOW).unwrap();
            }
        };
        walk(
            "1",
            &[
                State::Prepared,
                State::Submitted,
                State::Interview,
                State::Interview, // a second round
            ],
        );
        walk("2", &[State::Prepared, State::Submitted, State::Interview]);
        walk("3", &[State::Prepared, State::Submitted]);
        let since = *NOW - TimeDelta::days(1);
        assert_eq!(t.jobs_reaching(&[State::Interview], since).unwrap(), 2);
        assert_eq!(
            t.count_events(&[State::Interview], since).unwrap(),
            3,
            "events count the second round too"
        );
        assert_eq!(
            t.jobs_reaching(&[State::Interview, State::Submitted], since)
                .unwrap(),
            3
        );
        assert_eq!(t.jobs_reaching(&[], since).unwrap(), 0);
        assert_eq!(
            t.jobs_reaching(&[State::Interview], *NOW + TimeDelta::days(1))
                .unwrap(),
            0
        );
    }

    #[test]
    fn the_cohort_is_what_was_first_seen_since_with_its_application_when_it_has_one() {
        let t = Tracker::in_memory().unwrap();
        let since = *NOW - TimeDelta::days(7);
        stored_at(
            &t,
            "old",
            90.0,
            since - TimeDelta::seconds(1),
            &[State::Prepared],
        );
        stored_at(&t, "edge", 40.5, since, &[]);
        let sent = stored_at(
            &t,
            "sent",
            75.0,
            *NOW - TimeDelta::days(2),
            &[State::Prepared, State::Submitted],
        );
        t.set_cv_path(&sent, Path::new("out/cv.pdf")).unwrap();
        t.set_screenshot(&sent, Path::new("shots/a.png")).unwrap();
        stored_at(&t, "waiting", 62.0, *NOW, &[State::NeedsGonza]);

        let rows = t.cohort_since(since).unwrap();

        assert_eq!(
            rows.len(),
            3,
            "the posting from before the cut is not in it"
        );
        let by_score = |score: f64| rows.iter().find(|r| r.score == score).unwrap();
        let edge = by_score(40.5);
        assert!(
            edge.state.is_none() && edge.cv_path.is_none() && edge.submitted_at.is_none(),
            "a posting without an application still counts, with no application"
        );
        let sent_row = by_score(75.0);
        assert_eq!(sent_row.state, Some(State::Submitted));
        assert_eq!(sent_row.cv_path.as_deref(), Some("out/cv.pdf"));
        assert_eq!(sent_row.screenshot_path.as_deref(), Some("shots/a.png"));
        assert_eq!(sent_row.submitted_at, Some(*NOW - TimeDelta::days(2)));
        let waiting = by_score(62.0);
        assert_eq!(waiting.state, Some(State::NeedsGonza));
        assert!(waiting.submitted_at.is_none());
        assert!(
            t.cohort_since(*NOW + TimeDelta::days(1))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn moves_between_names_the_posting_newest_first_and_counts_what_the_limit_cut() {
        let t = Tracker::in_memory().unwrap();
        let from = *NOW - TimeDelta::hours(24);
        let minute = TimeDelta::minutes(1);
        // Before the span: not a move of it.
        let early = stored_at(&t, "early", 50.0, from - minute, &[State::Prepared]);
        let a = stored_at(&t, "a", 50.0, from, &[]);
        t.create_application(&a, from).unwrap();
        t.transition(&a, State::Prepared, None, from + minute * 10)
            .unwrap();
        let b = stored_at(&t, "b", 50.0, from + minute, &[State::Prepared]);
        t.transition(&b, State::Submitted, None, *NOW).unwrap();
        // After `until`: not a move of it either.
        t.transition(&early, State::Submitted, None, *NOW + minute)
            .unwrap();
        // An event whose posting is gone from `jobs` has no company or title to show.
        t.lock()
            .execute(
                "INSERT INTO events (job_key, at, from_state, to_state) \
                 VALUES ('ghost:1', ?1, NULL, 'scored')",
                [to_text(*NOW)],
            )
            .unwrap();

        let (total, moves) = t.moves_between(from, *NOW, 100).unwrap();

        // a: scored at `from`, prepared at +10; b: scored and prepared at +1, submitted at NOW.
        assert_eq!(total, 5);
        let seen: Vec<(&str, &str, &str, DateTime<Utc>)> = moves
            .iter()
            .map(|m| {
                (
                    m.company.as_str(),
                    m.title.as_str(),
                    m.to_state.as_str(),
                    m.at,
                )
            })
            .collect();
        assert_eq!(
            seen,
            vec![
                ("Co b", "Role b", "submitted", *NOW),
                ("Co a", "Role a", "prepared", from + minute * 10),
                ("Co b", "Role b", "prepared", from + minute),
                ("Co b", "Role b", "scored", from + minute),
                ("Co a", "Role a", "scored", from),
            ],
            "newest first; a tie keeps the later event first"
        );
        assert_eq!(moves[0].job_key, b);

        let (total, cut) = t.moves_between(from, *NOW, 2).unwrap();
        assert_eq!((total, cut.len()), (5, 2));
        assert_eq!(cut, moves[..2]);
        let (total, none) = t
            .moves_between(*NOW + TimeDelta::days(1), *NOW + TimeDelta::days(2), 5)
            .unwrap();
        assert_eq!((total, none.len()), (0, 0));
    }

    #[test]
    fn move_counts_between_group_the_same_span_by_state_and_add_up_to_its_total() {
        let t = Tracker::in_memory().unwrap();
        let from = *NOW - TimeDelta::hours(24);
        let minute = TimeDelta::minutes(1);
        assert!(t.move_counts_between(from, *NOW).unwrap().is_empty());
        // Before the span: not counted.
        let early = stored_at(&t, "early", 50.0, from - minute, &[State::Prepared]);
        let a = stored_at(&t, "a", 50.0, from, &[]);
        t.create_application(&a, from).unwrap();
        t.transition(&a, State::Prepared, None, from + minute * 10)
            .unwrap();
        let b = stored_at(&t, "b", 50.0, from + minute, &[State::Prepared]);
        t.transition(&b, State::Submitted, None, *NOW).unwrap();
        // After `until`: not counted either.
        t.transition(&early, State::Submitted, None, *NOW + minute)
            .unwrap();
        // An event whose posting is gone from `jobs` is left out, as `moves_between` leaves it.
        t.lock()
            .execute(
                "INSERT INTO events (job_key, at, from_state, to_state) \
                 VALUES ('ghost:1', ?1, NULL, 'scored')",
                [to_text(*NOW)],
            )
            .unwrap();

        let counts = t.move_counts_between(from, *NOW).unwrap();

        assert_eq!(
            counts,
            vec![
                (State::Scored, 2),
                (State::Prepared, 2),
                (State::Submitted, 1)
            ]
        );
        let (total, _) = t.moves_between(from, *NOW, 1).unwrap();
        assert_eq!(counts.iter().map(|(_, n)| n).sum::<i64>(), total);
    }

    #[test]
    fn the_funnel_by_source_counts_each_application_once_per_stage_it_reached() {
        let t = Tracker::in_memory().unwrap();
        let walk = |source: &str, id: &str, states: &[State]| {
            let job = make_job_with(|j| {
                j.source = source.into();
                j.external_id = id.into();
            });
            t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
            t.create_application(&job.key(), *NOW).unwrap();
            for (n, state) in states.iter().enumerate() {
                t.transition(
                    &job.key(),
                    *state,
                    None,
                    *NOW + TimeDelta::minutes(n as i64),
                )
                .unwrap();
            }
        };
        walk("ashby", "1", &[State::Prepared]);
        walk(
            "ashby",
            "2",
            &[State::Prepared, State::Submitted, State::Rejected],
        );
        walk(
            "ashby",
            "3",
            &[
                State::Prepared,
                State::Submitted,
                State::Replied,
                State::Interview,
                State::Interview,
                State::Offer,
            ],
        );
        walk(
            "lever",
            "4",
            &[State::Prepared, State::Submitted, State::TechnicalTest],
        );
        walk("lever", "5", &[State::Prepared, State::Withdrawn]);
        // A source none of whose applications got an answer counts zeros, not nothing.
        walk("workable", "6", &[State::Prepared, State::Submitted]);

        let by_source = t.funnel_by_source().unwrap();
        let sources: Vec<&str> = by_source.iter().map(|f| f.group.as_str()).collect();
        assert_eq!(sources, ["ashby", "lever", "workable"], "sorted by name");
        let stages = |row: &FunnelRow| {
            (
                row.n,
                row.sent,
                row.answered,
                row.rejected,
                row.tests,
                row.interviews,
                row.offers,
            )
        };
        assert_eq!(stages(&by_source[0]), (3, 2, 2, 1, 0, 1, 1), "ashby");
        assert_eq!(stages(&by_source[1]), (2, 1, 1, 0, 1, 0, 0), "lever");
        assert_eq!(stages(&by_source[2]), (1, 1, 0, 0, 0, 0, 0), "workable");
        assert!(
            Tracker::in_memory()
                .unwrap()
                .funnel_by_source()
                .unwrap()
                .is_empty()
        );
    }

    /// Stores a posting from `source` at `location` (remote), starts its application with `origin` and walks it
    /// through `states`.
    fn walk_to(
        t: &Tracker,
        id: &str,
        source: &str,
        location: &str,
        origin: Origin,
        states: &[State],
    ) {
        let job = make_job_with(|j| {
            j.source = source.into();
            j.external_id = id.into();
            j.location = location.into();
            j.remote = true;
        });
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        t.set_origin(&job.key(), origin).unwrap();
        for (n, state) in states.iter().enumerate() {
            t.transition(
                &job.key(),
                *state,
                None,
                *NOW + TimeDelta::minutes(n as i64),
            )
            .unwrap();
        }
    }

    /// A mixed data set: three origins, three regions, several stages.
    fn mixed_funnel_data() -> Tracker {
        let t = Tracker::in_memory().unwrap();
        let sent = [State::Prepared, State::Submitted];
        walk_to(&t, "1", "ashby", "Remote - LATAM", Origin::Cold, &sent);
        walk_to(
            &t,
            "2",
            "ashby",
            "Remote",
            Origin::Referral,
            &[State::Prepared, State::Submitted, State::Rejected],
        );
        walk_to(
            &t,
            "3",
            "lever",
            "Remote - US",
            Origin::Cold,
            &[
                State::Prepared,
                State::Submitted,
                State::Interview,
                State::Offer,
            ],
        );
        walk_to(&t, "4", "lever", "Remote", Origin::Unknown, &[]);
        t
    }

    #[test]
    fn a_rejection_counts_as_answered_and_as_rejected_once() {
        let t = Tracker::in_memory().unwrap();
        walk_to(
            &t,
            "1",
            "ashby",
            "Remote",
            Origin::Cold,
            &[State::Prepared, State::Submitted, State::Rejected],
        );
        walk_to(
            &t,
            "2",
            "ashby",
            "Remote",
            Origin::Cold,
            &[
                State::Prepared,
                State::Submitted,
                State::Ghosted,
                State::Rejected,
            ],
        );
        let rows = t.funnel_by_source().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].answered, rows[0].rejected), (2, 2));
    }

    #[test]
    fn the_funnel_by_origin_groups_by_the_stored_origin() {
        let t = Tracker::in_memory().unwrap();
        walk_to(&t, "1", "ashby", "Remote", Origin::Cold, &[]);
        walk_to(&t, "2", "ashby", "Remote", Origin::Cold, &[]);
        walk_to(&t, "3", "ashby", "Remote", Origin::Referral, &[]);
        let rows = t.funnel_by_origin().unwrap();
        let shape: Vec<(&str, i64)> = rows.iter().map(|r| (r.group.as_str(), r.n)).collect();
        assert_eq!(shape, [("cold", 2), ("referral", 1)]);
    }

    #[test]
    fn the_funnel_by_region_reads_the_posting_and_stores_nothing() {
        let t = Tracker::in_memory().unwrap();
        walk_to(&t, "1", "ashby", "Remote - LATAM", Origin::Cold, &[]);
        walk_to(&t, "2", "ashby", "Remote", Origin::Cold, &[]);
        walk_to(&t, "3", "ashby", "Remote - US", Origin::Cold, &[]);
        let groups = |rows: Vec<FunnelRow>| -> Vec<(String, i64)> {
            rows.into_iter().map(|r| (r.group, r.n)).collect()
        };
        assert_eq!(
            groups(t.funnel_by_region().unwrap()),
            [
                ("open".to_string(), 1),
                ("other".to_string(), 1),
                ("silent".to_string(), 1)
            ]
        );
        // The posting is read again with a place that names an open region: the split moves, with no
        // write to `applications`.
        let silent = make_job_with(|j| {
            j.source = "ashby".into();
            j.external_id = "2".into();
            j.location = "Worldwide".into();
            j.remote = true;
        });
        t.refresh_posting(&silent).unwrap();
        assert_eq!(
            groups(t.funnel_by_region().unwrap()),
            [("open".to_string(), 2), ("other".to_string(), 1)]
        );
    }

    #[test]
    fn the_three_funnels_count_each_group_as_the_seeded_rows_say() {
        // mixed_funnel_data seeds four applications; the expected lines are counted by hand from them:
        //   1 ashby   LATAM   cold      sent
        //   2 ashby   Remote  referral  sent, rejected
        //   3 lever   US      cold      sent, interview, offer
        //   4 lever   Remote  unknown   not sent
        let t = mixed_funnel_data();
        let line = |rows: &[FunnelRow], group: &str| -> [i64; 7] {
            let row = rows.iter().find(|r| r.group == group).expect(group);
            [
                row.n,
                row.sent,
                row.answered,
                row.rejected,
                row.tests,
                row.interviews,
                row.offers,
            ]
        };
        let source = t.funnel_by_source().unwrap();
        assert_eq!(line(&source, "ashby"), [2, 2, 1, 1, 0, 0, 0]);
        assert_eq!(line(&source, "lever"), [2, 1, 1, 0, 0, 1, 1]);
        let origin = t.funnel_by_origin().unwrap();
        assert_eq!(line(&origin, "cold"), [2, 2, 1, 0, 0, 1, 1]);
        assert_eq!(line(&origin, "referral"), [1, 1, 1, 1, 0, 0, 0]);
        assert_eq!(line(&origin, "unknown"), [1, 0, 0, 0, 0, 0, 0]);
        let region = t.funnel_by_region().unwrap();
        assert_eq!(line(&region, "open"), [1, 1, 0, 0, 0, 0, 0]);
        assert_eq!(line(&region, "silent"), [2, 1, 1, 1, 0, 0, 0]);
        assert_eq!(line(&region, "other"), [1, 1, 1, 0, 0, 1, 1]);
        // And the one-read version gives the same three.
        let all = t.funnels().unwrap();
        assert_eq!(
            (all.by_source, all.by_origin, all.by_region),
            (source, origin, region)
        );
    }

    #[test]
    fn every_funnel_of_an_empty_database_is_empty() {
        let t = Tracker::in_memory().unwrap();
        assert!(t.funnel_by_source().unwrap().is_empty());
        assert!(t.funnel_by_origin().unwrap().is_empty());
        assert!(t.funnel_by_region().unwrap().is_empty());
    }

    #[test]
    fn an_application_whose_posting_is_missing_is_in_no_funnel() {
        let t = mixed_funnel_data();
        let before = (
            t.funnel_by_source().unwrap(),
            t.funnel_by_origin().unwrap(),
            t.funnel_by_region().unwrap(),
        );
        {
            let conn = t.lock();
            conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
            conn.execute(
                "INSERT INTO applications (job_key, company_norm, state, updated_at)
                 VALUES ('gone:1', 'acme', 'scored', '2026-10-01T12:00:00Z')",
                [],
            )
            .unwrap();
        }
        let after = (
            t.funnel_by_source().unwrap(),
            t.funnel_by_origin().unwrap(),
            t.funnel_by_region().unwrap(),
        );
        assert_eq!(before, after);
    }

    #[test]
    fn every_answered_state_counts_as_answered_in_the_funnel() {
        let t = Tracker::in_memory().unwrap();
        for (n, state) in jobbot_domain::state::ANSWERED_STATES.iter().enumerate() {
            let job = make_job_with(|j| {
                j.source = "ashby".into();
                j.external_id = format!("answered-{n}");
            });
            t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
            t.create_application(&job.key(), *NOW).unwrap();
            // An offer can only follow an interview.
            let mut path = vec![State::Prepared, State::Submitted];
            if *state == State::Offer {
                path.push(State::Interview);
            }
            path.push(*state);
            for (step, next) in path.into_iter().enumerate() {
                t.transition(
                    &job.key(),
                    next,
                    None,
                    *NOW + TimeDelta::minutes(step as i64),
                )
                .unwrap();
            }
        }
        let rows = t.funnel_by_source().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].answered as usize,
            jobbot_domain::state::ANSWERED_STATES.len(),
            "one application per answered state, each counted as answered"
        );
    }
}
