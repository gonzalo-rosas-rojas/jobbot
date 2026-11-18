//! The connection settings both databases (`tracker`, `prep`) open with.

use std::time::Duration;

use rusqlite::Connection;

/// How long a statement waits for another process's write lock before it gives up.
pub(crate) const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// WAL lets the panel read while a run writes; `FULL` keeps every commit durable, which the
/// write-ahead intents depend on. A database that cannot switch to WAL right now (another process
/// holds it) keeps working in its current mode and says so; `synchronous=FULL` is not optional, so
/// its error is returned.
pub(crate) fn use_wal(conn: &Connection) -> rusqlite::Result<()> {
    let switched =
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0));
    if !matches!(switched.as_deref(), Ok(mode) if mode.eq_ignore_ascii_case("wal")) {
        eprintln!("aviso: la base no pudo pasar a modo WAL; sigue en su modo actual");
    }
    conn.pragma_update(None, "synchronous", "FULL")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synchronous(conn: &Connection) -> i64 {
        conn.pragma_query_value(None, "synchronous", |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn use_wal_makes_every_commit_durable_even_where_wal_is_unavailable() {
        // An in-memory database cannot switch to WAL; `synchronous=FULL` still applies.
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "synchronous", "OFF").unwrap();
        use_wal(&conn).unwrap();
        assert_eq!(synchronous(&conn), 2, "FULL");
    }

    #[test]
    fn use_wal_switches_a_file_database_to_wal() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join("x.db")).unwrap();
        use_wal(&conn).unwrap();
        let mode: String = conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        assert_eq!(synchronous(&conn), 2);
    }
}
