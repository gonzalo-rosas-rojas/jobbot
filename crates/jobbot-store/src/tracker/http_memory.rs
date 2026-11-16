//! What the HTTP client remembers between runs (resting hosts, kept pages) and the `meta`
//! key/value table the run settings live in.

use chrono::{DateTime, TimeDelta, Utc};
use rusqlite::{OptionalExtension, params};

use super::{Result, StoredPage, Tracker, from_text, to_text};

/// How long a kept page waits to be asked for again before it is dropped.
const PAGE_KEEP: TimeDelta = TimeDelta::days(30);

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

    // ---- what the job boards' GETs leave for the next run

    /// The hosts whose rest ends after `now`, with the end of each, by host. The rests that are
    /// over by `now` are forgotten.
    pub fn resting_hosts(&self, now: DateTime<Utc>) -> Result<Vec<(String, DateTime<Utc>)>> {
        let conn = self.lock();
        conn.execute(
            "DELETE FROM http_cooldowns WHERE until <= ?1",
            [to_text(now)],
        )?;
        let mut stmt = conn.prepare("SELECT host, until FROM http_cooldowns ORDER BY host")?;
        let rows = stmt.query_map([], |row| {
            let until: String = row.get(1)?;
            Ok((row.get(0)?, from_text(1, &until)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// `host` is left alone until `until`; a later rest of the same host replaces it.
    pub fn rest_host(&self, host: &str, until: DateTime<Utc>) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO http_cooldowns (host, until) VALUES (?1, ?2) \
             ON CONFLICT(host) DO UPDATE SET until = excluded.until",
            params![host, to_text(until)],
        )?;
        Ok(())
    }

    /// The page kept for `url` (its query included), if any. Asking counts as using it at `now`.
    pub fn recall_page(&self, url: &str, now: DateTime<Utc>) -> Result<Option<StoredPage>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "UPDATE http_pages SET used_at = ?2 WHERE url = ?1 \
                 RETURNING etag, last_modified, body",
                params![url, to_text(now)],
                |row| {
                    Ok(StoredPage {
                        etag: row.get(0)?,
                        last_modified: row.get(1)?,
                        body: row.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    /// Keeps `page` for `url`, replacing what was there. Pages nobody asked for in `PAGE_KEEP` go.
    pub fn store_page(&self, url: &str, page: &StoredPage, now: DateTime<Utc>) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO http_pages (url, etag, last_modified, body, used_at) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(url) DO UPDATE SET etag = excluded.etag, \
             last_modified = excluded.last_modified, body = excluded.body, \
             used_at = excluded.used_at",
            params![url, page.etag, page.last_modified, page.body, to_text(now)],
        )?;
        conn.execute(
            "DELETE FROM http_pages WHERE used_at < ?1",
            [to_text(now - PAGE_KEEP)],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use jobbot_domain::fixtures::NOW;

    use crate::tracker::{StoredPage, Tracker};

    #[test]
    fn a_resting_host_is_remembered_until_its_rest_ends() {
        let tracker = Tracker::in_memory().unwrap();
        let soon = *NOW + TimeDelta::minutes(5);
        let later = *NOW + TimeDelta::hours(2);
        tracker.rest_host("a.test:443", soon).unwrap();
        tracker.rest_host("b.test:443", later).unwrap();
        tracker
            .rest_host("c.test:443", *NOW - TimeDelta::minutes(1))
            .unwrap();
        assert_eq!(
            tracker.resting_hosts(*NOW).unwrap(),
            vec![
                ("a.test:443".to_string(), soon),
                ("b.test:443".to_string(), later)
            ],
            "a rest that is over is not reported"
        );
        tracker.rest_host("a.test:443", later).unwrap();
        assert_eq!(
            tracker
                .resting_hosts(*NOW + TimeDelta::minutes(10))
                .unwrap(),
            vec![
                ("a.test:443".to_string(), later),
                ("b.test:443".to_string(), later)
            ],
            "a new rest replaces the old one"
        );
        let rows: i64 = tracker
            .lock()
            .query_row("SELECT COUNT(*) FROM http_cooldowns", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 2, "the rest that was over is gone");
    }

    fn stored(etag: Option<&str>, body: &str) -> StoredPage {
        StoredPage {
            etag: etag.map(str::to_string),
            last_modified: Some("Wed, 21 Oct 2026 07:28:00 GMT".to_string()),
            body: body.to_string(),
        }
    }

    #[test]
    fn a_stored_page_comes_back_and_a_new_one_replaces_it() {
        let tracker = Tracker::in_memory().unwrap();
        let url = "https://a.test/feed?page=2";
        assert_eq!(tracker.recall_page(url, *NOW).unwrap(), None);
        tracker
            .store_page(url, &stored(Some("\"v1\""), "jobs"), *NOW)
            .unwrap();
        assert_eq!(
            tracker.recall_page(url, *NOW).unwrap(),
            Some(stored(Some("\"v1\""), "jobs"))
        );
        tracker
            .store_page(url, &stored(None, "more jobs"), *NOW)
            .unwrap();
        assert_eq!(
            tracker.recall_page(url, *NOW).unwrap(),
            Some(stored(None, "more jobs"))
        );
        assert_eq!(
            tracker.recall_page("https://a.test/feed", *NOW).unwrap(),
            None,
            "the query is part of the page"
        );
    }

    #[test]
    fn a_page_nobody_asked_for_in_a_month_is_dropped() {
        let tracker = Tracker::in_memory().unwrap();
        tracker
            .store_page("https://a.test/old", &stored(Some("o"), "old"), *NOW)
            .unwrap();
        tracker
            .store_page("https://a.test/used", &stored(Some("u"), "used"), *NOW)
            .unwrap();
        // Asking for a page counts as using it, even when the host answers that it did not change.
        let weeks_later = *NOW + TimeDelta::days(25);
        assert!(
            tracker
                .recall_page("https://a.test/used", weeks_later)
                .unwrap()
                .is_some()
        );
        let month_later = *NOW + TimeDelta::days(31);
        tracker
            .store_page("https://a.test/new", &stored(Some("n"), "new"), month_later)
            .unwrap();
        assert_eq!(
            tracker
                .recall_page("https://a.test/old", month_later)
                .unwrap(),
            None
        );
        assert!(
            tracker
                .recall_page("https://a.test/used", month_later)
                .unwrap()
                .is_some()
        );
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
}
