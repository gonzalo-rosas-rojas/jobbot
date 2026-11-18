//! The POST commands of the panel: the body each one expects, the action it calls in `actions.rs`
//! (or the importer of the mailbox, `jobbot_app::inbox`) and the reply it gives, for one key or
//! for a batch answered key by key.

use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::{Reply, Request};
use crate::Panel;
use jobbot_app::actions::{self, Outcome};
use jobbot_app::inbox;

/// How far ahead of the panel's clock a send time may be (the browser's clock drifts a little).
const FUTURE_TOLERANCE: TimeDelta = TimeDelta::minutes(5);

/// Keys one batch request may carry; `MAX_BODY` would stop it anyway, this says so clearly.
pub(super) const MAX_BATCH: usize = 200;

#[derive(Deserialize)]
struct KeyBody {
    key: String,
}

/// `{key}` for one application or `{keys: [...]}` for a batch; the reply follows the form used.
#[derive(Deserialize)]
struct KeysBody {
    key: Option<String>,
    keys: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct MarkBody {
    key: Option<String>,
    keys: Option<Vec<String>>,
    state: String,
    note: Option<String>,
}

#[derive(Deserialize)]
struct TaskAddBody {
    text: String,
    /// RFC 3339.
    due_at: Option<String>,
    job_key: Option<String>,
    link: Option<String>,
}

/// `jobbot record` from the page. No `cv` path: a file name typed in a browser means nothing here.
#[derive(Deserialize)]
struct RecordBody {
    key: Option<String>,
    company: Option<String>,
    title: Option<String>,
    url: Option<String>,
    /// RFC 3339; when it went out. Now when missing.
    at: Option<String>,
    note: Option<String>,
    location: Option<String>,
    salary_min: Option<f64>,
    salary_max: Option<f64>,
}

#[derive(Deserialize)]
struct SnoozeBody {
    key: String,
    /// `1d`, `3d`, `7d`, `monday` or RFC 3339 (see `actions::parse_snooze_until`).
    until: String,
}

#[derive(Deserialize)]
struct TaskBody {
    id: i64,
}

#[derive(Deserialize)]
struct GradeBody {
    id: String,
    grade: u8,
}

fn parse<T: DeserializeOwned>(req: &Request) -> std::result::Result<T, Reply> {
    serde_json::from_slice(&req.body).map_err(|_| Reply::message(400, "JSON inválido"))
}

fn outcome_reply(outcome: Outcome) -> Reply {
    Reply::json(
        if outcome.ok { 200 } else { 409 },
        json!({ "ok": outcome.ok, "message": outcome.message }),
    )
}

fn done(message: &str) -> Reply {
    Reply::json(200, json!({ "ok": true, "message": message }))
}

/// What a body asked for: one key (replied to as before) or a batch (replied to key by key).
enum Keys {
    One(String),
    Batch(Vec<String>),
}

fn keys_of(key: Option<String>, keys: Option<Vec<String>>) -> std::result::Result<Keys, Reply> {
    match (key, keys) {
        (Some(key), None) => Ok(Keys::One(key)),
        (None, Some(keys)) if keys.is_empty() => Err(Reply::message(400, "keys está vacía")),
        (None, Some(keys)) if keys.len() > MAX_BATCH => Err(Reply::message(
            400,
            &format!("Demasiadas claves: hasta {MAX_BATCH} por petición"),
        )),
        (None, Some(keys)) => Ok(Keys::Batch(keys)),
        _ => Err(Reply::message(
            400,
            "Falta key (una) o keys (varias), no ambas",
        )),
    }
}

/// Runs `act` on one key or on each key of a batch. A batch is answered with one result per key,
/// in order, `ok` only when every key went through (409 otherwise, like a single refusal); the
/// ones that went through stay done, since each action is its own write. An action that fails
/// outright for one key (a database error) is that key's result and the rest still run: a 500 for
/// the whole batch would hide which keys were already done.
fn apply(keys: Keys, act: impl Fn(&str) -> Result<Outcome>) -> Result<Reply> {
    match keys {
        Keys::One(key) => Ok(outcome_reply(act(&key)?)),
        Keys::Batch(keys) => {
            let mut results = Vec::with_capacity(keys.len());
            let mut done = 0usize;
            for key in &keys {
                let outcome = act(key).unwrap_or_else(|err| Outcome {
                    ok: false,
                    message: format!("Error interno: {err:#}"),
                });
                done += usize::from(outcome.ok);
                results.push(json!({ "key": key, "ok": outcome.ok, "message": outcome.message }));
            }
            let ok = done == keys.len();
            Ok(Reply::json(
                if ok { 200 } else { 409 },
                json!({
                    "ok": ok,
                    "message": format!("{done} de {} con éxito", keys.len()),
                    "results": results,
                }),
            ))
        }
    }
}

/// Runs an action that takes the `key` of the body, or each of its `keys`.
fn keyed(req: &Request, act: impl Fn(&str) -> Result<Outcome>) -> Result<Reply> {
    let body: KeysBody = match parse(req) {
        Ok(body) => body,
        Err(reply) => return Ok(reply),
    };
    match keys_of(body.key, body.keys) {
        Ok(keys) => apply(keys, act),
        Err(reply) => Ok(reply),
    }
}

/// An optional text field of the page without its surrounding blanks; `None` when it is missing or
/// only blanks.
fn trimmed(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|text| !text.is_empty())
}

/// An RFC 3339 moment from the page, as UTC.
fn moment(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text.trim())
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

/// `POST /api/settings`: `{auto_approve_min}`, a whole number or `null` (off). The only setting the
/// panel keeps; the batch threshold of the sheet is a preference of the browser and the daily
/// quota and the 90-day cool-down are not settings at all. Each key is checked by what it is: the
/// wrong shape is a 400 (the page sent something it should not), a number outside the range of
/// `actions::set_auto_approve_min` is a 409 like any refused action. The reply always says what is
/// kept now.
fn settings(panel: &Panel, req: &Request) -> Result<Reply> {
    let body: serde_json::Map<String, Value> = match parse(req) {
        Ok(body) => body,
        Err(reply) => return Ok(reply),
    };
    if let Some(unknown) = body.keys().find(|name| name.as_str() != "auto_approve_min") {
        return Ok(Reply::message(
            400,
            &format!(
                "Ajuste desconocido: {}",
                jobbot_domain::outbound::clean(unknown, 40)
            ),
        ));
    }
    let outcome = match body.get("auto_approve_min") {
        None => {
            return Ok(Reply::message(
                400,
                "Nada que cambiar: envía auto_approve_min (un entero o null)",
            ));
        }
        Some(Value::Null) => actions::set_auto_approve_min(&*panel.tracker, None, None)?,
        Some(Value::Number(number)) if number.is_i64() => {
            let threshold = panel.criteria().map(|criteria| criteria.apply_threshold);
            actions::set_auto_approve_min(&*panel.tracker, number.as_i64(), threshold)?
        }
        Some(_) => {
            return Ok(Reply::message(
                400,
                "auto_approve_min debe ser un entero o null",
            ));
        }
    };
    Ok(Reply::json(
        if outcome.ok { 200 } else { 409 },
        json!({
            "ok": outcome.ok,
            "message": outcome.message,
            "auto_approve_min": actions::auto_approve_min(&*panel.tracker)?,
        }),
    ))
}

fn task_add(panel: &Panel, req: &Request) -> Result<Reply> {
    let body: TaskAddBody = match parse(req) {
        Ok(body) => body,
        Err(reply) => return Ok(reply),
    };
    let due_at = match trimmed(body.due_at.as_deref()) {
        None => None,
        Some(text) => match moment(text) {
            Some(at) => Some(at),
            None => {
                // A 409 on purpose, not the 400 of a malformed body: the date is refused like the
                // other refusals of `task add`, and its test lists it with them.
                return Ok(outcome_reply(Outcome {
                    ok: false,
                    message: "La fecha límite debe ser RFC 3339, por ejemplo 2026-10-08T15:00:00Z"
                        .to_string(),
                }));
            }
        },
    };
    let spec = actions::TaskSpec {
        text: body.text,
        due_at,
        job_key: body.job_key,
        link: body.link,
    };
    Ok(outcome_reply(actions::task_add(
        &*panel.tracker,
        &spec,
        panel.now(),
    )?))
}

/// The same checks as `jobbot record`: `at` is RFC 3339 and not in the future.
fn record(panel: &Panel, req: &Request) -> Result<Reply> {
    let body: RecordBody = match parse(req) {
        Ok(body) => body,
        Err(reply) => return Ok(reply),
    };
    let now = panel.now();
    let refused = |message: String| Ok(outcome_reply(Outcome { ok: false, message }));
    let at = match trimmed(body.at.as_deref()) {
        None => now,
        Some(text) => match moment(text) {
            Some(at) => at,
            None => {
                // A 409 on purpose, like the future date below: both are refusals of `record`, as
                // `jobbot record` makes them, and its test lists them together.
                return refused(
                    "La fecha de envío (at) debe ser RFC 3339, por ejemplo 2026-10-05T05:21:42Z"
                        .to_string(),
                );
            }
        },
    };
    if at > now + FUTURE_TOLERANCE {
        return refused(format!(
            "La fecha de envío está en el futuro ({}): una postulación no se registra antes de enviarse",
            at.to_rfc3339()
        ));
    }
    let spec = actions::RecordSpec {
        key: body.key,
        company: body.company.unwrap_or_default(),
        title: body.title.unwrap_or_default(),
        url: body.url.unwrap_or_default(),
        location: body.location.unwrap_or_default(),
        cv_path: None,
        salary_min_monthly_usd: body.salary_min,
        salary_max_monthly_usd: body.salary_max,
        note: body.note,
    };
    Ok(outcome_reply(actions::record(&*panel.tracker, &spec, at)?))
}

/// `jobbot inbox import` with the file's content as the body: the same validation, the same
/// report. `ok` is false when a message was rejected, although the valid ones were imported.
fn inbox_import(panel: &Panel, req: &Request) -> Result<Reply> {
    let Ok(text) = std::str::from_utf8(&req.body) else {
        return Ok(Reply::message(400, "El cuerpo debe ser texto UTF-8"));
    };
    let items = match inbox::parse_items(text) {
        Ok(items) => items,
        Err(err) => return Ok(Reply::message(400, &format!("{err:#}"))),
    };
    let report = inbox::import(&*panel.tracker, &items, panel.now())?;
    let ok = report.rejected == 0;
    Ok(Reply::json(
        if ok { 200 } else { 409 },
        json!({
            "ok": ok,
            "message": report.summary(),
            "imported": report.imported,
            "skipped": report.skipped,
            "rejected": report.rejected,
            "state_changes": report.state_changes,
            "tasks_created": report.tasks_created,
            "unmatched": report.unmatched,
            "linked": report.linked,
            "lines": report.lines,
        }),
    ))
}

/// The POST route `api` (already guarded, its body within the limit).
pub(super) fn post(panel: &Panel, api: &str, req: &Request) -> Result<Reply> {
    match api {
        "approve" => keyed(req, |key| actions::approve(&*panel.tracker, key)),
        "resolve" => keyed(req, |key| {
            actions::resolve(&*panel.tracker, key, panel.now())
        }),
        "confirm" => keyed(req, |key| {
            actions::confirm(&*panel.tracker, key, panel.now())
        }),
        "mark" => {
            let body: MarkBody = match parse(req) {
                Ok(body) => body,
                Err(reply) => return Ok(reply),
            };
            let keys = match keys_of(body.key, body.keys) {
                Ok(keys) => keys,
                Err(reply) => return Ok(reply),
            };
            apply(keys, |key| {
                actions::mark(
                    &*panel.tracker,
                    key,
                    &body.state,
                    body.note.as_deref(),
                    panel.now(),
                )
            })
        }
        "mark/undo" => keyed(req, |key| {
            actions::unmark(&*panel.tracker, key, panel.now())
        }),
        "snooze" => {
            let body: SnoozeBody = match parse(req) {
                Ok(body) => body,
                Err(reply) => return Ok(reply),
            };
            Ok(outcome_reply(actions::snooze(
                &*panel.tracker,
                &body.key,
                &body.until,
                panel.now(),
            )?))
        }
        "unsnooze" => {
            let body: KeyBody = match parse(req) {
                Ok(body) => body,
                Err(reply) => return Ok(reply),
            };
            Ok(outcome_reply(actions::unsnooze(
                &*panel.tracker,
                &body.key,
            )?))
        }
        "record" => record(panel, req),
        "task/add" => task_add(panel, req),
        "settings" => settings(panel, req),
        "inbox/import" => inbox_import(panel, req),
        "task/done" => {
            let body: TaskBody = match parse(req) {
                Ok(body) => body,
                Err(reply) => return Ok(reply),
            };
            Ok(outcome_reply(actions::task_done(&*panel.tracker, body.id)?))
        }
        "learn/grade" => {
            let body: GradeBody = match parse(req) {
                Ok(body) => body,
                Err(reply) => return Ok(reply),
            };
            let questions = panel.store.all_questions()?;
            Ok(outcome_reply(actions::grade_card(
                &*panel.store,
                &questions,
                &body.id,
                body.grade,
                panel.now(),
            )?))
        }
        "stop" => {
            actions::stop(&panel.paths)?;
            Ok(done(
                "STOP creado: no corre nada automático hasta que lo quites.",
            ))
        }
        "resume" => {
            actions::resume(&panel.paths)?;
            Ok(done("STOP eliminado."))
        }
        _ => Ok(Reply::message(404, "No existe")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::support_tests::json_of;

    #[test]
    fn an_optional_text_field_without_blanks_is_none_when_nothing_is_left() {
        assert_eq!(trimmed(None), None);
        assert_eq!(trimmed(Some("")), None);
        assert_eq!(trimmed(Some("  \t ")), None);
        assert_eq!(
            trimmed(Some("  2026-10-08T15:00:00Z ")),
            Some("2026-10-08T15:00:00Z")
        );
    }

    #[test]
    fn a_batch_reports_every_key_even_when_one_fails_outright() {
        let reply = apply(
            Keys::Batch(vec!["a".into(), "b".into(), "c".into()]),
            |key| match key {
                "b" => Err(anyhow::anyhow!("disco lleno")),
                _ => Ok(Outcome {
                    ok: true,
                    message: format!("hecho {key}"),
                }),
            },
        )
        .unwrap();
        assert_eq!(reply.status, 409);
        let body = json_of(&reply);
        assert_eq!(body["ok"], false);
        assert_eq!(body["message"], "2 de 3 con éxito");
        let results = body["results"].as_array().unwrap();
        assert_eq!(results.len(), 3, "the key after the failure still ran");
        assert_eq!(results[0]["ok"], true);
        assert_eq!(results[1]["ok"], false);
        assert!(
            results[1]["message"]
                .as_str()
                .unwrap()
                .starts_with("Error interno: disco lleno"),
            "{}",
            results[1]
        );
        assert_eq!(results[2]["message"], "hecho c");
    }
}
