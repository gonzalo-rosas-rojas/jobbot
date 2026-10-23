//! SQLite tracker: jobs, applications and the application state machine.
//! Timestamps are stored as UTC ISO-8601 text (`2026-10-01T12:00:00Z`), which sorts
//! chronologically as plain text.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use anyhow::Context;
use chrono::{DateTime, NaiveDate, SecondsFormat, TimeDelta, Utc};
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params, params_from_iter};

use crate::models::Job;
use crate::text::{link_base, normalize};

// The state machine lives in the pure domain crate; the tracker stores the states.
pub use jobbot_domain::state::{SUBMITTED_STATES, State};

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

#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    #[error("illegal transition {from} -> {to}")]
    IllegalTransition { from: String, to: String },
    #[error("unknown job key {0}")]
    UnknownKey(String),
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
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

/// An email to remember. The body is never stored, only what the importer was given.
#[derive(Debug, Clone)]
pub struct NewEmail {
    pub message_id: String,
    pub thread_id: String,
    pub from_addr: String,
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
    pub from_addr: String,
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

/// Version of the database layout this build writes (`PRAGMA user_version`). Raise it when a
/// migration is added: `Tracker::open` copies an older database with data before it moves it up.
const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS jobs (
  key TEXT PRIMARY KEY, source TEXT NOT NULL, company TEXT NOT NULL, company_norm TEXT NOT NULL,
  title TEXT NOT NULL, title_lc TEXT NOT NULL, url TEXT NOT NULL, apply_url TEXT NOT NULL,
  location TEXT NOT NULL, remote INTEGER NOT NULL, description TEXT NOT NULL,
  salary_min REAL, salary_max REAL, posted_at TEXT,
  score REAL NOT NULL, score_detail TEXT NOT NULL, first_seen TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS applications (
  job_key TEXT PRIMARY KEY REFERENCES jobs(key), company_norm TEXT NOT NULL, state TEXT NOT NULL,
  note TEXT, needs_reason TEXT, cv_path TEXT, screenshot_path TEXT,
  approved INTEGER NOT NULL DEFAULT 0, submitted_at TEXT, updated_at TEXT NOT NULL,
  attempted_at TEXT, snoozed_until TEXT
);
CREATE TABLE IF NOT EXISTS events (
  id INTEGER PRIMARY KEY AUTOINCREMENT, job_key TEXT NOT NULL, at TEXT NOT NULL,
  from_state TEXT, to_state TEXT NOT NULL, note TEXT
);
CREATE TABLE IF NOT EXISTS meta (
  key TEXT PRIMARY KEY, value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS claude_calls (
  id INTEGER PRIMARY KEY AUTOINCREMENT, stage TEXT NOT NULL, at TEXT NOT NULL,
  cost_usd REAL NOT NULL, input_tokens INTEGER NOT NULL, output_tokens INTEGER NOT NULL,
  ok INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS emails (
  message_id TEXT PRIMARY KEY, thread_id TEXT NOT NULL, from_addr TEXT NOT NULL, subject TEXT NOT NULL,
  received_at TEXT NOT NULL, job_key TEXT, kind TEXT NOT NULL, summary TEXT NOT NULL,
  imported_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS gonza_tasks (
  id INTEGER PRIMARY KEY AUTOINCREMENT, job_key TEXT, kind TEXT NOT NULL, text TEXT NOT NULL,
  due_at TEXT, status TEXT NOT NULL DEFAULT 'open', created_at TEXT NOT NULL, source_message_id TEXT,
  link TEXT,
  UNIQUE(source_message_id, kind)
);
CREATE INDEX IF NOT EXISTS idx_emails_received ON emails (received_at);
CREATE INDEX IF NOT EXISTS idx_gonza_tasks_status ON gonza_tasks (status, due_at);
CREATE INDEX IF NOT EXISTS idx_jobs_company_title ON jobs (company_norm, title_lc);
CREATE INDEX IF NOT EXISTS idx_applications_state ON applications (state);
CREATE INDEX IF NOT EXISTS idx_events_to_state ON events (to_state, at);
-- What the panel asks on every poll, answered from these two small indexes instead of from the
-- tables. A job's text sits before its score, so reading the score of every posting means walking
-- every description; this index holds the score (and the key the applications join on) next to the
-- moment the posting was seen. The other one answers `state_since` of each row of an application
-- list (the latest event into its current state) with one lookup instead of a pass over every
-- event that went into that state.
CREATE INDEX IF NOT EXISTS idx_jobs_first_seen ON jobs (first_seen, key, score);
CREATE INDEX IF NOT EXISTS idx_events_job_state ON events (job_key, to_state, at);
-- Write-ahead log of the irreversible acts: a row is written BEFORE the program clicks submit and
-- closed after the result is recorded. A row still `started` at the next start is a click whose
-- outcome nobody saw (the process died in between). The partial unique index makes a second
-- open intent for the same application impossible, so two runs cannot click the same form.
CREATE TABLE IF NOT EXISTS intents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    job_key TEXT NOT NULL,
    state TEXT NOT NULL,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    outcome TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_intents_one_open ON intents (kind, job_key) WHERE state = 'started';
";

/// `state_since` is the latest event into the row's current state: an application that went to
/// `needs_gonza`, back to the queue and to `needs_gonza` again has waited since the second time.
const APPLICATION_SELECT: &str = "SELECT a.job_key, a.company_norm, a.state, a.note, a.needs_reason, \
     a.cv_path, a.screenshot_path, a.approved, a.submitted_at, a.updated_at, \
     j.title, j.company, j.score, j.url, j.source, \
     a.snoozed_until, j.posted_at, j.first_seen, j.salary_min, j.salary_max, \
     (SELECT MAX(e.at) FROM events e WHERE e.job_key = a.job_key AND e.to_state = a.state) \
     FROM applications a JOIN jobs j ON j.key = a.job_key";

const JOB_SELECT: &str = "SELECT key, source, company, title, url, apply_url, location, remote, \
     description, salary_min, salary_max, posted_at FROM jobs";

/// WAL lets the panel read while a run writes; `FULL` keeps every commit durable, which the
/// write-ahead intents depend on. A database that cannot switch right now (another process holds
/// it) keeps working in its current mode.
fn use_wal(conn: &Connection) {
    let switched =
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0));
    if !matches!(switched.as_deref(), Ok(mode) if mode.eq_ignore_ascii_case("wal")) {
        eprintln!("aviso: la base no pudo pasar a modo WAL; sigue en su modo actual");
    }
    let _ = conn.pragma_update(None, "synchronous", "FULL");
}

/// Copies the database (a consistent snapshot, `VACUUM INTO`) to `backups/jobbot-before-v<N>.db`
/// next to it. One copy per version: if it exists, an earlier try already saved the state from
/// before the migration. The copy is written under a `.partial` name and renamed when finished, so
/// a file with the final name is always a whole database, never what a cut try left behind.
fn back_up_before_migrating(conn: &Connection, db_path: &Path) -> anyhow::Result<()> {
    let dir = db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("backups");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let copy = dir.join(format!("jobbot-before-v{SCHEMA_VERSION}.db"));
    if copy.exists() {
        return Ok(());
    }
    let partial = dir.join(format!("jobbot-before-v{SCHEMA_VERSION}.db.partial"));
    if partial.exists() {
        std::fs::remove_file(&partial)
            .with_context(|| format!("removing the leftover {}", partial.display()))?;
    }
    conn.execute("VACUUM INTO ?1", [partial.to_string_lossy().as_ref()])
        .with_context(|| format!("copying the database to {}", partial.display()))?;
    std::fs::rename(&partial, &copy)
        .with_context(|| format!("finishing the copy at {}", copy.display()))?;
    Ok(())
}

fn to_text(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn from_text(column: usize, text: &str) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .map(|at| at.with_timezone(&Utc))
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(e)))
}

fn optional_from_text(
    column: usize,
    text: Option<String>,
) -> rusqlite::Result<Option<DateTime<Utc>>> {
    text.map(|t| from_text(column, &t)).transpose()
}

fn state_from_text(column: usize, text: &str) -> rusqlite::Result<State> {
    State::parse(text).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            Type::Text,
            format!("unknown state {text:?}").into(),
        )
    })
}

fn email_kind_from_text(column: usize, text: &str) -> rusqlite::Result<EmailKind> {
    EmailKind::parse(text).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            Type::Text,
            format!("unknown email kind {text:?}").into(),
        )
    })
}

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
        from_addr: row.get(2)?,
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

/// Rebuilds a `Job` from a `JOB_SELECT` row. The external id is the key without its `source:` prefix.
fn job_from_row(row: &Row<'_>) -> rusqlite::Result<Job> {
    let key: String = row.get(0)?;
    let source: String = row.get(1)?;
    let external_id = key
        .strip_prefix(&format!("{source}:"))
        .unwrap_or(&key)
        .to_string();
    Ok(Job {
        source,
        external_id,
        company: row.get(2)?,
        title: row.get(3)?,
        url: row.get(4)?,
        apply_url: row.get(5)?,
        location: row.get(6)?,
        remote: row.get(7)?,
        description: row.get(8)?,
        salary_min_monthly_usd: row.get(9)?,
        salary_max_monthly_usd: row.get(10)?,
        posted_at: optional_from_text(11, row.get(11)?)?,
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

/// The application database. The connection sits behind a `Mutex`, so a `Tracker`
/// can be shared by reference across threads and `.await` points.
pub struct Tracker {
    conn: Mutex<Connection>,
}

impl Tracker {
    /// Opens (creating it and its parent directory if needed) the database at `path`. The file runs
    /// in WAL mode with every commit durable. A database made by an earlier version, one that
    /// already holds data and whose `user_version` is behind, is copied to `backups/` next to it
    /// before anything is changed.
    pub fn open(path: &Path) -> anyhow::Result<Tracker> {
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let had_data = std::fs::metadata(path).is_ok_and(|meta| meta.len() > 0);
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.busy_timeout(Duration::from_secs(5))?;
        let found: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if had_data && found < SCHEMA_VERSION {
            back_up_before_migrating(&conn, path)?;
        }
        use_wal(&conn);
        let tracker = Self::from_connection(conn)?;
        if found < SCHEMA_VERSION {
            tracker
                .lock()
                .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(tracker)
    }

    pub fn in_memory() -> anyhow::Result<Tracker> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> anyhow::Result<Tracker> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        Self::add_missing_columns(&conn)?;
        Ok(Tracker {
            conn: Mutex::new(conn),
        })
    }

    /// `CREATE TABLE IF NOT EXISTS` leaves a database made by an earlier version as it was, so
    /// columns added later are added here. Another process may add the same column first.
    fn add_missing_columns(conn: &Connection) -> anyhow::Result<()> {
        for (table, column) in [
            ("applications", "attempted_at"),
            ("gonza_tasks", "link"),
            ("applications", "snoozed_until"),
        ] {
            let has_column = conn
                .prepare(&format!(
                    "SELECT 1 FROM pragma_table_info('{table}') WHERE name = '{column}'"
                ))?
                .exists([])?;
            if !has_column {
                match conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} TEXT")) {
                    Err(e) if e.to_string().contains("duplicate column name") => {}
                    other => other?,
                }
            }
        }
        Ok(())
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock cannot leave a statement half done, so a
        // poisoned lock is safe to reuse.
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    // ---- jobs

    /// True when this exact posting, or the same title at the same company, is known.
    /// Postings with an empty company or title only match by key.
    pub fn has_posting(&self, job: &Job) -> anyhow::Result<bool> {
        let company_norm = normalize(&job.company);
        let title_lc = job.title.trim().to_lowercase();
        let conn = self.lock();
        let found = conn
            .query_row(
                "SELECT 1 FROM jobs WHERE key = ?1 \
                 OR (?2 <> '' AND ?3 <> '' AND company_norm = ?2 AND title_lc = ?3) LIMIT 1",
                params![job.key(), company_norm, title_lc],
                |_| Ok(()),
            )
            .optional()?;
        Ok(found.is_some())
    }

    /// The key of the stored posting that `job` stands for, or `None` when it is new. In order: the
    /// same key; the same link (`link_base`: the query, the fragment and a final `/application` do
    /// not count, and `/jobs/1` is not `/jobs/12`); the same title at the same company.
    pub fn find_job_key(&self, job: &Job) -> anyhow::Result<Option<String>> {
        let conn = self.lock();
        let by_key: Option<String> = conn
            .query_row("SELECT key FROM jobs WHERE key = ?1", [job.key()], |row| {
                row.get(0)
            })
            .optional()?;
        if by_key.is_some() {
            return Ok(by_key);
        }
        let base = link_base(&job.url);
        if !base.is_empty() {
            let mut stmt = conn.prepare(
                "SELECT key, url, apply_url FROM jobs \
                 WHERE instr(lower(url), ?1) > 0 OR instr(lower(apply_url), ?1) > 0 ORDER BY key",
            )?;
            let rows = stmt.query_map([&base], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                ))
            })?;
            for row in rows {
                let (key, url, apply_url) = row?;
                if link_base(&url) == base || link_base(&apply_url) == base {
                    return Ok(Some(key));
                }
            }
        }
        let company_norm = normalize(&job.company);
        let title_lc = job.title.trim().to_lowercase();
        if company_norm.is_empty() || title_lc.is_empty() {
            return Ok(None);
        }
        Ok(conn
            .query_row(
                "SELECT key FROM jobs WHERE company_norm = ?1 AND title_lc = ?2 \
                 ORDER BY first_seen DESC LIMIT 1",
                params![company_norm, title_lc],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// `has_posting` for discovery: the same title at the same company only counts as a duplicate
    /// while the stored posting is still live, that is, dated (or, with no date, first seen) at or
    /// after `repost_cutoff`. A role the company posts again after the old one went stale or was
    /// withdrawn is a new posting. The same key is always the same posting.
    pub fn has_posting_since(
        &self,
        job: &Job,
        repost_cutoff: DateTime<Utc>,
    ) -> anyhow::Result<bool> {
        let company_norm = normalize(&job.company);
        let title_lc = job.title.trim().to_lowercase();
        let conn = self.lock();
        let found = conn
            .query_row(
                "SELECT 1 FROM jobs WHERE key = ?1 \
                 OR (?2 <> '' AND ?3 <> '' AND company_norm = ?2 AND title_lc = ?3 \
                     AND COALESCE(posted_at, first_seen) >= ?4) LIMIT 1",
                params![job.key(), company_norm, title_lc, to_text(repost_cutoff)],
                |_| Ok(()),
            )
            .optional()?;
        Ok(found.is_some())
    }

    /// Stores a scored posting. A job that is already stored is left untouched.
    pub fn add_job(
        &self,
        job: &Job,
        score: f64,
        detail: &BTreeMap<String, f64>,
        now: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        let detail_json = serde_json::to_string(detail)?;
        let conn = self.lock();
        conn.execute(
            "INSERT OR IGNORE INTO jobs (key, source, company, company_norm, title, title_lc, url, \
             apply_url, location, remote, description, salary_min, salary_max, posted_at, score, \
             score_detail, first_seen) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                job.key(),
                job.source,
                job.company,
                normalize(&job.company),
                job.title,
                job.title.trim().to_lowercase(),
                job.url,
                job.apply_url,
                job.location,
                job.remote,
                job.description,
                job.salary_min_monthly_usd,
                job.salary_max_monthly_usd,
                job.posted_at.map(to_text),
                score,
                detail_json,
                to_text(now),
            ],
        )?;
        Ok(())
    }

    /// Brings a stored posting up to date with what its source reports today: apply link, posting
    /// date, location and remote flag (a source may fix how it reads them after the posting was
    /// stored, and a posting's own data changes). The score, its detail, the pay and every other
    /// field stay as they were scored. Returns whether anything changed; one UPDATE.
    ///
    /// A blank never replaces a good value: not an empty apply link, not an empty location, not a
    /// missing date (a fetch that lacks them says nothing about them). A posting that is not
    /// stored is not created.
    pub fn refresh_posting(&self, job: &Job) -> anyhow::Result<bool> {
        let conn = self.lock();
        let old_apply_url: Option<String> = conn
            .query_row(
                "SELECT apply_url FROM jobs WHERE key = ?1",
                [job.key()],
                |row| row.get(0),
            )
            .optional()?;
        let changed = conn.execute(
            "UPDATE jobs SET \
               apply_url = CASE WHEN ?1 <> '' THEN ?1 ELSE apply_url END, \
               posted_at = COALESCE(?2, posted_at), \
               location = CASE WHEN ?3 <> '' THEN ?3 ELSE location END, \
               remote = ?4 \
             WHERE key = ?5 AND ( \
               (?1 <> '' AND apply_url <> ?1) \
               OR (?2 IS NOT NULL AND posted_at IS NOT ?2) \
               OR (?3 <> '' AND location <> ?3) \
               OR remote <> ?4)",
            params![
                job.apply_url,
                job.posted_at.map(to_text),
                job.location,
                job.remote,
                job.key()
            ],
        )?;
        // The review (the approval and the dry-run screenshot) belongs to the form at the OLD link.
        // A posting that now points to another form must be looked at again before anything is
        // sent; what was already sent or finished keeps its history.
        let relinked =
            !job.apply_url.is_empty() && old_apply_url.is_some_and(|old| old != job.apply_url);
        if changed > 0 && relinked {
            conn.execute(
                "UPDATE applications SET approved = 0, screenshot_path = NULL \
                 WHERE job_key = ?1 AND state IN ('prepared', 'needs_gonza')",
                [job.key()],
            )?;
        }
        Ok(changed > 0)
    }

    pub fn get_job(&self, key: &str) -> anyhow::Result<Option<Job>> {
        let conn = self.lock();
        let job = conn
            .query_row(&format!("{JOB_SELECT} WHERE key = ?1"), [key], job_from_row)
            .optional()?;
        Ok(job)
    }

    /// Stored postings that never got an application: discovery scored them below the
    /// threshold. `rescore` looks at them again when the criteria change.
    pub fn jobs_without_application(&self) -> anyhow::Result<Vec<Job>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "{JOB_SELECT} j WHERE NOT EXISTS \
             (SELECT 1 FROM applications a WHERE a.job_key = j.key) ORDER BY key"
        ))?;
        let jobs = stmt
            .query_map([], job_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(jobs)
    }

    /// Replaces the stored score and its detail (used by `rescore`).
    pub fn set_score(
        &self,
        key: &str,
        score: f64,
        detail: &BTreeMap<String, f64>,
    ) -> anyhow::Result<()> {
        let detail_json = serde_json::to_string(detail)?;
        let conn = self.lock();
        let changed = conn.execute(
            "UPDATE jobs SET score = ?1, score_detail = ?2 WHERE key = ?3",
            params![score, detail_json, key],
        )?;
        if changed == 0 {
            return Err(TrackerError::UnknownKey(key.to_string()).into());
        }
        Ok(())
    }

    /// When this posting was first seen (stored), or `None` when it is not stored.
    pub fn job_first_seen(&self, key: &str) -> anyhow::Result<Option<DateTime<Utc>>> {
        let conn = self.lock();
        let text: Option<String> = conn
            .query_row("SELECT first_seen FROM jobs WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(optional_from_text(0, text)?)
    }

    /// When the oldest stored job was first seen.
    pub fn first_seen_at(&self) -> anyhow::Result<Option<DateTime<Utc>>> {
        let conn = self.lock();
        let text: Option<String> =
            conn.query_row("SELECT MIN(first_seen) FROM jobs", [], |row| row.get(0))?;
        Ok(optional_from_text(0, text)?)
    }

    /// When each posting first seen at or after `since` was first seen, oldest first.
    pub fn first_seen_since(&self, since: DateTime<Utc>) -> anyhow::Result<Vec<DateTime<Utc>>> {
        let conn = self.lock();
        let mut stmt =
            conn.prepare("SELECT first_seen FROM jobs WHERE first_seen >= ?1 ORDER BY first_seen")?;
        let rows = stmt.query_map([to_text(since)], |row| {
            let text: String = row.get(0)?;
            from_text(0, &text)
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ---- run settings

    /// A small setting of the run itself, for example `last_discover_at`.
    pub fn get_meta(&self, key: &str) -> anyhow::Result<Option<String>> {
        let conn = self.lock();
        Ok(conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Removes `key` from `meta`; one that is not there is fine.
    pub fn delete_meta(&self, key: &str) -> anyhow::Result<()> {
        let conn = self.lock();
        conn.execute("DELETE FROM meta WHERE key = ?1", params![key])?;
        Ok(())
    }

    // ---- applications

    /// Starts an application at `Scored`. Does nothing if one already exists.
    pub fn create_application(&self, job_key: &str, now: DateTime<Utc>) -> anyhow::Result<()> {
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
            return Err(TrackerError::UnknownKey(job_key.to_string()).into());
        };
        let at = to_text(now);
        tx.execute(
            "INSERT INTO applications (job_key, company_norm, state, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![job_key, company_norm, State::Scored.as_str(), at],
        )?;
        tx.execute(
            "INSERT INTO events (job_key, at, from_state, to_state, note) VALUES (?1, ?2, NULL, ?3, NULL)",
            params![job_key, at, State::Scored.as_str()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_application(&self, job_key: &str) -> anyhow::Result<Option<ApplicationRow>> {
        let conn = self.lock();
        let mut rows = query_applications(&conn, "WHERE a.job_key = ?1", [job_key])?;
        Ok(rows.pop())
    }

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
    ) -> Result<(), TrackerError> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current_text: Option<String> = tx
            .query_row(
                "SELECT state FROM applications WHERE job_key = ?1",
                [job_key],
                |row| row.get(0),
            )
            .optional()?;
        let Some(current_text) = current_text else {
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
        let note = note.map(crate::outbound::plain_line);
        let note = note.as_deref();
        tx.execute(
            "UPDATE applications SET state = ?1, updated_at = ?2, note = ?3, \
             needs_reason = CASE WHEN ?1 = 'needs_gonza' THEN ?3 \
                                 WHEN state = 'needs_gonza' THEN NULL \
                                 ELSE needs_reason END, \
             submitted_at = CASE WHEN ?1 = 'submitted' THEN ?2 ELSE submitted_at END, \
             snoozed_until = NULL \
             WHERE job_key = ?4",
            params![new_state.as_str(), at, note, job_key],
        )?;
        tx.execute(
            "INSERT INTO events (job_key, at, from_state, to_state, note) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![job_key, at, current.as_str(), new_state.as_str(), note],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn update_column(
        &self,
        column: &str,
        value: Option<String>,
        job_key: &str,
    ) -> anyhow::Result<()> {
        let conn = self.lock();
        let changed = conn.execute(
            &format!("UPDATE applications SET {column} = ?1 WHERE job_key = ?2"),
            params![value, job_key],
        )?;
        if changed == 0 {
            return Err(TrackerError::UnknownKey(job_key.to_string()).into());
        }
        Ok(())
    }

    pub fn set_cv_path(&self, job_key: &str, path: &Path) -> anyhow::Result<()> {
        self.update_column(
            "cv_path",
            Some(path.to_string_lossy().into_owned()),
            job_key,
        )
    }

    pub fn set_screenshot(&self, job_key: &str, path: &Path) -> anyhow::Result<()> {
        self.update_column(
            "screenshot_path",
            Some(path.to_string_lossy().into_owned()),
            job_key,
        )
    }

    /// Stamps the moment the program clicked submit on a form whose page then did not confirm it.
    /// The click may have gone through, so the daily cap and the company cooldown count it
    /// (`clicked_since`, `company_in_cooldown`) while the digest and the review counter do not.
    pub fn record_attempt(&self, job_key: &str, now: DateTime<Utc>) -> anyhow::Result<()> {
        self.update_column("attempted_at", Some(to_text(now)), job_key)
    }

    /// Announces an irreversible act before it happens. `Some(id)` when this caller now owns it;
    /// `None` when another intent of the same kind for the same application is still open, in
    /// which case the caller must not act.
    pub fn begin_intent(
        &self,
        kind: &str,
        job_key: &str,
        now: DateTime<Utc>,
    ) -> anyhow::Result<Option<i64>> {
        let conn = self.lock();
        let inserted = conn.execute(
            "INSERT INTO intents (kind, job_key, state, started_at) VALUES (?1, ?2, 'started', ?3) \
             ON CONFLICT DO NOTHING",
            params![kind, job_key, to_text(now)],
        )?;
        Ok((inserted == 1).then(|| conn.last_insert_rowid()))
    }

    /// Closes an intent with what came of it (`submitted`, `refused`, `interrupted`, ...).
    pub fn finish_intent(&self, id: i64, outcome: &str, now: DateTime<Utc>) -> anyhow::Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE intents SET state = 'finished', finished_at = ?2, outcome = ?3 WHERE id = ?1",
            params![id, to_text(now), outcome],
        )?;
        Ok(())
    }

    /// The intents of `kind` that were announced and never closed, oldest first.
    pub fn open_intents(&self, kind: &str) -> anyhow::Result<Vec<Intent>> {
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

    /// Rows of the intents table, closed or not (a test hook for "this never wrote one").
    #[cfg(test)]
    pub(crate) fn count_intents(&self) -> anyhow::Result<i64> {
        Ok(self
            .lock()
            .query_row("SELECT COUNT(*) FROM intents", [], |row| row.get(0))?)
    }

    /// Run at start: every submit intent still open is a click whose outcome nobody recorded.
    /// An application still `Prepared` is stamped as attempted (the click may have gone through,
    /// so the daily cap and the company cooldown count it) and handed to Gonza with a reason that
    /// discovery will not overwrite. An application that already moved on, or that no longer
    /// exists, only has its intent closed. Returns the keys handed to Gonza.
    pub fn recover_open_intents(&self, now: DateTime<Utc>) -> anyhow::Result<Vec<String>> {
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

    /// For `resolve`: Gonza decided the form must be filled and sent again, so the screenshot of
    /// the earlier try (the next run takes a fresh one) and the attempt stamp are forgotten.
    pub fn clear_for_retry(&self, job_key: &str) -> anyhow::Result<()> {
        let conn = self.lock();
        let changed = conn.execute(
            "UPDATE applications SET screenshot_path = NULL, attempted_at = NULL WHERE job_key = ?1",
            [job_key],
        )?;
        if changed == 0 {
            return Err(TrackerError::UnknownKey(job_key.to_string()).into());
        }
        Ok(())
    }

    /// Puts an application off until `until` (`None` brings it back). The row keeps its state:
    /// only the panel's queues look at this.
    pub fn set_snoozed_until(
        &self,
        job_key: &str,
        until: Option<DateTime<Utc>>,
    ) -> anyhow::Result<()> {
        self.update_column("snoozed_until", until.map(to_text), job_key)
    }

    /// Marks an application as reviewed and approved by Gonza.
    pub fn approve(&self, job_key: &str) -> anyhow::Result<()> {
        let conn = self.lock();
        let changed = conn.execute(
            "UPDATE applications SET approved = 1 WHERE job_key = ?1",
            [job_key],
        )?;
        if changed == 0 {
            return Err(TrackerError::UnknownKey(job_key.to_string()).into());
        }
        Ok(())
    }

    // ---- queries

    /// Applications in `state`, best score first.
    pub fn list_by_state(&self, state: State) -> anyhow::Result<Vec<ApplicationRow>> {
        let conn = self.lock();
        Ok(query_applications(
            &conn,
            "WHERE a.state = ?1 ORDER BY j.score DESC, a.job_key",
            [state.as_str()],
        )?)
    }

    /// Applications that are not finished and were never sent: `Scored`, `Prepared` or
    /// `NeedsGonza`, with no submission stamp and no click the page did not confirm (that form may
    /// have gone out). The only ones that may still be withdrawn for a posting that stopped being
    /// eligible. In key order.
    pub fn unsent_applications(&self) -> anyhow::Result<Vec<ApplicationRow>> {
        let conn = self.lock();
        Ok(query_applications(
            &conn,
            "WHERE a.state IN ('scored', 'prepared', 'needs_gonza') \
             AND a.submitted_at IS NULL AND a.attempted_at IS NULL ORDER BY a.job_key",
            [],
        )?)
    }

    /// The `n` best-scored applications that are still `Scored` or `Prepared`.
    pub fn top_scored(&self, n: usize) -> anyhow::Result<Vec<ApplicationRow>> {
        let conn = self.lock();
        let limit = i64::try_from(n).unwrap_or(i64::MAX);
        Ok(query_applications(
            &conn,
            "WHERE a.state IN ('scored', 'prepared') ORDER BY j.score DESC, a.job_key LIMIT ?1",
            [limit],
        )?)
    }

    /// Number of applications per state name.
    pub fn counts(&self) -> anyhow::Result<BTreeMap<String, i64>> {
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
    pub fn submitted_on(&self, day: NaiveDate) -> anyhow::Result<i64> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM applications WHERE substr(submitted_at, 1, 10) = ?1",
            [day.to_string()],
            |row| row.get(0),
        )?)
    }

    /// Submissions that Gonza approved before they went out.
    pub fn approved_submitted_total(&self) -> anyhow::Result<i64> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM applications WHERE approved = 1 AND submitted_at IS NOT NULL",
            [],
            |row| row.get(0),
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
    ) -> anyhow::Result<bool> {
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

    /// Transitions into any of `to_states` recorded at or after `since`.
    pub fn count_events(&self, to_states: &[State], since: DateTime<Utc>) -> anyhow::Result<i64> {
        if to_states.is_empty() {
            return Ok(0);
        }
        let marks = vec!["?"; to_states.len()].join(", ");
        let since_text = to_text(since);
        let conn = self.lock();
        let count = conn.query_row(
            &format!("SELECT COUNT(*) FROM events WHERE to_state IN ({marks}) AND at >= ?"),
            params_from_iter(
                to_states
                    .iter()
                    .map(|state| state.as_str())
                    .chain(std::iter::once(since_text.as_str())),
            ),
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// The history of one application, oldest first. Empty when the key has none.
    pub fn events_for(&self, job_key: &str) -> anyhow::Result<Vec<EventRow>> {
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
    ) -> anyhow::Result<(i64, Vec<MoveRow>)> {
        let (from, until) = (to_text(from), to_text(until));
        let conn = self.lock();
        let total = conn.query_row(
            "SELECT COUNT(*) FROM events e JOIN jobs j ON j.key = e.job_key \
             WHERE e.at >= ?1 AND e.at <= ?2",
            [&from, &until],
            |row| row.get(0),
        )?;
        let mut stmt = conn.prepare(
            "SELECT e.at, e.job_key, j.company, j.title, e.to_state \
             FROM events e JOIN jobs j ON j.key = e.job_key \
             WHERE e.at >= ?1 AND e.at <= ?2 ORDER BY e.at DESC, e.id DESC LIMIT ?3",
        )?;
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
    ) -> anyhow::Result<Vec<(State, i64)>> {
        let (from, until) = (to_text(from), to_text(until));
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT e.to_state, COUNT(*) FROM events e JOIN jobs j ON j.key = e.job_key              WHERE e.at >= ?1 AND e.at <= ?2 GROUP BY e.to_state",
        )?;
        let rows = stmt.query_map([&from, &until], |row| {
            let to: String = row.get(0)?;
            Ok((state_from_text(0, &to)?, row.get::<_, i64>(1)?))
        })?;
        let mut counts = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        counts.sort_by_key(|(state, _)| *state);
        Ok(counts)
    }

    /// How the score of a stored posting was made up, by criterion.
    pub fn score_detail(&self, key: &str) -> anyhow::Result<BTreeMap<String, f64>> {
        let conn = self.lock();
        let text: Option<String> = conn
            .query_row(
                "SELECT score_detail FROM jobs WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        let Some(text) = text else {
            return Err(TrackerError::UnknownKey(key.to_string()).into());
        };
        serde_json::from_str(&text)
            .with_context(|| format!("score_detail of {key} is not valid JSON"))
    }

    /// Applications that reached any of `states` at or after `since`, each counted once even if it
    /// went through (or repeated) a state several times; `count_events` counts every event.
    pub fn jobs_reaching(&self, states: &[State], since: DateTime<Utc>) -> anyhow::Result<i64> {
        if states.is_empty() {
            return Ok(0);
        }
        let marks = vec!["?"; states.len()].join(", ");
        let since_text = to_text(since);
        let conn = self.lock();
        let count = conn.query_row(
            &format!(
                "SELECT COUNT(DISTINCT job_key) FROM events WHERE to_state IN ({marks}) AND at >= ?"
            ),
            params_from_iter(
                states
                    .iter()
                    .map(|state| state.as_str())
                    .chain(std::iter::once(since_text.as_str())),
            ),
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Applications clicked submit on at or after `since`: confirmed submissions plus attempts the
    /// page did not confirm. The gate's daily cap counts these.
    pub fn clicked_since(&self, since: DateTime<Utc>) -> anyhow::Result<i64> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM applications WHERE submitted_at >= ?1 OR attempted_at >= ?1",
            [to_text(since)],
            |row| row.get(0),
        )?)
    }

    /// The all-time funnel split by the source the posting came from, sorted by source name. The
    /// stages mean what `submitted_since` and `jobs_reaching` mean for the whole funnel: `sent`
    /// is a submission stamp, the rest "reached that state at least once"; `n` is every
    /// application of the source, sent or not, so the page can show a base with each count.
    pub fn funnel_by_source(&self) -> anyhow::Result<Vec<SourceFunnelRow>> {
        let reached = |states: &str| {
            format!(
                "SUM(EXISTS(SELECT 1 FROM events e WHERE e.job_key = a.job_key AND e.to_state IN ({states})))"
            )
        };
        let sql = format!(
            "SELECT j.source, COUNT(*), SUM(a.submitted_at IS NOT NULL), {}, {}, {}, {} \
             FROM applications a JOIN jobs j ON j.key = a.job_key GROUP BY j.source ORDER BY j.source",
            reached("'replied', 'technical_test', 'interview', 'offer', 'rejected'"),
            reached("'technical_test'"),
            reached("'interview'"),
            reached("'offer'"),
        );
        let conn = self.lock();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok(SourceFunnelRow {
                source: row.get(0)?,
                n: row.get(1)?,
                sent: row.get(2)?,
                answered: row.get(3)?,
                tests: row.get(4)?,
                interviews: row.get(5)?,
                offers: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Submissions stamped at or after `since`.
    pub fn submitted_since(&self, since: DateTime<Utc>) -> anyhow::Result<i64> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM applications WHERE submitted_at IS NOT NULL AND submitted_at >= ?1",
            [to_text(since)],
            |row| row.get(0),
        )?)
    }

    /// Every posting first seen at or after `since`, with its application when it has one
    /// (`jobs LEFT JOIN applications`, one query). Withdrawn applications are in too.
    pub fn cohort_since(&self, since: DateTime<Utc>) -> anyhow::Result<Vec<CohortRow>> {
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

    // ---- emails and Gonza's tasks

    pub fn email_exists(&self, message_id: &str) -> anyhow::Result<bool> {
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
    ) -> anyhow::Result<EmailAdded> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stored = tx.execute(
            "INSERT OR IGNORE INTO emails (message_id, thread_id, from_addr, subject, received_at, \
             job_key, kind, summary, imported_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                email.message_id,
                email.thread_id,
                email.from_addr,
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

    pub fn get_email(&self, message_id: &str) -> anyhow::Result<Option<EmailRow>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                &format!("{EMAIL_SELECT} WHERE e.message_id = ?1"),
                [message_id],
                email_from_row,
            )
            .optional()?)
    }

    /// Gives an email that was stored without a posting its posting, and the same to the task it
    /// made. False when the message is unknown or already has a posting: a link is made once.
    pub fn link_email(&self, message_id: &str, job_key: &str) -> anyhow::Result<bool> {
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
    pub fn add_task(&self, task: &NewTask, now: DateTime<Utc>) -> anyhow::Result<bool> {
        let conn = self.lock();
        Ok(insert_task(&conn, task, now)?)
    }

    /// Adds a task that came from no email (`jobbot task add`). The same text for the same
    /// posting is not added again while a task like it, from any source, is still open.
    pub fn add_manual_task(&self, task: &NewTask, now: DateTime<Utc>) -> anyhow::Result<TaskAdded> {
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

    pub fn get_task(&self, id: i64) -> anyhow::Result<Option<TaskRow>> {
        let conn = self.lock();
        Ok(conn
            .query_row(&format!("{TASK_SELECT} WHERE id = ?1"), [id], task_from_row)
            .optional()?)
    }

    /// The tasks still to do: soonest due first, the ones without a date last.
    pub fn open_tasks(&self) -> anyhow::Result<Vec<TaskRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "{TASK_SELECT} WHERE status = 'open' ORDER BY due_at IS NULL, due_at, id"
        ))?;
        let rows = stmt.query_map([], task_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Closes an open task. A task that is already closed, or does not exist, is reported, not changed.
    pub fn complete_task(&self, id: i64) -> anyhow::Result<TaskCompletion> {
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
    pub fn recent_emails(&self, since: DateTime<Utc>) -> anyhow::Result<Vec<EmailRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "{EMAIL_SELECT} WHERE e.received_at >= ?1 AND e.kind <> 'ack' \
             ORDER BY e.received_at DESC, e.message_id"
        ))?;
        let rows = stmt.query_map([to_text(since)], email_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Keys of the applications of a company (its `normalize`d name) that are in one of `states`.
    /// A blank name matches nothing.
    pub fn job_keys_of_company(
        &self,
        company_norm: &str,
        states: &[State],
    ) -> anyhow::Result<Vec<String>> {
        if company_norm.is_empty() || states.is_empty() {
            return Ok(Vec::new());
        }
        let marks = vec!["?"; states.len()].join(", ");
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT job_key FROM applications WHERE company_norm = ? AND state IN ({marks}) \
             ORDER BY job_key"
        ))?;
        let rows = stmt.query_map(
            params_from_iter(
                std::iter::once(company_norm).chain(states.iter().map(|state| state.as_str())),
            ),
            |row| row.get(0),
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ---- headless Claude spend

    /// One finished `claude -p` call, successful or not. A failed call costs nothing unless Claude
    /// itself reported a cost.
    pub fn record_claude_call(
        &self,
        stage: &str,
        at: DateTime<Utc>,
        cost_usd: f64,
        input_tokens: u64,
        output_tokens: u64,
        ok: bool,
    ) -> anyhow::Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO claude_calls (stage, at, cost_usd, input_tokens, output_tokens, ok) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                stage,
                to_text(at),
                cost_usd,
                input_tokens as i64,
                output_tokens as i64,
                ok
            ],
        )?;
        Ok(())
    }

    /// Dollars spent on Claude at or after `since`.
    pub fn claude_spend_since(&self, since: DateTime<Utc>) -> anyhow::Result<f64> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT COALESCE(SUM(cost_usd), 0.0) FROM claude_calls WHERE at >= ?1",
            [to_text(since)],
            |row| row.get(0),
        )?)
    }

    /// Calls and dollars per stage at or after `since`.
    pub fn claude_calls_by_stage(
        &self,
        since: DateTime<Utc>,
    ) -> anyhow::Result<BTreeMap<String, (i64, f64)>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT stage, COUNT(*), COALESCE(SUM(cost_usd), 0.0) FROM claude_calls \
             WHERE at >= ?1 GROUP BY stage",
        )?;
        let rows = stmt.query_map([to_text(since)], |row| {
            Ok((
                row.get::<_, String>(0)?,
                (row.get::<_, i64>(1)?, row.get::<_, f64>(2)?),
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<BTreeMap<_, _>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use chrono::TimeDelta;

    use super::*;
    use crate::models::Job;
    use crate::testing::{NOW, make_job, make_job_with};

    fn submit(t: &Tracker, job: &Job, approve: bool) {
        t.add_job(job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        t.transition(&job.key(), State::Prepared, None, *NOW)
            .unwrap();
        if approve {
            t.approve(&job.key()).unwrap();
        }
        t.transition(&job.key(), State::Submitted, None, *NOW)
            .unwrap();
    }

    fn state_of(t: &Tracker, key: &str) -> State {
        t.get_application(key).unwrap().unwrap().state
    }

    fn tracker_with_prepared(ids: &[&str]) -> Tracker {
        let tracker = Tracker::in_memory().unwrap();
        for id in ids {
            let job = make_job_with(|j| {
                j.external_id = (*id).into();
                j.company = format!("Co{id}");
                j.title = format!("Role {id}");
            });
            tracker
                .add_job(&job, 70.0, &Default::default(), *NOW)
                .unwrap();
            tracker.create_application(&job.key(), *NOW).unwrap();
            tracker
                .transition(&job.key(), State::Prepared, None, *NOW)
                .unwrap();
        }
        tracker
    }

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
    fn a_file_database_is_wal_durable_and_versioned() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobbot.db");
        let tracker = Tracker::open(&path).unwrap();
        {
            let conn = tracker.lock();
            let sync: i64 = conn
                .pragma_query_value(None, "synchronous", |row| row.get(0))
                .unwrap();
            let version: i64 = conn
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .unwrap();
            assert_eq!(sync, 2, "FULL: every commit survives a power cut");
            assert_eq!(version, SCHEMA_VERSION);
        }
        // `journal_mode = WAL` lives in the file: any new connection sees it.
        let other = rusqlite::Connection::open(&path).unwrap();
        let mode: String = other
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn two_handles_on_the_same_database_work_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobbot.db");
        let writer = Tracker::open(&path).unwrap();
        let reader = Tracker::open(&path).unwrap();
        writer
            .begin_intent(INTENT_SUBMIT, "test:1", *NOW)
            .unwrap()
            .unwrap();
        assert_eq!(reader.open_intents(INTENT_SUBMIT).unwrap().len(), 1);
    }

    /// A database made by an earlier version: its tables, user_version 0 and one stored fact.
    fn old_database_with_a_fact(path: &Path) {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute("INSERT INTO meta (key, value) VALUES ('probe', 'kept')", [])
            .unwrap();
    }

    fn backup_path(dir: &Path) -> std::path::PathBuf {
        dir.join("backups")
            .join(format!("jobbot-before-v{SCHEMA_VERSION}.db"))
    }

    #[test]
    fn an_old_database_is_copied_before_it_is_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobbot.db");
        old_database_with_a_fact(&path);
        let _tracker = Tracker::open(&path).unwrap();
        let backup = backup_path(dir.path());
        assert!(backup.is_file(), "the pre-migration copy exists");
        let copy = rusqlite::Connection::open(&backup).unwrap();
        let kept: String = copy
            .query_row("SELECT value FROM meta WHERE key = 'probe'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(kept, "kept");
    }

    #[test]
    fn a_leftover_partial_copy_is_not_mistaken_for_the_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobbot.db");
        old_database_with_a_fact(&path);
        // An earlier try died halfway through writing its copy.
        std::fs::create_dir_all(dir.path().join("backups")).unwrap();
        std::fs::write(
            dir.path()
                .join("backups")
                .join(format!("jobbot-before-v{SCHEMA_VERSION}.db.partial")),
            b"half a database",
        )
        .unwrap();
        let _tracker = Tracker::open(&path).unwrap();
        let copy = rusqlite::Connection::open(backup_path(dir.path())).unwrap();
        let kept: String = copy
            .query_row("SELECT value FROM meta WHERE key = 'probe'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(kept, "kept", "the real copy was made despite the leftover");
    }

    #[test]
    fn a_finished_copy_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobbot.db");
        old_database_with_a_fact(&path);
        let backup = backup_path(dir.path());
        std::fs::create_dir_all(backup.parent().unwrap()).unwrap();
        std::fs::write(&backup, b"the state from before").unwrap();
        let _tracker = Tracker::open(&path).unwrap();
        assert_eq!(std::fs::read(&backup).unwrap(), b"the state from before");
    }

    #[test]
    fn a_fresh_or_current_database_needs_no_copy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobbot.db");
        drop(Tracker::open(&path).unwrap());
        drop(Tracker::open(&path).unwrap());
        assert!(
            !dir.path().join("backups").exists(),
            "nothing was migrated, so nothing was copied"
        );
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

    #[test]
    fn job_and_application_lifecycle() {
        let now = *NOW;
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        let detail = BTreeMap::from([("fit".to_string(), 30.0)]);
        t.add_job(&job, 77.0, &detail, now).unwrap();
        assert!(t.has_posting(&job).unwrap());
        t.create_application(&job.key(), now).unwrap();
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
    fn job_round_trips_through_the_database() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job_with(|j| {
            j.source = "greenhouse".into();
            j.external_id = "acme/42:x".into();
            j.description = "Línea uno\nLínea dos".into();
            j.salary_min_monthly_usd = Some(4000.0);
            j.salary_max_monthly_usd = Some(6000.0);
            j.posted_at = Some(*NOW);
        });
        t.add_job(&job, 61.5, &BTreeMap::new(), *NOW).unwrap();
        assert_eq!(t.get_job(&job.key()).unwrap(), Some(job));
        assert_eq!(t.get_job("nope:1").unwrap(), None);
    }

    #[test]
    fn open_creates_the_directory_and_keeps_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data").join("tracker.db");
        let job = make_job();
        {
            let t = Tracker::open(&path).unwrap();
            t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        }
        let reopened = Tracker::open(&path).unwrap();
        assert!(reopened.has_posting(&job).unwrap());
    }

    #[test]
    fn add_job_twice_keeps_the_first_row() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.add_job(&job, 99.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        assert_eq!(t.get_application(&job.key()).unwrap().unwrap().score, 70.0);
    }

    /// A posting as an older version of a source stored it: dated, remote, with pay.
    fn stored_posting() -> Job {
        make_job_with(|j| {
            j.location = "Remote".into();
            j.remote = true;
            j.posted_at = Some(*NOW - TimeDelta::days(3));
            j.salary_min_monthly_usd = Some(5000.0);
            j.salary_max_monthly_usd = Some(7000.0);
        })
    }

    #[test]
    fn refresh_posting_updates_what_the_source_reports_and_nothing_else() {
        let t = Tracker::in_memory().unwrap();
        let job = stored_posting();
        let detail = BTreeMap::from([("fit".to_string(), 30.0)]);
        t.add_job(&job, 77.0, &detail, *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        let fresh = Job {
            url: "https://example.com/changed".into(),
            apply_url: "https://job-boards.greenhouse.io/acme/jobs/1".into(),
            location: "San Francisco, CA".into(),
            remote: false,
            posted_at: Some(*NOW - TimeDelta::days(90)),
            description: "rewritten".into(),
            salary_min_monthly_usd: Some(1.0),
            salary_max_monthly_usd: Some(2.0),
            ..job.clone()
        };

        assert!(t.refresh_posting(&fresh).unwrap());

        let after = t.get_job(&job.key()).unwrap().unwrap();
        assert_eq!(
            (
                after.apply_url.as_str(),
                after.location.as_str(),
                after.remote,
                after.posted_at
            ),
            (
                fresh.apply_url.as_str(),
                "San Francisco, CA",
                false,
                fresh.posted_at
            )
        );
        // Everything else, pay and score included, stays as it was scored.
        assert_eq!(
            Job {
                apply_url: after.apply_url.clone(),
                location: after.location.clone(),
                remote: after.remote,
                posted_at: after.posted_at,
                ..job.clone()
            },
            after
        );
        assert_eq!(t.get_application(&job.key()).unwrap().unwrap().score, 77.0);
        assert_eq!(t.score_detail(&job.key()).unwrap(), detail);
        // The same data again changes nothing.
        assert!(!t.refresh_posting(&fresh).unwrap());
    }

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
    fn a_new_apply_link_drops_the_review_of_the_form_it_replaces() {
        // Gonza approved the application from the screenshot of the form at the OLD link; a refresh
        // that points the posting at another form must not carry that approval over.
        let job = stored_posting();
        for state in [State::Prepared, State::NeedsGonza] {
            let t = Tracker::in_memory().unwrap();
            t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
            t.create_application(&job.key(), *NOW).unwrap();
            t.transition(&job.key(), State::Prepared, None, *NOW)
                .unwrap();
            if state == State::NeedsGonza {
                t.transition(&job.key(), State::NeedsGonza, Some("x"), *NOW)
                    .unwrap();
            }
            t.set_screenshot(&job.key(), std::path::Path::new("shots/a.png"))
                .unwrap();
            t.approve(&job.key()).unwrap();

            // A change that is not about the form keeps the review.
            let moved = Job {
                location: "Remote - Chile".into(),
                ..job.clone()
            };
            t.refresh_posting(&moved).unwrap();
            let app = t.get_application(&job.key()).unwrap().unwrap();
            assert!(app.approved && app.screenshot_path.is_some(), "{state:?}");

            let relinked = Job {
                apply_url: "https://job-boards.greenhouse.io/other/jobs/9".into(),
                ..moved
            };
            assert!(t.refresh_posting(&relinked).unwrap());
            let app = t.get_application(&job.key()).unwrap().unwrap();
            assert!(
                !app.approved,
                "{state:?}: the approval was for another form"
            );
            assert_eq!(app.screenshot_path, None, "{state:?}");
            assert_eq!(app.state, state);
        }
    }

    #[test]
    fn a_new_apply_link_leaves_a_sent_application_as_it_was() {
        let t = Tracker::in_memory().unwrap();
        let job = stored_posting();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        t.transition(&job.key(), State::Prepared, None, *NOW)
            .unwrap();
        t.approve(&job.key()).unwrap();
        t.transition(&job.key(), State::Submitted, None, *NOW)
            .unwrap();

        let relinked = Job {
            apply_url: "https://job-boards.greenhouse.io/other/jobs/9".into(),
            ..job.clone()
        };
        t.refresh_posting(&relinked).unwrap();

        assert!(t.get_application(&job.key()).unwrap().unwrap().approved);
    }

    #[test]
    fn refresh_posting_notices_each_field_on_its_own() {
        let job = stored_posting();
        for field in ["apply_url", "posted_at", "location", "remote"] {
            let t = Tracker::in_memory().unwrap();
            t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
            let fresh = make_job_with(|j| {
                *j = job.clone();
                match field {
                    "apply_url" => j.apply_url = "https://other/apply".into(),
                    "posted_at" => j.posted_at = Some(*NOW - TimeDelta::days(40)),
                    "location" => j.location = "Remote - Chile".into(),
                    _ => j.remote = false,
                }
            });
            assert!(t.refresh_posting(&fresh).unwrap(), "{field} changed");
            assert!(!t.refresh_posting(&fresh).unwrap(), "{field} twice");
        }
    }

    #[test]
    fn refresh_posting_never_blanks_a_good_value() {
        let t = Tracker::in_memory().unwrap();
        let job = stored_posting();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        // A fetch that came back without a link, a date or a location says nothing about them.
        let blank = Job {
            apply_url: String::new(),
            location: String::new(),
            posted_at: None,
            ..job.clone()
        };

        assert!(!t.refresh_posting(&blank).unwrap());

        assert_eq!(t.get_job(&job.key()).unwrap(), Some(job.clone()));
        // A blank field next to a real change: only the real change is written.
        let partial = Job {
            remote: false,
            ..blank
        };
        assert!(t.refresh_posting(&partial).unwrap());
        let after = t.get_job(&job.key()).unwrap().unwrap();
        assert_eq!(
            after,
            Job {
                remote: false,
                ..job
            }
        );
    }

    #[test]
    fn refresh_posting_does_not_create_a_posting_that_is_not_stored() {
        let t = Tracker::in_memory().unwrap();
        let stranger = make_job_with(|j| j.external_id = "zzz".into());
        assert!(!t.refresh_posting(&stranger).unwrap());
        assert!(t.get_job(&stranger.key()).unwrap().is_none());
    }

    #[test]
    fn unsent_applications_are_the_unfinished_ones_that_were_never_clicked() {
        let t = Tracker::in_memory().unwrap();
        let key_of = |id: &str| format!("test:{id}");
        let add = |id: &str| {
            let job = make_job_with(|j| j.external_id = id.into());
            t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
            t.create_application(&job.key(), *NOW).unwrap();
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
        assert!(matches!(
            err.downcast_ref::<TrackerError>(),
            Some(TrackerError::UnknownKey(_))
        ));
        assert!(t.approve("nope:1").is_err());
        assert!(t.set_cv_path("nope:1", Path::new("cv.pdf")).is_err());
        assert!(t.set_screenshot("nope:1", Path::new("shot.png")).is_err());
        assert!(t.get_application("nope:1").unwrap().is_none());
    }

    #[test]
    fn create_application_is_idempotent() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        t.transition(&job.key(), State::Prepared, None, *NOW)
            .unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        assert_eq!(state_of(&t, &job.key()), State::Prepared);
        assert_eq!(
            t.count_events(&[State::Scored], *NOW - TimeDelta::days(1))
                .unwrap(),
            1
        );
    }

    #[test]
    fn duplicate_posting_detected_by_company_and_title() {
        let t = Tracker::in_memory().unwrap();
        t.add_job(
            &make_job_with(|j| j.external_id = "1".into()),
            70.0,
            &BTreeMap::new(),
            *NOW,
        )
        .unwrap();
        assert!(
            t.has_posting(&make_job_with(|j| j.external_id = "2".into()))
                .unwrap()
        );
        let other_role = make_job_with(|j| {
            j.external_id = "3".into();
            j.title = "Other Role".into();
        });
        assert!(!t.has_posting(&other_role).unwrap());
    }

    #[test]
    fn a_fresh_repost_is_not_hidden_by_an_old_posting_of_the_same_title() {
        let cutoff = *NOW - TimeDelta::days(45);
        let at = |days_ago: i64| {
            make_job_with(|j| {
                j.external_id = "1".into();
                j.posted_at = Some(*NOW - TimeDelta::days(days_ago));
            })
        };
        let repost = make_job_with(|j| {
            j.external_id = "2".into(); // a new id for the same company and title
            j.posted_at = Some(*NOW - TimeDelta::days(2));
        });

        // The stored one is 60 days old: the same role posted again is a new posting.
        let t = Tracker::in_memory().unwrap();
        t.add_job(&at(60), 70.0, &BTreeMap::new(), *NOW).unwrap();
        assert!(!t.has_posting_since(&repost, cutoff).unwrap());
        // The same posting by its own key is always known, however old.
        assert!(t.has_posting_since(&at(60), cutoff).unwrap());

        // The stored one is recent: the repost is a duplicate.
        let t = Tracker::in_memory().unwrap();
        t.add_job(&at(10), 70.0, &BTreeMap::new(), *NOW).unwrap();
        assert!(t.has_posting_since(&repost, cutoff).unwrap());

        // An undated stored posting counts from the day it was first seen.
        let undated = make_job_with(|j| {
            j.external_id = "1".into();
            j.posted_at = None;
        });
        let t = Tracker::in_memory().unwrap();
        t.add_job(&undated, 70.0, &BTreeMap::new(), *NOW - TimeDelta::days(60))
            .unwrap();
        assert!(!t.has_posting_since(&repost, cutoff).unwrap());
        let t = Tracker::in_memory().unwrap();
        t.add_job(&undated, 70.0, &BTreeMap::new(), *NOW - TimeDelta::days(5))
            .unwrap();
        assert!(t.has_posting_since(&repost, cutoff).unwrap());
    }

    #[test]
    fn has_posting_ignores_empty_company_and_title() {
        let t = Tracker::in_memory().unwrap();
        let blank = |id: &str| {
            make_job_with(|j| {
                j.external_id = id.into();
                j.company.clear();
                j.title.clear();
            })
        };
        t.add_job(&blank("1"), 10.0, &BTreeMap::new(), *NOW)
            .unwrap();
        assert!(t.has_posting(&blank("1")).unwrap());
        assert!(!t.has_posting(&blank("2")).unwrap());
    }

    #[test]
    fn find_job_key_matches_by_key_exact_link_or_company_and_title() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        let probe = |f: &dyn Fn(&mut Job)| {
            let mut candidate = Job {
                source: "manual".into(),
                external_id: "x".into(),
                company: "Otra".into(),
                title: "Otro".into(),
                url: "https://nowhere.example/z".into(),
                ..Job::default()
            };
            f(&mut candidate);
            t.find_job_key(&candidate).unwrap()
        };
        let same = Some(job.key());

        assert_eq!(
            probe(&|c| c.external_id = "1".into()),
            None,
            "source differs"
        );
        assert_eq!(
            probe(&|c| {
                c.source = "test".into();
                c.external_id = "1".into();
            }),
            same
        );
        assert_eq!(
            probe(&|c| c.url = format!("{}?ref=x#y", job.apply_url)),
            same,
            "same link, with a query and a fragment"
        );
        assert_eq!(
            probe(&|c| c.url = format!("{}/application", job.url)),
            same,
            "same link plus /application"
        );
        assert_eq!(
            probe(&|c| {
                c.company = "ACME AI Inc.".into();
                c.title = " senior ai engineer".into();
            }),
            same
        );
        assert_eq!(probe(&|_| {}), None, "nothing in common");
    }

    #[test]
    fn find_job_key_does_not_confuse_a_link_with_a_longer_one() {
        let t = Tracker::in_memory().unwrap();
        let twelve = make_job_with(|j| {
            j.external_id = "12".into();
            j.company = "Zeta".into();
            j.url = "https://example.com/jobs/12".into();
            j.apply_url = "https://example.com/jobs/12/apply".into();
        });
        t.add_job(&twelve, 70.0, &BTreeMap::new(), *NOW).unwrap();
        let one = Job {
            source: "manual".into(),
            external_id: "m".into(),
            company: "Otra".into(),
            title: "Otro".into(),
            url: "https://example.com/jobs/1".into(),
            ..Job::default()
        };

        assert_eq!(t.find_job_key(&one).unwrap(), None);
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
    fn technical_test_and_ghosted_paths() {
        let t = Tracker::in_memory().unwrap();
        let first = make_job_with(|j| j.external_id = "1".into());
        submit(&t, &first, false);
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

        let second = make_job_with(|j| {
            j.external_id = "2".into();
            j.company = "Other Co".into();
        });
        submit(&t, &second, false);
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
            submit(&t, &job, false);
            t.transition(&job.key(), State::Ghosted, None, *NOW)
                .unwrap();
            t.transition(&job.key(), late, None, *NOW).unwrap();
            assert_eq!(state_of(&t, &job.key()), late);
        }
        // What was never answered cannot jump to the end of the process.
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        submit(&t, &job, false);
        t.transition(&job.key(), State::Ghosted, None, *NOW)
            .unwrap();
        for never in [State::Accepted, State::Withdrawn, State::Submitted] {
            assert!(t.transition(&job.key(), never, None, *NOW).is_err());
        }
    }

    #[test]
    fn daily_count_and_company_cooldown() {
        let now = *NOW;
        let t = Tracker::in_memory().unwrap();
        submit(&t, &make_job(), false);
        assert_eq!(t.submitted_on(now.date_naive()).unwrap(), 1);
        assert_eq!(
            t.submitted_on(now.date_naive() + TimeDelta::days(1))
                .unwrap(),
            0
        );
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
    fn approved_submitted_total_counts_only_approved() {
        let t = Tracker::in_memory().unwrap();
        submit(
            &t,
            &make_job_with(|j| {
                j.external_id = "1".into();
                j.company = "A".into();
            }),
            true,
        );
        submit(
            &t,
            &make_job_with(|j| {
                j.external_id = "2".into();
                j.company = "B".into();
            }),
            false,
        );
        assert_eq!(t.approved_submitted_total().unwrap(), 1);
    }

    #[test]
    fn lists_counts_and_first_seen() {
        let now = *NOW;
        let t = Tracker::in_memory().unwrap();
        assert_eq!(t.first_seen_at().unwrap(), None);
        for (i, score) in [50.0, 90.0, 70.0].into_iter().enumerate() {
            let job = make_job_with(|j| {
                j.external_id = i.to_string();
                j.company = format!("Co{i}");
                j.title = format!("Role {i}");
            });
            t.add_job(&job, score, &BTreeMap::new(), now).unwrap();
            t.create_application(&job.key(), now).unwrap();
        }
        let scores: Vec<f64> = t
            .top_scored(2)
            .unwrap()
            .iter()
            .map(|row| row.score)
            .collect();
        assert_eq!(scores, vec![90.0, 70.0]);
        assert_eq!(t.list_by_state(State::Scored).unwrap().len(), 3);
        assert!(t.list_by_state(State::Prepared).unwrap().is_empty());
        assert_eq!(
            t.counts().unwrap(),
            BTreeMap::from([("scored".to_string(), 3)])
        );
        assert_eq!(t.first_seen_at().unwrap(), Some(now));
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
        assert_eq!(t.submitted_since(now - TimeDelta::days(7)).unwrap(), 0);
    }

    #[test]
    fn a_posting_keeps_the_day_it_was_first_seen() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        let other = make_job_with(|j| {
            j.external_id = "2".into();
            j.company = "Other Co".into();
        });
        assert_eq!(t.job_first_seen(&job.key()).unwrap(), None);

        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.add_job(&other, 70.0, &BTreeMap::new(), *NOW + TimeDelta::days(3))
            .unwrap();
        // Seen again a week later and refreshed from a newer fetch: the first sighting stands.
        t.add_job(&job, 80.0, &BTreeMap::new(), *NOW + TimeDelta::days(7))
            .unwrap();
        let moved = Job {
            location: "Remote - Chile".into(),
            ..job.clone()
        };
        assert!(t.refresh_posting(&moved).unwrap());

        assert_eq!(t.job_first_seen(&job.key()).unwrap(), Some(*NOW));
        assert_eq!(
            t.job_first_seen(&other.key()).unwrap(),
            Some(*NOW + TimeDelta::days(3))
        );
        assert_eq!(t.job_first_seen("test:unknown").unwrap(), None);
    }

    #[test]
    fn paths_are_stored_on_the_application() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        t.set_cv_path(&job.key(), Path::new("out/cv.pdf")).unwrap();
        t.set_screenshot(&job.key(), Path::new("shots/a.png"))
            .unwrap();
        let app = t.get_application(&job.key()).unwrap().unwrap();
        assert_eq!(app.cv_path.as_deref(), Some("out/cv.pdf"));
        assert_eq!(app.screenshot_path.as_deref(), Some("shots/a.png"));
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
    fn jobs_without_application_and_set_score() {
        let t = Tracker::in_memory().unwrap();
        let queued = make_job_with(|j| j.external_id = "1".into());
        let stored = make_job_with(|j| {
            j.external_id = "2".into();
            j.company = "Other Co".into();
            j.title = "Other Role".into();
        });
        t.add_job(&queued, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&queued.key(), *NOW).unwrap();
        t.add_job(&stored, 40.0, &BTreeMap::new(), *NOW).unwrap();
        assert_eq!(t.jobs_without_application().unwrap(), vec![stored.clone()]);

        t.set_score(
            &stored.key(),
            65.0,
            &BTreeMap::from([("fit".to_string(), 20.0)]),
        )
        .unwrap();
        t.create_application(&stored.key(), *NOW).unwrap();
        assert_eq!(
            t.get_application(&stored.key()).unwrap().unwrap().score,
            65.0
        );
        assert!(t.jobs_without_application().unwrap().is_empty());
        assert!(t.set_score("nope:1", 1.0, &BTreeMap::new()).is_err());
    }

    #[test]
    fn meta_round_trip() {
        let t = Tracker::in_memory().unwrap();
        assert_eq!(t.get_meta("last_discover_at").unwrap(), None);
        t.set_meta("last_discover_at", "2026-10-01T12:00:00Z")
            .unwrap();
        t.set_meta("last_discover_at", "2026-10-01T13:00:00Z")
            .unwrap();
        assert_eq!(
            t.get_meta("last_discover_at").unwrap().as_deref(),
            Some("2026-10-01T13:00:00Z")
        );
    }

    #[test]
    fn a_meta_value_can_be_deleted_and_deleting_what_is_not_there_is_fine() {
        let t = Tracker::in_memory().unwrap();
        t.set_meta("auto_approve_min", "90").unwrap();
        t.set_meta("last_run_at", "x").unwrap();
        t.delete_meta("auto_approve_min").unwrap();
        assert_eq!(t.get_meta("auto_approve_min").unwrap(), None);
        assert_eq!(t.get_meta("last_run_at").unwrap().as_deref(), Some("x"));
        t.delete_meta("auto_approve_min").unwrap();
        t.delete_meta("never_there").unwrap();
    }

    #[test]
    fn tracker_can_be_shared_across_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Tracker>();
    }

    // ---- Final review fixes: unconfirmed clicks, migration, competing writers

    /// An application whose Live click the page did not confirm: `needs_gonza` with an attempt stamp.
    fn click_without_confirmation(t: &Tracker, job: &Job) {
        t.add_job(job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
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
    fn an_attempted_click_counts_for_the_cap_and_the_company_cooldown_but_not_as_sent() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        click_without_confirmation(&t, &job);
        let day = TimeDelta::hours(24);
        assert_eq!(t.clicked_since(*NOW - day).unwrap(), 1);
        assert!(
            t.company_in_cooldown(&normalize(&job.company), 90, *NOW)
                .unwrap()
        );
        // The digest and the review-phase counter only know about confirmed submissions.
        assert_eq!(t.submitted_since(*NOW - day).unwrap(), 0);
        assert_eq!(t.approved_submitted_total().unwrap(), 0);
        // Outside the window the attempt no longer counts.
        assert_eq!(t.clicked_since(*NOW + TimeDelta::hours(1)).unwrap(), 0);
        assert!(
            !t.company_in_cooldown(&normalize(&job.company), 90, *NOW + TimeDelta::days(91))
                .unwrap()
        );
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
    fn a_database_created_before_the_attempt_column_is_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        {
            // The schema as it was before `attempted_at` existed.
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE jobs (
                   key TEXT PRIMARY KEY, source TEXT NOT NULL, company TEXT NOT NULL, company_norm TEXT NOT NULL,
                   title TEXT NOT NULL, title_lc TEXT NOT NULL, url TEXT NOT NULL, apply_url TEXT NOT NULL,
                   location TEXT NOT NULL, remote INTEGER NOT NULL, description TEXT NOT NULL,
                   salary_min REAL, salary_max REAL, posted_at TEXT,
                   score REAL NOT NULL, score_detail TEXT NOT NULL, first_seen TEXT NOT NULL
                 );
                 CREATE TABLE applications (
                   job_key TEXT PRIMARY KEY REFERENCES jobs(key), company_norm TEXT NOT NULL, state TEXT NOT NULL,
                   note TEXT, needs_reason TEXT, cv_path TEXT, screenshot_path TEXT,
                   approved INTEGER NOT NULL DEFAULT 0, submitted_at TEXT, updated_at TEXT NOT NULL
                 );",
            )
            .unwrap();
        }
        let t = Tracker::open(&path).unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        t.record_attempt(&job.key(), *NOW).unwrap();
        assert_eq!(t.clicked_since(*NOW - TimeDelta::hours(1)).unwrap(), 1);
        // Opening it again does not try to add the column twice.
        drop(t);
        assert!(Tracker::open(&path).is_ok());
    }

    fn new_email(id: &str, kind: EmailKind, at: DateTime<Utc>) -> NewEmail {
        NewEmail {
            message_id: id.into(),
            thread_id: format!("th-{id}"),
            from_addr: "Ana <ana@acme.example>".into(),
            subject: format!("Asunto {id}"),
            received_at: at,
            job_key: None,
            kind,
            summary: format!("Resumen {id}"),
        }
    }

    fn new_task(text: &str, due: Option<DateTime<Utc>>, source: Option<&str>) -> NewTask {
        NewTask {
            job_key: None,
            kind: "action_required".into(),
            text: text.into(),
            due_at: due,
            source_message_id: source.map(str::to_string),
            link: None,
        }
    }

    #[test]
    fn waiting_reason_is_the_stored_reason_or_else_the_last_note() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
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
            ..new_task("Llamar a Globex", None, None)
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
    fn a_tasks_table_made_before_the_link_column_gets_it_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        {
            // The table as the first version of the inbox made it, with a task in it.
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE gonza_tasks (
                   id INTEGER PRIMARY KEY AUTOINCREMENT, job_key TEXT, kind TEXT NOT NULL,
                   text TEXT NOT NULL, due_at TEXT, status TEXT NOT NULL DEFAULT 'open',
                   created_at TEXT NOT NULL, source_message_id TEXT,
                   UNIQUE(source_message_id, kind)
                 );
                 INSERT INTO gonza_tasks (kind, text, created_at, source_message_id)
                   VALUES ('interview_invite', 'Responder a Acme', '2026-10-01T12:00:00Z', 'm1');",
            )
            .unwrap();
        }

        let t = Tracker::open(&path).unwrap();

        assert_eq!(t.open_tasks().unwrap()[0].link, None);
        let task = NewTask {
            kind: "manual".into(),
            link: Some("https://example.com/x".into()),
            ..new_task("Nueva", None, None)
        };
        assert!(matches!(
            t.add_manual_task(&task, *NOW).unwrap(),
            TaskAdded::Created(_)
        ));
        drop(t);
        // Opening it again does not try to add the column twice.
        let again = Tracker::open(&path).unwrap();
        assert_eq!(again.open_tasks().unwrap().len(), 2);
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

    #[test]
    fn company_lookup_only_sees_the_asked_states() {
        let t = Tracker::in_memory().unwrap();
        let sent = make_job_with(|j| j.external_id = "sent".into());
        let draft = make_job_with(|j| j.external_id = "draft".into());
        for (job, states) in [
            (&sent, vec![State::Prepared, State::Submitted]),
            (&draft, vec![State::Prepared]),
        ] {
            t.add_job(job, 70.0, &BTreeMap::new(), *NOW).unwrap();
            t.create_application(&job.key(), *NOW).unwrap();
            for state in states {
                t.transition(&job.key(), state, None, *NOW).unwrap();
            }
        }

        let keys = t
            .job_keys_of_company("acme ai", &[State::Submitted, State::Replied])
            .unwrap();

        assert_eq!(keys, [sent.key()]);
        assert!(
            t.job_keys_of_company("", &[State::Submitted])
                .unwrap()
                .is_empty(),
            "a blank name matches nothing"
        );
    }

    #[test]
    fn a_database_without_the_inbox_tables_gets_them_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
                .unwrap();
        }

        let t = Tracker::open(&path).unwrap();

        let email = new_email("m1", EmailKind::Other, *NOW);
        assert_eq!(
            t.add_email(&email, None, *NOW).unwrap(),
            EmailAdded::Stored {
                task_created: false
            }
        );
        assert!(t.add_task(&new_task("x", None, None), *NOW).unwrap());
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
    fn claude_calls_are_summed_by_stage_inside_the_window() {
        let t = Tracker::in_memory().unwrap();
        t.record_claude_call("dossier", *NOW - TimeDelta::hours(25), 1.0, 10, 5, true)
            .unwrap();
        t.record_claude_call("dossier", *NOW - TimeDelta::hours(1), 0.25, 10, 5, true)
            .unwrap();
        t.record_claude_call("dossier", *NOW, 0.5, 10, 5, true)
            .unwrap();
        t.record_claude_call("freelance-profile", *NOW, 0.1, 10, 5, false)
            .unwrap();
        let since = *NOW - TimeDelta::hours(24);
        assert!((t.claude_spend_since(since).unwrap() - 0.85).abs() < 1e-9);
        let by_stage = t.claude_calls_by_stage(since).unwrap();
        assert_eq!(by_stage.len(), 2);
        assert_eq!(by_stage["dossier"].0, 2);
        assert!((by_stage["dossier"].1 - 0.75).abs() < 1e-9);
        assert_eq!(by_stage["freelance-profile"].0, 1);
    }

    #[test]
    fn nothing_spent_is_zero_not_an_error() {
        let t = Tracker::in_memory().unwrap();
        assert_eq!(t.claude_spend_since(*NOW).unwrap(), 0.0);
        assert!(t.claude_calls_by_stage(*NOW).unwrap().is_empty());
    }

    #[test]
    fn the_claude_table_is_created_in_an_existing_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        // A database made before this table existed: only `meta`.
        let old = rusqlite::Connection::open(&path).unwrap();
        old.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .unwrap();
        drop(old);
        let t = Tracker::open(&path).unwrap();
        t.record_claude_call("dossier", *NOW, 0.1, 1, 1, true)
            .unwrap();
        assert!((t.claude_spend_since(*NOW - TimeDelta::hours(1)).unwrap() - 0.1).abs() < 1e-9);
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
    fn score_detail_round_trips_and_an_unknown_key_is_an_error() {
        let t = Tracker::in_memory().unwrap();
        let detail = BTreeMap::from([("fit".to_string(), 30.0), ("pay".to_string(), 12.5)]);
        t.add_job(&make_job(), 77.0, &detail, *NOW).unwrap();
        assert_eq!(t.score_detail(&make_job().key()).unwrap(), detail);
        assert!(t.score_detail("nope:1").is_err());
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

    /// Stores job `id` as first seen at `seen`; with `walk`, also its application at `seen`.
    fn stored_at(t: &Tracker, id: &str, score: f64, seen: DateTime<Utc>, walk: &[State]) -> String {
        let job = make_job_with(|j| {
            j.external_id = id.into();
            j.company = format!("Co {id}");
            j.title = format!("Role {id}");
        });
        t.add_job(&job, score, &BTreeMap::new(), seen).unwrap();
        if !walk.is_empty() {
            t.create_application(&job.key(), seen).unwrap();
            for state in walk {
                t.transition(&job.key(), *state, None, seen).unwrap();
            }
        }
        job.key()
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

    /// What the panel asks on every poll, in the order it asks it.
    fn panel_answers(t: &Tracker, since: DateTime<Utc>) -> String {
        let mut out = format!(
            "{:?}\n{:?}\n",
            t.cohort_since(since).unwrap(),
            t.first_seen_since(since).unwrap()
        );
        for state in State::ALL {
            out += &format!("{:?}\n", t.list_by_state(state).unwrap());
        }
        out + &format!("{:?}", t.funnel_by_source().unwrap())
    }

    #[test]
    fn a_database_made_before_the_read_indexes_gets_them_on_open_and_answers_the_same() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        let since = *NOW - TimeDelta::days(7);
        let before = {
            let t = Tracker::open(&path).unwrap();
            stored_at(
                &t,
                "old",
                90.0,
                since - TimeDelta::days(1),
                &[State::Prepared],
            );
            for (n, id) in ["a", "b", "c", "d"].into_iter().enumerate() {
                let seen = *NOW - TimeDelta::hours(5 * n as i64);
                // Two with the same score and two with the same moment, so the order is by key.
                let walk = if n % 2 == 0 {
                    vec![State::Prepared, State::NeedsGonza]
                } else {
                    vec![State::Prepared, State::Submitted, State::Replied]
                };
                stored_at(&t, id, 70.0 + (n / 2) as f64, seen, &walk);
            }
            stored_at(&t, "bare", 55.0, *NOW, &[]);
            panel_answers(&t, since)
        };
        // The database as the version before left it: without the two indexes.
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("DROP INDEX idx_jobs_first_seen; DROP INDEX idx_events_job_state;")
                .unwrap();
        }

        let t = Tracker::open(&path).unwrap();

        let indexes: Vec<String> = t
            .lock()
            .prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_%'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        for name in ["idx_jobs_first_seen", "idx_events_job_state"] {
            assert!(indexes.iter().any(|i| i == name), "{name} in {indexes:?}");
        }
        assert_eq!(
            panel_answers(&t, since),
            before,
            "the indexes change no answer"
        );
        drop(t);
        // Opening it again neither fails nor builds them twice.
        let again = Tracker::open(&path).unwrap();
        assert_eq!(panel_answers(&again, since), before);
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
    fn first_seen_since_lists_the_moments_postings_were_stored_oldest_first() {
        let t = Tracker::in_memory().unwrap();
        assert!(t.first_seen_since(*NOW).unwrap().is_empty());
        let since = *NOW - TimeDelta::hours(24);
        stored_at(&t, "late", 50.0, *NOW - TimeDelta::hours(1), &[]);
        stored_at(&t, "edge", 50.0, since, &[]);
        stored_at(&t, "before", 50.0, since - TimeDelta::seconds(1), &[]);
        stored_at(&t, "twin", 50.0, since, &[]);

        assert_eq!(
            t.first_seen_since(since).unwrap(),
            vec![since, since, *NOW - TimeDelta::hours(1)]
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
                "INSERT INTO events (job_key, at, from_state, to_state)                  VALUES ('ghost:1', ?1, NULL, 'scored')",
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
    fn a_database_created_before_the_snooze_column_is_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        {
            // The schema as it was before `snoozed_until` existed (`attempted_at` already there).
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE jobs (
                   key TEXT PRIMARY KEY, source TEXT NOT NULL, company TEXT NOT NULL, company_norm TEXT NOT NULL,
                   title TEXT NOT NULL, title_lc TEXT NOT NULL, url TEXT NOT NULL, apply_url TEXT NOT NULL,
                   location TEXT NOT NULL, remote INTEGER NOT NULL, description TEXT NOT NULL,
                   salary_min REAL, salary_max REAL, posted_at TEXT,
                   score REAL NOT NULL, score_detail TEXT NOT NULL, first_seen TEXT NOT NULL
                 );
                 CREATE TABLE applications (
                   job_key TEXT PRIMARY KEY REFERENCES jobs(key), company_norm TEXT NOT NULL, state TEXT NOT NULL,
                   note TEXT, needs_reason TEXT, cv_path TEXT, screenshot_path TEXT,
                   approved INTEGER NOT NULL DEFAULT 0, submitted_at TEXT, updated_at TEXT NOT NULL,
                   attempted_at TEXT
                 );",
            )
            .unwrap();
        }
        let t = Tracker::open(&path).unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        let until = *NOW + TimeDelta::days(3);
        t.set_snoozed_until(&job.key(), Some(until)).unwrap();
        assert_eq!(
            t.get_application(&job.key())
                .unwrap()
                .unwrap()
                .snoozed_until,
            Some(until)
        );
        drop(t);
        assert!(Tracker::open(&path).is_ok());
    }

    #[test]
    fn a_snooze_is_stored_cleared_by_hand_and_forgotten_by_a_state_change() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
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
        assert!(t.set_snoozed_until("nope:1", Some(until)).is_err());
    }

    #[test]
    fn a_row_carries_the_posting_dates_the_pay_and_when_it_entered_its_state() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job_with(|j| {
            j.posted_at = Some(*NOW - TimeDelta::days(10));
            j.salary_min_monthly_usd = Some(4000.0);
            j.salary_max_monthly_usd = Some(6000.0);
        });
        let seen = *NOW - TimeDelta::days(2);
        t.add_job(&job, 70.0, &BTreeMap::new(), seen).unwrap();
        t.create_application(&job.key(), seen).unwrap();
        let row = t.get_application(&job.key()).unwrap().unwrap();
        assert_eq!(row.posted_at, job.posted_at);
        assert_eq!(row.first_seen, seen);
        assert_eq!(
            (row.salary_min, row.salary_max),
            (Some(4000.0), Some(6000.0))
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

        let by_source = t.funnel_by_source().unwrap();
        let sources: Vec<&str> = by_source.iter().map(|f| f.source.as_str()).collect();
        assert_eq!(sources, ["ashby", "lever"], "sorted by name");
        let ashby = &by_source[0];
        assert_eq!(
            (
                ashby.n,
                ashby.sent,
                ashby.answered,
                ashby.tests,
                ashby.interviews,
                ashby.offers
            ),
            (3, 2, 2, 0, 1, 1)
        );
        let lever = &by_source[1];
        assert_eq!(
            (
                lever.n,
                lever.sent,
                lever.answered,
                lever.tests,
                lever.interviews,
                lever.offers
            ),
            (2, 1, 1, 1, 0, 0)
        );
        assert!(
            Tracker::in_memory()
                .unwrap()
                .funnel_by_source()
                .unwrap()
                .is_empty()
        );
    }
}
