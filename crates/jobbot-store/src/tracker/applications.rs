//! The application row: creating it, its columns and the lists the queue and the panel read.

use std::path::Path;

use chrono::{DateTime, TimeDelta, Utc};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params, params_from_iter};

use jobbot_domain::gate::{REGION_OVERRIDE_PREFIX, region_override_note};
use jobbot_domain::origin::Origin;
use jobbot_domain::outbound::plain_line;

use super::{
    ApplicationRow, Result, State, Tracker, TrackerError, from_text, optional_from_text,
    origin_from_text, placeholders, state_from_text, to_text,
};

/// The states of an application that is not finished and was never sent: the ones
/// `unsent_applications` reads.
const UNSENT_STATES: [State; 3] = [State::Scored, State::Prepared, State::NeedsGonza];

/// The events that are a move out of `needs_gonza` back to the queue, as an SQL condition on `events`: the only
/// moves `resolve` makes when Gonza sends a parked application on, and so the only ones that can carry an override.
const BACK_TO_THE_QUEUE: &str = "from_state = 'needs_gonza' AND to_state IN ('scored', 'prepared')";

/// The states of an application waiting in the queue: the ones `top_scored` reads.
const QUEUED_STATES: [State; 2] = [State::Scored, State::Prepared];

/// `state_since` is the latest event into the row's current state: an application that went to
/// `needs_gonza`, back to the queue and to `needs_gonza` again has waited since the second time.
const APPLICATION_SELECT: &str = "SELECT a.job_key, a.company_norm, a.state, a.note, a.needs_reason, \
     a.cv_path, a.screenshot_path, a.approved, a.submitted_at, a.updated_at, \
     j.title, j.company, j.score, j.url, j.source, \
     a.snoozed_until, j.posted_at, j.first_seen, j.salary_min, j.salary_max, \
     (SELECT MAX(e.at) FROM events e WHERE e.job_key = a.job_key AND e.to_state = a.state) \
     FROM applications a JOIN jobs j ON j.key = a.job_key";

fn application_from_row(row: &Row<'_>) -> rusqlite::Result<ApplicationRow> {
    let state_text: String = row.get(2)?;
    let submitted_text: Option<String> = row.get(8)?;
    let updated_text: String = row.get(9)?;
    Ok(ApplicationRow {
        job_key: row.get(0)?,
        company_norm: row.get(1)?,
        state: state_from_text(2, &state_text)?,
        note: row.get(3)?,
        needs_reason: row.get(4)?,
        cv_path: row.get(5)?,
        screenshot_path: row.get(6)?,
        approved: row.get(7)?,
        submitted_at: optional_from_text(8, submitted_text)?,
        updated_at: from_text(9, &updated_text)?,
        title: row.get(10)?,
        company: row.get(11)?,
        score: row.get(12)?,
        url: row.get(13)?,
        source: row.get(14)?,
        snoozed_until: optional_from_text(15, row.get(15)?)?,
        posted_at: optional_from_text(16, row.get(16)?)?,
        first_seen: from_text(17, &row.get::<_, String>(17)?)?,
        salary_min: row.get(18)?,
        salary_max: row.get(19)?,
        state_since: optional_from_text(20, row.get(20)?)?,
    })
}

fn query_applications(
    conn: &Connection,
    tail: &str,
    params: impl rusqlite::Params,
) -> rusqlite::Result<Vec<ApplicationRow>> {
    let mut stmt = conn.prepare(&format!("{APPLICATION_SELECT} {tail}"))?;
    let rows = stmt.query_map(params, application_from_row)?;
    rows.collect()
}

impl Tracker {
    /// Starts an application at `Scored` with the origin it begins with. Discovery passes `Cold`; `record` passes
    /// what it decided. An application that already exists keeps the origin it has. The column's default stays
    /// `'cold'` so a running older process, whose INSERT names no origin, still inserts. The one way the code that
    /// opens real applications does it.
    pub fn start_application(
        &self,
        job_key: &str,
        origin: Origin,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM applications WHERE job_key = ?1)",
            [job_key],
            |row| row.get(0),
        )?;
        if exists {
            return Ok(());
        }
        let company_norm: Option<String> = tx
            .query_row(
                "SELECT company_norm FROM jobs WHERE key = ?1",
                [job_key],
                |row| row.get(0),
            )
            .optional()?;
        let Some(company_norm) = company_norm else {
            return Err(TrackerError::UnknownKey(job_key.to_string()));
        };
        let at = to_text(now);
        tx.execute(
            "INSERT INTO applications (job_key, company_norm, state, updated_at, origin) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                job_key,
                company_norm,
                State::Scored.as_str(),
                at,
                origin.as_str()
            ],
        )?;
        tx.execute(
            "INSERT INTO events (job_key, at, from_state, to_state, note) VALUES (?1, ?2, NULL, ?3, NULL)",
            params![job_key, at, State::Scored.as_str()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_application(&self, job_key: &str) -> Result<Option<ApplicationRow>> {
        let conn = self.lock();
        let mut rows = query_applications(&conn, "WHERE a.job_key = ?1", [job_key])?;
        Ok(rows.pop())
    }

    /// `UPDATE applications SET <set> WHERE job_key = <job_key>`, where `<set>` uses `?1..` for
    /// `params` and the key is bound as the parameter after them; no such application is an error.
    fn update_application(
        &self,
        set: &str,
        params: &[&dyn rusqlite::ToSql],
        job_key: &str,
    ) -> Result<()> {
        let key_slot = params.len() + 1;
        let mut all = params.to_vec();
        all.push(&job_key);
        let conn = self.lock();
        let changed = conn.execute(
            &format!("UPDATE applications SET {set} WHERE job_key = ?{key_slot}"),
            all.as_slice(),
        )?;
        if changed == 0 {
            return Err(TrackerError::UnknownKey(job_key.to_string()));
        }
        Ok(())
    }

    fn update_column(&self, column: &str, value: Option<String>, job_key: &str) -> Result<()> {
        self.update_application(&format!("{column} = ?1"), &[&value], job_key)
    }

    pub fn set_cv_path(&self, job_key: &str, path: &Path) -> Result<()> {
        self.update_column(
            "cv_path",
            Some(path.to_string_lossy().into_owned()),
            job_key,
        )
    }

    pub fn set_screenshot(&self, job_key: &str, path: &Path) -> Result<()> {
        self.update_column(
            "screenshot_path",
            Some(path.to_string_lossy().into_owned()),
            job_key,
        )
    }

    /// Stamps the moment the program clicked submit on a form whose page then did not confirm it.
    /// The click may have gone through, so the daily cap and the company cooldown count it
    /// (`clicked_since`, `company_in_cooldown`) while the digest and the review counter do not.
    pub fn record_attempt(&self, job_key: &str, now: DateTime<Utc>) -> Result<()> {
        self.update_column("attempted_at", Some(to_text(now)), job_key)
    }

    /// For `resolve`: Gonza decided the form must be filled and sent again, so the screenshot of
    /// the earlier try (the next run takes a fresh one) and the attempt stamp are forgotten.
    pub fn clear_for_retry(&self, job_key: &str) -> Result<()> {
        self.update_application("screenshot_path = NULL, attempted_at = NULL", &[], job_key)
    }

    /// Puts an application off until `until` (`None` brings it back). The row keeps its state:
    /// only the panel's queues look at this.
    pub fn set_snoozed_until(&self, job_key: &str, until: Option<DateTime<Utc>>) -> Result<()> {
        self.update_column("snoozed_until", until.map(to_text), job_key)
    }

    /// How the application came about, or `None` when there is no application for `job_key`.
    pub fn origin_of(&self, job_key: &str) -> Result<Option<Origin>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT origin FROM applications WHERE job_key = ?1",
                [job_key],
                |row| origin_from_text(0, &row.get::<_, String>(0)?),
            )
            .optional()?)
    }

    /// Sets how the application came about. False when there is no application for `job_key` (nothing written).
    /// Nothing else of the row changes: not `updated_at`, not the state, no event. The origin is a fact about how
    /// the application began, not a move, and the board orders by `updated_at`.
    pub fn set_origin(&self, job_key: &str, origin: Origin) -> Result<bool> {
        let conn = self.lock();
        let changed = conn.execute(
            "UPDATE applications SET origin = ?1 WHERE job_key = ?2",
            params![origin.as_str(), job_key],
        )?;
        Ok(changed > 0)
    }

    /// Whether Gonza cleared the region of this application as it stands: some move of it carries the note
    /// `region_override_note` builds from the `location` and `remote` the stored posting has now (the move's note went
    /// through `plain_line`, so the expected one does too). The fact is the event, not a column, so it outlives every
    /// later move (the row's own note is overwritten by each); and because the note holds what he saw, a refresh that
    /// changes the location or the remote flag makes it stop applying. Only the move out of `needs_gonza` back to the
    /// queue counts (`resolve` makes it), so a note typed on any other move can never be an override.
    pub fn region_override(&self, job_key: &str) -> Result<bool> {
        let conn = self.lock();
        let seen: Option<(String, bool)> = conn
            .query_row(
                "SELECT location, remote FROM jobs WHERE key = ?1",
                [job_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((location, remote)) = seen else {
            return Ok(false);
        };
        let expected = plain_line(&region_override_note(&location, remote));
        Ok(conn.query_row(
            &format!(
                "SELECT EXISTS(SELECT 1 FROM events WHERE job_key = ?1 AND note = ?2 AND {BACK_TO_THE_QUEUE})"
            ),
            params![job_key, expected],
            |row| row.get(0),
        )?)
    }

    /// Whether Gonza EVER sent this application back past the region check, whatever the posting said then or says
    /// now: some move out of `needs_gonza` back to the queue has a note that starts with `REGION_OVERRIDE_PREFIX`
    /// (the notes of an older build count too: any text after the prefix). The prune uses it to park such an
    /// application again when its posting changed, instead of withdrawing it.
    pub fn had_region_override(&self, job_key: &str) -> Result<bool> {
        let conn = self.lock();
        Ok(conn.query_row(
            &format!(
                "SELECT EXISTS(SELECT 1 FROM events WHERE job_key = ?1 AND substr(note, 1, ?2) = ?3 AND {BACK_TO_THE_QUEUE})"
            ),
            params![
                job_key,
                REGION_OVERRIDE_PREFIX.len() as i64,
                REGION_OVERRIDE_PREFIX
            ],
            |row| row.get(0),
        )?)
    }

    /// Marks an application as reviewed and approved by Gonza.
    pub fn approve(&self, job_key: &str) -> Result<()> {
        self.update_application("approved = 1", &[], job_key)
    }

    /// `start_application` as `Cold`, for fixtures and tests that do not care how the application came about. Not
    /// for the code that opens real applications: that names the origin with `start_application`.
    pub fn start_cold_for_tests(&self, job_key: &str, now: DateTime<Utc>) -> Result<()> {
        self.start_application(job_key, Origin::Cold, now)
    }

    // ---- queries

    /// Applications in `state`, best score first.
    pub fn list_by_state(&self, state: State) -> Result<Vec<ApplicationRow>> {
        let conn = self.lock();
        Ok(query_applications(
            &conn,
            "WHERE a.state = ?1 ORDER BY j.score DESC, a.job_key",
            [state.as_str()],
        )?)
    }

    /// The first `n` applications of `list_by_state(state)`: a board column that shows only its
    /// first cards need not read the rest.
    pub fn first_by_state(&self, state: State, n: usize) -> Result<Vec<ApplicationRow>> {
        let conn = self.lock();
        let limit = i64::try_from(n).unwrap_or(i64::MAX);
        Ok(query_applications(
            &conn,
            "WHERE a.state = ?1 ORDER BY j.score DESC, a.job_key LIMIT ?2",
            params![state.as_str(), limit],
        )?)
    }

    /// The applications, in any state, that Gonza put off until after `now`.
    pub fn still_snoozed(&self, now: DateTime<Utc>) -> Result<Vec<ApplicationRow>> {
        let conn = self.lock();
        Ok(query_applications(
            &conn,
            "WHERE a.snoozed_until > ?1 ORDER BY a.snoozed_until, a.job_key",
            [to_text(now)],
        )?)
    }

    /// Applications that are not finished and were never sent: `Scored`, `Prepared` or
    /// `NeedsGonza`, with no submission stamp and no click the page did not confirm (that form may
    /// have gone out). The only ones that may still be withdrawn for a posting that stopped being
    /// eligible. In key order.
    pub fn unsent_applications(&self) -> Result<Vec<ApplicationRow>> {
        let conn = self.lock();
        Ok(query_applications(
            &conn,
            &format!(
                "WHERE a.state IN ({}) \
                 AND a.submitted_at IS NULL AND a.attempted_at IS NULL ORDER BY a.job_key",
                placeholders(UNSENT_STATES.len())
            ),
            params_from_iter(UNSENT_STATES.iter().map(State::as_str)),
        )?)
    }

    /// The `n` best-scored applications that are still `Scored` or `Prepared`.
    pub fn top_scored(&self, n: usize) -> Result<Vec<ApplicationRow>> {
        let conn = self.lock();
        let limit = i64::try_from(n).unwrap_or(i64::MAX);
        Ok(query_applications(
            &conn,
            &format!(
                "WHERE a.state IN ({}) ORDER BY j.score DESC, a.job_key LIMIT ?",
                placeholders(QUEUED_STATES.len())
            ),
            params_from_iter(
                QUEUED_STATES
                    .iter()
                    .map(|state| Value::from(state.as_str().to_string()))
                    .chain(std::iter::once(Value::from(limit))),
            ),
        )?)
    }

    /// True when an application to this company was submitted, or clicked without the page
    /// confirming it, within the last `days`.
    /// An empty `company_norm` is never in cooldown.
    pub fn company_in_cooldown(
        &self,
        company_norm: &str,
        days: i64,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        if company_norm.is_empty() {
            return Ok(false);
        }
        let cutoff = to_text(now - TimeDelta::days(days));
        let conn = self.lock();
        let found = conn
            .query_row(
                "SELECT 1 FROM applications WHERE company_norm = ?1 \
                 AND (submitted_at >= ?2 OR attempted_at >= ?2) LIMIT 1",
                params![company_norm, cutoff],
                |_| Ok(()),
            )
            .optional()?;
        Ok(found.is_some())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use jobbot_domain::fixtures::{NOW, make_job, make_job_with};
    use jobbot_domain::models::Job;
    use jobbot_domain::text::normalize;

    use crate::tracker::test_support::{state_of, submit, three_scored};

    use super::*;

    #[test]
    fn set_origin_changes_only_that_row_and_writes_no_event() {
        let t = Tracker::in_memory().unwrap();
        let a = make_job_with(|j| j.external_id = "a".into());
        let b = make_job_with(|j| j.external_id = "b".into());
        for job in [&a, &b] {
            t.add_job(job, 70.0, &BTreeMap::new(), *NOW).unwrap();
            t.start_cold_for_tests(&job.key(), *NOW).unwrap();
        }
        let events_before = t.events_for(&a.key()).unwrap().len();
        let updated_before = t.get_application(&a.key()).unwrap().unwrap().updated_at;

        assert!(t.set_origin(&a.key(), Origin::Recruiter).unwrap());

        assert_eq!(t.origin_of(&a.key()).unwrap(), Some(Origin::Recruiter));
        assert_eq!(t.origin_of(&b.key()).unwrap(), Some(Origin::Cold));
        assert_eq!(t.events_for(&a.key()).unwrap().len(), events_before);
        assert_eq!(
            t.get_application(&a.key()).unwrap().unwrap().updated_at,
            updated_before
        );
    }

    #[test]
    fn set_origin_of_an_unknown_key_is_false_and_writes_nothing() {
        let t = Tracker::in_memory().unwrap();
        assert!(!t.set_origin("nope:1", Origin::Agency).unwrap());
        assert_eq!(t.origin_of("nope:1").unwrap(), None);
    }

    #[test]
    fn job_and_application_lifecycle() {
        let now = *NOW;
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        let detail = BTreeMap::from([("fit".to_string(), 30.0)]);
        t.add_job(&job, 77.0, &detail, now).unwrap();
        assert!(t.has_posting(&job).unwrap());
        t.start_cold_for_tests(&job.key(), now).unwrap();
        assert_eq!(state_of(&t, &job.key()), State::Scored);
        t.transition(&job.key(), State::Prepared, None, now)
            .unwrap();
        t.transition(&job.key(), State::Submitted, None, now)
            .unwrap();
        let app = t.get_application(&job.key()).unwrap().unwrap();
        assert_eq!(app.state, State::Submitted);
        assert_eq!(app.submitted_at, Some(now));
        assert_eq!(
            (app.company.as_str(), app.title.as_str()),
            ("Acme AI", "Senior AI Engineer")
        );
        assert_eq!(app.score, 77.0);
        assert_eq!(t.get_job(&job.key()).unwrap(), Some(job));
    }

    #[test]
    fn unsent_applications_are_the_unfinished_ones_that_were_never_clicked() {
        let t = Tracker::in_memory().unwrap();
        let key_of = |id: &str| format!("test:{id}");
        let add = |id: &str| {
            let job = make_job_with(|j| j.external_id = id.into());
            t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
            t.start_cold_for_tests(&job.key(), *NOW).unwrap();
            job.key()
        };
        add("scored");
        let prepared = add("prepared");
        t.transition(&prepared, State::Prepared, None, *NOW)
            .unwrap();
        let needs = add("needs");
        t.transition(
            &needs,
            State::NeedsGonza,
            Some("faltan respuestas: x"),
            *NOW,
        )
        .unwrap();
        // A click the page never confirmed: the form may have gone out.
        let clicked = add("clicked");
        t.transition(
            &clicked,
            State::NeedsGonza,
            Some("unconfirmed_submit: x"),
            *NOW,
        )
        .unwrap();
        t.record_attempt(&clicked, *NOW).unwrap();
        let sent = add("sent");
        t.transition(&sent, State::Prepared, None, *NOW).unwrap();
        t.transition(&sent, State::Submitted, None, *NOW).unwrap();
        let gone = add("gone");
        t.transition(&gone, State::Withdrawn, None, *NOW).unwrap();

        let keys: Vec<String> = t
            .unsent_applications()
            .unwrap()
            .into_iter()
            .map(|row| row.job_key)
            .collect();

        assert_eq!(
            keys,
            [key_of("needs"), key_of("prepared"), key_of("scored")]
        );
    }

    #[test]
    fn start_cold_for_tests_is_idempotent() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.start_cold_for_tests(&job.key(), *NOW).unwrap();
        t.transition(&job.key(), State::Prepared, None, *NOW)
            .unwrap();
        t.start_cold_for_tests(&job.key(), *NOW).unwrap();
        assert_eq!(state_of(&t, &job.key()), State::Prepared);
        assert_eq!(
            t.count_events(&[State::Scored], *NOW - TimeDelta::days(1))
                .unwrap(),
            1
        );
    }

    #[test]
    fn submitted_on_counts_the_utc_day() {
        let now = *NOW;
        let t = Tracker::in_memory().unwrap();
        submit(&t, &make_job());
        assert_eq!(t.submitted_on(now.date_naive()).unwrap(), 1);
        assert_eq!(
            t.submitted_on(now.date_naive() + TimeDelta::days(1))
                .unwrap(),
            0
        );
    }

    #[test]
    fn company_cooldown_lasts_the_window() {
        let now = *NOW;
        let t = Tracker::in_memory().unwrap();
        submit(&t, &make_job());
        assert!(
            t.company_in_cooldown("acme ai", 90, now + TimeDelta::days(89))
                .unwrap()
        );
        assert!(
            !t.company_in_cooldown("acme ai", 90, now + TimeDelta::days(91))
                .unwrap()
        );
        assert!(!t.company_in_cooldown("other co", 90, now).unwrap());
        assert!(!t.company_in_cooldown("", 90, now).unwrap());
    }

    #[test]
    fn top_scored_returns_best_first() {
        let t = Tracker::in_memory().unwrap();
        three_scored(&t, *NOW);
        let scores: Vec<f64> = t
            .top_scored(2)
            .unwrap()
            .iter()
            .map(|row| row.score)
            .collect();
        assert_eq!(scores, vec![90.0, 70.0]);
    }

    #[test]
    fn top_scored_reads_only_the_scored_and_prepared_queue() {
        let t = Tracker::in_memory().unwrap();
        let add = |id: &str, score: f64, walk: &[State]| {
            let job = make_job_with(|j| j.external_id = id.into());
            t.add_job(&job, score, &BTreeMap::new(), *NOW).unwrap();
            t.start_cold_for_tests(&job.key(), *NOW).unwrap();
            for state in walk {
                t.transition(&job.key(), *state, None, *NOW).unwrap();
            }
        };
        add("scored", 60.0, &[]);
        add("prepared", 70.0, &[State::Prepared]);
        add("needs", 95.0, &[State::NeedsGonza]);
        add("sent", 99.0, &[State::Prepared, State::Submitted]);
        let keys: Vec<String> = t
            .top_scored(10)
            .unwrap()
            .into_iter()
            .map(|row| row.job_key)
            .collect();
        assert_eq!(keys, ["test:prepared", "test:scored"]);
        assert_eq!(t.top_scored(1).unwrap().len(), 1);
    }

    #[test]
    fn list_by_state_returns_only_that_state() {
        let t = Tracker::in_memory().unwrap();
        three_scored(&t, *NOW);
        assert_eq!(t.list_by_state(State::Scored).unwrap().len(), 3);
        assert!(t.list_by_state(State::Prepared).unwrap().is_empty());
    }

    #[test]
    fn paths_are_stored_on_the_application() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.start_cold_for_tests(&job.key(), *NOW).unwrap();
        t.set_cv_path(&job.key(), Path::new("out/cv.pdf")).unwrap();
        t.set_screenshot(&job.key(), Path::new("shots/a.png"))
            .unwrap();
        let app = t.get_application(&job.key()).unwrap().unwrap();
        assert_eq!(app.cv_path.as_deref(), Some("out/cv.pdf"));
        assert_eq!(app.screenshot_path.as_deref(), Some("shots/a.png"));
    }

    /// An application whose click the page did not confirm: `needs_gonza` with an attempt stamp.
    fn click_without_confirmation(t: &Tracker, job: &Job) {
        t.add_job(job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.start_cold_for_tests(&job.key(), *NOW).unwrap();
        t.transition(&job.key(), State::Prepared, None, *NOW)
            .unwrap();
        t.record_attempt(&job.key(), *NOW).unwrap();
        t.transition(
            &job.key(),
            State::NeedsGonza,
            Some("unconfirmed_submit: sin confirmación visible"),
            *NOW,
        )
        .unwrap();
    }

    #[test]
    fn an_attempted_click_counts_for_the_cap_and_the_company_cooldown() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        click_without_confirmation(&t, &job);
        let day = TimeDelta::hours(24);
        assert_eq!(t.clicked_since(*NOW - day).unwrap(), 1);
        assert!(
            t.company_in_cooldown(&normalize(&job.company), 90, *NOW)
                .unwrap()
        );
        // Outside the window the attempt no longer counts.
        assert_eq!(t.clicked_since(*NOW + TimeDelta::hours(1)).unwrap(), 0);
        assert!(
            !t.company_in_cooldown(&normalize(&job.company), 90, *NOW + TimeDelta::days(91))
                .unwrap()
        );
    }

    #[test]
    fn an_attempted_click_is_not_counted_as_sent() {
        let t = Tracker::in_memory().unwrap();
        click_without_confirmation(&t, &make_job());
        // The digest and the review-phase counter only know about confirmed submissions.
        assert_eq!(t.submitted_since(*NOW - TimeDelta::hours(24)).unwrap(), 0);
        assert_eq!(t.approved_submitted_total().unwrap(), 0);
    }

    #[test]
    fn retrying_forgets_the_attempted_click() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        click_without_confirmation(&t, &job);
        let day = TimeDelta::hours(24);
        assert_eq!(t.clicked_since(*NOW - day).unwrap(), 1);
        // Gonza sends it again (`resolve`): the attempt is forgotten.
        t.clear_for_retry(&job.key()).unwrap();
        assert_eq!(t.clicked_since(*NOW - day).unwrap(), 0);
        assert!(
            !t.company_in_cooldown(&normalize(&job.company), 90, *NOW)
                .unwrap()
        );
        assert!(t.record_attempt("nope:1", *NOW).is_err());
    }

    #[test]
    fn clear_for_retry_drops_the_screenshot() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        click_without_confirmation(&t, &job);
        t.set_screenshot(&job.key(), Path::new("shot.png")).unwrap();
        t.clear_for_retry(&job.key()).unwrap();
        let row = t.get_application(&job.key()).unwrap().unwrap();
        assert_eq!(row.screenshot_path, None);
        assert!(t.clear_for_retry("nope:1").is_err());
    }

    #[test]
    fn waiting_reason_is_the_stored_reason_or_else_the_last_note() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.start_cold_for_tests(&job.key(), *NOW).unwrap();
        t.transition(&job.key(), State::NeedsGonza, Some("falta email"), *NOW)
            .unwrap();
        let row = t.get_application(&job.key()).unwrap().unwrap();
        assert_eq!(row.waiting_reason(), Some("falta email"));

        let note_only = ApplicationRow {
            needs_reason: None,
            note: Some("nota".into()),
            ..row.clone()
        };
        assert_eq!(note_only.waiting_reason(), Some("nota"));
        let neither = ApplicationRow {
            needs_reason: None,
            note: None,
            ..row
        };
        assert_eq!(neither.waiting_reason(), None);
    }

    /// The point of the two indexes: a poll reads the small indexes and not every posting's text.
    #[test]
    fn the_panels_reads_are_served_by_the_indexes_and_not_by_scanning_every_posting() {
        let t = Tracker::in_memory().unwrap();
        let plan = |sql: &str| -> String {
            let conn = t.lock();
            let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
            stmt.query_map([&to_text(*NOW)], |row| row.get::<_, String>(3))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
                .join(" | ")
        };
        let cohort = plan(
            "SELECT j.score, a.state FROM jobs j LEFT JOIN applications a ON a.job_key = j.key \
             WHERE j.first_seen >= ?1 ORDER BY j.first_seen, j.key",
        );
        assert!(
            cohort.contains("COVERING INDEX idx_jobs_first_seen"),
            "cohort: {cohort}"
        );
        let first_seen =
            plan("SELECT first_seen FROM jobs WHERE first_seen >= ?1 ORDER BY first_seen");
        assert!(
            first_seen.contains("COVERING INDEX idx_jobs_first_seen"),
            "first_seen_since: {first_seen}"
        );
        let applications = plan(&format!("{APPLICATION_SELECT} WHERE a.state = ?1"));
        assert!(
            applications.contains("COVERING INDEX idx_events_job_state"),
            "state_since: {applications}"
        );
    }

    #[test]
    fn a_snooze_is_stored_and_cleared_by_hand() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.start_cold_for_tests(&job.key(), *NOW).unwrap();
        let row = t.get_application(&job.key()).unwrap().unwrap();
        assert_eq!(row.snoozed_until, None);

        let until = *NOW + TimeDelta::days(1);
        t.set_snoozed_until(&job.key(), Some(until)).unwrap();
        assert_eq!(
            t.get_application(&job.key())
                .unwrap()
                .unwrap()
                .snoozed_until,
            Some(until)
        );
        t.set_snoozed_until(&job.key(), None).unwrap();
        assert_eq!(
            t.get_application(&job.key())
                .unwrap()
                .unwrap()
                .snoozed_until,
            None
        );
        assert!(t.set_snoozed_until("nope:1", Some(until)).is_err());
    }

    #[test]
    fn a_row_carries_the_posting_dates_the_pay_and_when_it_entered_its_state() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job_with(|j| {
            j.posted_at = Some(*NOW - TimeDelta::days(10));
            j.salary_min_monthly_usd = Some(5200.0);
            j.salary_max_monthly_usd = Some(7300.0);
        });
        let seen = *NOW - TimeDelta::days(2);
        t.add_job(&job, 70.0, &BTreeMap::new(), seen).unwrap();
        t.start_cold_for_tests(&job.key(), seen).unwrap();
        let row = t.get_application(&job.key()).unwrap().unwrap();
        assert_eq!(row.posted_at, job.posted_at);
        assert_eq!(row.first_seen, seen);
        assert_eq!(
            (row.salary_min, row.salary_max),
            (Some(5200.0), Some(7300.0))
        );
        assert_eq!(row.state_since, Some(seen), "created at `scored` then");

        let later = *NOW - TimeDelta::days(1);
        t.transition(&job.key(), State::Prepared, None, later)
            .unwrap();
        t.transition(&job.key(), State::NeedsGonza, Some("captcha"), *NOW)
            .unwrap();
        let row = t.get_application(&job.key()).unwrap().unwrap();
        assert_eq!(row.state_since, Some(*NOW));

        // Back to the queue and stopped again: the latest entry into the state counts.
        t.transition(
            &job.key(),
            State::Prepared,
            None,
            *NOW + TimeDelta::hours(1),
        )
        .unwrap();
        let again = *NOW + TimeDelta::hours(2);
        t.transition(&job.key(), State::NeedsGonza, Some("captcha"), again)
            .unwrap();
        let row = t.get_application(&job.key()).unwrap().unwrap();
        assert_eq!(row.state_since, Some(again));
    }

    #[test]
    fn the_first_rows_of_a_state_are_the_head_of_its_whole_list() {
        let t = Tracker::in_memory().unwrap();
        for (id, score) in [("1", 50.0), ("2", 90.0), ("3", 70.0), ("4", 70.0)] {
            let job = make_job_with(|j| j.external_id = id.into());
            t.add_job(&job, score, &BTreeMap::new(), *NOW).unwrap();
            t.start_cold_for_tests(&job.key(), *NOW).unwrap();
        }
        let keys = |rows: Vec<ApplicationRow>| -> Vec<String> {
            rows.into_iter().map(|r| r.job_key).collect()
        };
        let whole = keys(t.list_by_state(State::Scored).unwrap());
        assert_eq!(whole, ["test:2", "test:3", "test:4", "test:1"]);
        assert_eq!(
            keys(t.first_by_state(State::Scored, 2).unwrap()),
            whole[..2]
        );
        assert_eq!(keys(t.first_by_state(State::Scored, 10).unwrap()), whole);
        assert!(t.first_by_state(State::Offer, 5).unwrap().is_empty());
    }

    #[test]
    fn still_snoozed_lists_what_is_put_off_past_now_in_any_state() {
        let t = Tracker::in_memory().unwrap();
        for id in ["1", "2", "3"] {
            let job = make_job_with(|j| j.external_id = id.into());
            t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
            t.start_cold_for_tests(&job.key(), *NOW).unwrap();
        }
        t.transition("test:2", State::NeedsGonza, Some("captcha"), *NOW)
            .unwrap();
        t.set_snoozed_until("test:1", Some(*NOW + TimeDelta::hours(1)))
            .unwrap();
        t.set_snoozed_until("test:2", Some(*NOW + TimeDelta::days(2)))
            .unwrap();
        t.set_snoozed_until("test:3", Some(*NOW - TimeDelta::hours(1)))
            .unwrap();
        let mut keys: Vec<String> = t
            .still_snoozed(*NOW)
            .unwrap()
            .into_iter()
            .map(|r| r.job_key)
            .collect();
        keys.sort();
        assert_eq!(
            keys,
            ["test:1", "test:2"],
            "a snooze that is over is not listed"
        );
    }
}
