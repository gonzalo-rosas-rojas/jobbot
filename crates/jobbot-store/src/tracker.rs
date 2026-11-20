//! SQLite tracker: jobs, applications and the application state machine.
//! Timestamps are stored as UTC ISO-8601 text (`2026-10-01T12:00:00Z`), which sorts
//! chronologically as plain text.
//!
//! This file holds the `Tracker` type, its errors and the text conversions several parts share;
//! each part of its `impl` lives in `tracker/<part>.rs` with its own tests.

mod applications;
mod attempts;
mod claude_ledger;
mod http_memory;
mod inbox;
mod intents;
mod jobs;
mod meta;
mod metrics;
mod moves;
mod schema;
#[cfg(test)]
mod test_support;

use std::sync::{Mutex, MutexGuard};

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::Connection;
use rusqlite::types::Type;

// The state machine lives in the pure domain crate; the tracker stores the states.
use jobbot_domain::origin::Origin;
pub use jobbot_domain::state::{SUBMITTED_STATES, State};

// The records the tracker reads and writes are the application's port types.
pub use jobbot_app::ports::store::{
    ApplicationRow, AttemptStart, CohortRow, EmailAdded, EmailKind, EmailRow, EventRow,
    INTENT_SUBMIT, INTERRUPTED_PREFIX, Intent, MoveRow, NewEmail, NewTask, SourceFunnelRow,
    StoredPage, TaskAdded, TaskCompletion, TaskRow, UndoRefusal, UndoableMark,
};

/// Everything the tracker can fail with. The first two are answers about the data (the caller
/// usually explains them to Gonza); the rest are the database or the disk failing.
#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    #[error("illegal transition {from} -> {to}")]
    IllegalTransition { from: String, to: String },
    #[error("unknown job key {0}")]
    UnknownKey(String),
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
    /// A database step that names what it was doing (opening the file, copying it).
    #[error("{doing}")]
    DbAt {
        doing: String,
        #[source]
        source: rusqlite::Error,
    },
    /// The file system around the database (its folder, the backup copy).
    #[error("{doing}")]
    Io {
        doing: String,
        #[source]
        source: std::io::Error,
    },
    /// A JSON column that cannot be written.
    #[error("encoding {what} as JSON")]
    Encode {
        what: String,
        #[source]
        source: serde_json::Error,
    },
    /// The file was written by a newer build than this one.
    #[error(
        "la base es de una versión más nueva que este programa: {found} > {supported} (the database is from a newer version than this program; nothing was changed)"
    )]
    NewerSchema { found: i64, supported: i64 },
    /// A stored JSON column that no longer parses.
    #[error("{what} is not valid JSON")]
    Corrupt {
        what: String,
        #[source]
        source: serde_json::Error,
    },
}

/// The tracker's result: `TrackerError` unless a signature names another error.
pub type Result<T, E = TrackerError> = std::result::Result<T, E>;

fn io(doing: String) -> impl FnOnce(std::io::Error) -> TrackerError {
    move |source| TrackerError::Io { doing, source }
}

fn db_at(doing: String) -> impl FnOnce(rusqlite::Error) -> TrackerError {
    move |source| TrackerError::DbAt { doing, source }
}

/// `?, ?, ?` for an `IN (...)` list of `count` values.
fn placeholders(count: usize) -> String {
    vec!["?"; count].join(", ")
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

/// A stored name read back as its enum; a name no variant has is a conversion failure of
/// `column`, worded "unknown `what` ...".
fn parse_column<T>(
    column: usize,
    text: &str,
    what: &str,
    parse: fn(&str) -> Option<T>,
) -> rusqlite::Result<T> {
    parse(text).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            Type::Text,
            format!("unknown {what} {text:?}").into(),
        )
    })
}

fn state_from_text(column: usize, text: &str) -> rusqlite::Result<State> {
    parse_column(column, text, "state", State::parse)
}

fn origin_from_text(column: usize, text: &str) -> rusqlite::Result<Origin> {
    parse_column(column, text, "origin", Origin::parse)
}

fn email_kind_from_text(column: usize, text: &str) -> rusqlite::Result<EmailKind> {
    parse_column(column, text, "email kind", EmailKind::parse)
}

/// The application database. The connection sits behind a `Mutex`, so a `Tracker`
/// can be shared by reference across threads and `.await` points.
pub struct Tracker {
    conn: Mutex<Connection>,
}

impl Tracker {
    fn lock(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock cannot leave a statement half done, so a
        // poisoned lock is safe to reuse.
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
