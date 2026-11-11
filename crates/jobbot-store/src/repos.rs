//! `Tracker` and `PrepStore` implement the application's storage ports. Each method forwards to the
//! inherent one of the same name; the errors become the port's, the data answers kept apart from a
//! failing backend.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, NaiveDate, Utc};
use jobbot_app::ports::store::*;
use jobbot_domain::models::Job;
use jobbot_domain::prep::Question;
use jobbot_domain::state::State;

use crate::prep::{PrepError, PrepStore};
use crate::tracker::{Tracker, TrackerError};

impl From<TrackerError> for StoreError {
    fn from(error: TrackerError) -> Self {
        match error {
            TrackerError::IllegalTransition { from, to } => {
                StoreError::IllegalTransition { from, to }
            }
            TrackerError::UnknownKey(key) => StoreError::UnknownKey(key),
            other => StoreError::Backend(Box::new(other)),
        }
    }
}

impl From<PrepError> for StoreError {
    fn from(error: PrepError) -> Self {
        match error {
            PrepError::GradeOutOfRange(grade) => StoreError::GradeOutOfRange(grade),
            other => StoreError::Backend(Box::new(other)),
        }
    }
}

impl JobRepo for Tracker {
    fn has_posting(&self, job: &Job) -> StoreResult<bool> {
        Tracker::has_posting(self, job).map_err(StoreError::from)
    }
    fn find_job_key(&self, job: &Job) -> StoreResult<Option<String>> {
        Tracker::find_job_key(self, job).map_err(StoreError::from)
    }
    fn has_posting_since(&self, job: &Job, repost_cutoff: DateTime<Utc>) -> StoreResult<bool> {
        Tracker::has_posting_since(self, job, repost_cutoff).map_err(StoreError::from)
    }
    fn add_job(
        &self,
        job: &Job,
        score: f64,
        detail: &BTreeMap<String, f64>,
        now: DateTime<Utc>,
    ) -> StoreResult<()> {
        Tracker::add_job(self, job, score, detail, now).map_err(StoreError::from)
    }
    fn refresh_posting(&self, job: &Job) -> StoreResult<bool> {
        Tracker::refresh_posting(self, job).map_err(StoreError::from)
    }
    fn get_job(&self, key: &str) -> StoreResult<Option<Job>> {
        Tracker::get_job(self, key).map_err(StoreError::from)
    }
    fn jobs_without_application(&self) -> StoreResult<Vec<Job>> {
        Tracker::jobs_without_application(self).map_err(StoreError::from)
    }
    fn set_score(&self, key: &str, score: f64, detail: &BTreeMap<String, f64>) -> StoreResult<()> {
        Tracker::set_score(self, key, score, detail).map_err(StoreError::from)
    }
    fn job_first_seen(&self, key: &str) -> StoreResult<Option<DateTime<Utc>>> {
        Tracker::job_first_seen(self, key).map_err(StoreError::from)
    }
    fn first_seen_at(&self) -> StoreResult<Option<DateTime<Utc>>> {
        Tracker::first_seen_at(self).map_err(StoreError::from)
    }
    fn score_detail(&self, key: &str) -> StoreResult<BTreeMap<String, f64>> {
        Tracker::score_detail(self, key).map_err(StoreError::from)
    }
}

impl ApplicationRepo for Tracker {
    fn create_application(&self, job_key: &str, now: DateTime<Utc>) -> StoreResult<()> {
        Tracker::create_application(self, job_key, now).map_err(StoreError::from)
    }
    fn get_application(&self, job_key: &str) -> StoreResult<Option<ApplicationRow>> {
        Tracker::get_application(self, job_key).map_err(StoreError::from)
    }
    fn transition(
        &self,
        job_key: &str,
        new_state: State,
        note: Option<&str>,
        now: DateTime<Utc>,
    ) -> StoreResult<()> {
        Tracker::transition(self, job_key, new_state, note, now).map_err(StoreError::from)
    }
    fn mark_transition(
        &self,
        job_key: &str,
        new_state: State,
        note: Option<&str>,
        now: DateTime<Utc>,
    ) -> StoreResult<()> {
        Tracker::mark_transition(self, job_key, new_state, note, now).map_err(StoreError::from)
    }
    fn undo_mark(
        &self,
        job_key: &str,
        since: DateTime<Utc>,
    ) -> StoreResult<Result<UndoableMark, UndoRefusal>> {
        Tracker::undo_mark(self, job_key, since).map_err(StoreError::from)
    }
    fn undoable_marks(&self, since: DateTime<Utc>) -> StoreResult<BTreeMap<String, UndoableMark>> {
        Tracker::undoable_marks(self, since).map_err(StoreError::from)
    }
    fn set_cv_path(&self, job_key: &str, path: &Path) -> StoreResult<()> {
        Tracker::set_cv_path(self, job_key, path).map_err(StoreError::from)
    }
    fn set_screenshot(&self, job_key: &str, path: &Path) -> StoreResult<()> {
        Tracker::set_screenshot(self, job_key, path).map_err(StoreError::from)
    }
    fn record_attempt(&self, job_key: &str, now: DateTime<Utc>) -> StoreResult<()> {
        Tracker::record_attempt(self, job_key, now).map_err(StoreError::from)
    }
    fn clear_for_retry(&self, job_key: &str) -> StoreResult<()> {
        Tracker::clear_for_retry(self, job_key).map_err(StoreError::from)
    }
    fn set_snoozed_until(&self, job_key: &str, until: Option<DateTime<Utc>>) -> StoreResult<()> {
        Tracker::set_snoozed_until(self, job_key, until).map_err(StoreError::from)
    }
    fn approve(&self, job_key: &str) -> StoreResult<()> {
        Tracker::approve(self, job_key).map_err(StoreError::from)
    }
    fn list_by_state(&self, state: State) -> StoreResult<Vec<ApplicationRow>> {
        Tracker::list_by_state(self, state).map_err(StoreError::from)
    }
    fn first_by_state(&self, state: State, n: usize) -> StoreResult<Vec<ApplicationRow>> {
        Tracker::first_by_state(self, state, n).map_err(StoreError::from)
    }
    fn still_snoozed(&self, now: DateTime<Utc>) -> StoreResult<Vec<ApplicationRow>> {
        Tracker::still_snoozed(self, now).map_err(StoreError::from)
    }
    fn unsent_applications(&self) -> StoreResult<Vec<ApplicationRow>> {
        Tracker::unsent_applications(self).map_err(StoreError::from)
    }
    fn top_scored(&self, n: usize) -> StoreResult<Vec<ApplicationRow>> {
        Tracker::top_scored(self, n).map_err(StoreError::from)
    }
    fn counts(&self) -> StoreResult<BTreeMap<String, i64>> {
        Tracker::counts(self).map_err(StoreError::from)
    }
    fn submitted_on(&self, day: NaiveDate) -> StoreResult<i64> {
        Tracker::submitted_on(self, day).map_err(StoreError::from)
    }
    fn approved_submitted_total(&self) -> StoreResult<i64> {
        Tracker::approved_submitted_total(self).map_err(StoreError::from)
    }
    fn company_in_cooldown(
        &self,
        company_norm: &str,
        days: i64,
        now: DateTime<Utc>,
    ) -> StoreResult<bool> {
        Tracker::company_in_cooldown(self, company_norm, days, now).map_err(StoreError::from)
    }
    fn count_events(&self, to_states: &[State], since: DateTime<Utc>) -> StoreResult<i64> {
        Tracker::count_events(self, to_states, since).map_err(StoreError::from)
    }
    fn clicked_since(&self, since: DateTime<Utc>) -> StoreResult<i64> {
        Tracker::clicked_since(self, since).map_err(StoreError::from)
    }
    fn submitted_since(&self, since: DateTime<Utc>) -> StoreResult<i64> {
        Tracker::submitted_since(self, since).map_err(StoreError::from)
    }
    fn job_keys_of_company(
        &self,
        company_norm: &str,
        states: &[State],
    ) -> StoreResult<Vec<String>> {
        Tracker::job_keys_of_company(self, company_norm, states).map_err(StoreError::from)
    }
}

impl IntentRepo for Tracker {
    fn begin_intent(
        &self,
        kind: &str,
        job_key: &str,
        now: DateTime<Utc>,
    ) -> StoreResult<Option<i64>> {
        Tracker::begin_intent(self, kind, job_key, now).map_err(StoreError::from)
    }
    fn finish_intent(&self, id: i64, outcome: &str, now: DateTime<Utc>) -> StoreResult<()> {
        Tracker::finish_intent(self, id, outcome, now).map_err(StoreError::from)
    }
    fn open_intents(&self, kind: &str) -> StoreResult<Vec<Intent>> {
        Tracker::open_intents(self, kind).map_err(StoreError::from)
    }
    fn count_intents(&self) -> StoreResult<i64> {
        Tracker::count_intents(self).map_err(StoreError::from)
    }
    fn recover_open_intents(&self, now: DateTime<Utc>) -> StoreResult<Vec<String>> {
        Tracker::recover_open_intents(self, now).map_err(StoreError::from)
    }
}

impl EmailRepo for Tracker {
    fn email_exists(&self, message_id: &str) -> StoreResult<bool> {
        Tracker::email_exists(self, message_id).map_err(StoreError::from)
    }
    fn add_email(
        &self,
        email: &NewEmail,
        task: Option<&NewTask>,
        now: DateTime<Utc>,
    ) -> StoreResult<EmailAdded> {
        Tracker::add_email(self, email, task, now).map_err(StoreError::from)
    }
    fn get_email(&self, message_id: &str) -> StoreResult<Option<EmailRow>> {
        Tracker::get_email(self, message_id).map_err(StoreError::from)
    }
    fn link_email(&self, message_id: &str, job_key: &str) -> StoreResult<bool> {
        Tracker::link_email(self, message_id, job_key).map_err(StoreError::from)
    }
    fn recent_emails(&self, since: DateTime<Utc>) -> StoreResult<Vec<EmailRow>> {
        Tracker::recent_emails(self, since).map_err(StoreError::from)
    }
}

impl TaskRepo for Tracker {
    fn add_task(&self, task: &NewTask, now: DateTime<Utc>) -> StoreResult<bool> {
        Tracker::add_task(self, task, now).map_err(StoreError::from)
    }
    fn add_manual_task(&self, task: &NewTask, now: DateTime<Utc>) -> StoreResult<TaskAdded> {
        Tracker::add_manual_task(self, task, now).map_err(StoreError::from)
    }
    fn get_task(&self, id: i64) -> StoreResult<Option<TaskRow>> {
        Tracker::get_task(self, id).map_err(StoreError::from)
    }
    fn open_tasks(&self) -> StoreResult<Vec<TaskRow>> {
        Tracker::open_tasks(self).map_err(StoreError::from)
    }
    fn complete_task(&self, id: i64) -> StoreResult<TaskCompletion> {
        Tracker::complete_task(self, id).map_err(StoreError::from)
    }
}

impl MetaRepo for Tracker {
    fn get_meta(&self, key: &str) -> StoreResult<Option<String>> {
        Tracker::get_meta(self, key).map_err(StoreError::from)
    }
    fn set_meta(&self, key: &str, value: &str) -> StoreResult<()> {
        Tracker::set_meta(self, key, value).map_err(StoreError::from)
    }
    fn delete_meta(&self, key: &str) -> StoreResult<()> {
        Tracker::delete_meta(self, key).map_err(StoreError::from)
    }
}

impl SpendRepo for Tracker {
    fn record_claude_call(
        &self,
        stage: &str,
        at: DateTime<Utc>,
        cost_usd: f64,
        input_tokens: u64,
        output_tokens: u64,
        ok: bool,
    ) -> StoreResult<()> {
        Tracker::record_claude_call(self, stage, at, cost_usd, input_tokens, output_tokens, ok)
            .map_err(StoreError::from)
    }
    fn claude_spend_since(&self, since: DateTime<Utc>) -> StoreResult<f64> {
        Tracker::claude_spend_since(self, since).map_err(StoreError::from)
    }
    fn claude_calls_by_stage(
        &self,
        since: DateTime<Utc>,
    ) -> StoreResult<BTreeMap<String, (i64, f64)>> {
        Tracker::claude_calls_by_stage(self, since).map_err(StoreError::from)
    }
}

impl HistoryRepo for Tracker {
    fn first_seen_since(&self, since: DateTime<Utc>) -> StoreResult<Vec<DateTime<Utc>>> {
        Tracker::first_seen_since(self, since).map_err(StoreError::from)
    }
    fn events_for(&self, job_key: &str) -> StoreResult<Vec<EventRow>> {
        Tracker::events_for(self, job_key).map_err(StoreError::from)
    }
    fn moves_between(
        &self,
        from: DateTime<Utc>,
        until: DateTime<Utc>,
        limit: usize,
    ) -> StoreResult<(i64, Vec<MoveRow>)> {
        Tracker::moves_between(self, from, until, limit).map_err(StoreError::from)
    }
    fn move_counts_between(
        &self,
        from: DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> StoreResult<Vec<(State, i64)>> {
        Tracker::move_counts_between(self, from, until).map_err(StoreError::from)
    }
    fn jobs_reaching(&self, states: &[State], since: DateTime<Utc>) -> StoreResult<i64> {
        Tracker::jobs_reaching(self, states, since).map_err(StoreError::from)
    }
    fn submitted_since_reaching(&self, since: DateTime<Utc>, states: &[State]) -> StoreResult<i64> {
        Tracker::submitted_since_reaching(self, since, states).map_err(StoreError::from)
    }
    fn jobs_reaching_both(&self, first: State, then: State) -> StoreResult<i64> {
        Tracker::jobs_reaching_both(self, first, then).map_err(StoreError::from)
    }
    fn funnel_by_source(&self) -> StoreResult<Vec<SourceFunnelRow>> {
        Tracker::funnel_by_source(self).map_err(StoreError::from)
    }
    fn cohort_since(&self, since: DateTime<Utc>) -> StoreResult<Vec<CohortRow>> {
        Tracker::cohort_since(self, since).map_err(StoreError::from)
    }
}

impl AttemptRepo for Tracker {
    fn begin_attempt(
        &self,
        id: &str,
        command: &str,
        request: &[u8],
        now: DateTime<Utc>,
    ) -> StoreResult<AttemptStart> {
        Tracker::begin_attempt(self, id, command, request, now).map_err(StoreError::from)
    }
    fn finish_attempt(&self, id: &str, status: u16, reply: &[u8]) -> StoreResult<()> {
        Tracker::finish_attempt(self, id, status, reply).map_err(StoreError::from)
    }
    fn forget_attempt(&self, id: &str) -> StoreResult<()> {
        Tracker::forget_attempt(self, id).map_err(StoreError::from)
    }
}

impl HttpMemory for Tracker {
    fn resting_hosts(&self, now: DateTime<Utc>) -> StoreResult<Vec<(String, DateTime<Utc>)>> {
        Tracker::resting_hosts(self, now).map_err(StoreError::from)
    }
    fn rest_host(&self, host: &str, until: DateTime<Utc>) -> StoreResult<()> {
        Tracker::rest_host(self, host, until).map_err(StoreError::from)
    }
    fn recall_page(&self, url: &str, now: DateTime<Utc>) -> StoreResult<Option<StoredPage>> {
        Tracker::recall_page(self, url, now).map_err(StoreError::from)
    }
    fn store_page(&self, url: &str, page: &StoredPage, now: DateTime<Utc>) -> StoreResult<()> {
        Tracker::store_page(self, url, page, now).map_err(StoreError::from)
    }
}

impl LearnRepo for PrepStore {
    fn add_cards(
        &self,
        cards: &[Question],
        source: &str,
        now: DateTime<Utc>,
    ) -> StoreResult<usize> {
        PrepStore::add_cards(self, cards, source, now).map_err(StoreError::from)
    }
    fn all_questions(&self) -> StoreResult<Vec<Question>> {
        PrepStore::all_questions(self).map_err(StoreError::from)
    }
    fn next_question<'a>(
        &self,
        questions: &'a [Question],
        now: DateTime<Utc>,
        topic: Option<&str>,
    ) -> StoreResult<Option<&'a Question>> {
        PrepStore::next_question(self, questions, now, topic).map_err(StoreError::from)
    }
    fn grade(&self, q: &Question, grade: u8, now: DateTime<Utc>) -> StoreResult<()> {
        PrepStore::grade(self, q, grade, now).map_err(StoreError::from)
    }
    fn weak_topics(&self, limit: usize, last_n: usize) -> StoreResult<Vec<(String, f64)>> {
        PrepStore::weak_topics(self, limit, last_n).map_err(StoreError::from)
    }
    fn stats(&self, questions: &[Question], now: DateTime<Utc>) -> StoreResult<PrepStats> {
        PrepStore::stats(self, questions, now).map_err(StoreError::from)
    }
    fn topic_progress(
        &self,
        questions: &[Question],
        now: DateTime<Utc>,
    ) -> StoreResult<Vec<TopicProgress>> {
        PrepStore::topic_progress(self, questions, now).map_err(StoreError::from)
    }
    fn gap_rows(&self) -> StoreResult<Vec<GapRow>> {
        PrepStore::gap_rows(self).map_err(StoreError::from)
    }
}

#[cfg(test)]
mod tests {
    use jobbot_domain::fixtures::{NOW, make_job};

    use super::*;

    fn now() -> DateTime<Utc> {
        *NOW
    }

    #[test]
    fn a_refused_transition_through_the_port_is_an_answer_not_a_failure() {
        let tracker = Tracker::in_memory().unwrap();
        let job = make_job();
        tracker.add_job(&job, 0.9, &BTreeMap::new(), now()).unwrap();
        tracker.create_application(&job.key(), now()).unwrap();
        let store: &dyn Store = &tracker;

        let refused = store.transition(&job.key(), State::Offer, None, now());

        assert!(
            matches!(refused, Err(StoreError::IllegalTransition { .. })),
            "{refused:?}"
        );
        assert!(matches!(
            store.transition("nope:1", State::Prepared, None, now()),
            Err(StoreError::UnknownKey(key)) if key == "nope:1"
        ));
    }

    #[test]
    fn a_failing_database_is_a_backend_error_that_keeps_its_cause() {
        let error = StoreError::from(TrackerError::Io {
            doing: "creating data/backups".to_string(),
            source: std::io::Error::other("disk full"),
        });

        assert!(matches!(error, StoreError::Backend(_)), "{error:?}");
        assert_eq!(error.to_string(), "creating data/backups");
        let cause = std::error::Error::source(&error).map(ToString::to_string);
        assert_eq!(cause.as_deref(), Some("disk full"));
    }

    #[test]
    fn a_grade_out_of_range_stays_an_answer() {
        let store = PrepStore::in_memory().unwrap();
        let card = Question {
            id: "q1".into(),
            topic: "rust".into(),
            question: "?".into(),
            answer: "!".into(),
        };
        let learn: &dyn LearnRepo = &store;

        assert!(matches!(
            learn.grade(&card, 9, now()),
            Err(StoreError::GradeOutOfRange(9))
        ));
    }
}
