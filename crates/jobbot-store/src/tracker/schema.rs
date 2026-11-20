//! The database layout: opening the file, migrating older versions and the copy made before
//! a migration.

use std::path::Path;
use std::sync::Mutex;

use jobbot_domain::handoff::MANUAL_RECORD_PREFIX;
use jobbot_domain::models::MANUAL_SOURCE;
use jobbot_domain::text::company_identity;
use rusqlite::{Connection, TransactionBehavior, params};

use super::{Result, Tracker, TrackerError, db_at, io};
use crate::pragmas::{BUSY_TIMEOUT, use_wal};

/// Version of the database layout this build writes (`PRAGMA user_version`). Raise it when a
/// migration is added: `Tracker::open` copies an older database with data before it moves it up.
const SCHEMA_VERSION: i64 = 5;

/// Indexes on columns `add_missing_columns` may have just added, so they run after it. The marks
/// the panel can still undo are asked on every poll; only a hand mark keeps an `undo`.
const INDEXES_ON_ADDED_COLUMNS: &str =
    "CREATE INDEX IF NOT EXISTS idx_events_marks ON events (at) WHERE undo IS NOT NULL;";

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
  attempted_at TEXT, snoozed_until TEXT,
  -- Version 5: how the application came about (`jobbot_domain::origin::Origin`, by its stored name).
  origin TEXT NOT NULL DEFAULT 'cold'
);
CREATE TABLE IF NOT EXISTS events (
  id INTEGER PRIMARY KEY AUTOINCREMENT, job_key TEXT NOT NULL, at TEXT NOT NULL,
  from_state TEXT, to_state TEXT NOT NULL, note TEXT,
  -- Only on a move Gonza marked by hand: what it overwrote in the row (`undo_mark`).
  undo TEXT
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
-- Version 2: what the job boards' GETs of one run leave for the next. A host that asked to be left
-- alone is left alone until `until` (UTC) by the next hourly run too; a page that came with a
-- validator is asked for again with it, and a 304 answer reuses `body`. `used_at` is the last run
-- that asked for the page: pages nobody asked for in a month are dropped.
CREATE TABLE IF NOT EXISTS http_cooldowns (
    host TEXT PRIMARY KEY,
    until TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS http_pages (
    url TEXT PRIMARY KEY,
    etag TEXT,
    last_modified TEXT,
    body TEXT NOT NULL,
    used_at TEXT NOT NULL
);
-- Version 3: the panel's commands that came with an attempt id of their own. The id is UNIQUE: a
-- repeat gets `reply` back instead of running the command again. `started` without a reply is a
-- command that is running or whose process died; it is not run again under that id. `request` is
-- the SHA-256 of the body, never the body.
CREATE TABLE IF NOT EXISTS command_attempts (
    id TEXT PRIMARY KEY,
    command TEXT NOT NULL,
    request BLOB NOT NULL,
    state TEXT NOT NULL,
    status INTEGER,
    reply BLOB,
    started_at TEXT NOT NULL
);
";

/// Copies the database (a consistent snapshot, `VACUUM INTO`) to `backups/jobbot-before-v<N>.db`
/// next to it. One copy per version: if it exists, an earlier try already saved the state from
/// before the migration. The copy is written under a `.partial` name and renamed when finished, so
/// a file with the final name is always a whole database, never what a cut try left behind.
fn back_up_before_migrating(conn: &Connection, db_path: &Path) -> Result<()> {
    let dir = db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("backups");
    std::fs::create_dir_all(&dir).map_err(io(format!("creating {}", dir.display())))?;
    let copy = dir.join(format!("jobbot-before-v{SCHEMA_VERSION}.db"));
    if copy.exists() {
        return Ok(());
    }
    let partial = dir.join(format!("jobbot-before-v{SCHEMA_VERSION}.db.partial"));
    if partial.exists() {
        std::fs::remove_file(&partial)
            .map_err(io(format!("removing the leftover {}", partial.display())))?;
    }
    conn.execute("VACUUM INTO ?1", [partial.to_string_lossy().as_ref()])
        .map_err(db_at(format!(
            "copying the database to {}",
            partial.display()
        )))?;
    std::fs::rename(&partial, &copy)
        .map_err(io(format!("finishing the copy at {}", copy.display())))?;
    Ok(())
}

impl Tracker {
    /// Opens (creating it and its parent directory if needed) the database at `path`. The file runs
    /// in WAL mode with every commit durable. A database made by an earlier version, one that
    /// already holds data and whose `user_version` is behind, is copied to `backups/` next to it
    /// before anything is changed.
    pub fn open(path: &Path) -> Result<Tracker> {
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(io(format!("creating {}", dir.display())))?;
        }
        let had_data = std::fs::metadata(path).is_ok_and(|meta| meta.len() > 0);
        let conn = Connection::open(path).map_err(db_at(format!("opening {}", path.display())))?;
        // Set before the first statement: the version read and the WAL switch can meet a lock too.
        conn.busy_timeout(BUSY_TIMEOUT)?;
        let found: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        // A newer build wrote this file: reading it with this build can fail on states and
        // columns it does not know, and writing could damage it. Nothing has been touched yet.
        if found > SCHEMA_VERSION {
            return Err(TrackerError::NewerSchema {
                found,
                supported: SCHEMA_VERSION,
            });
        }
        if had_data && found < SCHEMA_VERSION {
            back_up_before_migrating(&conn, path)?;
        }
        use_wal(&conn)?;
        let tracker = Self::from_connection(conn)?;
        if found < SCHEMA_VERSION {
            tracker
                .lock()
                .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(tracker)
    }

    pub fn in_memory() -> Result<Tracker> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut conn: Connection) -> Result<Tracker> {
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        Self::add_missing_columns(&conn)?;
        Self::add_origin_column(&mut conn)?;
        Self::give_blank_company_norms_their_identity(&mut conn)?;
        conn.execute_batch(INDEXES_ON_ADDED_COLUMNS)?;
        Ok(Tracker {
            conn: Mutex::new(conn),
        })
    }

    /// `CREATE TABLE IF NOT EXISTS` leaves a database made by an earlier version as it was, so
    /// columns added later are added here. Another process may add the same column first.
    fn add_missing_columns(conn: &Connection) -> Result<()> {
        for (table, column) in [
            ("applications", "attempted_at"),
            ("gonza_tasks", "link"),
            ("applications", "snoozed_until"),
            ("events", "undo"),
        ] {
            if !Self::has_column(conn, table, column)? {
                let added =
                    conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} TEXT"));
                Self::tolerate_lost_race(conn, table, column, added)?;
            }
        }
        Ok(())
    }

    /// Version 5: how each application came about (`applications.origin`). The column and the values of the
    /// applications already stored are written in one IMMEDIATE transaction, so another process opening the same
    /// file waits and then finds the column, and a cut between the two cannot leave a column without its values.
    /// The column is looked for first without a lock, so every later open (the common case) never takes the write
    /// lock; the transaction looks again, because another process may have added it in between.
    /// Everything stored is `cold` except what Gonza recorded by hand, which is `unknown`: a posting whose source
    /// is `manual`, or an application that went through `record`'s `manual_record:` step, which `record` writes
    /// only when the application was still `scored` (`record` sets `unknown` under the same rule for new
    /// applications). Nobody knows how those came about, and `cold` would claim jobbot found them and applied
    /// without contact. `substr` and not `LIKE`: in `LIKE` the `_` of `manual_record` matches any character. The
    /// statements are built with `format!` from constants, as `add_missing_columns` builds its `ALTER`. Filling the
    /// blank `company_norm` of older rows is not part of this step: it runs on every open
    /// (`give_blank_company_norms_their_identity`), so a file that already has the column gets it too.
    fn add_origin_column(conn: &mut Connection) -> Result<()> {
        if Self::has_column(conn, "applications", "origin")? {
            return Ok(());
        }
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !Self::has_column(&tx, "applications", "origin")? {
            let prefix = MANUAL_RECORD_PREFIX;
            tx.execute_batch(&format!(
                "ALTER TABLE applications ADD COLUMN origin TEXT NOT NULL DEFAULT 'cold';
                 UPDATE applications SET origin = 'unknown'
                  WHERE job_key IN (SELECT key FROM jobs WHERE source = '{MANUAL_SOURCE}')
                     OR job_key IN (SELECT job_key FROM events
                                     WHERE substr(note, 1, length('{prefix}')) = '{prefix}');"
            ))?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Rows stored before `company_identity` existed kept `company_norm = ''` when `normalize` could not fold the
    /// name (another script, or only a legal suffix), so such a company was nobody's and the cap, the cooldown and
    /// the e-mail match never saw it. Those rows, and only those, get the identity now. A row that already has a
    /// norm keeps it, and a company with no name stays blank, so a second run finds nothing to do. It runs on every
    /// open, whatever the version of the file: the rows are looked for first without a lock (the common case finds
    /// none and never takes the write lock), and the writes go in their own short IMMEDIATE transaction, which looks
    /// again because another process may have done it in between.
    fn give_blank_company_norms_their_identity(conn: &mut Connection) -> Result<()> {
        if Self::blank_company_norms(conn)?.is_empty() {
            return Ok(());
        }
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (table, key_column, key, identity) in Self::blank_company_norms(&tx)? {
            tx.execute(
                &format!("UPDATE {table} SET company_norm = ?1 WHERE {key_column} = ?2"),
                params![identity, key],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The rows with `company_norm = ''` whose company has an identity: (table, key column, key, identity).
    fn blank_company_norms(
        conn: &Connection,
    ) -> Result<Vec<(&'static str, &'static str, String, String)>> {
        let mut found = Vec::new();
        for (table, key_column, company_of) in [
            ("jobs", "key", "company"),
            (
                "applications",
                "job_key",
                "(SELECT company FROM jobs WHERE jobs.key = applications.job_key)",
            ),
        ] {
            let mut blank = conn.prepare(&format!(
                "SELECT {key_column}, {company_of} FROM {table} WHERE company_norm = ''"
            ))?;
            let rows: Vec<(String, Option<String>)> = blank
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            for (key, company) in rows {
                let identity = company_identity(&company.unwrap_or_default());
                if !identity.is_empty() {
                    found.push((table, key_column, key, identity));
                }
            }
        }
        Ok(found)
    }

    fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
        Ok(conn
            .prepare(&format!(
                "SELECT 1 FROM pragma_table_info('{table}') WHERE name = '{column}'"
            ))?
            .exists([])?)
    }

    /// An `ALTER TABLE ... ADD COLUMN` that failed is fine when the column is there now (another
    /// process added it between our check and our `ALTER`); otherwise the original error stands.
    /// The column is asked for again instead of reading SQLite's message text.
    fn tolerate_lost_race(
        conn: &Connection,
        table: &str,
        column: &str,
        added: rusqlite::Result<()>,
    ) -> Result<()> {
        match added {
            Err(error) if !Self::has_column(conn, table, column)? => Err(error.into()),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::{DateTime, TimeDelta, Utc};
    use jobbot_domain::fixtures::{NOW, make_job};
    use jobbot_domain::origin::Origin;

    use crate::tracker::test_support::{new_email, new_task, state_of, stored_at};
    use crate::tracker::{EmailAdded, EmailKind, INTENT_SUBMIT, NewTask, State, TaskAdded};

    use super::*;

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
    fn a_database_from_a_newer_version_is_refused_and_left_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobbot.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE from_the_future (x INTEGER); PRAGMA user_version = 99;",
            )
            .unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        let err = Tracker::open(&path).err().expect("refused");
        let text = err.to_string();
        assert!(
            text.contains("99") && text.contains(&SCHEMA_VERSION.to_string()),
            "{text}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before, "not a byte changed");
        assert!(!backup_path(dir.path()).exists(), "no backup either");
    }

    #[test]
    fn a_version_1_database_is_copied_and_gains_the_http_memory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobbot.db");
        old_database_with_a_fact(&path);
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch("DROP TABLE http_cooldowns; DROP TABLE http_pages;")
                .unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
        }
        let tracker = Tracker::open(&path).unwrap();
        assert!(
            backup_path(dir.path()).is_file(),
            "the state from before the http memory is kept"
        );
        tracker
            .rest_host("a.test:443", *NOW + TimeDelta::minutes(5))
            .unwrap();
        assert_eq!(tracker.resting_hosts(*NOW).unwrap().len(), 1);
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
    fn tracker_can_be_shared_across_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Tracker>();
    }

    // ---- unconfirmed clicks, migration, competing writers

    /// The `jobs` and `applications` tables as an early version made them, with the columns added
    /// since (`extra_application_columns`, e.g. `"attempted_at TEXT"`) left out or given.
    fn old_schema(extra_application_columns: &str) -> String {
        let extra = if extra_application_columns.is_empty() {
            String::new()
        } else {
            format!(", {extra_application_columns}")
        };
        format!(
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
               approved INTEGER NOT NULL DEFAULT 0, submitted_at TEXT, updated_at TEXT NOT NULL{extra}
             );"
        )
    }

    #[test]
    fn a_database_created_before_the_attempt_column_is_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        {
            // The schema as it was before `attempted_at` existed.
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(&old_schema("")).unwrap();
        }
        let t = Tracker::open(&path).unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.start_cold_for_tests(&job.key(), *NOW).unwrap();
        t.record_attempt(&job.key(), *NOW).unwrap();
        assert_eq!(t.clicked_since(*NOW - TimeDelta::hours(1)).unwrap(), 1);
        // Opening it again does not try to add the column twice.
        drop(t);
        assert!(Tracker::open(&path).is_ok());
    }

    #[test]
    fn a_failed_add_column_is_tolerated_when_the_column_is_there() {
        let t = Tracker::in_memory().unwrap();
        let conn = t.lock();
        // The column is there (the other process won the race): the failed ALTER is not an error.
        let lost_race =
            conn.execute_batch("ALTER TABLE applications ADD COLUMN snoozed_until TEXT");
        assert!(
            lost_race.is_err(),
            "adding an existing column fails in SQLite"
        );
        assert!(
            Tracker::tolerate_lost_race(&conn, "applications", "snoozed_until", lost_race).is_ok()
        );
    }

    #[test]
    fn a_failed_add_column_is_an_error_when_the_column_is_still_missing() {
        let t = Tracker::in_memory().unwrap();
        let conn = t.lock();
        let failed = conn.execute_batch("ALTER TABLE no_such_table ADD COLUMN extra TEXT");
        assert!(failed.is_err());
        assert!(Tracker::tolerate_lost_race(&conn, "no_such_table", "extra", failed).is_err());
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

    #[test]
    fn a_database_created_before_the_snooze_column_is_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        {
            // The schema as it was before `snoozed_until` existed (`attempted_at` already there).
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(&old_schema("attempted_at TEXT"))
                .unwrap();
        }
        let t = Tracker::open(&path).unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.start_cold_for_tests(&job.key(), *NOW).unwrap();
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

    /// A version 4 database: a job from a board with its application, a posting recorded by hand with its
    /// application, an application recorded by hand on a board's posting (`manual_record:` event), and an
    /// application whose posting row is missing. Returns the four job keys in that order.
    fn version_4_database(path: &Path) -> [&'static str; 4] {
        let conn = Connection::open(path).unwrap();
        // The orphan below is only possible with foreign keys off, as in old files.
        conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
        conn.execute_batch(&format!(
            "{}
             CREATE TABLE events (
               id INTEGER PRIMARY KEY AUTOINCREMENT, job_key TEXT NOT NULL, at TEXT NOT NULL,
               from_state TEXT, to_state TEXT NOT NULL, note TEXT, undo TEXT
             );
             PRAGMA user_version = 4;",
            old_schema("attempted_at TEXT, snoozed_until TEXT")
        ))
        .unwrap();
        for (key, source) in [
            ("board:1", "greenhouse"),
            ("manual:1", "manual"),
            ("board:2", "lever"),
        ] {
            conn.execute(
                "INSERT INTO jobs (key, source, company, company_norm, title, title_lc, url, apply_url,
                                   location, remote, description, score, score_detail, first_seen)
                 VALUES (?1, ?2, 'Acme', 'acme', 'Engineer', 'engineer', 'u', 'u', '', 1, '', 50, '{}',
                         '2026-10-01T12:00:00Z')",
                [key, source],
            )
            .unwrap();
        }
        for key in ["board:1", "manual:1", "board:2", "gone:1"] {
            conn.execute(
                "INSERT INTO applications (job_key, company_norm, state, updated_at)
                 VALUES (?1, 'acme', 'scored', '2026-10-01T12:00:00Z')",
                [key],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO events (job_key, at, from_state, to_state, note)
             VALUES ('board:2', '2026-10-01T12:00:00Z', 'scored', 'needs_gonza', ?1)",
            [format!(
                "{MANUAL_RECORD_PREFIX} enviada a mano, registrándola"
            )],
        )
        .unwrap();
        ["board:1", "manual:1", "board:2", "gone:1"]
    }

    #[test]
    fn a_version_4_database_is_copied_and_gains_origin_cold_except_what_was_recorded_by_hand() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        let [board, manual, recorded, orphan] = version_4_database(&path);

        let t = Tracker::open(&path).unwrap();

        assert!(
            dir.path()
                .join("backups")
                .join("jobbot-before-v5.db")
                .is_file()
        );
        let version: i64 = t
            .lock()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 5);
        assert_eq!(t.origin_of(board).unwrap(), Some(Origin::Cold));
        assert_eq!(t.origin_of(manual).unwrap(), Some(Origin::Unknown));
        assert_eq!(t.origin_of(recorded).unwrap(), Some(Origin::Unknown));
        assert_eq!(t.origin_of(orphan).unwrap(), Some(Origin::Cold));
    }

    /// Adds to a version 4 database what `normalize` alone left blank: a company in a script it cannot fold to ASCII
    /// (job and application both blank), and a company with no name at all (blank for good).
    fn blank_company_norms(path: &Path) {
        let conn = Connection::open(path).unwrap();
        for (key, company) in [("blank:1", "ЖК Строй"), ("blank:2", "")] {
            conn.execute(
                "INSERT INTO jobs (key, source, company, company_norm, title, title_lc, url, apply_url,
                                   location, remote, description, score, score_detail, first_seen)
                 VALUES (?1, 'lever', ?2, '', 'Engineer', 'engineer', 'u', 'u', '', 1, '', 50, '{}',
                         '2026-10-01T12:00:00Z')",
                [key, company],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO applications (job_key, company_norm, state, updated_at)
                 VALUES (?1, '', 'scored', '2026-10-01T12:00:00Z')",
                [key],
            )
            .unwrap();
        }
    }

    /// Every stored `company_norm`, jobs then applications, by key.
    fn company_norms(t: &Tracker) -> Vec<(String, String)> {
        let conn = t.lock();
        let mut stmt = conn
            .prepare(
                "SELECT 'job ' || key, company_norm FROM jobs
                 UNION ALL SELECT 'application ' || job_key, company_norm FROM applications
                 ORDER BY 1",
            )
            .unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn the_migration_gives_a_blank_company_norm_its_identity_and_only_those() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        version_4_database(&path);
        blank_company_norms(&path);

        let t = Tracker::open(&path).unwrap();

        let norms = company_norms(&t);
        let norm_of = |what: &str| {
            norms
                .iter()
                .find(|(name, _)| name == what)
                .map(|(_, norm)| norm.as_str())
                .unwrap()
        };
        assert_eq!(norm_of("job blank:1"), "жк строй");
        assert_eq!(norm_of("application blank:1"), "жк строй");
        assert_eq!(norm_of("job blank:2"), "", "no name, no identity");
        assert_eq!(norm_of("application blank:2"), "");
        assert_eq!(
            norm_of("job board:1"),
            "acme",
            "a filled one is left as it is"
        );
        assert_eq!(norm_of("application board:1"), "acme");
    }

    #[test]
    fn a_database_that_already_has_origin_gets_its_blank_company_norms_on_open_and_a_second_open_writes_nothing()
     {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("current.db");
        drop(Tracker::open(&path).unwrap());
        blank_company_norms(&path);

        let t = Tracker::open(&path).unwrap();

        let norms = company_norms(&t);
        assert!(norms.contains(&("job blank:1".to_string(), "жк строй".to_string())));
        assert!(norms.contains(&("application blank:1".to_string(), "жк строй".to_string())));
        assert!(norms.contains(&("job blank:2".to_string(), String::new())));
        let mut conn = t.lock();
        let before = conn.total_changes();
        Tracker::give_blank_company_norms_their_identity(&mut conn).unwrap();
        assert_eq!(conn.total_changes(), before, "nothing left to write");
    }

    #[test]
    fn opening_a_migrated_database_again_changes_no_company_norm() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        version_4_database(&path);
        blank_company_norms(&path);
        let first = company_norms(&Tracker::open(&path).unwrap());

        let second = company_norms(&Tracker::open(&path).unwrap());

        assert_eq!(second, first);
    }

    #[test]
    fn the_backup_is_the_version_4_state_without_the_origin_column() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        version_4_database(&path);
        drop(Tracker::open(&path).unwrap());

        let copy =
            Connection::open(dir.path().join("backups").join("jobbot-before-v5.db")).unwrap();

        let version: i64 = copy
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 4);
        assert!(!Tracker::has_column(&copy, "applications", "origin").unwrap());
    }

    #[test]
    fn a_database_with_the_column_that_still_says_version_4_finishes_without_a_second_backup_or_backfill()
     {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        let [_, manual, _, _] = version_4_database(&path);
        let t = Tracker::open(&path).unwrap();
        t.set_origin(manual, Origin::Referral).unwrap();
        t.lock().pragma_update(None, "user_version", 4).unwrap();
        drop(t);
        let backup = dir.path().join("backups").join("jobbot-before-v5.db");
        let backup_before = std::fs::read(&backup).unwrap();

        let again = Tracker::open(&path).unwrap();

        let version: i64 = again
            .lock()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 5, "the migration is finished");
        assert_eq!(
            again.origin_of(manual).unwrap(),
            Some(Origin::Referral),
            "the backfill did not run again"
        );
        assert_eq!(std::fs::read(&backup).unwrap(), backup_before);
    }

    #[test]
    fn reopening_a_migrated_database_keeps_a_corrected_origin() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        let [_, manual, _, _] = version_4_database(&path);
        let t = Tracker::open(&path).unwrap();
        assert!(t.set_origin(manual, Origin::Referral).unwrap());
        drop(t);

        let again = Tracker::open(&path).unwrap();

        assert_eq!(again.origin_of(manual).unwrap(), Some(Origin::Referral));
    }

    #[test]
    fn a_fresh_database_starts_every_application_cold() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.start_cold_for_tests(&job.key(), *NOW).unwrap();
        assert_eq!(t.origin_of(&job.key()).unwrap(), Some(Origin::Cold));
    }

    #[test]
    fn an_unknown_stored_origin_is_an_error_not_a_guess() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.start_cold_for_tests(&job.key(), *NOW).unwrap();
        t.lock()
            .execute("UPDATE applications SET origin = 'friend'", [])
            .unwrap();
        let error = t.origin_of(&job.key()).unwrap_err().to_string();
        assert!(error.contains("unknown origin \"friend\""), "{error}");
    }

    #[test]
    fn a_database_created_before_undo_is_migrated_and_marks_can_be_undone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        {
            // The schema as it was before marks could be undone.
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(&format!(
                "{}
                 CREATE TABLE events (
                   id INTEGER PRIMARY KEY AUTOINCREMENT, job_key TEXT NOT NULL, at TEXT NOT NULL,
                   from_state TEXT, to_state TEXT NOT NULL, note TEXT
                 );
                 PRAGMA user_version = 3;",
                old_schema("attempted_at TEXT, snoozed_until TEXT")
            ))
            .unwrap();
        }
        let t = Tracker::open(&path).unwrap();
        let job = make_job();
        t.add_job(&job, 70.0, &BTreeMap::new(), *NOW).unwrap();
        t.start_cold_for_tests(&job.key(), *NOW).unwrap();
        t.transition(&job.key(), State::Prepared, None, *NOW)
            .unwrap();
        t.transition(&job.key(), State::Submitted, None, *NOW)
            .unwrap();
        let later = *NOW + TimeDelta::minutes(1);
        t.mark_transition(&job.key(), State::Rejected, None, later)
            .unwrap();
        assert!(t.undo_mark(&job.key(), *NOW).unwrap().is_ok());
        assert_eq!(state_of(&t, &job.key()), State::Submitted);
        drop(t);
        assert!(Tracker::open(&path).is_ok());
    }
}
