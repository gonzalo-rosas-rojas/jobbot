//! The postings (`jobs`): finding duplicates, storing, refreshing and scoring them.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use jobbot_domain::models::Job;
use jobbot_domain::text::{link_base, normalize};
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params, params_from_iter};

use super::{
    Result, State, Tracker, TrackerError, from_text, optional_from_text, placeholders, to_text,
};

/// What makes two postings the same role: the normalized company and the lowercased, trimmed
/// title, as stored in `jobs.company_norm` and `jobs.title_lc`.
fn posting_identity(job: &Job) -> (String, String) {
    (normalize(&job.company), job.title.trim().to_lowercase())
}

/// The one rule for "the same role", as a SQL condition over `jobs` rows, used by `posting_known`
/// (has this been seen?) and by `find_job_key` (which copy is it?) so they cannot disagree.
/// Parameters: `?2` company, `?3` title (both as `posting_identity`), `?5` the new posting's
/// place, lowercased, `?6` its source. Same company and title is the same role, except that two
/// rows of one source whose places differ are two openings (a company's own board listing Berlin
/// and Lisbon). Across sources the place is not compared: each writes it in its own words.
const SAME_ROLE: &str = "(?2 <> '' AND ?3 <> '' AND company_norm = ?2 AND title_lc = ?3 \
     AND (source <> ?6 OR ?5 = '' OR location = '' OR instr(lower(location), ?5) > 0 \
          OR instr(?5, lower(location)) > 0))";

const JOB_SELECT: &str = "SELECT key, source, company, title, url, apply_url, location, remote, \
     description, salary_min, salary_max, posted_at FROM jobs";

/// The score detail as the JSON text of its column.
fn encode_detail(detail: &BTreeMap<String, f64>) -> Result<String> {
    serde_json::to_string(detail).map_err(|source| TrackerError::Encode {
        what: "score_detail".to_string(),
        source,
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

/// The key of the stored posting that points to the same link as `job` (`link_base`), by key
/// order, if any.
fn find_by_link(conn: &Connection, job: &Job) -> Result<Option<String>> {
    let base = link_base(&job.url);
    if base.is_empty() {
        return Ok(None);
    }
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
    Ok(None)
}

/// The four columns `refresh_posting` may rewrite, as stored.
#[derive(Debug, PartialEq)]
struct RefreshedFields {
    apply_url: String,
    posted_at: Option<String>,
    location: String,
    remote: bool,
}

impl RefreshedFields {
    /// What these columns become after a fetch reports `job`: a blank (an empty link or location,
    /// a missing date) never replaces a good value.
    fn refreshed_by(&self, job: &Job) -> RefreshedFields {
        let keep_if_blank =
            |fresh: &str, old: &str| if fresh.is_empty() { old } else { fresh }.to_string();
        RefreshedFields {
            apply_url: keep_if_blank(&job.apply_url, &self.apply_url),
            posted_at: job
                .posted_at
                .map(to_text)
                .or_else(|| self.posted_at.clone()),
            location: keep_if_blank(&job.location, &self.location),
            remote: job.remote,
        }
    }
}

impl Tracker {
    /// True when this exact posting, or the same title at the same company, is known.
    /// Postings with an empty company or title only match by key.
    pub fn has_posting(&self, job: &Job) -> Result<bool> {
        self.posting_known(job, None)
    }

    /// The key of the stored posting that `job` stands for, or `None` when it is new. In order: the
    /// same key; the same link (`link_base`: the query, the fragment and a final `/application` do
    /// not count, and `/jobs/1` is not `/jobs/12`); the same title at the same company.
    pub fn find_job_key(&self, job: &Job) -> Result<Option<String>> {
        let conn = self.lock();
        let by_key: Option<String> = conn
            .query_row("SELECT key FROM jobs WHERE key = ?1", [job.key()], |row| {
                row.get(0)
            })
            .optional()?;
        if by_key.is_some() {
            return Ok(by_key);
        }
        if let Some(key) = find_by_link(&conn, job)? {
            return Ok(Some(key));
        }
        let (company_norm, title_lc) = posting_identity(job);
        if company_norm.is_empty() || title_lc.is_empty() {
            return Ok(None);
        }
        // The newest copy wins; among copies first seen at the same moment, one whose application
        // is not withdrawn (or has none) comes first, then the key, so the answer never depends
        // on the order the rows were stored in.
        Ok(conn
            .query_row(
                &format!(
                    "SELECT key FROM jobs WHERE {SAME_ROLE} ORDER BY first_seen DESC, \
                     COALESCE((SELECT a.state = 'withdrawn' FROM applications a \
                               WHERE a.job_key = jobs.key), 0), key LIMIT 1"
                ),
                params![
                    "",
                    company_norm,
                    title_lc,
                    "",
                    job.location.trim().to_lowercase(),
                    job.source
                ],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// `has_posting` for discovery: the same title at the same company only counts as a duplicate
    /// while the stored posting is still live, that is, dated (or, with no date, first seen) at or
    /// after `repost_cutoff`. A role the company posts again after the old one went stale or was
    /// withdrawn is a new posting. The same key is always the same posting.
    pub fn has_posting_since(&self, job: &Job, repost_cutoff: DateTime<Utc>) -> Result<bool> {
        self.posting_known(job, Some(repost_cutoff))
    }

    /// The question behind `has_posting` and `has_posting_since`: the same key, or the same role
    /// (`SAME_ROLE`; when `live_since` is given, one dated or first seen at or after it). The
    /// same title at the same company is one role across sources, whatever place each writes
    /// (`Remote (LATAM)` and `Remote - Latin America` are one posting). Only inside one source do
    /// the places tell openings apart (a company's own `Berlin` and `Lisbon`), by containment.
    /// Trade-off: the same role from one source in places written in unrelated words
    /// (`Worldwide`, `Anywhere`) is stored twice, and a role that several sources list for
    /// different cities is stored once. Both are cheaper than applying twice to the same role.
    fn posting_known(&self, job: &Job, live_since: Option<DateTime<Utc>>) -> Result<bool> {
        let (company_norm, title_lc) = posting_identity(job);
        let conn = self.lock();
        let found = conn
            .query_row(
                &format!(
                    "SELECT 1 FROM jobs WHERE key = ?1 OR ({SAME_ROLE} \
                     AND (?4 IS NULL OR COALESCE(posted_at, first_seen) >= ?4)) LIMIT 1"
                ),
                params![
                    job.key(),
                    company_norm,
                    title_lc,
                    live_since.map(to_text),
                    job.location.trim().to_lowercase(),
                    job.source
                ],
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
    ) -> Result<()> {
        let detail_json = encode_detail(detail)?;
        let (company_norm, title_lc) = posting_identity(job);
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
                company_norm,
                job.title,
                title_lc,
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
    /// field stay as they were scored. Returns whether the posting changed. One UPDATE on the
    /// posting; when its link changed, the review of the old form is reset in the same
    /// transaction.
    ///
    /// A blank never replaces a good value: not an empty apply link, not an empty location, not a
    /// missing date (a fetch that lacks them says nothing about them). A posting that is not
    /// stored is not created.
    pub fn refresh_posting(&self, job: &Job) -> Result<bool> {
        let mut guard = self.lock();
        // One transaction: the new link and the dropped approval land together or not at all (a
        // link changed with the old form's approval still standing would be sent unreviewed).
        let tx = guard.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let old: Option<RefreshedFields> = tx
            .query_row(
                "SELECT apply_url, posted_at, location, remote FROM jobs WHERE key = ?1",
                [job.key()],
                |row| {
                    Ok(RefreshedFields {
                        apply_url: row.get(0)?,
                        posted_at: row.get(1)?,
                        location: row.get(2)?,
                        remote: row.get(3)?,
                    })
                },
            )
            .optional()?;
        let Some(old) = old else {
            return Ok(false);
        };
        let new = old.refreshed_by(job);
        if new == old {
            return Ok(false);
        }
        tx.execute(
            "UPDATE jobs SET apply_url = ?1, posted_at = ?2, location = ?3, remote = ?4 \
             WHERE key = ?5",
            params![
                new.apply_url,
                new.posted_at,
                new.location,
                new.remote,
                job.key()
            ],
        )?;
        // The review (the approval and the dry-run screenshot) belongs to the form at the OLD link.
        // A posting that now points to another form must be looked at again before anything is
        // sent; what was already sent or finished keeps its history.
        if new.apply_url != old.apply_url {
            let reviewed = [State::Prepared, State::NeedsGonza];
            tx.execute(
                &format!(
                    "UPDATE applications SET approved = 0, screenshot_path = NULL \
                     WHERE job_key = ? AND state IN ({})",
                    placeholders(reviewed.len())
                ),
                params_from_iter(
                    std::iter::once(job.key())
                        .chain(reviewed.iter().map(|s| s.as_str().to_string())),
                ),
            )?;
        }
        tx.commit()?;
        Ok(true)
    }

    pub fn get_job(&self, key: &str) -> Result<Option<Job>> {
        let conn = self.lock();
        let job = conn
            .query_row(&format!("{JOB_SELECT} WHERE key = ?1"), [key], job_from_row)
            .optional()?;
        Ok(job)
    }

    /// Stored postings that never got an application: discovery scored them below the
    /// threshold. `rescore` looks at them again when the criteria change.
    pub fn jobs_without_application(&self) -> Result<Vec<Job>> {
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
    pub fn set_score(&self, key: &str, score: f64, detail: &BTreeMap<String, f64>) -> Result<()> {
        let detail_json = encode_detail(detail)?;
        let conn = self.lock();
        let changed = conn.execute(
            "UPDATE jobs SET score = ?1, score_detail = ?2 WHERE key = ?3",
            params![score, detail_json, key],
        )?;
        if changed == 0 {
            return Err(TrackerError::UnknownKey(key.to_string()));
        }
        Ok(())
    }

    /// When this posting was first seen (stored), or `None` when it is not stored.
    pub fn job_first_seen(&self, key: &str) -> Result<Option<DateTime<Utc>>> {
        let conn = self.lock();
        let text: Option<String> = conn
            .query_row("SELECT first_seen FROM jobs WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(optional_from_text(0, text)?)
    }

    /// When the oldest stored job was first seen.
    pub fn first_seen_at(&self) -> Result<Option<DateTime<Utc>>> {
        let conn = self.lock();
        let text: Option<String> =
            conn.query_row("SELECT MIN(first_seen) FROM jobs", [], |row| row.get(0))?;
        Ok(optional_from_text(0, text)?)
    }

    /// When each posting first seen at or after `since` was first seen, oldest first.
    pub fn first_seen_since(&self, since: DateTime<Utc>) -> Result<Vec<DateTime<Utc>>> {
        let conn = self.lock();
        let mut stmt =
            conn.prepare("SELECT first_seen FROM jobs WHERE first_seen >= ?1 ORDER BY first_seen")?;
        let rows = stmt.query_map([to_text(since)], |row| {
            let text: String = row.get(0)?;
            from_text(0, &text)
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// How the score of a stored posting was made up, by criterion.
    pub fn score_detail(&self, key: &str) -> Result<BTreeMap<String, f64>> {
        let conn = self.lock();
        let text: Option<String> = conn
            .query_row(
                "SELECT score_detail FROM jobs WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        let Some(text) = text else {
            return Err(TrackerError::UnknownKey(key.to_string()));
        };
        serde_json::from_str(&text).map_err(|source| TrackerError::Corrupt {
            what: format!("score_detail of {key}"),
            source,
        })
    }

    /// Keys of the applications of a company (its `normalize`d name) that are in one of `states`.
    /// A blank name matches nothing.
    pub fn job_keys_of_company(&self, company_norm: &str, states: &[State]) -> Result<Vec<String>> {
        if company_norm.is_empty() || states.is_empty() {
            return Ok(Vec::new());
        }
        let marks = placeholders(states.len());
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
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::TimeDelta;
    use jobbot_domain::fixtures::{NOW, make_job, make_job_with};
    use jobbot_domain::models::Job;

    use crate::tracker::test_support::{stored_at, three_scored};
    use crate::tracker::{State, Tracker};

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
    fn a_relink_whose_approval_cannot_be_dropped_leaves_the_old_link_too() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobbot.db");
        let t = Tracker::open(&path).unwrap();
        let job = stored_posting();
        t.add_job(&job, 77.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        t.transition(&job.key(), State::Prepared, None, *NOW)
            .unwrap();
        t.set_screenshot(&job.key(), std::path::Path::new("shots/a.png"))
            .unwrap();
        t.approve(&job.key()).unwrap();
        // The second write cannot happen (a trigger refuses it, as a full disk or a lock might).
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER no_review_reset BEFORE UPDATE OF approved ON applications BEGIN SELECT RAISE(ABORT, 'refused'); END;",
            )
            .unwrap();
        let relinked = Job {
            apply_url: "https://job-boards.greenhouse.io/other/jobs/9".into(),
            ..job.clone()
        };
        assert!(t.refresh_posting(&relinked).is_err());
        let after = t.get_job(&job.key()).unwrap().unwrap();
        assert_eq!(
            after.apply_url, job.apply_url,
            "the link did not change without the approval being dropped"
        );
        assert!(t.get_application(&job.key()).unwrap().unwrap().approved);
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
                    "remote" => j.remote = false,
                    other => panic!("no case for the field {other}"),
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
    fn find_job_key_prefers_a_copy_whose_application_is_not_withdrawn_when_first_seen_ties() {
        for (withdrawn, live) in [("1", "2"), ("2", "1")] {
            let t = Tracker::in_memory().unwrap();
            for id in [withdrawn, live] {
                let copy = make_job_with(|j| {
                    j.external_id = id.into();
                    j.url = format!("https://jobs.example.com/{id}");
                    j.apply_url = j.url.clone();
                });
                t.add_job(&copy, 70.0, &BTreeMap::new(), *NOW).unwrap();
                t.create_application(&copy.key(), *NOW).unwrap();
            }
            t.transition(&format!("test:{withdrawn}"), State::Withdrawn, None, *NOW)
                .unwrap();
            let third = make_job_with(|j| {
                j.external_id = "3".into();
                j.url = "https://jobs.example.com/3".into();
                j.apply_url = j.url.clone();
            });

            assert_eq!(
                t.find_job_key(&third).unwrap(),
                Some(format!("test:{live}")),
                "withdrawn copy is test:{withdrawn}"
            );
        }
    }

    #[test]
    fn the_place_tells_two_openings_apart_only_inside_one_source() {
        let t = Tracker::in_memory().unwrap();
        let at = |source: &str, id: &str, place: &str| {
            make_job_with(|j| {
                j.source = source.into();
                j.external_id = id.into();
                j.url = format!("https://jobs.example.com/{source}/{id}");
                j.apply_url = j.url.clone();
                j.location = place.into();
            })
        };
        t.add_job(&at("ashby", "1", "Berlin"), 70.0, &BTreeMap::new(), *NOW)
            .unwrap();
        // Same source, another place: a second opening. Another source: the same role, whatever
        // words it uses for the place.
        assert!(!t.has_posting(&at("ashby", "2", "Lisbon")).unwrap());
        assert!(t.has_posting(&at("himalayas", "3", "Lisbon")).unwrap());
        assert_eq!(
            t.find_job_key(&at("ashby", "2", "Lisbon")).unwrap(),
            None,
            "the lookup agrees with has_posting"
        );
        assert_eq!(
            t.find_job_key(&at("himalayas", "3", "Lisbon")).unwrap(),
            Some("ashby:1".to_string())
        );
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
    fn first_seen_is_the_first_add() {
        let t = Tracker::in_memory().unwrap();
        assert_eq!(t.first_seen_at().unwrap(), None);
        three_scored(&t, *NOW);
        assert_eq!(t.first_seen_at().unwrap(), Some(*NOW));
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
    fn jobs_without_application_lists_only_unqueued_postings() {
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
        t.create_application(&stored.key(), *NOW).unwrap();
        assert!(t.jobs_without_application().unwrap().is_empty());
    }

    #[test]
    fn set_score_is_kept_when_the_application_is_created() {
        let t = Tracker::in_memory().unwrap();
        let stored = make_job_with(|j| {
            j.external_id = "2".into();
            j.company = "Other Co".into();
            j.title = "Other Role".into();
        });
        t.add_job(&stored, 40.0, &BTreeMap::new(), *NOW).unwrap();
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
        assert!(t.set_score("nope:1", 1.0, &BTreeMap::new()).is_err());
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
    fn score_detail_round_trips_and_an_unknown_key_is_an_error() {
        let t = Tracker::in_memory().unwrap();
        let detail = BTreeMap::from([("fit".to_string(), 30.0), ("pay".to_string(), 12.5)]);
        t.add_job(&make_job(), 77.0, &detail, *NOW).unwrap();
        assert_eq!(t.score_detail(&make_job().key()).unwrap(), detail);
        assert!(t.score_detail("nope:1").is_err());
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
}
