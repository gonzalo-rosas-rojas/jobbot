//! The `meta` key/value table: the small settings of the run itself (`last_discover_at`, the
//! auto-approve score, the last run's result).

use rusqlite::{OptionalExtension, params};

use super::{Result, Tracker};

impl Tracker {
    /// A small setting of the run itself, for example `last_discover_at`.
    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let conn = self.lock();
        Ok(conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Removes `key` from `meta`; one that is not there is fine.
    pub fn delete_meta(&self, key: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute("DELETE FROM meta WHERE key = ?1", params![key])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::tracker::Tracker;

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
}
