//! The panel's HTTP layer as plain functions over a request value, so every guard is tested
//! without a socket (`server.rs` only moves bytes). The guards, in order: `Host` must be this
//! socket (against DNS rebinding); `/api/*` needs the per-launch token; a foreign `Origin` is
//! refused; a POST must be `application/json` (the one thing a hostile page cannot send without a
//! CORS preflight that this server never answers) and at most `body_limit` bytes. The page's own
//! files (`assets/`) are served under `/a/<launch-id>/` with the `Host` guard alone, like `/`.
//! A POST may carry an attempt id (`Idempotency-Key`): a repeat of it gets the first reply back
//! and the action runs once.
//!
//! Every action goes through `actions.rs`, the same functions the commands call (the import of
//! the mailbox calls the importer, `jobbot_app::inbox`, as its command does), so the page can do
//! whatever the command line can to an application, a task or the results log, and nothing more:
//! nothing here starts discovery, a browser or Claude.
//!
//! Each part has its own file:
//!
//! - `attempts`: the attempt id of a POST, and the first reply given again to a repeat of it.
//! - `commands`: the POST commands: the body each one expects and the action it calls.
//! - `files`: the CV, dossier, hand-over sheet and screenshot served from the user's folders.
//! - `page`: the page with its content policy, and the fonts embedded with it.
//! - `reads`: the results log and the study cards.

use std::collections::BTreeMap;
use std::panic::AssertUnwindSafe;

use anyhow::Result;
use serde_json::{Value, json};
use url::Url;

use super::Panel;
use super::jobs::{JobsQuery, build_jobs_page};
use super::snapshot::{build_job_detail, build_snapshot};
use jobbot_app::inbox;
use jobbot_app::privacy::UserPaths;

mod attempts;
mod commands;
mod files;
mod page;
mod reads;

use attempts::{attempt_id, once};
use commands::post;
use files::{DocumentKind, document, screenshot};
pub use page::ASSETS;
use page::{asset, page_reply};
use reads::{learn_answer, learn_next, results};

#[cfg(test)]
mod commands_tests;
#[cfg(test)]
mod guard_tests;
#[cfg(test)]
mod privacy_tests;
#[cfg(test)]
mod route_tests;
#[cfg(test)]
mod support_tests;

const MAX_BODY: usize = 16 * 1024;
/// `/api/inbox/import` carries a whole export of the mailbox: the importer's own file limit.
const MAX_IMPORT_BODY: usize = inbox::MAX_FILE_BYTES as usize;
const TOKEN_HEADER: &str = "x-panel-token";

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
const POST_ROUTES: [&str; 15] = [
    "approve",
    "resolve",
    "confirm",
    "mark",
    "mark/undo",
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

/// The value of the first header called `name` (any letter case).
fn find_header<'a, K: AsRef<str>, V: AsRef<str>>(
    headers: &'a [(K, V)],
    name: &str,
) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.as_ref().eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_ref())
}

pub struct Request {
    pub method: String,
    /// Path and query, exactly as received (`/api/job?key=a%2Fb`).
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        find_header(&self.headers, name)
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
        Reply::json_body(status, value.to_string().into_bytes())
    }

    /// A reply whose body is JSON text already (a kept reply given again).
    fn json_body(status: u16, body: Vec<u8>) -> Reply {
        Reply {
            status,
            content_type: "application/json; charset=utf-8",
            headers: Vec::new(),
            body,
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
        find_header(&self.headers, name)
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
        self.is_local_address(host, "")
    }

    fn origin_ok(&self, origin: &str) -> bool {
        self.is_local_address(origin, "http://")
    }

    /// `given` is `<scheme>127.0.0.1:<port>` or `<scheme>localhost:<port>` of this server, in any
    /// letter case; `scheme` is empty for a Host and `http://` for an Origin.
    fn is_local_address(&self, given: &str, scheme: &str) -> bool {
        ["127.0.0.1", "localhost"]
            .iter()
            .any(|name| given.eq_ignore_ascii_case(&format!("{scheme}{name}:{}", self.port)))
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

/// Answers one request. Never panics: an error inside a handler is a 500 with a JSON message, and
/// so is a panic (a generic one, the cause goes to the log); the server goes on with the next
/// request.
pub fn handle(panel: &Panel, guard: &Guard, req: &Request) -> Reply {
    let mut reply = match std::panic::catch_unwind(AssertUnwindSafe(|| route(panel, guard, req))) {
        Ok(Ok(reply)) => reply,
        Ok(Err(err)) => Reply::message(500, &format!("Error interno: {err:#}")),
        Err(payload) => {
            tracing::error!(
                method = %req.method,
                panic = %panic_text(payload.as_ref()),
                "a request handler panicked; the panel keeps serving"
            );
            Reply::message(500, "Error interno")
        }
    };
    // The one place where what leaves the panel is filtered: his folders and his own addresses.
    if reply.content_type.starts_with("application/json") {
        reply = redact_reply(&panel.privacy.current(&panel.paths.profile_dir), reply);
    }
    with_common_headers(reply)
}

/// The message of a panic payload, for the log.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".to_string())
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
        &panel.privacy.current(&panel.paths.profile_dir),
        refusal,
    )))
}

/// The guards of `route` that need no body, in their order: Host first, then (for the API) the
/// token, the Origin, the route and its method and, for a POST, the JSON content type.
fn refuse_before_body(guard: &Guard, req: &Request) -> Option<Reply> {
    if !req.header("host").is_some_and(|host| guard.host_ok(host)) {
        return Some(Reply::message(403, "Host no permitido"));
    }
    let Some(url) = parse_target(&req.target) else {
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

/// The path and query of a request target as a URL (the host is a placeholder; only the parts after
/// it are used), or `None` when the target is not a valid address.
fn parse_target(target: &str) -> Option<Url> {
    Url::parse(&format!("http://panel.invalid{target}")).ok()
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

fn route(panel: &Panel, guard: &Guard, req: &Request) -> Result<Reply> {
    if let Some(refusal) = refuse_before_body(guard, req) {
        return Ok(refusal);
    }
    let Some(url) = parse_target(&req.target) else {
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
    if POST_ROUTES.contains(&api) {
        return match attempt_id(req) {
            Ok(None) => post(panel, api, req),
            Ok(Some(id)) => once(panel, id, api, req),
            Err(refusal) => Ok(refusal),
        };
    }
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
        _ => Ok(Reply::message(404, "No existe")),
    }
}

#[cfg(test)]
mod tests {
    use super::support_tests::{TOKEN, guard, request};
    use super::*;

    #[test]
    fn host_and_origin_must_be_this_server_exactly() {
        let g = guard();
        for host in ["127.0.0.1:4321", "localhost:4321", "LocalHost:4321"] {
            assert!(g.host_ok(host), "{host}");
            assert!(g.origin_ok(&format!("http://{host}")), "{host}");
        }
        for wrong in [
            "127.0.0.1:4322",
            "127.0.0.1",
            "evil.test:4321",
            "localhost:4321.evil.test",
            "127.0.0.1:4321.evil.test",
            "127.0.0.1:4321/",
            "127.0.0.1:4321 ",
            "[::1]:4321",
            "",
        ] {
            assert!(!g.host_ok(wrong), "{wrong}");
            assert!(!g.origin_ok(&format!("http://{wrong}")), "{wrong}");
        }
        assert!(!g.host_ok("http://127.0.0.1:4321"), "a Host has no scheme");
        assert!(!g.origin_ok("127.0.0.1:4321"), "an Origin has its scheme");
        assert!(!g.origin_ok("https://127.0.0.1:4321"), "only http");
        assert!(!g.origin_ok("null"), "a sandboxed page says null");
        assert!(
            g.origin_ok("HTTP://127.0.0.1:4321"),
            "the scheme is read in any letter case, like the name"
        );
    }

    #[test]
    fn of_two_headers_with_one_name_the_first_is_the_one_read() {
        let mut req = request("GET", "/", "");
        req.headers
            .push(("host".to_string(), "evil.test:4321".to_string()));
        assert_eq!(req.header("Host"), Some("127.0.0.1:4321"));
    }

    #[test]
    fn a_header_is_found_by_name_in_any_letter_case() {
        let req = request("GET", "/", "");
        assert_eq!(req.header("x-panel-token"), Some(TOKEN));
        assert_eq!(req.header("HOST"), Some("127.0.0.1:4321"));
        assert_eq!(req.header("origin"), None);
        let reply = with_common_headers(Reply::empty(204));
        assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
        assert_eq!(reply.header("x-nope"), None);
    }

    #[test]
    fn a_target_is_parsed_with_its_path_and_query_or_not_at_all() {
        let url = parse_target("/api/job?key=a%2Fb").unwrap();
        assert_eq!(url.path(), "/api/job");
        assert_eq!(url.query(), Some("key=a%2Fb"));
        assert!(parse_target("[").is_none());
    }

    #[test]
    fn random_hex_is_the_right_length_and_never_repeats() {
        let a = random_hex(16).unwrap();
        let b = random_hex(16).unwrap();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}
