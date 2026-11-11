//! Claude as the use cases see it: every call goes through `ClaudeGate`, which refuses once the
//! rolling 24-hour spend reaches the daily budget and writes each call to the ledger. The runner
//! (`claude -p` as a child process) is the `jobbot-claude` adapter; the port it implements
//! (`ClaudeRunner` and its request, reply and error types) is in `crate::ports`, re-exported here.

use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::Value;

use crate::ports::store::Store;

pub use crate::ports::{
    AccountSeen, ClaudeError, ClaudeReply, ClaudeRequest, ClaudeRunner, ShapeError,
};

/// Every call the program makes goes through here: refused once the rolling 24-hour spend has
/// reached the daily budget, and written to the ledger whatever the outcome.
pub struct ClaudeGate<'a> {
    runner: &'a dyn ClaudeRunner,
    tracker: &'a dyn Store,
    /// The rolling 24-hour spend at which `ask` refuses (`claude.toml`'s `daily_budget_usd`).
    daily_budget_usd: f64,
    /// What a call that was paid for but reported no cost (it timed out, or ended without a JSON
    /// result) is written to the ledger as: the most it could have cost (`per_call_budget_usd`).
    /// Zero (the default) writes such a call as free, as before.
    per_call_cap_usd: f64,
}

impl<'a> ClaudeGate<'a> {
    pub fn new(
        runner: &'a dyn ClaudeRunner,
        tracker: &'a dyn Store,
        daily_budget_usd: f64,
    ) -> ClaudeGate<'a> {
        ClaudeGate {
            runner,
            tracker,
            daily_budget_usd,
            per_call_cap_usd: 0.0,
        }
    }

    /// Writes a call that did its work but reported no cost (a timeout, an unreadable result) as
    /// costing `cap_usd`, so the daily budget cannot be undercounted by a night of cut-off calls.
    /// A process that exits with an error and no result stays free (see `ask`).
    pub fn with_per_call_cap(mut self, cap_usd: f64) -> ClaudeGate<'a> {
        self.per_call_cap_usd = cap_usd;
        self
    }

    /// The runner's preflight, written to the tracker's `meta` whatever the verdict
    /// (`claude_account_last_check`, `claude_account_last`), so the panel and the digest can show
    /// later which login the run used or why it stopped. Callers run it before the first `ask`.
    pub async fn check_account(&self, now: DateTime<Utc>) -> Result<AccountSeen> {
        let result = self.runner.preflight().await;
        let mut record = match &result {
            Ok(seen) => serde_json::to_value(seen)?,
            Err(ClaudeError::Account {
                seen: Some(seen), ..
            }) => serde_json::to_value(seen)?,
            Err(_) => serde_json::json!({}),
        };
        record["ok"] = Value::Bool(result.is_ok());
        record["message"] = match &result {
            Ok(_) => Value::Null,
            Err(err) => Value::String(err.to_string()),
        };
        record["checked_at"] = Value::String(now.to_rfc3339());
        self.tracker
            .set_meta("claude_account_last_check", &now.to_rfc3339())?;
        self.tracker
            .set_meta("claude_account_last", &record.to_string())?;
        Ok(result?)
    }

    pub async fn ask(&self, req: &ClaudeRequest, now: DateTime<Utc>) -> Result<ClaudeReply> {
        let spent = self
            .tracker
            .claude_spend_since(now - TimeDelta::hours(24))?;
        if spent >= self.daily_budget_usd {
            return Err(ClaudeError::BudgetExhausted {
                spent,
                limit: self.daily_budget_usd,
            }
            .into());
        }
        match self.runner.run(req).await {
            Ok(reply) => {
                // The answer is already paid for: a ledger that cannot be written does not throw
                // it away.
                if let Err(write_err) = self.tracker.record_claude_call(
                    &req.stage,
                    now,
                    reply.cost_usd,
                    reply.input_tokens,
                    reply.output_tokens,
                    true,
                ) {
                    tracing::warn!(
                        stage = %req.stage,
                        cost_usd = reply.cost_usd,
                        error = %write_err,
                        "could not record a paid Claude call in the ledger"
                    );
                }
                Ok(reply)
            }
            Err(err) => {
                // A call that ran out of time, or ended well with a result nobody could read, did
                // its work and was paid for. A process that exits with an error and no result
                // (`Failed`) most often never reached the model (a flag a new CLI refuses, a
                // crash at start): charging the cap for each would use up the day's budget in a
                // few calls and hide the real fault behind "budget reached".
                let cost = match &err {
                    ClaudeError::Timeout(_) | ClaudeError::Unreadable(_) => self.per_call_cap_usd,
                    other => other.cost_usd(),
                };
                self.tracker
                    .record_claude_call(&req.stage, now, cost, 0, 0, false)?;
                Err(err.into())
            }
        }
    }
}

/// True when `err` is the daily budget running out (callers stop asking, nothing else is wrong).
pub fn is_budget_error(err: &anyhow::Error) -> bool {
    matches!(
        err.downcast_ref::<ClaudeError>(),
        Some(ClaudeError::BudgetExhausted { .. })
    )
}

/// True when `err` is the preflight refusing the login (the run must stop before any prompt).
pub fn is_account_error(err: &anyhow::Error) -> bool {
    matches!(
        err.downcast_ref::<ClaudeError>(),
        Some(ClaudeError::Account { .. })
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_budget_error_is_a_budget_error() {
        let budget: anyhow::Error = ClaudeError::BudgetExhausted {
            spent: 1.0,
            limit: 1.0,
        }
        .into();
        let other: anyhow::Error = ClaudeError::Timeout(1).into();
        assert!(is_budget_error(&budget) && !is_budget_error(&other));
        assert!(!is_budget_error(&anyhow::anyhow!("plain")));
    }

    #[test]
    fn only_the_preflight_refusal_is_an_account_error() {
        let account: anyhow::Error = ClaudeError::Account {
            message: "x".into(),
            seen: None,
        }
        .into();
        let other: anyhow::Error = ClaudeError::NotFound("y".into()).into();
        assert!(is_account_error(&account) && !is_account_error(&other));
        assert!(!is_account_error(&anyhow::anyhow!("plain")));
    }
}
