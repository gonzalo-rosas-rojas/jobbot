//! What `jobbot status` says about the engine, and the record each `run-daily` leaves for it and
//! for the panel in `meta` (`last_run_at`, `last_run_result`, `last_run_steps`): how the run ended
//! and what every step did.

use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};
use chrono_tz::America::Santiago;
use jobbot_domain::outbound::plain_line;

use crate::actions::auto_approve_min;
use crate::ports::store::Store;

/// `run-daily` fires every hour, but the boards do not change that fast: discovery is skipped
/// while the last one (`meta.last_discover_at`) is younger than this. `jobbot discover` itself
/// always runs. Six hours keeps the scheduled runs to four a day at most: Remotive's terms ask
/// for no more than four requests a day, and the other boards do not change faster than that.
pub const DISCOVER_COOLDOWN_HOURS: i64 = 6;

/// The steps of `run-daily`, in order: the key in `meta.last_run_steps` and the word `status` uses.
pub const RUN_STEPS: [(&str, &str); 4] = [
    ("discover", "descubrir"),
    ("prepare", "preparar"),
    ("submit", "enviar"),
    ("dossiers", "dossiers"),
];

/// Longest error text kept in `meta.last_run_result`: one line the panel can show whole.
pub const RUN_ERROR_CHARS: usize = 300;

/// How a `run-daily` ended: the value of `meta.last_run_result`.
pub enum RunEnd {
    /// Every step that was due ran (a failure of the dossiers alone does not count).
    Done,
    /// A STOP file appeared between two steps.
    Stopped,
    /// The execution window closed between two steps.
    WindowClosed,
    /// A step failed; the text is its error, cut and on one line when written.
    Failed(String),
}

impl RunEnd {
    pub fn as_meta(&self) -> String {
        match self {
            RunEnd::Done => "ok".to_string(),
            RunEnd::Stopped => "stopped".to_string(),
            RunEnd::WindowClosed => "window_closed".to_string(),
            RunEnd::Failed(why) => format!(
                "error: {}",
                plain_line(why)
                    .chars()
                    .take(RUN_ERROR_CHARS)
                    .collect::<String>()
            ),
        }
    }

    /// Why a step that never started did not: the `skipped` of its entry in the log.
    pub fn skip_reason(&self) -> &'static str {
        match self {
            RunEnd::Done => "not_due",
            RunEnd::Stopped => "stop",
            RunEnd::WindowClosed => "window_closed",
            RunEnd::Failed(_) => "after_error",
        }
    }
}

/// What each step of one `run-daily` did, as the JSON of `meta.last_run_steps`.
#[derive(Default)]
pub struct StepLog(serde_json::Map<String, serde_json::Value>);

impl StepLog {
    pub fn ran(&mut self, step: &str, mut items: serde_json::Value) {
        items["ran"] = serde_json::Value::Bool(true);
        self.0.insert(step.to_string(), items);
    }

    pub fn skipped(&mut self, step: &str, why: &str) {
        self.0.insert(
            step.to_string(),
            serde_json::json!({"ran": false, "skipped": why}),
        );
    }

    /// The steps the run never reached say why, so the record always lists all four.
    pub fn close(&mut self, end: &RunEnd) {
        for (step, _) in RUN_STEPS {
            if !self.0.contains_key(step) {
                self.skipped(step, end.skip_reason());
            }
        }
    }
}

/// Writes `meta.last_run_at` (the end of the run), `last_run_result` and `last_run_steps`. Only a
/// run that got past STOP, the window and the lock gets here: the hourly task turned away at the
/// door writes nothing, so the last real run stays visible and `run-daily.cmd` keeps its log empty.
/// The stamp goes last, so whoever sees a new one finds the result that belongs to it.
#[tracing::instrument(skip_all)]
pub fn record_run(
    store: &dyn Store,
    finished: DateTime<Utc>,
    end: &RunEnd,
    log: &mut StepLog,
) -> Result<()> {
    log.close(end);
    store.set_meta("last_run_result", &end.as_meta())?;
    store.set_meta(
        "last_run_steps",
        &serde_json::Value::Object(log.0.clone()).to_string(),
    )?;
    store.set_meta("last_run_at", &finished.to_rfc3339())?;
    Ok(())
}

/// A stamp for Gonza: his clock (Santiago) and how long ago, e.g. `2026-10-14 19:00 (hace 2 h)`;
/// `name_the_zone` makes it `(hora de Santiago, hace 2 h)`.
pub fn stamp_with_age(at: DateTime<Utc>, now: DateTime<Utc>, name_the_zone: bool) -> String {
    let age = now - at;
    let ago = if age < TimeDelta::minutes(1) {
        "hace un momento".to_string()
    } else if age < TimeDelta::hours(1) {
        format!("hace {} min", age.num_minutes())
    } else if age < TimeDelta::days(2) {
        format!("hace {} h", age.num_hours())
    } else {
        format!("hace {} días", age.num_days())
    };
    let zone = if name_the_zone {
        "hora de Santiago, "
    } else {
        ""
    };
    format!(
        "{} ({zone}{ago})",
        at.with_timezone(&Santiago).format("%Y-%m-%d %H:%M")
    )
}

/// The `Última corrida` lines of `status`, from `meta.last_run_*`.
pub fn describe_last_run(store: &dyn Store, now: DateTime<Utc>) -> Result<String> {
    let at = store
        .get_meta("last_run_at")?
        .and_then(|text| DateTime::parse_from_rfc3339(&text).ok())
        .map(|time| time.with_timezone(&Utc));
    let Some(at) = at else {
        return Ok("Última corrida: ninguna todavía (la tarea programada la anota al terminar cada run-daily).".to_string());
    };
    let result = store.get_meta("last_run_result")?.unwrap_or_default();
    let result = match result.as_str() {
        "ok" => "ok".to_string(),
        "stopped" => "se detuvo por STOP".to_string(),
        "window_closed" => "se cerró la ventana de ejecución".to_string(),
        other => other.to_string(),
    };
    let mut text = format!(
        "Última corrida: {}: {result}",
        stamp_with_age(at, now, true)
    );
    let steps = store
        .get_meta("last_run_steps")?
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok());
    if let Some(steps) = steps {
        text.push_str("\n  ");
        text.push_str(&describe_steps(&steps));
    }
    Ok(text)
}

/// `descubrir: 1 nuevas · preparar: 1 preparadas · ...` from the JSON of `meta.last_run_steps`.
pub fn describe_steps(steps: &serde_json::Value) -> String {
    let count = |step: &str, key: &str| steps[step][key].as_i64().unwrap_or(0);
    let mut parts = Vec::new();
    for (step, label) in RUN_STEPS {
        let entry = &steps[step];
        if entry.is_null() {
            continue;
        }
        let what = if entry["ran"].as_bool() != Some(true) {
            match entry["skipped"].as_str().unwrap_or_default() {
                "cooldown" => format!("omitido (hace menos de {DISCOVER_COOLDOWN_HOURS} h)"),
                "stop" => "omitido (STOP)".to_string(),
                "window_closed" => "omitido (ventana cerrada)".to_string(),
                "after_error" => "omitido (tras un error)".to_string(),
                "error" => "con error".to_string(),
                other => format!("omitido ({other})"),
            }
        } else {
            match step {
                "discover" => format!("{} nuevas", count(step, "new")),
                "prepare" => format!("{} preparadas", count(step, "prepared")),
                "submit" => format!(
                    "{} enviadas, {} para ti",
                    count(step, "submitted"),
                    count(step, "needs_gonza")
                ),
                _ => format!("{} escritos", count(step, "written")),
            }
        };
        parts.push(format!("{label}: {what}"));
    }
    parts.join(" · ")
}

/// The `Cuenta de Claude` line of `status`, from what the last preflight left in `meta`.
pub fn describe_last_account(store: &dyn Store, now: DateTime<Utc>) -> Result<String> {
    let record = store
        .get_meta("claude_account_last")?
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok());
    let Some(record) = record else {
        return Ok(
            "Cuenta de Claude: sin verificar todavía (jobbot claude-login, o jobbot claude-check)."
                .to_string(),
        );
    };
    let when = record["checked_at"]
        .as_str()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|time| stamp_with_age(time.with_timezone(&Utc), now, false))
        .unwrap_or_else(|| "—".to_string());
    // What the check noticed without refusing (a managed policy on this PC), under either verdict.
    let warnings: String = record["warnings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|warning| warning.as_str())
        .map(|warning| format!("\n  Aviso: {warning}"))
        .collect();
    if record["ok"].as_bool() != Some(true) {
        return Ok(format!(
            "Cuenta de Claude: la última verificación falló ({when}): {}{warnings}",
            record["message"].as_str().unwrap_or("sin detalle")
        ));
    }
    let field = |key: &str| match record[key].as_str() {
        Some(text) if !text.is_empty() => text.to_string(),
        _ => "—".to_string(),
    };
    Ok(format!(
        "Cuenta de Claude: verificada {when} · método {} · proveedor {} · suscripción {} · correo {} · org {}{warnings}",
        field("auth_method"),
        field("api_provider"),
        field("subscription"),
        field("email"),
        field("org_id"),
    ))
}

/// The `Auto-aprobación` line of `status` and of `jobbot auto-approve`.
pub fn describe_auto_approve(store: &dyn Store) -> Result<String> {
    Ok(match auto_approve_min(store)? {
        None => "Auto-aprobación: apagada.".to_string(),
        Some(min) => format!("Auto-aprobación: desde {min} puntos."),
    })
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;

    use super::*;

    #[test]
    fn a_failed_run_is_kept_as_one_line_cut_to_the_limit() {
        let why = format!("first line\nsecond\tline {}", "x".repeat(500));
        let meta = RunEnd::Failed(why).as_meta();
        assert!(
            meta.starts_with("error: first line second line x"),
            "{meta}"
        );
        assert!(!meta.contains('\n') && !meta.contains('\t'), "{meta}");
        assert_eq!(meta.chars().count(), "error: ".len() + RUN_ERROR_CHARS);
    }

    #[test]
    fn a_run_that_stops_early_still_lists_every_step_and_why() {
        let mut log = StepLog::default();
        log.ran("discover", json!({"new": 2}));
        log.close(&RunEnd::Stopped);
        let steps = serde_json::Value::Object(log.0.clone());
        assert_eq!(steps["discover"], json!({"new": 2, "ran": true}));
        for step in ["prepare", "submit", "dossiers"] {
            assert_eq!(
                steps[step],
                json!({"ran": false, "skipped": "stop"}),
                "{step}"
            );
        }
    }

    #[test]
    fn the_steps_read_as_one_line_in_gonzas_words() {
        let steps = json!({
            "discover": {"ran": false, "skipped": "cooldown"},
            "prepare": {"ran": true, "prepared": 3},
            "submit": {"ran": true, "submitted": 1, "needs_gonza": 2},
            "dossiers": {"ran": false, "skipped": "error"},
        });
        assert_eq!(
            describe_steps(&steps),
            format!(
                "descubrir: omitido (hace menos de {DISCOVER_COOLDOWN_HOURS} h) · preparar: 3 preparadas · \
                 enviar: 1 enviadas, 2 para ti · dossiers: con error"
            )
        );
    }

    #[test]
    fn a_stamp_is_santiago_time_with_its_age() {
        // July: Santiago is four hours behind UTC.
        let at = Utc.with_ymd_and_hms(2026, 7, 15, 15, 0, 0).unwrap();
        let now = at + TimeDelta::hours(2);
        assert_eq!(
            stamp_with_age(at, now, false),
            "2026-07-15 11:00 (hace 2 h)"
        );
        assert_eq!(
            stamp_with_age(at, now, true),
            "2026-07-15 11:00 (hora de Santiago, hace 2 h)"
        );
        assert_eq!(
            stamp_with_age(at, at + TimeDelta::seconds(20), false),
            "2026-07-15 11:00 (hace un momento)"
        );
        assert_eq!(
            stamp_with_age(at, at + TimeDelta::days(3), false),
            "2026-07-15 11:00 (hace 3 días)"
        );
    }
}
