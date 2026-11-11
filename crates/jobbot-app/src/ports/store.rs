//! The application's storage, as ports: one trait per aggregate, the records they read and write,
//! and the one error they share. `jobbot-store` implements them on SQLite; a use case receives the
//! `Store` handle (every aggregate of `jobbot.db`) or the one repository it needs.
//!
//! Timestamps are UTC; the records carry no SQL and no file paths of their own.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, NaiveDate, Utc};
use jobbot_domain::models::Job;
use jobbot_domain::prep::Question;
use jobbot_domain::privacy::Pii;
use jobbot_domain::state::State;
use serde::Serialize;

/// Why a store call failed. The first three are answers about the data, which the caller usually
/// explains to Gonza; `Backend` is the storage itself failing (the database, the disk, a stored
/// value that no longer parses), shown with its whole chain of causes.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("illegal transition {from} -> {to}")]
    IllegalTransition { from: String, to: String },
    #[error("unknown job key {0}")]
    UnknownKey(String),
    #[error("grade must be between 0 and 5, got {0}")]
    GradeOutOfRange(u8),
    #[error(transparent)]
    Backend(Box<dyn std::error::Error + Send + Sync>),
}

/// A store call's result.
pub type StoreResult<T> = Result<T, StoreError>;

/// The kind of intent written before clicking submit on an application form.
pub const INTENT_SUBMIT: &str = "submit";

/// Start of the `needs_reason` of an application whose run was cut between the click and its
/// confirmation. Discovery refuses to overwrite a row carrying it (see `MAY_HAVE_BEEN_SENT`).
pub const INTERRUPTED_PREFIX: &str = "interrupted_submit";

/// An act announced and not yet closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intent {
    pub id: i64,
    pub kind: String,
    pub job_key: String,
    pub started_at: String,
}

/// An application joined with the fields of its job that the digest and the gate need.
#[derive(Debug, Clone)]
pub struct ApplicationRow {
    pub job_key: String,
    pub company_norm: String,
    pub state: State,
    pub note: Option<String>,
    pub needs_reason: Option<String>,
    pub cv_path: Option<String>,
    pub screenshot_path: Option<String>,
    pub approved: bool,
    pub submitted_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub title: String,
    pub company: String,
    pub score: f64,
    pub url: String,
    pub source: String,
    /// Gonza put it off until then (`snooze`): the panel hides it from his queue meanwhile.
    pub snoozed_until: Option<DateTime<Utc>>,
    /// The posting's own date, when it has one, and the moment it was first stored.
    pub posted_at: Option<DateTime<Utc>>,
    pub first_seen: DateTime<Utc>,
    /// Monthly USD, as the posting says it.
    pub salary_min: Option<f64>,
    pub salary_max: Option<f64>,
    /// When it last entered its current state; `None` only for a row whose history is missing.
    pub state_since: Option<DateTime<Utc>>,
}

impl ApplicationRow {
    /// Why an application that waits for Gonza is waiting: the reason stored when it entered
    /// `needs_gonza`, or else its last note. The digest, the panel and `jobbot today` all say it
    /// this way.
    pub fn waiting_reason(&self) -> Option<&str> {
        self.needs_reason.as_deref().or(self.note.as_deref())
    }
}

/// One line of an application's history (`events` table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRow {
    pub at: DateTime<Utc>,
    pub from_state: Option<State>,
    pub to_state: State,
    pub note: Option<String>,
}

/// One source's line of the funnel (`funnel_by_source`): plain counts, never rates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFunnelRow {
    pub source: String,
    /// Applications of the source, whatever their state.
    pub n: i64,
    pub sent: i64,
    pub answered: i64,
    pub tests: i64,
    pub interviews: i64,
    pub offers: i64,
}

/// A stored posting with the application made for it, when there is one: one row of the weekly
/// pipeline (`cohort_since`).
#[derive(Debug, Clone, PartialEq)]
pub struct CohortRow {
    pub score: f64,
    /// `None` when no application was made for the posting.
    pub state: Option<State>,
    pub cv_path: Option<String>,
    pub screenshot_path: Option<String>,
    pub submitted_at: Option<DateTime<Utc>>,
}

/// A state change together with the posting it happened to (`events` joined with `jobs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveRow {
    pub at: DateTime<Utc>,
    pub job_key: String,
    pub company: String,
    pub title: String,
    pub to_state: State,
}

/// A mark that can still be taken back: the newest event of its application, recorded by `mark`.
#[derive(Debug, Clone, PartialEq)]
pub struct UndoableMark {
    pub from: State,
    pub to: State,
    pub at: DateTime<Utc>,
}

/// Why `undo_mark` took nothing back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoRefusal {
    /// No application has that key.
    UnknownKey,
    /// The application has no mark in the window.
    NoRecentMark,
    /// A mark in the window was followed by another change.
    ChangedSince,
    /// The newest change is a mark, made before the window.
    TooOld,
}

/// What an email about a job process is (`emails.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailKind {
    Ack,
    Rejection,
    Recruiter,
    InterviewInvite,
    TechnicalTest,
    ActionRequired,
    Other,
}

impl EmailKind {
    pub const ALL: [EmailKind; 7] = [
        EmailKind::Ack,
        EmailKind::Rejection,
        EmailKind::Recruiter,
        EmailKind::InterviewInvite,
        EmailKind::TechnicalTest,
        EmailKind::ActionRequired,
        EmailKind::Other,
    ];

    /// The name stored in the database and written in the import file.
    pub fn as_str(&self) -> &'static str {
        match self {
            EmailKind::Ack => "ack",
            EmailKind::Rejection => "rejection",
            EmailKind::Recruiter => "recruiter",
            EmailKind::InterviewInvite => "interview_invite",
            EmailKind::TechnicalTest => "technical_test",
            EmailKind::ActionRequired => "action_required",
            EmailKind::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<EmailKind> {
        EmailKind::ALL.into_iter().find(|kind| kind.as_str() == s)
    }

    /// How Gonza reads it in `jobbot today`.
    pub fn label(&self) -> &'static str {
        match self {
            EmailKind::Ack => "acuse",
            EmailKind::Rejection => "rechazo",
            EmailKind::Recruiter => "reclutador",
            EmailKind::InterviewInvite => "invitación a entrevista",
            EmailKind::TechnicalTest => "prueba técnica",
            EmailKind::ActionRequired => "requiere acción",
            EmailKind::Other => "otro",
        }
    }
}

/// An email to remember. The body is never stored, only what the importer was given. The sender
/// is a person's address, so it never prints (`Pii`).
#[derive(Debug, Clone)]
pub struct NewEmail {
    pub message_id: String,
    pub thread_id: String,
    pub from_addr: Pii<String>,
    pub subject: String,
    pub received_at: DateTime<Utc>,
    pub job_key: Option<String>,
    pub kind: EmailKind,
    pub summary: String,
}

/// A stored email, with the company of the posting it is linked to (when it is linked).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailRow {
    pub message_id: String,
    pub thread_id: String,
    pub from_addr: Pii<String>,
    pub subject: String,
    pub received_at: DateTime<Utc>,
    pub job_key: Option<String>,
    pub kind: EmailKind,
    pub summary: String,
    pub imported_at: DateTime<Utc>,
    pub company: Option<String>,
}

/// Something Gonza has to do (`gonza_tasks`). `kind` is free text; the importer uses the kind of
/// the email it came from and `jobbot task add` uses `manual`. A task is unique per
/// `(source_message_id, kind)`; one with no source message is never merged by that key.
#[derive(Debug, Clone)]
pub struct NewTask {
    pub job_key: Option<String>,
    pub kind: String,
    pub text: String,
    pub due_at: Option<DateTime<Utc>>,
    pub source_message_id: Option<String>,
    /// An address to open for it (http or https), shown after the text.
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRow {
    pub id: i64,
    pub job_key: Option<String>,
    pub kind: String,
    pub text: String,
    pub due_at: Option<DateTime<Utc>>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub source_message_id: Option<String>,
    pub link: Option<String>,
}

impl TaskRow {
    /// Past its deadline. A task without a date is never overdue.
    pub fn is_overdue(&self, now: DateTime<Utc>) -> bool {
        self.due_at.is_some_and(|due| due < now)
    }
}

/// What `add_email` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailAdded {
    /// The message id was already stored: nothing was written.
    AlreadyKnown,
    Stored {
        task_created: bool,
    },
}

/// What `add_manual_task` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskAdded {
    Created(i64),
    /// An open task with the same text and posting exists (this is its id): nothing was written.
    AlreadyOpen(i64),
}

/// What `complete_task` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskCompletion {
    Done,
    AlreadyDone,
    Unknown,
}

/// Counts for `prep stats`. `due` is every question `next_question` could serve right now:
/// reviews that have come due plus questions never seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrepStats {
    pub total: usize,
    pub seen: usize,
    pub due: usize,
}

/// Progress of one topic: how many cards it has, how many Gonza has answered at least once, and
/// how many he could be served right now (never seen, or come due).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TopicProgress {
    pub topic: String,
    pub total: usize,
    pub seen: usize,
    pub due: usize,
}

/// A card a dossier added (a gap to study), with the posting it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GapRow {
    pub id: String,
    pub topic: String,
    /// The job key of the posting whose dossier produced the card.
    pub source: String,
    /// Gonza has answered it at least once.
    pub seen: bool,
}

/// The stored postings (`jobs`): what discovery writes and the scoring reads.
pub trait JobRepo {
    /// True when this exact posting, or the same title at the same company, is known.
    /// Postings with an empty company or title only match by key.
    fn has_posting(&self, job: &Job) -> StoreResult<bool>;
    /// The key of the stored posting that `job` stands for, or `None` when it is new. In order: the
    /// same key; the same link (`link_base`: the query, the fragment and a final `/application` do
    /// not count, and `/jobs/1` is not `/jobs/12`); the same title at the same company.
    fn find_job_key(&self, job: &Job) -> StoreResult<Option<String>>;
    /// `has_posting` for discovery: the same title at the same company only counts as a duplicate
    /// while the stored posting is still live, that is, dated (or, with no date, first seen) at or
    /// after `repost_cutoff`. A role the company posts again after the old one went stale or was
    /// withdrawn is a new posting. The same key is always the same posting.
    fn has_posting_since(&self, job: &Job, repost_cutoff: DateTime<Utc>) -> StoreResult<bool>;
    /// Stores a scored posting. A job that is already stored is left untouched.
    fn add_job(
        &self,
        job: &Job,
        score: f64,
        detail: &BTreeMap<String, f64>,
        now: DateTime<Utc>,
    ) -> StoreResult<()>;
    /// Brings a stored posting up to date with what its source reports today: apply link, posting
    /// date, location and remote flag (a source may fix how it reads them after the posting was
    /// stored, and a posting's own data changes). The score, its detail, the pay and every other
    /// field stay as they were scored. Returns whether anything changed; one UPDATE.
    ///
    /// A blank never replaces a good value: not an empty apply link, not an empty location, not a
    /// missing date (a fetch that lacks them says nothing about them). A posting that is not
    /// stored is not created.
    fn refresh_posting(&self, job: &Job) -> StoreResult<bool>;
    fn get_job(&self, key: &str) -> StoreResult<Option<Job>>;
    /// Stored postings that never got an application: discovery scored them below the
    /// threshold. `rescore` looks at them again when the criteria change.
    fn jobs_without_application(&self) -> StoreResult<Vec<Job>>;
    /// Replaces the stored score and its detail (used by `rescore`).
    fn set_score(&self, key: &str, score: f64, detail: &BTreeMap<String, f64>) -> StoreResult<()>;
    /// When this posting was first seen (stored), or `None` when it is not stored.
    fn job_first_seen(&self, key: &str) -> StoreResult<Option<DateTime<Utc>>>;
    /// When the oldest stored job was first seen.
    fn first_seen_at(&self) -> StoreResult<Option<DateTime<Utc>>>;
    /// How the score of a stored posting was made up, by criterion.
    fn score_detail(&self, key: &str) -> StoreResult<BTreeMap<String, f64>>;
}

/// The applications and their state machine (`applications`, `events`).
pub trait ApplicationRepo {
    /// Starts an application at `Scored`. Does nothing if one already exists.
    fn create_application(&self, job_key: &str, now: DateTime<Utc>) -> StoreResult<()>;
    fn get_application(&self, job_key: &str) -> StoreResult<Option<ApplicationRow>>;
    /// Moves an application along the state machine and records the event. Moving to
    /// `NeedsGonza` stores `note` as the reason; leaving it clears the reason; moving to
    /// `Submitted` stamps `submitted_at`. Any move ends a snooze: what Gonza put off was the
    /// application as it stood.
    fn transition(
        &self,
        job_key: &str,
        new_state: State,
        note: Option<&str>,
        now: DateTime<Utc>,
    ) -> StoreResult<()>;
    /// `transition` for a move Gonza recorded by hand (`mark`): the event says it was a mark and
    /// keeps what the move overwrites in the row (note, reason, snooze, stamps), so `undo_mark` can
    /// put the row back exactly as it was.
    fn mark_transition(
        &self,
        job_key: &str,
        new_state: State,
        note: Option<&str>,
        now: DateTime<Utc>,
    ) -> StoreResult<()>;
    /// Takes back the newest event of `job_key` when it is a mark made after `since` and the row
    /// still shows it (same state, no snooze or other move written over it): the row gets its
    /// earlier state and values back and the event is deleted (the history keeps no move that was
    /// taken back). Otherwise nothing changes and the reason is returned.
    fn undo_mark(
        &self,
        job_key: &str,
        since: DateTime<Utc>,
    ) -> StoreResult<Result<UndoableMark, UndoRefusal>>;
    /// The applications whose newest event is a mark made after `since` that `undo_mark` would
    /// take back, by key.
    fn undoable_marks(&self, since: DateTime<Utc>) -> StoreResult<BTreeMap<String, UndoableMark>>;
    fn set_cv_path(&self, job_key: &str, path: &Path) -> StoreResult<()>;
    fn set_screenshot(&self, job_key: &str, path: &Path) -> StoreResult<()>;
    /// Stamps the moment the program clicked submit on a form whose page then did not confirm it.
    /// The click may have gone through, so the daily cap and the company cooldown count it
    /// (`clicked_since`, `company_in_cooldown`) while the digest and the review counter do not.
    fn record_attempt(&self, job_key: &str, now: DateTime<Utc>) -> StoreResult<()>;
    /// For `resolve`: Gonza decided the form must be filled and sent again, so the screenshot of
    /// the earlier try (the next run takes a fresh one) and the attempt stamp are forgotten.
    fn clear_for_retry(&self, job_key: &str) -> StoreResult<()>;
    /// Puts an application off until `until` (`None` brings it back). The row keeps its state:
    /// only the panel's queues look at this.
    fn set_snoozed_until(&self, job_key: &str, until: Option<DateTime<Utc>>) -> StoreResult<()>;
    /// Marks an application as reviewed and approved by Gonza.
    fn approve(&self, job_key: &str) -> StoreResult<()>;
    /// Applications in `state`, best score first.
    fn list_by_state(&self, state: State) -> StoreResult<Vec<ApplicationRow>>;
    /// The first `n` of `list_by_state(state)`.
    fn first_by_state(&self, state: State, n: usize) -> StoreResult<Vec<ApplicationRow>>;
    /// The applications, in any state, that Gonza put off until after `now`.
    fn still_snoozed(&self, now: DateTime<Utc>) -> StoreResult<Vec<ApplicationRow>>;
    /// Applications that are not finished and were never sent: `Scored`, `Prepared` or
    /// `NeedsGonza`, with no submission stamp and no click the page did not confirm (that form may
    /// have gone out). The only ones that may still be withdrawn for a posting that stopped being
    /// eligible. In key order.
    fn unsent_applications(&self) -> StoreResult<Vec<ApplicationRow>>;
    /// The `n` best-scored applications that are still `Scored` or `Prepared`.
    fn top_scored(&self, n: usize) -> StoreResult<Vec<ApplicationRow>>;
    /// Number of applications per state name.
    fn counts(&self) -> StoreResult<BTreeMap<String, i64>>;
    /// Submissions stamped on this UTC day.
    fn submitted_on(&self, day: NaiveDate) -> StoreResult<i64>;
    /// Submissions that Gonza approved before they went out.
    fn approved_submitted_total(&self) -> StoreResult<i64>;
    /// True when an application to this company was submitted, or clicked without the page
    /// confirming it, within the last `days`.
    /// An empty `company_norm` is never in cooldown.
    fn company_in_cooldown(
        &self,
        company_norm: &str,
        days: i64,
        now: DateTime<Utc>,
    ) -> StoreResult<bool>;
    /// Transitions into any of `to_states` recorded at or after `since`.
    fn count_events(&self, to_states: &[State], since: DateTime<Utc>) -> StoreResult<i64>;
    /// Applications clicked submit on at or after `since`: confirmed submissions plus attempts the
    /// page did not confirm. The gate's daily cap counts these.
    fn clicked_since(&self, since: DateTime<Utc>) -> StoreResult<i64>;
    /// Submissions stamped at or after `since`.
    fn submitted_since(&self, since: DateTime<Utc>) -> StoreResult<i64>;
    /// Keys of the applications of a company (its `normalize`d name) that are in one of `states`.
    /// A blank name matches nothing.
    fn job_keys_of_company(&self, company_norm: &str, states: &[State])
    -> StoreResult<Vec<String>>;
}

/// Acts announced before they happen (`intents`): a submit whose run was cut is found, never retried.
pub trait IntentRepo {
    /// Announces an irreversible act before it happens. `Some(id)` when this caller now owns it;
    /// `None` when another intent of the same kind for the same application is still open, in
    /// which case the caller must not act.
    fn begin_intent(
        &self,
        kind: &str,
        job_key: &str,
        now: DateTime<Utc>,
    ) -> StoreResult<Option<i64>>;
    /// Closes an intent with what came of it (`submitted`, `refused`, `interrupted`, ...).
    fn finish_intent(&self, id: i64, outcome: &str, now: DateTime<Utc>) -> StoreResult<()>;
    /// The intents of `kind` that were announced and never closed, oldest first.
    fn open_intents(&self, kind: &str) -> StoreResult<Vec<Intent>>;
    /// Rows of the intents table, closed or not. The tests use it to prove that a path never
    /// wrote one; public because those tests live in the application crate.
    fn count_intents(&self) -> StoreResult<i64>;
    /// Run at start: every submit intent still open is a click whose outcome nobody recorded.
    /// An application still `Prepared` is stamped as attempted (the click may have gone through,
    /// so the daily cap and the company cooldown count it) and handed to Gonza with a reason that
    /// discovery will not overwrite. An application that already moved on, or that no longer
    /// exists, only has its intent closed. Returns the keys handed to Gonza.
    fn recover_open_intents(&self, now: DateTime<Utc>) -> StoreResult<Vec<String>>;
}

/// The emails about job processes that were imported (`emails`); never their bodies.
pub trait EmailRepo {
    fn email_exists(&self, message_id: &str) -> StoreResult<bool>;
    /// Stores an email and its task in one transaction. A message id that is already stored writes
    /// nothing at all (not even the task), so importing the same file twice changes nothing.
    fn add_email(
        &self,
        email: &NewEmail,
        task: Option<&NewTask>,
        now: DateTime<Utc>,
    ) -> StoreResult<EmailAdded>;
    fn get_email(&self, message_id: &str) -> StoreResult<Option<EmailRow>>;
    /// Gives an email that was stored without a posting its posting, and the same to the task it
    /// made. False when the message is unknown or already has a posting: a link is made once.
    fn link_email(&self, message_id: &str, job_key: &str) -> StoreResult<bool>;
    /// Emails received at or after `since` that are not acknowledgements, newest first.
    fn recent_emails(&self, since: DateTime<Utc>) -> StoreResult<Vec<EmailRow>>;
}

/// What Gonza has to do (`gonza_tasks`).
pub trait TaskRepo {
    /// Adds a task for Gonza; false when that message already produced one of this kind.
    fn add_task(&self, task: &NewTask, now: DateTime<Utc>) -> StoreResult<bool>;
    /// Adds a task that came from no email (`jobbot task add`). The same text for the same
    /// posting is not added again while a task like it, from any source, is still open.
    fn add_manual_task(&self, task: &NewTask, now: DateTime<Utc>) -> StoreResult<TaskAdded>;
    fn get_task(&self, id: i64) -> StoreResult<Option<TaskRow>>;
    /// The tasks still to do: soonest due first, the ones without a date last.
    fn open_tasks(&self) -> StoreResult<Vec<TaskRow>>;
    /// Closes an open task. A task that is already closed, or does not exist, is reported, not changed.
    fn complete_task(&self, id: i64) -> StoreResult<TaskCompletion>;
}

/// Small named values the program remembers between runs (`meta`).
pub trait MetaRepo {
    /// A small setting of the run itself, for example `last_discover_at`.
    fn get_meta(&self, key: &str) -> StoreResult<Option<String>>;
    fn set_meta(&self, key: &str, value: &str) -> StoreResult<()>;
    /// Removes `key` from `meta`; one that is not there is fine.
    fn delete_meta(&self, key: &str) -> StoreResult<()>;
}

/// The ledger of every Claude call and its cost (`claude_calls`).
pub trait SpendRepo {
    /// One finished `claude -p` call, successful or not. A failed call costs nothing unless Claude
    /// itself reported a cost.
    fn record_claude_call(
        &self,
        stage: &str,
        at: DateTime<Utc>,
        cost_usd: f64,
        input_tokens: u64,
        output_tokens: u64,
        ok: bool,
    ) -> StoreResult<()>;
    /// Dollars spent on Claude at or after `since`.
    fn claude_spend_since(&self, since: DateTime<Utc>) -> StoreResult<f64>;
    /// Calls and dollars per stage at or after `since`.
    fn claude_calls_by_stage(
        &self,
        since: DateTime<Utc>,
    ) -> StoreResult<BTreeMap<String, (i64, f64)>>;
}

/// Read models over the history, for the panel and the reports.
pub trait HistoryRepo {
    /// When each posting first seen at or after `since` was first seen, oldest first.
    fn first_seen_since(&self, since: DateTime<Utc>) -> StoreResult<Vec<DateTime<Utc>>>;
    /// The history of one application, oldest first. Empty when the key has none.
    fn events_for(&self, job_key: &str) -> StoreResult<Vec<EventRow>>;
    /// State changes recorded from `from` to `until` (both included), newest first, with the
    /// company and title of the posting each one belongs to (an event whose posting is not stored
    /// is left out). Returns how many there are in that span and the first `limit` of them.
    fn moves_between(
        &self,
        from: DateTime<Utc>,
        until: DateTime<Utc>,
        limit: usize,
    ) -> StoreResult<(i64, Vec<MoveRow>)>;
    /// How many of the state changes `moves_between` counts went into each state, by state: the
    /// same span and the same join, so the counts add up to its total whatever its limit cuts.
    fn move_counts_between(
        &self,
        from: DateTime<Utc>,
        until: DateTime<Utc>,
    ) -> StoreResult<Vec<(State, i64)>>;
    /// Applications that reached any of `states` at or after `since`, each counted once even if it
    /// went through (or repeated) a state several times; `count_events` counts every event.
    fn jobs_reaching(&self, states: &[State], since: DateTime<Utc>) -> StoreResult<i64>;
    /// Applications submitted at or after `since` that have reached any of `states` since: the
    /// numerator of a response rate over `submitted_since`.
    fn submitted_since_reaching(&self, since: DateTime<Utc>, states: &[State]) -> StoreResult<i64>;
    /// Applications that reached both `first` and `then` at some point.
    fn jobs_reaching_both(&self, first: State, then: State) -> StoreResult<i64>;
    /// The all-time funnel split by the source the posting came from, sorted by source name. The
    /// stages mean what `submitted_since` and `jobs_reaching` mean for the whole funnel: `sent`
    /// is a submission stamp, the rest "reached that state at least once"; `n` is every
    /// application of the source, sent or not, so the page can show a base with each count.
    fn funnel_by_source(&self) -> StoreResult<Vec<SourceFunnelRow>>;
    /// Every posting first seen at or after `since`, with its application when it has one
    /// (`jobs LEFT JOIN applications`, one query). Withdrawn applications are in too.
    fn cohort_since(&self, since: DateTime<Utc>) -> StoreResult<Vec<CohortRow>>;
}

/// The interview drill's progress (`prep.db`): cards, reviews and the gaps the dossiers added.
pub trait LearnRepo {
    /// Adds cards the compiled bank does not have (the gaps of a dossier). A card whose id is
    /// already stored is left alone, so running the same dossier twice adds nothing. Returns how
    /// many cards were new.
    fn add_cards(&self, cards: &[Question], source: &str, now: DateTime<Utc>)
    -> StoreResult<usize>;
    /// The compiled bank followed by the extra cards. A bank card is never replaced by an extra
    /// card with the same id.
    fn all_questions(&self) -> StoreResult<Vec<Question>>;
    /// The returned reference points into `questions`, not into the store, hence the lifetime `'a`.
    fn next_question<'a>(
        &self,
        questions: &'a [Question],
        now: DateTime<Utc>,
        topic: Option<&str>,
    ) -> StoreResult<Option<&'a Question>>;
    /// Records a grade from 0 to 5: reschedules the card with SM-2 and logs the review.
    fn grade(&self, q: &Question, grade: u8, now: DateTime<Utc>) -> StoreResult<()>;
    /// Topics whose average grade over their last `last_n` reviews is below 4, worst first,
    /// at most `limit` of them.
    fn weak_topics(&self, limit: usize, last_n: usize) -> StoreResult<Vec<(String, f64)>>;
    fn stats(&self, questions: &[Question], now: DateTime<Utc>) -> StoreResult<PrepStats>;
    /// Per topic, sorted by topic name. "Due" counts the same cards `stats` and `next_question`
    /// would serve.
    fn topic_progress(
        &self,
        questions: &[Question],
        now: DateTime<Utc>,
    ) -> StoreResult<Vec<TopicProgress>>;
    /// The cards that dossiers added, oldest first.
    fn gap_rows(&self) -> StoreResult<Vec<GapRow>>;
}

/// What `begin_attempt` found for an attempt id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptStart {
    /// The id is new: the command runs, and `finish_attempt` keeps its reply.
    New,
    /// The command already ran under this id: its reply, to be given again instead of running it.
    Done { status: u16, reply: Vec<u8> },
    /// A command under this id started and never finished: it is still running, or its process
    /// died. It is not run again.
    Unfinished,
    /// The id was used for another command or another request body.
    Mismatch,
}

/// The commands that came with an attempt id of their own (`command_attempts`, UNIQUE id): a
/// repeated id gets the first reply back and the command runs once.
pub trait AttemptRepo {
    /// Starts the attempt `id` of `command` with `request` (its body), unless the id is known.
    /// Only a fingerprint of the body is kept, to tell a repeat from another request. Attempts
    /// begun more than a week before `now` are dropped first.
    fn begin_attempt(
        &self,
        id: &str,
        command: &str,
        request: &[u8],
        now: DateTime<Utc>,
    ) -> StoreResult<AttemptStart>;
    /// Keeps the reply of the attempt `id`, whose command has run.
    fn finish_attempt(&self, id: &str, status: u16, reply: &[u8]) -> StoreResult<()>;
    /// Drops the attempt `id`, whose command failed, so the same id may try again.
    fn forget_attempt(&self, id: &str) -> StoreResult<()>;
}

/// A page kept to be asked for again with its validator: a host whose page did not change answers
/// 304 (not modified) and the kept body stands in for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPage {
    /// The `ETag` header the page came with.
    pub etag: Option<String>,
    /// The `Last-Modified` header the page came with.
    pub last_modified: Option<String>,
    pub body: String,
}

/// What the job boards' GETs of one run leave for the next (`http_cooldowns`, `http_pages`): the
/// hosts that asked to be left alone and until when, and the pages that can be asked for again
/// with a validator.
pub trait HttpMemory: Send + Sync {
    /// The hosts whose rest ends after `now`, with the end of each, by host. The rests that are
    /// over by `now` are forgotten.
    fn resting_hosts(&self, now: DateTime<Utc>) -> StoreResult<Vec<(String, DateTime<Utc>)>>;
    /// `host` is left alone until `until`; a later rest of the same host replaces it.
    fn rest_host(&self, host: &str, until: DateTime<Utc>) -> StoreResult<()>;
    /// The page kept for `url` (its query included), if any. Asking counts as using it at `now`.
    fn recall_page(&self, url: &str, now: DateTime<Utc>) -> StoreResult<Option<StoredPage>>;
    /// Keeps `page` for `url`, replacing what was there. Pages nobody asked for in a month go.
    fn store_page(&self, url: &str, page: &StoredPage, now: DateTime<Utc>) -> StoreResult<()>;
}

/// Every aggregate of `jobbot.db` behind one handle: what most use cases receive. Implemented for
/// anything that implements all of them.
pub trait Store:
    JobRepo
    + ApplicationRepo
    + IntentRepo
    + EmailRepo
    + TaskRepo
    + MetaRepo
    + SpendRepo
    + HistoryRepo
    + AttemptRepo
    + Send
    + Sync
{
}

impl<T> Store for T where
    T: JobRepo
        + ApplicationRepo
        + IntentRepo
        + EmailRepo
        + TaskRepo
        + MetaRepo
        + SpendRepo
        + HistoryRepo
        + AttemptRepo
        + Send
        + Sync
        + ?Sized
{
}
