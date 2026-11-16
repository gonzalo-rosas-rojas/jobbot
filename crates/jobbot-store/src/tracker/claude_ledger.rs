//! What the calls to Claude cost: one row per call, summed by window and by stage.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use rusqlite::params;

use super::{Result, Tracker, to_text};

impl Tracker {
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
    ) -> Result<()> {
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
    pub fn claude_spend_since(&self, since: DateTime<Utc>) -> Result<f64> {
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
    ) -> Result<BTreeMap<String, (i64, f64)>> {
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
    use chrono::TimeDelta;
    use jobbot_domain::fixtures::NOW;

    use crate::tracker::Tracker;

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
}
