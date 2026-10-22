//! The panel's HTTP layer as plain functions over a request value, so every guard is tested
//! without a socket (`server.rs` only moves bytes). The guards, in order: `Host` must be this
//! socket (against DNS rebinding); `/api/*` needs the per-launch token; a foreign `Origin` is
//! refused; a POST must be `application/json` (the one thing a hostile page cannot send without a
//! CORS preflight that this server never answers) and at most `body_limit` bytes. The page's own
//! files (`assets/`) are served under `/a/<launch-id>/` with the `Host` guard alone, like `/`.
//!
//! Every action goes through `actions.rs`, the same functions the commands call, so the page can
//! do whatever the command line can to an application, a task or the results log, and nothing
//! more: nothing here starts discovery, a browser or Claude.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use url::Url;

use super::Panel;
use super::jobs::{JobsQuery, build_jobs_page};
use super::privacy::UserPaths;
use super::snapshot::{build_job_detail, build_snapshot, offer_label};
use crate::actions::{self, Outcome};
use crate::dossier::dossier_path;
use crate::handoff::sheet_path;
use crate::inbox;
use crate::results::{read_last, results_path};
use crate::rules::load_criteria;

pub const MAX_BODY: usize = 16 * 1024;
/// `/api/inbox/import` carries a whole export of the mailbox: the importer's own file limit.
pub const MAX_IMPORT_BODY: usize = inbox::MAX_FILE_BYTES as usize;
pub const TOKEN_HEADER: &str = "x-panel-token";
const MAX_SCREENSHOT_BYTES: u64 = 8 * 1024 * 1024;
/// The most a CV, dossier or hand-over sheet served by `/api/file` may weigh.
const MAX_DOCUMENT_BYTES: u64 = 8 * 1024 * 1024;
/// Keys one batch request may carry; `MAX_BODY` would stop it anyway, this says so clearly.
const MAX_BATCH: usize = 200;
/// Lines `/api/results` serves at most.
const MAX_RESULTS: usize = 500;
const DEFAULT_RESULTS: usize = 20;
const PAGE: &str = include_str!("page.html");

/// What the binary serves besides the page: (name, MIME type, bytes), each embedded at build time.
/// The route compares a name against this table and nothing else, so the table is the whole of what
/// can be fetched; the MIME type is written here, never guessed from the name. Every row has a
/// line in `THIRD_PARTY_NOTICES.md` (rule in `assets/README.md`).
pub const ASSETS: &[(&str, &str, &[u8])] = &[
    // The four families of the page, subset and sanitised (`panel-src/assets/fonts/VENDORED.md`);
    // `panel-src/tools/assets-sync.mjs` copies them here after checking each sha256.
    (
        "barlow-condensed-700.woff2",
        "font/woff2",
        include_bytes!("assets/barlow-condensed-700.woff2"),
    ),
    (
        "geist-mono-400.woff2",
        "font/woff2",
        include_bytes!("assets/geist-mono-400.woff2"),
    ),
    (
        "instrument-serif-400italic.woff2",
        "font/woff2",
        include_bytes!("assets/instrument-serif-400italic.woff2"),
    ),
    (
        "inter-400.woff2",
        "font/woff2",
        include_bytes!("assets/inter-400.woff2"),
    ),
    (
        "inter-600.woff2",
        "font/woff2",
        include_bytes!("assets/inter-600.woff2"),
    ),
];

/// Where the page finds its assets this launch: the `__ASSETS__` placeholder becomes this.
pub fn asset_prefix(launch_id: &str) -> String {
    format!("/a/{launch_id}")
}

/// The most bytes a POST to `target` (path, with or without a query) may carry.
pub fn body_limit(target: &str) -> usize {
    let path = target.split('?').next().unwrap_or(target);
    if path == "/api/inbox/import" {
        MAX_IMPORT_BODY
    } else {
        MAX_BODY
    }
}

/// Every route of the API by method (the names after `/api/`). The guards in `route` apply to
/// exactly these, and a test walks the lists, so a new route is guarded and tested by being listed;
/// a name that is only in the `match` below answers 404.
const POST_ROUTES: [&str; 14] = [
    "approve",
    "resolve",
    "confirm",
    "mark",
    "snooze",
    "unsnooze",
    "record",
    "task/add",
    "task/done",
    "inbox/import",
    "stop",
    "resume",
    "settings",
    "learn/grade",
];
const GET_ROUTES: [&str; 8] = [
    "snapshot",
    "job",
    "jobs",
    "file",
    "shot",
    "results",
    "learn/next",
    "learn/answer",
];

/// Sent with every reply, whatever it is.
const COMMON_HEADERS: [(&str, &str); 4] = [
    ("Cache-Control", "no-store"),
    ("X-Content-Type-Options", "nosniff"),
    ("Referrer-Policy", "no-referrer"),
    ("Cross-Origin-Resource-Policy", "same-origin"),
];

pub struct Request {
    pub method: String,
    /// Path and query, exactly as received (`/api/job?key=a%2Fb`).
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    /// Besides `Content-Type`.
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    fn json(status: u16, value: Value) -> Reply {
        Reply {
            status,
            content_type: "application/json; charset=utf-8",
            headers: Vec::new(),
            body: value.to_string().into_bytes(),
        }
    }

    pub fn message(status: u16, text: &str) -> Reply {
        Reply::json(status, json!({ "ok": false, "message": text }))
    }

    fn empty(status: u16) -> Reply {
        Reply {
            status,
            content_type: "text/plain; charset=utf-8",
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// What a request has to match: the port the server really listens on and the token printed at
/// start. `launch_id` is the segment under which this launch serves its assets: random like the
/// token but not a secret, it only makes every start a new cache key, so an asset may be cached for
/// good.
///
/// Token model: 128 random bits per start (32 hex characters). It travels in the `/?t=` address the
/// CLI opens and in the `x-panel-token` header of every API call, and is compared in constant time.
/// Together with the exact Host (`127.0.0.1:<port>` or `localhost:<port>`) and Origin checks it stops
/// other web pages and DNS rebinding. It does not protect against another program running as the same
/// Windows user: the panel's threat model is a single-user machine.
pub struct Guard {
    pub port: u16,
    pub token: String,
    pub launch_id: String,
}

impl Guard {
    fn host_ok(&self, host: &str) -> bool {
        host.eq_ignore_ascii_case(&format!("127.0.0.1:{}", self.port))
            || host.eq_ignore_ascii_case(&format!("localhost:{}", self.port))
    }

    fn origin_ok(&self, origin: &str) -> bool {
        origin.eq_ignore_ascii_case(&format!("http://127.0.0.1:{}", self.port))
            || origin.eq_ignore_ascii_case(&format!("http://localhost:{}", self.port))
    }

    fn token_ok(&self, given: &str) -> bool {
        let (a, b) = (given.as_bytes(), self.token.as_bytes());
        // The token's length is not a secret; its bytes are, so compare them without stopping early.
        a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
    }
}

/// `bytes` random bytes as lowercase hex.
pub fn random_hex(bytes: usize) -> Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf)
        .map_err(|e| anyhow::anyhow!("no se pudo generar un número al azar: {e}"))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

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

/// Answers one request. Never panics: an error inside a handler is a 500 with a JSON message, and
/// the server goes on with the next request.
pub fn handle(panel: &Panel, guard: &Guard, req: &Request) -> Reply {
    let mut reply = match route(panel, guard, req) {
        Ok(reply) => reply,
        Err(err) => Reply::message(500, &format!("Error interno: {err:#}")),
    };
    // The one place where what leaves the panel is filtered: his folders and his own addresses.
    if reply.content_type.starts_with("application/json") {
        reply = redact_reply(&UserPaths::for_profile(&panel.paths.profile_dir), reply);
    }
    with_common_headers(reply)
}

/// `reply` with the headers every reply carries. A route may have set one of them itself (an
/// asset's long cache); the rest it gets here.
pub fn with_common_headers(mut reply: Reply) -> Reply {
    for (name, value) in COMMON_HEADERS {
        if reply.header(name).is_none() {
            reply.headers.push((name, value.to_string()));
        }
    }
    reply
}

/// The refusal a request earns from its head alone (Host, token, Origin, route, method, content
/// type), or `None` when it has to be read on. The server asks this before it reads a byte of the
/// body, so a request that is going to be refused anyway never holds the panel waiting for it.
/// `handle` applies the same checks again, so this is a shortcut and never the only guard.
pub fn refuse_early(panel: &Panel, guard: &Guard, req: &Request) -> Option<Reply> {
    let refusal = refuse_before_body(guard, req)?;
    Some(with_common_headers(redact_reply(
        &UserPaths::for_profile(&panel.paths.profile_dir),
        refusal,
    )))
}

/// The guards of `route` that need no body, in their order: Host first, then (for the API) the
/// token, the Origin, the route and its method and, for a POST, the JSON content type.
fn refuse_before_body(guard: &Guard, req: &Request) -> Option<Reply> {
    if !req.header("host").is_some_and(|host| guard.host_ok(host)) {
        return Some(Reply::message(403, "Host no permitido"));
    }
    let Ok(url) = Url::parse(&format!("http://panel.invalid{}", req.target)) else {
        return Some(Reply::message(400, "Dirección inválida"));
    };
    let method = req.method.as_str();
    match url.path() {
        "/" => {
            return (method != "GET").then(|| Reply::message(405, "Método no permitido"));
        }
        "/favicon.ico" => return None,
        _ => {}
    }
    if url.path().starts_with("/a/") {
        // The asset decides for itself (method, launch id, name).
        return None;
    }
    let Some(api) = url.path().strip_prefix("/api/") else {
        return Some(Reply::message(404, "No existe"));
    };
    if !req.header(TOKEN_HEADER).is_some_and(|t| guard.token_ok(t)) {
        let mut reply = Reply::message(401, "Falta el token del panel o no es válido");
        // RFC 9110: a 401 carries a challenge. The browser ignores an unknown scheme (no dialog).
        reply.headers.push((
            "WWW-Authenticate",
            "X-Panel-Token realm=\"jobbot-panel\"".to_string(),
        ));
        return Some(reply);
    }
    if req.header("origin").is_some_and(|o| !guard.origin_ok(o)) {
        return Some(Reply::message(403, "Origen no permitido"));
    }
    let wants_post = POST_ROUTES.contains(&api);
    let wants_get = GET_ROUTES.contains(&api);
    if !wants_post && !wants_get {
        return Some(Reply::message(404, "No existe"));
    }
    let expected_method = if wants_post { "POST" } else { "GET" };
    if method != expected_method {
        return Some(Reply::message(405, "Método no permitido"));
    }
    if wants_post {
        let json_type = req.header("content-type").is_some_and(|c| {
            c.trim()
                .to_ascii_lowercase()
                .starts_with("application/json")
        });
        if !json_type {
            return Some(Reply::message(415, "Se espera application/json"));
        }
    }
    None
}

/// A JSON reply with the folder of the Windows user and his own addresses taken out of every
/// string but the links (`UserPaths::redact_json`); any other reply is returned as it came. Every
/// route's JSON, an error message included, passes here, so no route has to remember to filter.
fn redact_reply(user: &UserPaths, mut reply: Reply) -> Reply {
    if !reply.content_type.starts_with("application/json") {
        return reply;
    }
    if let Ok(value) = serde_json::from_slice::<Value>(&reply.body) {
        reply.body = user.redact_json(value).to_string().into_bytes();
    }
    reply
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
                crate::outbound::clean(unknown, 40)
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
        Some(Value::Null) => actions::set_auto_approve_min(&panel.tracker, None, None)?,
        Some(Value::Number(number)) if number.is_i64() => {
            let threshold = load_criteria(&panel.paths.profile_dir.join("criteria.toml"))
                .ok()
                .map(|criteria| criteria.apply_threshold);
            actions::set_auto_approve_min(&panel.tracker, number.as_i64(), threshold)?
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
            "auto_approve_min": actions::auto_approve_min(&panel.tracker)?,
        }),
    ))
}

fn task_add(panel: &Panel, req: &Request) -> Result<Reply> {
    let body: TaskAddBody = match parse(req) {
        Ok(body) => body,
        Err(reply) => return Ok(reply),
    };
    let due_at = match body
        .due_at
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        None => None,
        Some(text) => match moment(text) {
            Some(at) => Some(at),
            None => {
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
        &panel.tracker,
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
    let at = match body.at.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        None => now,
        Some(text) => match moment(text) {
            Some(at) => at,
            None => {
                return refused(
                    "La fecha de envío (at) debe ser RFC 3339, por ejemplo 2026-10-05T05:21:42Z"
                        .to_string(),
                );
            }
        },
    };
    if at > now + TimeDelta::minutes(5) {
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
    Ok(outcome_reply(actions::record(&panel.tracker, &spec, at)?))
}

/// The tail of `data/results.jsonl`, newest last, as `jobbot results --last N` prints it.
fn results(panel: &Panel, last: Option<&str>) -> Result<Reply> {
    let last = match last {
        None => DEFAULT_RESULTS,
        Some(text) => match text.trim().parse::<usize>() {
            Ok(n) => n.clamp(1, MAX_RESULTS),
            Err(_) => return Ok(Reply::message(400, "last debe ser un número")),
        },
    };
    let found = read_last(&results_path(&panel.paths.data_dir), last)?;
    Ok(Reply::json(
        200,
        json!({ "total": found.total, "lines": found.lines }),
    ))
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
    let report = inbox::import(&panel.tracker, &items, panel.now())?;
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

fn route(panel: &Panel, guard: &Guard, req: &Request) -> Result<Reply> {
    if let Some(refusal) = refuse_before_body(guard, req) {
        return Ok(refusal);
    }
    let Ok(url) = Url::parse(&format!("http://panel.invalid{}", req.target)) else {
        return Ok(Reply::message(400, "Dirección inválida"));
    };
    let method = req.method.as_str();
    match url.path() {
        "/" => return page_reply(guard),
        "/favicon.ico" => return Ok(Reply::empty(204)),
        _ => {}
    }
    if let Some(rest) = url.path().strip_prefix("/a/") {
        return Ok(asset(guard, method, rest, url.query()));
    }
    // `refuse_before_body` let it through, so it is an API route with its method and token.
    let Some(api) = url.path().strip_prefix("/api/") else {
        return Ok(Reply::message(404, "No existe"));
    };
    if POST_ROUTES.contains(&api) && req.body.len() > body_limit(url.path()) {
        return Ok(Reply::message(413, "Cuerpo demasiado grande"));
    }
    let query: BTreeMap<String, String> = url.query_pairs().into_owned().collect();
    let need = |name: &str| query.get(name).map(String::as_str);
    match api {
        "snapshot" => Ok(Reply::json(
            200,
            serde_json::to_value(build_snapshot(panel)?)?,
        )),
        "job" => {
            let Some(key) = need("key") else {
                return Ok(Reply::message(400, "Falta key"));
            };
            Ok(match build_job_detail(panel, key)? {
                Some(detail) => Reply::json(200, serde_json::to_value(detail)?),
                None => Reply::message(404, "No existe esa postulación"),
            })
        }
        "jobs" => match JobsQuery::from_pairs(&query) {
            Ok(query) => Ok(Reply::json(
                200,
                serde_json::to_value(build_jobs_page(panel, &query)?)?,
            )),
            Err(message) => Ok(Reply::message(400, &message)),
        },
        "file" => {
            let Some(key) = need("key").filter(|key| !key.is_empty()) else {
                return Ok(Reply::message(400, "Falta key"));
            };
            let Some(kind) = need("kind").and_then(DocumentKind::parse) else {
                return Ok(Reply::message(400, "kind debe ser cv, dossier o handoff"));
            };
            document(panel, key, kind)
        }
        "shot" => {
            let Some(key) = need("key") else {
                return Ok(Reply::message(400, "Falta key"));
            };
            screenshot(panel, key)
        }
        "results" => results(panel, need("last")),
        "learn/next" => learn_next(panel, need("topic")),
        "learn/answer" => {
            let Some(id) = need("id") else {
                return Ok(Reply::message(400, "Falta id"));
            };
            learn_answer(panel, id)
        }
        "approve" => keyed(req, |key| actions::approve(&panel.tracker, key)),
        "resolve" => keyed(req, |key| {
            actions::resolve(&panel.tracker, key, panel.now())
        }),
        "confirm" => keyed(req, |key| {
            actions::confirm(&panel.tracker, key, panel.now())
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
                    &panel.tracker,
                    key,
                    &body.state,
                    body.note.as_deref(),
                    panel.now(),
                )
            })
        }
        "snooze" => {
            let body: SnoozeBody = match parse(req) {
                Ok(body) => body,
                Err(reply) => return Ok(reply),
            };
            Ok(outcome_reply(actions::snooze(
                &panel.tracker,
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
            Ok(outcome_reply(actions::unsnooze(&panel.tracker, &body.key)?))
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
            Ok(outcome_reply(actions::task_done(&panel.tracker, body.id)?))
        }
        "learn/grade" => {
            let body: GradeBody = match parse(req) {
                Ok(body) => body,
                Err(reply) => return Ok(reply),
            };
            let questions = panel.store.all_questions()?;
            Ok(outcome_reply(actions::grade_card(
                &panel.store,
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

/// The page, with a fresh nonce in its `<script>` and `<style>` tags and in the policy that allows
/// only those two. No `unsafe-inline`: text injected into the page cannot run. Fonts and images
/// may only come from this server (`'self'`: the assets under `/a/`), screenshots still arrive as
/// `blob:`; `require-trusted-types-for 'script'` makes a string handed to `innerHTML`, `eval` or a
/// script's `src` throw instead of run, should a library ever try. `X-Frame-Options` repeats
/// `frame-ancestors 'none'` for browsers that only know the old header.
fn page_reply(guard: &Guard) -> Result<Reply> {
    let nonce = random_hex(16)?;
    let policy = format!(
        "default-src 'none'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}'; \
         connect-src 'self'; img-src 'self' blob:; font-src 'self'; base-uri 'none'; \
         form-action 'none'; frame-ancestors 'none'; require-trusted-types-for 'script'"
    );
    Ok(Reply {
        status: 200,
        content_type: "text/html; charset=utf-8",
        headers: vec![
            ("Content-Security-Policy", policy),
            ("X-Frame-Options", "DENY".to_string()),
        ],
        body: fill_page(PAGE, &nonce, &guard.launch_id).into_bytes(),
    })
}

/// The page's two placeholders filled: `__NONCE__` with this load's nonce, `__ASSETS__` with this
/// launch's asset prefix (so a stylesheet's `url("__ASSETS__/inter-400.woff2")` points at `/a/<id>/inter-400.woff2`).
fn fill_page(template: &str, nonce: &str, launch_id: &str) -> String {
    template
        .replace("__NONCE__", nonce)
        .replace("__ASSETS__", &asset_prefix(launch_id))
}

/// `GET /a/<launch-id>/<name>`: one row of `ASSETS`, or a 404. `rest` is what follows `/a/`. The
/// launch id must be this launch's and the name must equal a row's, so neither `..` nor a sub-path
/// nor a query can reach anything else; the browser may keep the reply for a year because the next
/// launch serves the same file under another path.
fn asset(guard: &Guard, method: &str, rest: &str, query: Option<&str>) -> Reply {
    let found = rest
        .split_once('/')
        .filter(|(launch, _)| *launch == guard.launch_id && query.is_none())
        .and_then(|(_, name)| ASSETS.iter().find(|(known, _, _)| *known == name));
    let Some((_, mime, bytes)) = found else {
        return Reply::message(404, "No existe");
    };
    if method != "GET" {
        return Reply::message(405, "Método no permitido");
    }
    Reply {
        status: 200,
        content_type: mime,
        headers: vec![(
            "Cache-Control",
            "public, max-age=31536000, immutable".to_string(),
        )],
        body: bytes.to_vec(),
    }
}

fn learn_next(panel: &Panel, topic: Option<&str>) -> Result<Reply> {
    let questions = panel.store.all_questions()?;
    Ok(
        match panel.store.next_question(&questions, panel.now(), topic)? {
            Some(q) => {
                let offer = panel
                    .store
                    .gap_rows()?
                    .into_iter()
                    .find(|gap| gap.id == q.id)
                    .map(|gap| offer_label(panel, &gap.source));
                Reply::json(
                    200,
                    json!({ "card": { "id": q.id, "topic": q.topic, "question": q.question, "offer": offer } }),
                )
            }
            None => Reply::json(200, json!({ "card": null })),
        },
    )
}

fn learn_answer(panel: &Panel, id: &str) -> Result<Reply> {
    let questions = panel.store.all_questions()?;
    Ok(match questions.iter().find(|q| q.id == id) {
        Some(q) => Reply::json(200, json!({ "answer": q.answer })),
        None => Reply::message(404, "Pregunta desconocida"),
    })
}

/// The three documents an application can have on disk.
#[derive(Clone, Copy)]
enum DocumentKind {
    Cv,
    Dossier,
    Handoff,
}

impl DocumentKind {
    fn parse(text: &str) -> Option<DocumentKind> {
        match text {
            "cv" => Some(DocumentKind::Cv),
            "dossier" => Some(DocumentKind::Dossier),
            "handoff" => Some(DocumentKind::Handoff),
            _ => None,
        }
    }

    /// The folder under `data/` it lives in, the extension it must have, what it is served as
    /// and what is said when it is not there.
    fn rules(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            DocumentKind::Cv => ("cv", "pdf", "application/pdf", "Sin CV"),
            DocumentKind::Dossier => (
                "dossiers",
                "md",
                "text/markdown; charset=utf-8",
                "Sin dossier",
            ),
            DocumentKind::Handoff => (
                "handoff",
                "md",
                "text/markdown; charset=utf-8",
                "Sin hoja de traspaso",
            ),
        }
    }
}

/// The CV, dossier or hand-over sheet of an application, as the file it is. It is the same guard as
/// `screenshot`, because this is not a file server either: the request names an application and a
/// kind and never a path; the file is the one the engine keeps for that application (the CV from
/// the database, the others from the key), it must sit under its own folder of `data/` once both
/// are `canonicalize`d (so a `..` or a link cannot leave it), have the extension of its kind, be a
/// plain file and weigh at most `MAX_DOCUMENT_BYTES`. Anything else is a 404 that does not say
/// which of those failed.
fn document(panel: &Panel, key: &str, kind: DocumentKind) -> Result<Reply> {
    let (folder, extension, content_type, missing) = kind.rules();
    let none = || Ok(Reply::message(404, missing));
    let Some(row) = panel.tracker.get_application(key)? else {
        return Ok(Reply::message(404, "No existe esa postulación"));
    };
    let root = panel.paths.data_dir.join(folder);
    let stored = match kind {
        DocumentKind::Cv => row.cv_path.map(PathBuf::from),
        DocumentKind::Dossier => Some(dossier_path(&root, key)),
        DocumentKind::Handoff => Some(sheet_path(&root, key)),
    };
    let Some(stored) = stored else {
        return none();
    };
    let (Ok(root), Ok(file)) = (std::fs::canonicalize(&root), std::fs::canonicalize(&stored))
    else {
        return none();
    };
    if !file.starts_with(&root)
        || !file
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case(extension))
    {
        return none();
    }
    match std::fs::metadata(&file) {
        Ok(meta) if meta.is_file() && meta.len() <= MAX_DOCUMENT_BYTES => {}
        _ => return none(),
    }
    let name = file
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| {
            n.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        })
        .map_or_else(|| format!("{folder}.{extension}"), str::to_string);
    let mut headers = vec![(
        "Content-Disposition",
        format!("inline; filename=\"{name}\""),
    )];
    if content_type.starts_with("text/") {
        // Text shown by the browser must not be able to run or load anything.
        headers.push((
            "Content-Security-Policy",
            "default-src 'none'; sandbox".to_string(),
        ));
    }
    Ok(Reply {
        status: 200,
        content_type,
        headers,
        body: std::fs::read(&file)?,
    })
}

/// The dry-run screenshot of an application. Only files under `data/screenshots` are served: this
/// endpoint shows screenshots, it is not a file server, whatever path the database holds.
fn screenshot(panel: &Panel, key: &str) -> Result<Reply> {
    let none = || Ok(Reply::message(404, "Sin captura"));
    let Some(row) = panel.tracker.get_application(key)? else {
        return Ok(Reply::message(404, "No existe esa postulación"));
    };
    let Some(stored) = row.screenshot_path else {
        return none();
    };
    let Ok(root) = std::fs::canonicalize(panel.paths.data_dir.join("screenshots")) else {
        return none();
    };
    let Ok(file) = std::fs::canonicalize(&stored) else {
        return none();
    };
    if !file.starts_with(&root) {
        return none();
    }
    let content_type = match file
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        _ => return none(),
    };
    if std::fs::metadata(&file)?.len() > MAX_SCREENSHOT_BYTES {
        return none();
    }
    Ok(Reply {
        status: 200,
        content_type,
        headers: Vec::new(),
        body: std::fs::read(&file)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::fixtures::{add, panel};
    use crate::tracker::State;

    const PORT: u16 = 4321;
    const TOKEN: &str = "tok-0123456789abcdef";
    const LAUNCH: &str = "f00ba4f00ba4f00b";

    fn guard() -> Guard {
        Guard {
            port: PORT,
            token: TOKEN.to_string(),
            launch_id: LAUNCH.to_string(),
        }
    }

    /// A well-formed request: right Host, right token, JSON content type.
    fn request(method: &str, target: &str, body: &str) -> Request {
        Request {
            method: method.to_string(),
            target: target.to_string(),
            headers: vec![
                ("Host".into(), format!("127.0.0.1:{PORT}")),
                ("X-Panel-Token".into(), TOKEN.into()),
                ("Content-Type".into(), "application/json".into()),
            ],
            body: body.as_bytes().to_vec(),
        }
    }

    fn without(mut req: Request, name: &str) -> Request {
        req.headers.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
        req
    }

    fn with(req: Request, name: &str, value: &str) -> Request {
        let mut req = without(req, name);
        req.headers.push((name.to_string(), value.to_string()));
        req
    }

    fn text(reply: &Reply) -> String {
        String::from_utf8(reply.body.clone()).unwrap()
    }

    fn json_of(reply: &Reply) -> Value {
        serde_json::from_slice(&reply.body).unwrap()
    }

    fn approved(p: &Panel, key: &str) -> bool {
        p.tracker.get_application(key).unwrap().unwrap().approved
    }

    fn encode(key: &str) -> String {
        url::form_urlencoded::byte_serialize(key.as_bytes()).collect()
    }

    #[test]
    fn the_explorer_pages_filters_and_refuses_what_it_does_not_understand() {
        let (p, _d) = panel();
        for n in 0..3u32 {
            add(&p, &n.to_string(), 70.0 + f64::from(n), &[State::Prepared]);
        }
        add(&p, "9", 99.0, &[State::Prepared, State::NeedsGonza]);
        let before = p.tracker.counts().unwrap();
        let get = |target: &str| handle(&p, &guard(), &request("GET", target, ""));

        let all = get("/api/jobs");
        assert_eq!(all.status, 200);
        let page = json_of(&all);
        assert_eq!(
            (
                &page["page"],
                &page["per_page"],
                &page["total"],
                &page["pages"]
            ),
            (&json!(1), &json!(50), &json!(4), &json!(1))
        );
        assert_eq!(page["cards"].as_array().unwrap().len(), 4);
        assert_eq!(page["cards"][0]["company"], "Co 9", "best score first");
        assert!(page["cards"][0]["actions"].is_array(), "a whole card");

        let some = json_of(&get("/api/jobs?state=prepared&min_score=71&q=co&page=1"));
        assert_eq!(some["total"], 2);
        assert_eq!(json_of(&get("/api/jobs?page=2"))["cards"], json!([]));

        for bad in [
            "/api/jobs?state=nope",
            "/api/jobs?min_score=500",
            "/api/jobs?page=0",
            "/api/jobs?pagina=2",
        ] {
            let reply = get(bad);
            assert_eq!(reply.status, 400, "{bad}");
            assert_eq!(json_of(&reply)["ok"], false, "{bad}");
        }
        assert_eq!(p.tracker.counts().unwrap(), before, "it only reads");
    }

    #[test]
    fn what_the_head_alone_can_refuse_is_refused_before_the_body_is_read() {
        let (p, _d) = panel();
        let g = guard();
        let post = |target: &str| request("POST", target, "");
        let cases: Vec<(Request, u16)> = vec![
            (with(post("/api/approve"), "host", "evil.example:80"), 403),
            (without(post("/api/approve"), "x-panel-token"), 401),
            (with(post("/api/approve"), "x-panel-token", "nope"), 401),
            (
                with(post("/api/approve"), "origin", "https://evil.example"),
                403,
            ),
            (post("/api/no-such-route"), 404),
            (request("GET", "/api/approve", ""), 405),
            (post("/api/snapshot"), 405),
            (
                with(post("/api/approve"), "content-type", "text/plain"),
                415,
            ),
            (without(post("/api/approve"), "content-type"), 415),
            (post("/"), 405),
            (post("/elsewhere"), 404),
        ];
        for (req, want) in cases {
            let early = refuse_early(&p, &g, &req).unwrap_or_else(|| {
                panic!("{} {} is refused from its head", req.method, req.target)
            });
            assert_eq!(early.status, want, "{} {}", req.method, req.target);
            // The same answer `handle` gives, with the same headers on it.
            assert_eq!(handle(&p, &g, &req).status, want, "{}", req.target);
            assert_eq!(early.header("cache-control"), Some("no-store"));
            assert_eq!(early.header("x-content-type-options"), Some("nosniff"));
        }
        // What a good head carries is not refused here, whatever the body turns out to be.
        for api in POST_ROUTES {
            let req = post(&format!("/api/{api}"));
            assert!(refuse_early(&p, &g, &req).is_none(), "{api}");
        }
        for api in GET_ROUTES {
            let req = request("GET", &format!("/api/{api}"), "");
            assert!(refuse_early(&p, &g, &req).is_none(), "{api}");
        }
        assert!(refuse_early(&p, &g, &request("GET", "/", "")).is_none());
        let asset = request("GET", &format!("/a/{LAUNCH}/inter-400.woff2"), "");
        assert!(refuse_early(&p, &g, &asset).is_none());
    }

    #[test]
    fn the_page_needs_the_right_host_but_no_token_and_gets_a_fresh_nonce() {
        let (p, _d) = panel();
        let req = without(request("GET", "/", ""), "x-panel-token");
        let page = handle(&p, &guard(), &req);
        assert_eq!(page.status, 200);
        assert!(page.content_type.starts_with("text/html"));
        let html = text(&page);
        assert!(!html.contains("__NONCE__"));
        let csp = page.header("content-security-policy").unwrap().to_string();
        let nonce = csp
            .split("'nonce-")
            .nth(1)
            .unwrap()
            .split('\'')
            .next()
            .unwrap()
            .to_string();
        assert!(
            html.contains(&format!("<script nonce=\"{nonce}\"")),
            "{html}"
        );
        assert!(csp.contains("default-src 'none'") && csp.contains("connect-src 'self'"));
        assert!(
            !csp.contains("unsafe-inline") && !csp.contains("unsafe-eval"),
            "{csp}"
        );
        assert!(!html.contains(TOKEN), "the token is never inside the page");
        let again = handle(&p, &guard(), &req);
        assert_ne!(
            again.header("content-security-policy"),
            page.header("content-security-policy"),
            "every load gets its own nonce"
        );
    }

    #[test]
    fn a_foreign_host_is_refused_everywhere() {
        let (p, _d) = panel();
        for host in [
            "evil.example:4321",
            "127.0.0.1:9999",
            "localhost.evil.com:4321",
            "127.0.0.1",
            "127.0.0.1:4321.evil.com",
            "",
            "[::1]:4321",
            "evil.localhost:4321",
            "localhost.:4321",
            "127.0.0.1:04321",
            "0x7f.0.0.1:4321",
            "127.1:4321",
            "2130706433:4321",
            "localhost:4321@evil.example",
        ] {
            for target in ["/", "/api/snapshot"] {
                let reply = handle(
                    &p,
                    &guard(),
                    &with(request("GET", target, ""), "host", host),
                );
                assert_eq!(reply.status, 403, "{host:?} {target}");
            }
        }
        let no_host = handle(&p, &guard(), &without(request("GET", "/", ""), "host"));
        assert_eq!(no_host.status, 403);
        let localhost = handle(
            &p,
            &guard(),
            &with(
                request("GET", "/api/snapshot", ""),
                "host",
                "localhost:4321",
            ),
        );
        assert_eq!(
            localhost.status, 200,
            "localhost is a name for the same socket"
        );
    }

    #[test]
    fn a_401_by_token_names_the_token_it_wants() {
        let (p, _d) = panel();
        let req = without(request("GET", "/api/snapshot", ""), TOKEN_HEADER);
        let reply = handle(&p, &guard(), &req);
        assert_eq!(reply.status, 401);
        assert_eq!(
            reply.header("www-authenticate"),
            Some("X-Panel-Token realm=\"jobbot-panel\""),
            "RFC 9110: a 401 carries a challenge"
        );
        let early = refuse_early(&p, &guard(), &req).expect("refused from the head alone");
        assert_eq!(early.status, 401);
        assert!(early.header("www-authenticate").is_some());
    }

    #[test]
    fn the_api_needs_the_token_and_a_refused_post_changes_nothing() {
        let (p, _d) = panel();
        let key = add(&p, "1", 80.0, &[State::Prepared]);
        let body = format!(r#"{{"key":"{key}"}}"#);
        for token in [
            None,
            Some("wrong"),
            Some("tok-0123456789abcde"),
            Some("tok-0123456789abcdeX"),
            Some(""),
        ] {
            let mut req = without(request("POST", "/api/approve", &body), TOKEN_HEADER);
            if let Some(token) = token {
                req = with(req, TOKEN_HEADER, token);
            }
            assert_eq!(handle(&p, &guard(), &req).status, 401, "{token:?}");
        }
        assert!(!approved(&p, &key));
        let ok = handle(&p, &guard(), &request("POST", "/api/approve", &body));
        assert_eq!(ok.status, 200);
        assert!(approved(&p, &key));
    }

    #[test]
    fn every_api_route_goes_through_the_same_guards_before_it_does_anything() {
        let (p, _d) = panel();
        let key = add(&p, "1", 80.0, &[State::Prepared]);
        let cases = GET_ROUTES
            .iter()
            .map(|route| ("GET", *route))
            .chain(POST_ROUTES.iter().map(|route| ("POST", *route)));
        for (method, route) in cases {
            let target = format!("/api/{route}");
            let body = format!(r#"{{"key":"{key}"}}"#);
            let base = || request(method, &target, &body);
            let status = |req: Request| handle(&p, &guard(), &req).status;
            let who = format!("{method} {target}");
            assert_eq!(
                status(without(base(), TOKEN_HEADER)),
                401,
                "{who}: no token"
            );
            assert_eq!(
                status(with(base(), TOKEN_HEADER, "wrong")),
                401,
                "{who}: wrong token"
            );
            assert_eq!(
                status(with(base(), "host", "evil.example:4321")),
                403,
                "{who}: foreign host"
            );
            assert_eq!(
                status(with(base(), "origin", "https://evil.example")),
                403,
                "{who}: foreign origin"
            );
            let other = if method == "GET" { "POST" } else { "GET" };
            assert_eq!(
                status(request(other, &target, &body)),
                405,
                "{who}: wrong verb"
            );
            if method == "POST" {
                assert_eq!(
                    status(with(base(), "content-type", "text/plain")),
                    415,
                    "{who}: not JSON"
                );
                let big = "x".repeat(body_limit(&target) + 1);
                assert_eq!(
                    status(request("POST", &target, &big)),
                    413,
                    "{who}: over its limit"
                );
            }
        }
        assert!(!approved(&p, &key), "no refused request did anything");
        // A route that is not in the lists is not there, whatever the `match` below says.
        assert_eq!(
            handle(&p, &guard(), &request("POST", "/api/not-a-route", "{}")).status,
            404
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

    #[test]
    fn a_post_must_be_json_so_a_hostile_form_cannot_send_one() {
        let (p, _d) = panel();
        let key = add(&p, "1", 80.0, &[State::Prepared]);
        let body = format!(r#"{{"key":"{key}"}}"#);
        for content_type in [
            Some("text/plain"),
            Some("application/x-www-form-urlencoded"),
            Some("multipart/form-data; boundary=x"),
            None,
        ] {
            let mut req = without(request("POST", "/api/approve", &body), "content-type");
            if let Some(ct) = content_type {
                req = with(req, "content-type", ct);
            }
            assert_eq!(handle(&p, &guard(), &req).status, 415, "{content_type:?}");
        }
        assert!(!approved(&p, &key));
        let charset = with(
            request("POST", "/api/approve", &body),
            "content-type",
            "Application/JSON; charset=utf-8",
        );
        assert_eq!(handle(&p, &guard(), &charset).status, 200);
    }

    #[test]
    fn a_foreign_origin_is_refused_even_with_the_token() {
        let (p, _d) = panel();
        let key = add(&p, "1", 80.0, &[State::Prepared]);
        let body = format!(r#"{{"key":"{key}"}}"#);
        for origin in [
            "https://evil.example",
            "null",
            "http://127.0.0.1:9999",
            "http://localhost:4321.evil.com",
        ] {
            let post = with(request("POST", "/api/approve", &body), "origin", origin);
            assert_eq!(handle(&p, &guard(), &post).status, 403, "{origin}");
            let get = with(request("GET", "/api/snapshot", ""), "origin", origin);
            assert_eq!(handle(&p, &guard(), &get).status, 403, "{origin}");
        }
        assert!(!approved(&p, &key));
        for origin in ["http://127.0.0.1:4321", "http://localhost:4321"] {
            let post = with(request("POST", "/api/approve", &body), "origin", origin);
            assert_eq!(handle(&p, &guard(), &post).status, 200, "{origin}");
        }
    }

    #[test]
    fn an_oversized_body_is_refused() {
        let (p, _d) = panel();
        let big = format!(r#"{{"key":"{}"}}"#, "a".repeat(MAX_BODY));
        assert_eq!(
            handle(&p, &guard(), &request("POST", "/api/approve", &big)).status,
            413
        );
    }

    #[test]
    fn awkward_keys_survive_queries_and_bodies() {
        let (p, _d) = panel();
        for id in ["acme/42", "a b&c+d%e", "ñandú", "q?x=1#frag", "x\"y\\z"] {
            let key = add(&p, id, 80.0, &[State::Prepared]);
            let get = handle(
                &p,
                &guard(),
                &request("GET", &format!("/api/job?key={}", encode(&key)), ""),
            );
            assert_eq!(get.status, 200, "{key}");
            assert_eq!(json_of(&get)["card"]["key"], key.as_str());
            let body = serde_json::json!({ "key": key }).to_string();
            let post = handle(&p, &guard(), &request("POST", "/api/approve", &body));
            assert_eq!(post.status, 200, "{key}: {}", text(&post));
            assert!(approved(&p, &key), "{key}");
        }
        let unknown = handle(&p, &guard(), &request("GET", "/api/job?key=nope%3A1", ""));
        assert_eq!(unknown.status, 404);
        assert_eq!(
            handle(&p, &guard(), &request("GET", "/api/job", "")).status,
            400
        );
        let refused = handle(
            &p,
            &guard(),
            &request("POST", "/api/approve", r#"{"key":"nope:1"}"#),
        );
        assert_eq!(refused.status, 409);
        assert_eq!(json_of(&refused)["message"], "No existe: nope:1");
    }

    #[test]
    fn refusals_are_409_with_a_spanish_message_and_replays_are_safe() {
        let (p, _d) = panel();
        let key = add(&p, "1", 80.0, &[State::Prepared, State::NeedsGonza]);
        let body = serde_json::json!({ "key": key }).to_string();
        let first = handle(&p, &guard(), &request("POST", "/api/confirm", &body));
        assert_eq!(first.status, 200);
        let again = handle(&p, &guard(), &request("POST", "/api/confirm", &body));
        assert_eq!(again.status, 409);
        assert!(
            json_of(&again)["message"]
                .as_str()
                .unwrap()
                .starts_with("No se pudo confirmar")
        );
        for _ in 0..2 {
            assert_eq!(
                handle(&p, &guard(), &request("POST", "/api/approve", &body)).status,
                200
            );
        }
        let mark = |state: &str| {
            let b = serde_json::json!({ "key": key, "state": state, "note": "martes 19:00" })
                .to_string();
            handle(&p, &guard(), &request("POST", "/api/mark", &b))
        };
        assert_eq!(mark("interview").status, 200);
        let bad = mark("submitted");
        assert_eq!(bad.status, 409);
        assert!(
            json_of(&bad)["message"]
                .as_str()
                .unwrap()
                .starts_with("Estado no válido")
        );
        assert_eq!(
            p.tracker.get_application(&key).unwrap().unwrap().state,
            State::Interview
        );
        for body in [
            "not json",
            "{}",
            r#"{"key":1}"#,
            r#"{"key":"x","state":null}"#,
        ] {
            let reply = handle(&p, &guard(), &request("POST", "/api/mark", body));
            assert_eq!(reply.status, 400, "{body}");
            assert_eq!(json_of(&reply)["message"], "JSON inválido");
        }
    }

    #[test]
    fn unknown_routes_and_wrong_methods_are_404_and_405() {
        let (p, _d) = panel();
        let status = |m: &str, t: &str| handle(&p, &guard(), &request(m, t, "{}")).status;
        assert_eq!(status("GET", "/api/nope"), 404);
        assert_eq!(status("GET", "/nope"), 404);
        assert_eq!(status("GET", "/api/approve"), 405);
        assert_eq!(status("POST", "/api/snapshot"), 405);
        assert_eq!(status("POST", "/"), 405);
        assert_eq!(status("DELETE", "/api/snapshot"), 405);
        assert_eq!(status("GET", "/favicon.ico"), 204);
    }

    #[test]
    fn stop_and_resume_toggle_the_file_and_the_snapshot_shows_it() {
        let (p, _d) = panel();
        let stopped = |p: &Panel| {
            json_of(&handle(p, &guard(), &request("GET", "/api/snapshot", "")))["controls"]
                ["stopped"]
                .as_bool()
                .unwrap()
        };
        assert!(!stopped(&p));
        assert_eq!(
            handle(&p, &guard(), &request("POST", "/api/stop", "{}")).status,
            200
        );
        assert!(p.paths.stop_file.exists() && stopped(&p));
        assert_eq!(
            handle(&p, &guard(), &request("POST", "/api/resume", "{}")).status,
            200
        );
        assert!(!p.paths.stop_file.exists() && !stopped(&p));
    }

    #[test]
    fn the_snapshot_carries_today_and_unknown_task_routes_are_404() {
        let (p, _d) = panel();
        p.tracker
            .add_task(
                &crate::tracker::NewTask {
                    job_key: None,
                    kind: "action_required".into(),
                    text: "Responder".into(),
                    due_at: None,
                    source_message_id: None,
                    link: None,
                },
                *crate::testing::NOW,
            )
            .unwrap();
        let snapshot = json_of(&handle(&p, &guard(), &request("GET", "/api/snapshot", "")));
        assert_eq!(snapshot["today"]["tasks"][0]["text"], "Responder");
        assert_eq!(snapshot["today"]["emails_total"], 0);

        for target in ["/api/task", "/api/today", "/api/tasks/add"] {
            let reply = handle(
                &p,
                &guard(),
                &request("POST", target, r#"{"id":1,"key":"x"}"#),
            );
            assert_eq!(reply.status, 404, "{target}");
        }
        assert_eq!(p.tracker.open_tasks().unwrap().len(), 1);
    }

    #[test]
    fn a_task_can_be_added_from_the_page_with_its_date_posting_and_link() {
        let (p, _d) = panel();
        let key = add(&p, "1", 80.0, &[State::NeedsGonza]);
        let post = |body: Value| {
            handle(
                &p,
                &guard(),
                &request("POST", "/api/task/add", &body.to_string()),
            )
        };

        let ok = post(serde_json::json!({
            "text": "Postular a mano",
            "due_at": "2026-10-02T15:00:00Z",
            "job_key": key,
            "link": "https://example.com/apply",
        }));
        assert_eq!(ok.status, 200, "{}", text(&ok));
        assert!(
            json_of(&ok)["message"]
                .as_str()
                .unwrap()
                .starts_with("Tarea agregada: #")
        );
        let snapshot = json_of(&handle(&p, &guard(), &request("GET", "/api/snapshot", "")));
        let task = &snapshot["today"]["tasks"][0];
        assert_eq!(task["text"], "Postular a mano");
        assert_eq!(task["due_at"], "2026-10-02T15:00:00Z");
        assert_eq!(task["job_key"], key.as_str());
        assert_eq!(task["link"], "https://example.com/apply");
        assert_eq!(task["kind"], "manual");

        // Without a date, a posting or a link is fine; the same text twice is not.
        assert_eq!(post(serde_json::json!({ "text": "Llamar" })).status, 200);
        assert_eq!(
            post(serde_json::json!({ "text": "Llamar", "due_at": "" })).status,
            409
        );

        // The refusals of `task add`, plus a date the page got wrong.
        for (body, starts) in [
            (serde_json::json!({ "text": "   " }), "La tarea está vacía"),
            (
                serde_json::json!({ "text": "x", "due_at": "mañana" }),
                "La fecha límite debe ser RFC 3339",
            ),
            (
                serde_json::json!({ "text": "x", "due_at": "1999-01-01T00:00:00Z" }),
                "La fecha límite está fuera de rango",
            ),
            (
                serde_json::json!({ "text": "x", "job_key": "nope:1" }),
                "No existe el aviso",
            ),
            (
                serde_json::json!({ "text": "x", "link": "javascript:alert(1)" }),
                "El enlace no sirve",
            ),
        ] {
            let reply = post(body.clone());
            assert_eq!(reply.status, 409, "{body}");
            assert!(
                json_of(&reply)["message"]
                    .as_str()
                    .unwrap()
                    .starts_with(starts),
                "{body}: {}",
                text(&reply)
            );
        }
        assert_eq!(
            post(serde_json::json!({ "due_at": "x" })).status,
            400,
            "no text at all"
        );
        assert_eq!(
            handle(&p, &guard(), &request("GET", "/api/task/add", "")).status,
            405
        );
        assert_eq!(p.tracker.open_tasks().unwrap().len(), 2);
    }

    #[test]
    fn an_application_sent_by_hand_can_be_recorded_from_the_page() {
        let (p, _d) = panel();
        let post = |body: Value| {
            handle(
                &p,
                &guard(),
                &request("POST", "/api/record", &body.to_string()),
            )
        };

        let ok = post(serde_json::json!({
            "company": "Acme",
            "title": "Backend Engineer",
            "url": "https://jobs.example.com/acme/42?ref=x",
            "at": "2026-10-01T09:00:00Z",
            "note": "desde el sitio",
            "location": "Remote",
            "salary_min": 5000,
            "salary_max": 7000,
        }));
        assert_eq!(ok.status, 200, "{}", text(&ok));
        let message = json_of(&ok)["message"].as_str().unwrap().to_string();
        assert!(
            message.starts_with("Registrada como enviada: Acme — Backend Engineer (manual:"),
            "{message}"
        );
        let key = message
            .rsplit('(')
            .next()
            .unwrap()
            .trim_end_matches(')')
            .to_string();
        let row = p.tracker.get_application(&key).unwrap().unwrap();
        assert_eq!(row.state, State::Submitted);
        assert_eq!(
            row.submitted_at.map(|at| at.to_rfc3339()),
            Some("2026-10-01T09:00:00+00:00".to_string())
        );
        assert_eq!(row.note.as_deref(), Some("desde el sitio"));
        assert_eq!(
            (row.salary_min, row.salary_max),
            (Some(5000.0), Some(7000.0))
        );
        assert!(!row.approved, "a form the bot never filled is no review");

        // A stored posting is recorded by its key, at "now" when no `at` is given.
        let stored = add(&p, "1", 80.0, &[State::Prepared]);
        let by_key = post(serde_json::json!({ "key": stored }));
        assert_eq!(by_key.status, 200, "{}", text(&by_key));
        let row = p.tracker.get_application(&stored).unwrap().unwrap();
        assert_eq!(row.submitted_at, Some(*crate::testing::NOW));

        for (body, starts) in [
            (serde_json::json!({ "company": "Acme" }), "Faltan datos"),
            (serde_json::json!({ "key": "nope:1" }), "No existe"),
            (
                serde_json::json!({ "key": stored }),
                "No se registró: ya está registrada",
            ),
            (
                serde_json::json!({ "key": stored, "at": "ayer" }),
                "La fecha de envío (at) debe ser RFC 3339",
            ),
            (
                serde_json::json!({ "key": stored, "at": "2026-10-01T13:00:00Z" }),
                "La fecha de envío está en el futuro",
            ),
        ] {
            let reply = post(body.clone());
            assert_eq!(reply.status, 409, "{body}: {}", text(&reply));
            assert!(
                json_of(&reply)["message"]
                    .as_str()
                    .unwrap()
                    .starts_with(starts),
                "{body}: {}",
                text(&reply)
            );
        }
        assert_eq!(
            post(serde_json::json!({ "salary_min": "mucho" })).status,
            400
        );
        assert_eq!(
            handle(&p, &guard(), &request("GET", "/api/record", "")).status,
            405
        );
    }

    #[test]
    fn approve_resolve_confirm_and_mark_take_a_batch_of_keys() {
        let (p, _d) = panel();
        let a = add(&p, "a", 80.0, &[State::Prepared]);
        let b = add(&p, "b", 70.0, &[State::Prepared]);
        let post = |route: &str, body: Value| {
            handle(&p, &guard(), &request("POST", route, &body.to_string()))
        };

        // One bad key: 409, but the good ones went through and the reply says which.
        let mixed = post(
            "/api/approve",
            serde_json::json!({ "keys": [a, "nope:1", b] }),
        );
        assert_eq!(mixed.status, 409);
        let body = json_of(&mixed);
        assert_eq!(body["ok"], false);
        assert_eq!(body["message"], "2 de 3 con éxito");
        let results = body["results"].as_array().unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(
            (results[0]["key"].as_str(), results[0]["ok"].as_bool()),
            (Some(a.as_str()), Some(true))
        );
        assert_eq!(
            (results[1]["key"].as_str(), results[1]["ok"].as_bool()),
            (Some("nope:1"), Some(false))
        );
        assert_eq!(results[1]["message"], "No existe: nope:1");
        assert!(
            results[2]["message"]
                .as_str()
                .unwrap()
                .starts_with("Aprobada")
        );
        assert!(approved(&p, &a) && approved(&p, &b));

        // All good: 200, still one result per key, in order.
        let all = post("/api/approve", serde_json::json!({ "keys": [b, a] }));
        assert_eq!(all.status, 200);
        assert_eq!(json_of(&all)["message"], "2 de 2 con éxito");
        assert_eq!(json_of(&all)["results"][0]["key"], b.as_str());

        // The single form keeps its shape.
        let one = json_of(&post("/api/approve", serde_json::json!({ "key": a })));
        assert_eq!(one["ok"], true);
        assert!(one.get("results").is_none());

        // Malformed batches change nothing.
        for body in [
            serde_json::json!({ "keys": [] }),
            serde_json::json!({ "key": a, "keys": [b] }),
            serde_json::json!({}),
            serde_json::json!({ "keys": vec!["x"; MAX_BATCH + 1] }),
        ] {
            assert_eq!(post("/api/confirm", body.clone()).status, 400, "{body}");
        }

        // resolve, confirm and mark, in batch too.
        let c = add(&p, "c", 60.0, &[State::Prepared, State::NeedsGonza]);
        let d = add(&p, "d", 60.0, &[State::Prepared, State::NeedsGonza]);
        let resolved = post("/api/resolve", serde_json::json!({ "keys": [c, d] }));
        assert_eq!(resolved.status, 200, "{}", text(&resolved));
        let e = add(&p, "e", 60.0, &[State::Prepared, State::NeedsGonza]);
        let confirmed = post("/api/confirm", serde_json::json!({ "keys": [e] }));
        assert_eq!(confirmed.status, 200, "{}", text(&confirmed));
        let state = |key: &str| p.tracker.get_application(key).unwrap().unwrap().state;
        assert_eq!(
            (state(&c), state(&d), state(&e)),
            (State::Scored, State::Scored, State::Submitted)
        );

        let sent_a = add(&p, "f", 60.0, &[State::Prepared, State::Submitted]);
        let sent_b = add(&p, "g", 60.0, &[State::Prepared, State::Submitted]);
        let marked = post(
            "/api/mark",
            serde_json::json!({ "keys": [sent_a, sent_b, a], "state": "rejected", "note": "sin plata" }),
        );
        assert_eq!(marked.status, 409, "{}", text(&marked));
        let body = json_of(&marked);
        assert_eq!(body["message"], "2 de 3 con éxito");
        assert!(
            body["results"][2]["message"]
                .as_str()
                .unwrap()
                .starts_with("No se pudo marcar")
        );
        assert_eq!(
            (state(&sent_a), state(&sent_b)),
            (State::Rejected, State::Rejected)
        );
        assert_eq!(
            p.tracker
                .get_application(&sent_a)
                .unwrap()
                .unwrap()
                .note
                .as_deref(),
            Some("sin plata")
        );
        assert_eq!(
            post("/api/mark", serde_json::json!({ "state": "rejected" })).status,
            400
        );
    }

    #[test]
    fn snooze_takes_a_card_out_of_the_queues_and_unsnooze_brings_it_back() {
        let (p, _d) = panel();
        let key = add(&p, "1", 80.0, &[State::NeedsGonza]);
        let snapshot = || json_of(&handle(&p, &guard(), &request("GET", "/api/snapshot", "")));
        let post = |route: &str, body: Value| {
            handle(&p, &guard(), &request("POST", route, &body.to_string()))
        };
        assert_eq!(snapshot()["needs_gonza"]["total"], 1);

        let ok = post(
            "/api/snooze",
            serde_json::json!({ "key": key, "until": "3d" }),
        );
        assert_eq!(ok.status, 200, "{}", text(&ok));
        assert_eq!(
            json_of(&ok)["message"],
            format!("Pospuesta hasta 2026-10-04 09:00: {key}")
        );
        let snap = snapshot();
        assert_eq!(snap["needs_gonza"]["total"], 0);
        assert_eq!(snap["snoozed"]["total"], 1);
        assert_eq!(snap["snoozed"]["cards"][0]["key"], key.as_str());
        assert_eq!(
            snap["snoozed"]["cards"][0]["snoozed_until"],
            "2026-10-04T12:00:00Z"
        );

        let later = post(
            "/api/snooze",
            serde_json::json!({ "key": key, "until": "2026-10-20T12:00:00Z" }),
        );
        assert_eq!(later.status, 200, "a snooze can be moved");
        assert_eq!(
            snapshot()["snoozed"]["cards"][0]["snoozed_until"],
            "2026-10-20T12:00:00Z"
        );

        let back = post("/api/unsnooze", serde_json::json!({ "key": key }));
        assert_eq!(back.status, 200, "{}", text(&back));
        assert_eq!(snapshot()["needs_gonza"]["total"], 1);
        assert_eq!(
            post("/api/unsnooze", serde_json::json!({ "key": key })).status,
            409
        );

        for (body, starts) in [
            (
                serde_json::json!({ "key": key, "until": "ayer" }),
                "No entiendo hasta cuándo",
            ),
            (
                serde_json::json!({ "key": key, "until": "2026-09-01T00:00:00Z" }),
                "La fecha ya pasó",
            ),
            (
                serde_json::json!({ "key": "nope:1", "until": "1d" }),
                "No existe",
            ),
        ] {
            let reply = post("/api/snooze", body.clone());
            assert_eq!(reply.status, 409, "{body}");
            assert!(
                json_of(&reply)["message"]
                    .as_str()
                    .unwrap()
                    .starts_with(starts),
                "{}",
                text(&reply)
            );
        }
        assert_eq!(
            post("/api/snooze", serde_json::json!({ "key": key })).status,
            400
        );
        assert_eq!(
            snapshot()["needs_gonza"]["total"],
            1,
            "a refusal changes nothing"
        );
    }

    #[test]
    fn results_serves_the_tail_of_the_log() {
        let (p, _d) = panel();
        let get = |query: &str| {
            handle(
                &p,
                &guard(),
                &request("GET", &format!("/api/results{query}"), ""),
            )
        };

        let empty = json_of(&get(""));
        assert_eq!(
            (
                empty["total"].as_u64(),
                empty["lines"].as_array().map(Vec::len)
            ),
            (Some(0), Some(0))
        );

        let path = results_path(&p.paths.data_dir);
        for n in 1..=3 {
            let job = crate::testing::make_job_with(|j| j.external_id = n.to_string());
            let line = crate::results::ResultLine::new(
                &job,
                crate::results::AttemptMode::DryRun,
                State::Prepared,
                crate::handoff::FormTally {
                    filled: Some(4),
                    left: vec!["Pronombres".into()],
                },
                None,
                None,
                *crate::testing::NOW + TimeDelta::minutes(n),
            );
            crate::results::append_result(&path, &line).unwrap();
        }
        let two = json_of(&get("?last=2"));
        assert_eq!(two["total"], 3);
        let keys: Vec<&str> = two["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["key"].as_str().unwrap())
            .collect();
        assert_eq!(keys, ["test:2", "test:3"], "the newest two, oldest first");
        assert_eq!(two["lines"][0]["mode"], "dry_run");
        assert_eq!(two["lines"][0]["fields_left"][0], "Pronombres");
        assert_eq!(
            json_of(&get(""))["lines"].as_array().unwrap().len(),
            3,
            "20 by default"
        );
        assert_eq!(
            json_of(&get("?last=0"))["lines"].as_array().unwrap().len(),
            1,
            "held to 1..=500"
        );
        assert_eq!(get("?last=muchos").status, 400);
        assert_eq!(
            handle(&p, &guard(), &request("POST", "/api/results", "{}")).status,
            405
        );
        let no_token = without(request("GET", "/api/results", ""), TOKEN_HEADER);
        assert_eq!(handle(&p, &guard(), &no_token).status, 401);
    }

    #[test]
    fn the_inbox_import_takes_a_bigger_body_and_reports_like_the_command() {
        let (p, _d) = panel();
        let key = add(&p, "1", 80.0, &[State::Prepared, State::Submitted]);
        let message = |id: &str, kind: &str| {
            serde_json::json!({
                "message_id": id,
                "thread_id": format!("th-{id}"),
                "from": "Ana <ana@acme.example>",
                "subject": format!("Asunto {id}"),
                "received_at": "2026-10-01T12:30:00Z",
                "kind": kind,
                "summary": "x".repeat(600),
                "job_key": key,
            })
        };
        let post = |body: &str| handle(&p, &guard(), &request("POST", "/api/inbox/import", body));

        // 40 messages with long summaries: well past MAX_BODY, inside this route's own limit.
        let many: Vec<Value> = (0..40).map(|n| message(&format!("m{n}"), "ack")).collect();
        let body = Value::Array(many).to_string();
        assert!(body.len() > MAX_BODY && body.len() <= body_limit("/api/inbox/import"));
        let ok = post(&body);
        assert_eq!(ok.status, 200, "{}", text(&ok));
        let report = json_of(&ok);
        assert_eq!(
            (report["ok"].as_bool(), report["imported"].as_u64()),
            (Some(true), Some(40))
        );
        assert!(
            report["message"]
                .as_str()
                .unwrap()
                .starts_with("Importados: 40")
        );
        assert_eq!(report["lines"].as_array().unwrap().len(), 40);

        // Importing again skips the known ones; a rejection makes it 409 but the rest is imported.
        let again = Value::Array(vec![
            message("m0", "ack"),
            message("r1", "rejection"),
            serde_json::json!({ "message_id": "bad", "kind": "nope" }),
        ])
        .to_string();
        let partial = post(&again);
        assert_eq!(partial.status, 409);
        let report = json_of(&partial);
        assert_eq!(
            (
                report["imported"].as_u64(),
                report["skipped"].as_u64(),
                report["rejected"].as_u64(),
                report["state_changes"].as_u64()
            ),
            (Some(1), Some(1), Some(1), Some(1))
        );
        assert_eq!(
            p.tracker.get_application(&key).unwrap().unwrap().state,
            State::Rejected
        );

        // Not a list, or too much: refused whole, before anything is written.
        let not_a_list = post(r#"{"message_id":"x"}"#);
        assert_eq!(not_a_list.status, 400);
        assert!(
            json_of(&not_a_list)["message"]
                .as_str()
                .unwrap()
                .contains("lista JSON")
        );
        let big = format!("[{}]", "1".repeat(body_limit("/api/inbox/import")));
        assert_eq!(post(&big).status, 413);
        assert_eq!(body_limit("/api/inbox/import?x=1"), MAX_IMPORT_BODY);
        assert_eq!(body_limit("/api/approve"), MAX_BODY);
        assert_eq!(
            handle(&p, &guard(), &request("GET", "/api/inbox/import", "")).status,
            405
        );
        let no_token = without(request("POST", "/api/inbox/import", "[]"), TOKEN_HEADER);
        assert_eq!(handle(&p, &guard(), &no_token).status, 401);
    }

    #[test]
    fn a_task_can_be_closed_from_the_page_once_and_only_with_the_token() {
        let (p, _d) = panel();
        for text in ["Responder a Acme", "Grabar el video"] {
            p.tracker
                .add_task(
                    &crate::tracker::NewTask {
                        job_key: None,
                        kind: "action_required".into(),
                        text: text.into(),
                        due_at: None,
                        source_message_id: None,
                        link: None,
                    },
                    *crate::testing::NOW,
                )
                .unwrap();
        }
        let id = p.tracker.open_tasks().unwrap()[0].id;
        let body = format!(r#"{{"id":{id}}}"#);

        let no_token = without(request("POST", "/api/task/done", &body), TOKEN_HEADER);
        assert_eq!(handle(&p, &guard(), &no_token).status, 401);
        let not_json = without(request("POST", "/api/task/done", &body), "content-type");
        assert_eq!(handle(&p, &guard(), &not_json).status, 415);
        assert_eq!(
            p.tracker.open_tasks().unwrap().len(),
            2,
            "refused: nothing changed"
        );

        let ok = handle(&p, &guard(), &request("POST", "/api/task/done", &body));
        assert_eq!(ok.status, 200);
        assert!(
            json_of(&ok)["message"]
                .as_str()
                .is_some_and(|m| m.starts_with("Hecha:")),
            "{}",
            text(&ok)
        );
        let open = p.tracker.open_tasks().unwrap();
        assert_eq!(open.len(), 1, "only that task was closed");
        assert_ne!(open[0].id, id);

        // Replays and unknown numbers are refused with a Spanish message; a bad body is a 400.
        let again = handle(&p, &guard(), &request("POST", "/api/task/done", &body));
        assert_eq!(again.status, 409);
        let unknown = handle(
            &p,
            &guard(),
            &request("POST", "/api/task/done", r#"{"id":99999}"#),
        );
        assert_eq!(unknown.status, 409);
        let bad = handle(
            &p,
            &guard(),
            &request("POST", "/api/task/done", r#"{"id":"uno"}"#),
        );
        assert_eq!(bad.status, 400);
        assert_eq!(
            handle(&p, &guard(), &request("GET", "/api/task/done", "")).status,
            405
        );
        assert_eq!(p.tracker.open_tasks().unwrap().len(), 1);
    }

    #[test]
    fn the_drill_serves_a_card_without_its_answer_then_grades_it() {
        let (p, _d) = panel();
        let next = handle(&p, &guard(), &request("GET", "/api/learn/next", ""));
        assert_eq!(next.status, 200);
        assert!(
            !text(&next).contains("\"answer\""),
            "the answer waits for the reveal"
        );
        let card = json_of(&next)["card"].clone();
        let id = card["id"].as_str().unwrap().to_string();
        assert!(card["question"].as_str().is_some_and(|q| !q.is_empty()));
        let reveal = handle(
            &p,
            &guard(),
            &request("GET", &format!("/api/learn/answer?id={}", encode(&id)), ""),
        );
        assert!(
            json_of(&reveal)["answer"]
                .as_str()
                .is_some_and(|a| !a.is_empty())
        );
        let grade = |id: &str, grade: u32| {
            let body = serde_json::json!({ "id": id, "grade": grade }).to_string();
            handle(&p, &guard(), &request("POST", "/api/learn/grade", &body))
        };
        assert_eq!(grade(&id, 5).status, 200);
        let after = json_of(&handle(
            &p,
            &guard(),
            &request("GET", "/api/learn/next", ""),
        ));
        assert_ne!(
            after["card"]["id"],
            id.as_str(),
            "a graded card is not served again right away"
        );
        assert_eq!(grade("nope", 3).status, 409);
        assert_eq!(grade(&id, 6).status, 409, "above five is a refusal");
        assert_eq!(
            grade(&id, 300).status,
            400,
            "not even a number of the right size"
        );
        let none = json_of(&handle(
            &p,
            &guard(),
            &request("GET", "/api/learn/next?topic=nope", ""),
        ));
        assert!(none["card"].is_null());
        let snapshot = json_of(&handle(&p, &guard(), &request("GET", "/api/snapshot", "")));
        assert_eq!(snapshot["learn"]["seen"], 1);
    }

    #[test]
    fn a_screenshot_is_served_only_from_the_screenshots_folder_and_a_failure_is_a_500() {
        let (p, dir) = panel();
        let shots = dir.path().join("data").join("screenshots");
        std::fs::create_dir_all(&shots).unwrap();
        let get = |key: &str| {
            handle(
                &p,
                &guard(),
                &request("GET", &format!("/api/shot?key={}", encode(key)), ""),
            )
        };

        let inside = add(&p, "in", 80.0, &[]);
        let file = shots.join("a.png");
        std::fs::write(&file, b"PNGDATA").unwrap();
        p.tracker.set_screenshot(&inside, &file).unwrap();
        let ok = get(&inside);
        assert_eq!((ok.status, ok.content_type), (200, "image/png"));
        assert_eq!(ok.body, b"PNGDATA");

        let outside = add(&p, "out", 80.0, &[]);
        // Next to the screenshots folder, not in it: `shots/../secret.png` is this same file.
        let secret = dir.path().join("data").join("secret.png");
        std::fs::write(&secret, b"SECRET").unwrap();
        p.tracker.set_screenshot(&outside, &secret).unwrap();
        assert_eq!(get(&outside).status, 404);

        let dotdot = add(&p, "dots", 80.0, &[]);
        p.tracker
            .set_screenshot(&dotdot, &shots.join("..").join("secret.png"))
            .unwrap();
        assert_eq!(get(&dotdot).status, 404, "a .. path that leaves the folder");

        let none = add(&p, "none", 80.0, &[]);
        assert_eq!(get(&none).status, 404);
        assert_eq!(get("nope:1").status, 404);

        let broken = add(&p, "dir", 80.0, &[]);
        let folder = shots.join("x.png");
        std::fs::create_dir_all(&folder).unwrap();
        p.tracker.set_screenshot(&broken, &folder).unwrap();
        let failed = get(&broken);
        assert_eq!(failed.status, 500);
        assert!(
            json_of(&failed)["message"]
                .as_str()
                .unwrap()
                .starts_with("Error interno")
        );
        assert_eq!(
            get(&inside).status,
            200,
            "the next request is served normally"
        );
    }

    fn post(p: &Panel, target: &str, body: &str) -> Reply {
        handle(p, &guard(), &request("POST", target, body))
    }

    #[test]
    fn settings_keep_the_auto_approval_threshold_and_refuse_what_is_not_valid() {
        // The shipped profile queues from 60.
        let (p, _d) = panel();
        let snapshot_value = || {
            json_of(&handle(&p, &guard(), &request("GET", "/api/snapshot", "")))["controls"]
                ["auto_approve_min"]
                .clone()
        };
        assert!(snapshot_value().is_null());

        let ok = post(&p, "/api/settings", r#"{"auto_approve_min": 90}"#);
        assert_eq!(ok.status, 200);
        let body = json_of(&ok);
        assert_eq!(body["ok"], true);
        assert_eq!(body["auto_approve_min"], 90);
        assert!(body["message"].as_str().unwrap().contains("90"));
        assert_eq!(snapshot_value(), 90, "the snapshot shows what was kept");

        // Out of range is a refusal of the action: 409, nothing changes, and it says the range.
        for value in ["59", "101", "0", "-1"] {
            let refused = post(
                &p,
                "/api/settings",
                &format!(r#"{{"auto_approve_min": {value}}}"#),
            );
            assert_eq!(refused.status, 409, "{value}");
            let body = json_of(&refused);
            assert_eq!(body["ok"], false);
            assert_eq!(body["auto_approve_min"], 90, "it reports what is kept");
            assert!(body["message"].as_str().unwrap().contains("60"), "{value}");
        }
        assert_eq!(snapshot_value(), 90);

        // Not a whole number, or not a number: the page sent something it should not (400).
        for bad in [
            r#"{"auto_approve_min": 85.5}"#,
            r#"{"auto_approve_min": 90.0}"#,
            r#"{"auto_approve_min": "90"}"#,
            r#"{"auto_approve_min": true}"#,
            r#"{"auto_approve_min": [90]}"#,
            r#"{"auto_approve_min": 1e2}"#,
            r#"{"auto_approve_min": 99999999999999999999}"#,
            r#"{"batch_min": 85}"#,
            r#"{"auto_approve_min": 90, "batch_min": 85}"#,
            r#"{}"#,
            r#"[]"#,
            r#"90"#,
            "not json",
        ] {
            let reply = post(&p, "/api/settings", bad);
            assert_eq!(reply.status, 400, "{bad}");
            assert_eq!(json_of(&reply)["ok"], false, "{bad}");
        }
        assert_eq!(snapshot_value(), 90, "none of those changed anything");

        // null turns it off, twice is fine, and the whole range edge is accepted.
        for _ in 0..2 {
            let off = post(&p, "/api/settings", r#"{"auto_approve_min": null}"#);
            assert_eq!(off.status, 200);
            assert!(json_of(&off)["auto_approve_min"].is_null());
        }
        assert!(snapshot_value().is_null());
        for edge in [60, 100] {
            let reply = post(
                &p,
                "/api/settings",
                &format!(r#"{{"auto_approve_min": {edge}}}"#),
            );
            assert_eq!(reply.status, 200, "{edge}");
        }
    }

    #[test]
    fn settings_without_a_readable_profile_keep_no_number() {
        let (mut p, dir) = panel();
        p.paths.profile_dir = dir.path().join("no-profile-here");
        let refused = post(&p, "/api/settings", r#"{"auto_approve_min": 90}"#);
        assert_eq!(refused.status, 409);
        assert!(
            json_of(&refused)["message"]
                .as_str()
                .unwrap()
                .contains("criteria.toml")
        );
        assert_eq!(
            post(&p, "/api/settings", r#"{"auto_approve_min": null}"#).status,
            200
        );
    }

    /// `GET /api/file` for `key` and `kind`.
    fn file_of(p: &Panel, key: &str, kind: &str) -> Reply {
        handle(
            p,
            &guard(),
            &request(
                "GET",
                &format!("/api/file?key={}&kind={kind}", encode(key)),
                "",
            ),
        )
    }

    #[test]
    fn a_document_is_served_by_key_and_kind_with_its_own_type_and_untouched() {
        let (p, dir) = panel();
        let data = dir.path().join("data");
        let key = add(&p, "1", 80.0, &[]);
        let other = add(&p, "2", 80.0, &[]);
        // The CV is where the engine put it, named by the key; the dossier and the hand-over
        // sheet are found from the key alone.
        let cv = data
            .join("cv")
            .join(format!("{}.pdf", crate::text::safe_key(&key)));
        let dossier = crate::dossier::dossier_path(&data.join("dossiers"), &key);
        let sheet = crate::handoff::sheet_path(&data.join("handoff"), &key);
        for path in [&cv, &dossier, &sheet] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        }
        std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
        std::fs::write(
            &dossier,
            "# Dossier\nme@mail.example en C:\\Users\\ana\\x\n",
        )
        .unwrap();
        std::fs::write(&sheet, "# Hoja\nResponde: si\n").unwrap();
        p.tracker.set_cv_path(&key, &cv).unwrap();

        let pdf = file_of(&p, &key, "cv");
        assert_eq!((pdf.status, pdf.content_type), (200, "application/pdf"));
        assert_eq!(pdf.body, b"%PDF-1.4 fake");
        assert_eq!(
            pdf.header("content-disposition"),
            Some(format!("inline; filename=\"{}.pdf\"", crate::text::safe_key(&key)).as_str())
        );
        for (kind, text) in [("dossier", "# Dossier"), ("handoff", "# Hoja")] {
            let reply = file_of(&p, &key, kind);
            assert_eq!(reply.status, 200, "{kind}");
            assert_eq!(reply.content_type, "text/markdown; charset=utf-8", "{kind}");
            // A document that is shown as text must not be able to run anything.
            assert!(
                reply
                    .header("content-security-policy")
                    .is_some_and(|csp| csp.contains("sandbox")),
                "{kind}"
            );
            assert_eq!(reply.header("cache-control"), Some("no-store"));
            assert!(String::from_utf8(reply.body).unwrap().starts_with(text));
        }
        // It is the document itself, not a view of it: the filter that keeps his folder and his
        // address out of the snapshot does not rewrite his own file.
        let raw = String::from_utf8(file_of(&p, &key, "dossier").body).unwrap();
        assert!(raw.contains("me@mail.example") && raw.contains("C:\\Users\\ana\\x"));
        // Another application has none of these.
        for kind in ["cv", "dossier", "handoff"] {
            assert_eq!(file_of(&p, &other, kind).status, 404, "{kind}");
        }
    }

    #[test]
    fn a_document_is_never_served_from_outside_its_folder_nor_as_something_else() {
        let (p, dir) = panel();
        let data = dir.path().join("data");
        std::fs::create_dir_all(data.join("cv")).unwrap();
        let secret = data.join("secret.pdf");
        std::fs::write(&secret, b"SECRET").unwrap();
        let outside_the_data = dir.path().join("elsewhere.pdf");
        std::fs::write(&outside_the_data, b"ELSEWHERE").unwrap();

        let set = |id: &str, path: &std::path::Path| {
            let key = add(&p, id, 80.0, &[]);
            p.tracker.set_cv_path(&key, path).unwrap();
            key
        };
        // A CV recorded by hand with `jobbot record --cv <anywhere>` is not in the CV folder.
        let manual = set("manual", &outside_the_data);
        // Next to the folder, not in it: `cv/../secret.pdf` is this same file.
        let sibling = set("sibling", &secret);
        let dotdot = set("dotdot", &data.join("cv").join("..").join("secret.pdf"));
        // In the folder but not a PDF.
        let text = data.join("cv").join("notes.txt");
        std::fs::write(&text, b"notes").unwrap();
        let not_pdf = set("text", &text);
        // A folder where a file should be, and a name that is gone.
        let folder = data.join("cv").join("folder.pdf");
        std::fs::create_dir_all(&folder).unwrap();
        let is_dir = set("dir", &folder);
        let gone = set("gone", &data.join("cv").join("gone.pdf"));
        // Too big to serve.
        let huge = data.join("cv").join("huge.pdf");
        std::fs::write(&huge, vec![0u8; (MAX_DOCUMENT_BYTES + 1) as usize]).unwrap();
        let too_big = set("huge", &huge);
        // And one that is fine, so these 404s are the guard and not a broken test.
        let fine_path = data.join("cv").join("fine.pdf");
        std::fs::write(&fine_path, b"%PDF").unwrap();
        let fine = set("fine", &fine_path);

        for (what, key) in [
            ("outside data", &manual),
            ("sibling folder", &sibling),
            ("dotdot", &dotdot),
            ("not a pdf", &not_pdf),
            ("a folder", &is_dir),
            ("gone", &gone),
            ("too big", &too_big),
        ] {
            let reply = file_of(&p, key, "cv");
            assert_eq!(reply.status, 404, "{what}");
            assert!(
                !String::from_utf8_lossy(&reply.body).contains("SECRET"),
                "{what}"
            );
        }
        assert_eq!(file_of(&p, &fine, "cv").status, 200);
        // The key names an application, never a path.
        for key in ["../../secret", "..\\..\\x", "nope:1"] {
            assert_eq!(file_of(&p, key, "dossier").status, 404, "{key}");
        }
    }

    #[test]
    fn a_document_request_that_makes_no_sense_is_a_400() {
        let (p, _d) = panel();
        let key = add(&p, "1", 80.0, &[]);
        let get = |target: &str| handle(&p, &guard(), &request("GET", target, ""));
        for target in [
            "/api/file".to_string(),
            format!("/api/file?key={}", encode(&key)),
            "/api/file?kind=cv".to_string(),
            format!("/api/file?key={}&kind=", encode(&key)),
            format!("/api/file?key={}&kind=shot", encode(&key)),
            format!("/api/file?key={}&kind=..%2F..%2Fx", encode(&key)),
        ] {
            let reply = get(&target);
            assert_eq!(reply.status, 400, "{target}");
            assert_eq!(json_of(&reply)["ok"], false, "{target}");
        }
    }

    #[test]
    fn every_reply_carries_the_security_headers() {
        let (p, _d) = panel();
        let replies = [
            handle(&p, &guard(), &request("GET", "/", "")),
            handle(
                &p,
                &guard(),
                &without(request("GET", "/api/snapshot", ""), TOKEN_HEADER),
            ),
            handle(&p, &guard(), &request("GET", "/api/snapshot", "")),
            handle(&p, &guard(), &request("GET", "/nope", "")),
            handle(
                &p,
                &guard(),
                &with(request("GET", "/", ""), "host", "evil.example"),
            ),
        ];
        for reply in replies {
            assert_eq!(
                reply.header("cache-control"),
                Some("no-store"),
                "{}",
                reply.status
            );
            assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
            assert_eq!(reply.header("referrer-policy"), Some("no-referrer"));
        }
    }

    #[test]
    fn random_hex_is_the_right_length_and_never_repeats() {
        let a = random_hex(16).unwrap();
        let b = random_hex(16).unwrap();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn the_page_policy_allows_own_fonts_and_images_and_refuses_every_frame() {
        let (p, _d) = panel();
        let page = handle(&p, &guard(), &request("GET", "/", ""));
        let csp = page.header("content-security-policy").unwrap();
        let nonce = csp
            .split("'nonce-")
            .nth(1)
            .unwrap()
            .split('\'')
            .next()
            .unwrap();
        assert_eq!(
            csp,
            format!(
                "default-src 'none'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}'; connect-src 'self'; img-src 'self' blob:; font-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; require-trusted-types-for 'script'"
            )
        );
        assert_eq!(page.header("x-frame-options"), Some("DENY"));
        let api = handle(&p, &guard(), &request("GET", "/api/snapshot", ""));
        assert!(
            api.header("content-security-policy").is_none(),
            "the policy belongs to the page"
        );
    }

    #[test]
    fn the_page_gets_the_asset_prefix_the_way_it_gets_the_nonce() {
        let filled = fill_page(
            "<style nonce=\"__NONCE__\">.a{background:url(\"__ASSETS__/inter-400.woff2\")}</style><script nonce=\"__NONCE__\">var a=\"__ASSETS__\"+\"/inter-400.woff2\";</script>",
            "n0nce",
            LAUNCH,
        );
        assert_eq!(
            filled,
            "<style nonce=\"n0nce\">.a{background:url(\"/a/f00ba4f00ba4f00b/inter-400.woff2\")}</style><script nonce=\"n0nce\">var a=\"/a/f00ba4f00ba4f00b\"+\"/inter-400.woff2\";</script>"
        );
        assert_eq!(asset_prefix(LAUNCH), "/a/f00ba4f00ba4f00b");

        let (p, _d) = panel();
        let html = text(&handle(&p, &guard(), &request("GET", "/", "")));
        assert!(
            !html.contains("__ASSETS__") && !html.contains("__NONCE__"),
            "no placeholder survives"
        );
    }

    #[test]
    fn an_asset_is_served_under_this_launch_with_its_type_and_a_long_cache() {
        let (p, _d) = panel();
        let req = without(
            request("GET", &format!("/a/{LAUNCH}/inter-400.woff2"), ""),
            TOKEN_HEADER,
        );
        let reply = handle(&p, &guard(), &req);
        assert_eq!(reply.status, 200);
        assert_eq!(reply.content_type, "font/woff2");
        assert_eq!(reply.body, include_bytes!("assets/inter-400.woff2"));
        assert_eq!(
            reply.header("cache-control"),
            Some("public, max-age=31536000, immutable"),
            "a new start is a new path, so the browser may keep it for good"
        );
        assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
        assert_eq!(
            reply.header("cross-origin-resource-policy"),
            Some("same-origin")
        );
        assert_eq!(reply.header("referrer-policy"), Some("no-referrer"));
        assert!(
            reply.header("content-security-policy").is_none(),
            "an image is not a document"
        );

        // Every row of the table is reachable, and only with its own name.
        for (name, mime, bytes) in ASSETS {
            let got = handle(
                &p,
                &guard(),
                &request("GET", &format!("/a/{LAUNCH}/{name}"), ""),
            );
            assert_eq!((got.status, got.content_type), (200, *mime), "{name}");
            assert_eq!(got.body, *bytes, "{name}");
            assert!(
                !name.contains('/') && !name.contains("..") && !mime.is_empty(),
                "{name}: a plain file name with an explicit type"
            );
        }
    }

    /// The four families of the page (criterion A4), as `panel-src/tools/assets-sync.mjs` copies them.
    const FONTS: [&str; 5] = [
        "barlow-condensed-700.woff2",
        "geist-mono-400.woff2",
        "instrument-serif-400italic.woff2",
        "inter-400.woff2",
        "inter-600.woff2",
    ];

    #[test]
    fn each_font_is_served_as_woff2_under_this_launch_with_the_long_cache() {
        let (p, _d) = panel();
        let get = |target: &str| {
            handle(
                &p,
                &guard(),
                &without(request("GET", target, ""), TOKEN_HEADER),
            )
        };
        let mut total = 0;
        for name in FONTS {
            let reply = get(&format!("/a/{LAUNCH}/{name}"));
            assert_eq!(reply.status, 200, "{name}");
            assert_eq!(reply.content_type, "font/woff2", "{name}");
            assert_eq!(
                reply.header("cache-control"),
                Some("public, max-age=31536000, immutable"),
                "{name}"
            );
            assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
            assert!(
                reply.body.starts_with(b"wOF2"),
                "{name} is a WOFF2 file, not a page the server made up"
            );
            total += reply.body.len();
            // Only under this launch and with exactly this name.
            assert_eq!(get(&format!("/a/0000000000000000/{name}")).status, 404);
            assert_eq!(
                get(&format!("/a/{LAUNCH}/{}", name.to_uppercase())).status,
                404
            );
            assert_eq!(
                get(&format!("/a/{LAUNCH}/{}", name.replace(".woff2", ".woff"))).status,
                404
            );
            assert_eq!(get(&format!("/a/{LAUNCH}/{name}?v=1")).status, 404);
            assert_eq!(get(&format!("/a/{LAUNCH}/sub/{name}")).status, 404);
        }
        assert!(
            total <= 120 * 1024,
            "the five fonts weigh {total} bytes; the ceiling is 120 KB (R8 rule 10)"
        );
        // And nothing else is a font: the table holds exactly these five besides the mark.
        let fonts_in_table = ASSETS.iter().filter(|(_, mime, _)| *mime == "font/woff2");
        assert_eq!(fonts_in_table.count(), FONTS.len());
    }

    #[test]
    fn every_asset_has_a_row_with_its_hash_in_the_notices_and_no_row_lacks_its_file() {
        // The rule of `assets/README.md`: no row, no asset. The notices file is read at build time,
        // so a new `ASSETS` entry without its line fails here before it reaches a release.
        let notices = include_str!("../../THIRD_PARTY_NOTICES.md");
        let rows: Vec<&str> = notices
            .lines()
            .filter(|line| line.starts_with("| `"))
            .collect();
        for (name, mime, bytes) in ASSETS {
            let row = rows
                .iter()
                .find(|row| row.starts_with(&format!("| `{name}` |")))
                .unwrap_or_else(|| panic!("{name} has no row in THIRD_PARTY_NOTICES.md"));
            assert!(row.contains(mime), "{name}: the row names its type {mime}");
            let hash = row
                .split('`')
                .rev()
                .nth(1)
                .unwrap_or_else(|| panic!("{name}: the row ends with its sha256 in backticks"));
            assert!(
                hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
                "{name}: {hash:?} is not a sha256"
            );
            assert!(!bytes.is_empty(), "{name} is empty");
        }
        let in_table: Vec<&str> = ASSETS.iter().map(|(name, _, _)| *name).collect();
        for row in rows
            .iter()
            .filter(|row| row.contains("image/") || row.contains("font/"))
        {
            let name = row.trim_start_matches("| `").split('`').next().unwrap();
            assert!(
                in_table.contains(&name),
                "{name} has a row in THIRD_PARTY_NOTICES.md but no entry in ASSETS"
            );
        }
        // Names are unique, or the first would shadow the second.
        let mut sorted = in_table.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), in_table.len(), "duplicate asset names");
    }

    #[test]
    fn anything_but_a_listed_asset_under_this_launch_is_404() {
        let (p, _d) = panel();
        let status = |target: &str| {
            handle(
                &p,
                &guard(),
                &without(request("GET", target, ""), TOKEN_HEADER),
            )
            .status
        };
        for target in [
            &format!("/a/{LAUNCH}/nope.woff2"),
            &format!("/a/{LAUNCH}/INTER-400.WOFF2"),
            &format!("/a/{LAUNCH}/inter-400.woff2/"),
            &format!("/a/{LAUNCH}/../inter-400.woff2"),
            &format!("/a/{LAUNCH}/%2e%2e/inter-400.woff2"),
            &format!("/a/{LAUNCH}/sub/inter-400.woff2"),
            &format!("/a/{LAUNCH}/..%2finter-400.woff2"),
            &format!("/a/{LAUNCH}/inter-400.woff2?x=1"),
            &format!("/a/{LAUNCH}/"),
            &format!("/a/{LAUNCH}"),
            "/a/0000000000000000/inter-400.woff2",
            "/a/inter-400.woff2",
            "/a/",
            "/a",
            "/assets/inter-400.woff2",
        ] {
            assert_eq!(status(target), 404, "{target}");
        }
        // `sub/../inter-400.woff2` is not a way out: the address is normalised before it is routed, so it IS
        // `inter-400.woff2` (a browser does the same before it ever sends it), and `/a/<id>/../x` lands on `/a/x`.
        assert_eq!(status(&format!("/a/{LAUNCH}/sub/../inter-400.woff2")), 200);
        let host = with(
            request("GET", &format!("/a/{LAUNCH}/inter-400.woff2"), ""),
            "host",
            "evil.example:4321",
        );
        assert_eq!(
            handle(&p, &guard(), &host).status,
            403,
            "the Host guard comes first"
        );
        let post = request("POST", &format!("/a/{LAUNCH}/inter-400.woff2"), "{}");
        assert_eq!(handle(&p, &guard(), &post).status, 405);
    }

    // ---- what leaves the panel (criterion A9, widened): one filter on every JSON reply

    /// Every string of `value` with the path to it, leaving out the ones under a link field.
    fn leaves(value: &Value, path: &str, out: &mut Vec<(String, String)>) {
        match value {
            Value::String(text) => out.push((path.to_string(), text.clone())),
            Value::Array(items) => {
                for (n, item) in items.iter().enumerate() {
                    leaves(item, &format!("{path}[{n}]"), out);
                }
            }
            Value::Object(map) => {
                for (key, item) in map {
                    if ["url", "apply_url", "link"].contains(&key.as_str()) && item.is_string() {
                        continue;
                    }
                    leaves(item, &format!("{path}.{key}"), out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn the_folder_of_the_user_and_his_own_addresses_never_leave_the_panel_in_any_text_field() {
        const FOLDER: &str = r"C:\Users\ana\AppData\Local\jobbot\x.json";
        const MINE: &str = "mio@mail.example";
        const ALSO_MINE: &str = "yo.mismo@mail.example";
        let now = *crate::testing::NOW;
        // The profile says which addresses are his: `expected_email` and the e-mail the answer bank
        // fills into forms.
        let (mut p, dir) = panel();
        let profile = dir.path().join("profile");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(
            profile.join("claude.toml"),
            format!("expected_email = \"{MINE}\"\n"),
        )
        .unwrap();
        std::fs::write(
            profile.join("answers.toml"),
            format!("[[entry]]\npatterns = [\"email\"]\nvalue = \"{ALSO_MINE}\"\n"),
        )
        .unwrap();
        p.paths.profile_dir = profile;
        let tainted = |what: &str| format!("{what} {FOLDER} {MINE} / {ALSO_MINE}");

        let job = crate::testing::make_job_with(|j| {
            j.external_id = "leak".into();
            j.company = tainted("Empresa");
            j.title = tainted("Cargo");
            j.location = tainted("Lugar");
            j.description = tainted("Descripción");
            j.url = format!("https://empresa.example/jobs/1?c={MINE}");
            j.apply_url = format!("https://empresa.example/apply?c={MINE}");
        });
        p.tracker
            .add_job(&job, 80.0, &std::collections::BTreeMap::new(), now)
            .unwrap();
        let key = job.key();
        p.tracker.create_application(&key, now).unwrap();
        p.tracker
            .transition(&key, State::Prepared, None, now)
            .unwrap();
        p.tracker
            .transition(&key, State::NeedsGonza, Some(&tainted("page_error:")), now)
            .unwrap();
        std::fs::create_dir_all(p.paths.data_dir.join("dossiers")).unwrap();
        std::fs::write(
            crate::dossier::dossier_path(&p.paths.data_dir.join("dossiers"), &key),
            tainted("# Dossier"),
        )
        .unwrap();
        p.tracker
            .add_task(
                &crate::tracker::NewTask {
                    job_key: Some(key.clone()),
                    kind: "action_required".into(),
                    text: tainted("Tarea"),
                    due_at: None,
                    source_message_id: None,
                    link: Some(format!("https://empresa.example/t?c={MINE}")),
                },
                now,
            )
            .unwrap();
        let mail = |id: &str, from: &str| crate::tracker::NewEmail {
            message_id: id.into(),
            thread_id: format!("th-{id}"),
            from_addr: from.into(),
            subject: tainted("Asunto"),
            received_at: now,
            job_key: Some(key.clone()),
            kind: crate::tracker::EmailKind::Recruiter,
            summary: tainted("Resumen"),
        };
        p.tracker
            .add_email(&mail("m1", "Zoe <zoe@acme.example>"), None, now)
            .unwrap();
        p.tracker
            .add_email(&mail("m2", &format!("Yo <{MINE}>")), None, now)
            .unwrap();

        let snapshot = json_of(&handle(&p, &guard(), &request("GET", "/api/snapshot", "")));
        let detail = json_of(&handle(
            &p,
            &guard(),
            &request("GET", &format!("/api/job?key={}", encode(&key)), ""),
        ));

        let listing = json_of(&handle(&p, &guard(), &request("GET", "/api/jobs", "")));
        for (name, reply, least) in [
            ("snapshot", &snapshot, 20),
            ("job", &detail, 20),
            ("jobs", &listing, 5),
        ] {
            let mut all = Vec::new();
            leaves(reply, "", &mut all);
            assert!(
                all.len() > least,
                "{name}: the walk saw {} strings",
                all.len()
            );
            for (path, text) in &all {
                for secret in [MINE, ALSO_MINE, r"C:\Users\ana", "AppData"] {
                    assert!(
                        !text.contains(secret),
                        "{name}{path} carries {secret:?}: {text:?}"
                    );
                }
            }
        }
        // They were seeded in every field that has text, and each one came out masked, not cut.
        let masked = "m***@mail.example";
        assert!(
            listing["cards"][0]["company"]
                .as_str()
                .unwrap()
                .contains(masked),
            "the explorer is filtered like the snapshot"
        );
        let card = &snapshot["needs_gonza"]["cards"][0];
        let events = detail["events"].as_array().unwrap();
        for (what, got) in [
            ("task", &snapshot["today"]["tasks"][0]["text"]),
            ("mail subject", &snapshot["today"]["emails"][0]["subject"]),
            ("mail summary", &snapshot["today"]["emails"][0]["summary"]),
            ("card company", &card["company"]),
            ("card title", &card["title"]),
            ("card reason", &card["reason"]),
            ("card note", &card["note"]),
            ("move company", &snapshot["activity"]["moves"][0]["company"]),
            ("detail location", &detail["location"]),
            ("detail description", &detail["description"]),
            ("detail dossier", &detail["dossier"]),
            ("detail event note", &events[events.len() - 1]["note"]),
        ] {
            let got = got
                .as_str()
                .unwrap_or_else(|| panic!("{what} is not text: {got}"));
            assert!(got.contains(masked), "{what}: {got}");
            assert!(got.contains("%LOCALAPPDATA%"), "{what}: {got}");
        }
        // A company's address is content the page needs; a link still opens as it was.
        let senders: Vec<&str> = snapshot["today"]["emails"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["from"].as_str().unwrap())
            .collect();
        assert!(senders.contains(&"Zoe <zoe@acme.example>"));
        assert!(senders.contains(&"Yo <m***@mail.example>"));
        assert_eq!(
            card["url"],
            format!("https://empresa.example/jobs/1?c={MINE}")
        );
        assert_eq!(
            detail["apply_url"],
            format!("https://empresa.example/apply?c={MINE}")
        );
        assert_eq!(
            snapshot["today"]["tasks"][0]["link"],
            format!("https://empresa.example/t?c={MINE}")
        );
    }

    #[test]
    fn an_error_that_names_a_folder_of_the_user_reaches_the_page_without_it() {
        let user = UserPaths::new(Some(r"C:\Users\ana"), None, None);
        let direct = redact_reply(
            &user,
            Reply::message(
                500,
                r"Error interno: no pude abrir C:\Users\ana\data\jobbot.db",
            ),
        );
        assert_eq!(
            json_of(&direct)["message"],
            r"Error interno: no pude abrir %USERPROFILE%\data\jobbot.db"
        );
        // Only JSON is touched: a font, a screenshot or a document keeps its bytes.
        let page = Reply {
            status: 200,
            content_type: "text/markdown; charset=utf-8",
            headers: Vec::new(),
            body: br"ruta C:\Users\ana\x".to_vec(),
        };
        assert_eq!(redact_reply(&user, page).body, br"ruta C:\Users\ana\x");
    }
}
