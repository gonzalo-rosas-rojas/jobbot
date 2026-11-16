//! The attempt id of a POST (`Idempotency-Key`): a repeat of it gets the first reply back and the
//! command runs once.

use std::panic::AssertUnwindSafe;

use anyhow::Result;

use super::{Reply, Request, panic_text, post};
use crate::Panel;
use jobbot_app::ports::store::AttemptStart;

/// The header of the attempt id a POST may carry: a repeat of the id gets the first reply back
/// and the command runs once, so a retry never does the action twice. Optional: a POST without it runs as it always did.
const ATTEMPT_HEADER: &str = "Idempotency-Key";

/// The longest attempt id (a `crypto.randomUUID()` is 36 characters).
const MAX_ATTEMPT_ID: usize = 128;

/// The attempt id a POST may carry in `Idempotency-Key` (the IETF httpapi draft's header): 1 to
/// `MAX_ATTEMPT_ID` visible ASCII characters, in quotes or not. `None` when the header is absent;
/// a 400 when it is there and malformed.
pub(super) fn attempt_id(req: &Request) -> std::result::Result<Option<&str>, Reply> {
    let Some(value) = req.header(ATTEMPT_HEADER) else {
        return Ok(None);
    };
    let value = value.trim();
    let id = value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(value);
    let visible = id.bytes().all(|b| b.is_ascii_graphic() && b != b'"');
    if (1..=MAX_ATTEMPT_ID).contains(&id.len()) && visible {
        Ok(Some(id))
    } else {
        Err(Reply::message(
            400,
            "Idempotency-Key debe tener de 1 a 128 caracteres ASCII visibles",
        ))
    }
}

/// Runs the POST `api` once per attempt id: a repeat gets the first reply back, marked
/// `Idempotent-Replayed`. A reply that is a server failure (5xx, or an error) is not kept, so the
/// same id may try again.
pub(super) fn once(panel: &Panel, id: &str, api: &str, req: &Request) -> Result<Reply> {
    match panel
        .tracker
        .begin_attempt(id, api, &req.body, panel.now())?
    {
        AttemptStart::New => {
            // A panic in the command is a failure like any other: the attempt id is released, not
            // left `started` for a retry to be told "begun and not finished".
            let reply = std::panic::catch_unwind(AssertUnwindSafe(|| post(panel, api, req)))
                .unwrap_or_else(|payload| {
                    tracing::error!(
                        api,
                        panic = %panic_text(payload.as_ref()),
                        "a command panicked"
                    );
                    Err(anyhow::anyhow!("fallo interno"))
                });
            match &reply {
                Ok(reply) if reply.status < 500 => {
                    panel
                        .tracker
                        .finish_attempt(id, reply.status, &reply.body)?;
                }
                // The failure is what the caller needs to see; a row that could not be dropped only
                // answers 409 to a retry with the same id.
                _ => {
                    if let Err(err) = panel.tracker.forget_attempt(id) {
                        tracing::error!(attempt = id, error = %err, "could not release the attempt");
                    }
                }
            }
            reply
        }
        AttemptStart::Done { status, reply } => {
            let mut replayed = Reply::json_body(status, reply);
            replayed
                .headers
                .push(("Idempotent-Replayed", "true".to_string()));
            Ok(replayed)
        }
        AttemptStart::Unfinished => Ok(Reply::message(
            409,
            "Una petición con este Idempotency-Key empezó y no terminó: revisa el estado antes de \
             reintentar con un id nuevo",
        )),
        AttemptStart::Mismatch => Ok(Reply::message(
            422,
            "Este Idempotency-Key ya se usó para otra petición",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{add, panel};
    use crate::http::handle;
    use crate::http::support_tests::{
        guard, json_of, panel_whose_clock_can_panic, request, text, with,
    };

    const ATTEMPT: &str = "8e03978e-40d5-43e8-bc93-6894a57f9324";

    /// A POST of `body` to `/api/{route}`, with the attempt id `id` when there is one.
    fn post_once(p: &Panel, route: &str, body: &str, id: Option<&str>) -> Reply {
        let req = request("POST", &format!("/api/{route}"), body);
        let req = match id {
            Some(id) => with(req, "Idempotency-Key", id),
            None => req,
        };
        handle(p, &guard(), &req)
    }

    #[test]
    fn a_repeated_attempt_id_gets_the_first_reply_and_the_command_runs_once() {
        let (p, _d) = panel();
        let body = serde_json::json!({ "text": "Llamar" }).to_string();

        let first = post_once(&p, "task/add", &body, Some(ATTEMPT));
        assert_eq!(first.status, 200, "{}", text(&first));
        assert_eq!(first.header("Idempotent-Replayed"), None);
        let again = post_once(&p, "task/add", &body, Some(ATTEMPT));
        assert_eq!(again.status, 200, "{}", text(&again));
        assert_eq!(again.body, first.body);
        assert_eq!(again.header("Idempotent-Replayed"), Some("true"));
        let quoted = post_once(&p, "task/add", &body, Some(&format!("\"{ATTEMPT}\"")));
        assert_eq!(
            quoted.header("Idempotent-Replayed"),
            Some("true"),
            "a quoted id is the same id"
        );
        assert_eq!(p.tracker.open_tasks().unwrap().len(), 1);

        // Without an id the panel answers as before: the same task twice is refused.
        assert_eq!(post_once(&p, "task/add", &body, None).status, 409);
    }

    #[test]
    fn a_panic_in_a_command_releases_its_attempt_id() {
        use std::sync::atomic::Ordering;
        let (p, _d, calls, panic_at) = panel_whose_clock_can_panic();
        let body = serde_json::json!({ "text": "Llamar" }).to_string();
        // The first clock read is `begin_attempt`'s; the command's own read is the second.
        panic_at.store(calls.load(Ordering::SeqCst) + 1, Ordering::SeqCst);
        let broken = post_once(&p, "task/add", &body, Some(ATTEMPT));
        assert_eq!(broken.status, 500, "{}", text(&broken));
        panic_at.store(usize::MAX, Ordering::SeqCst);
        let retry = post_once(&p, "task/add", &body, Some(ATTEMPT));
        assert_eq!(
            retry.status,
            200,
            "the id is free again, not 'begun and not finished': {}",
            text(&retry)
        );
        assert_eq!(p.tracker.open_tasks().unwrap().len(), 1);
    }

    #[test]
    fn an_attempt_id_used_for_another_request_is_refused() {
        let (p, _d) = panel();
        let key = add(&p, "1", 80.0, &[]);
        let call = serde_json::json!({ "text": "Llamar" }).to_string();
        assert_eq!(post_once(&p, "task/add", &call, Some(ATTEMPT)).status, 200);

        let write = serde_json::json!({ "text": "Escribir" }).to_string();
        let other_body = post_once(&p, "task/add", &write, Some(ATTEMPT));
        assert_eq!(other_body.status, 422, "{}", text(&other_body));
        let approve = serde_json::json!({ "key": key }).to_string();
        assert_eq!(
            post_once(&p, "approve", &approve, Some(ATTEMPT)).status,
            422
        );
        assert_eq!(p.tracker.open_tasks().unwrap().len(), 1);
        assert!(!p.tracker.get_application(&key).unwrap().unwrap().approved);
    }

    #[test]
    fn a_malformed_attempt_id_is_refused_before_anything_runs() {
        let (p, _d) = panel();
        let body = serde_json::json!({ "text": "Llamar" }).to_string();
        let long = "x".repeat(129);
        for id in ["", "   ", "\"\"", long.as_str(), "tab\there", "ñandú"] {
            let reply = post_once(&p, "task/add", &body, Some(id));
            assert_eq!(reply.status, 400, "{id:?}: {}", text(&reply));
        }
        assert!(p.tracker.open_tasks().unwrap().is_empty());
        let longest = "x".repeat(128);
        assert_eq!(post_once(&p, "task/add", &body, Some(&longest)).status, 200);
    }

    #[test]
    fn an_attempt_left_unfinished_is_not_run_again() {
        let (p, _d) = panel();
        let body = serde_json::json!({ "text": "Llamar" }).to_string();
        // The process that began it died before the reply was kept.
        p.tracker
            .begin_attempt(ATTEMPT, "task/add", body.as_bytes(), p.now())
            .unwrap();
        let reply = post_once(&p, "task/add", &body, Some(ATTEMPT));
        assert_eq!(reply.status, 409, "{}", text(&reply));
        assert!(p.tracker.open_tasks().unwrap().is_empty());
    }

    #[test]
    fn a_grade_sent_twice_with_one_attempt_id_is_recorded_once() {
        let (p, _d) = panel();
        let next = json_of(&handle(
            &p,
            &guard(),
            &request("GET", "/api/learn/next", ""),
        ));
        let id = next["card"]["id"].as_str().unwrap().to_string();
        let body = serde_json::json!({ "id": id, "grade": 4 }).to_string();
        let first = post_once(&p, "learn/grade", &body, Some(ATTEMPT));
        assert_eq!(first.status, 200, "{}", text(&first));
        let again = post_once(&p, "learn/grade", &body, Some(ATTEMPT));
        assert_eq!(again.header("Idempotent-Replayed"), Some("true"));
        assert_eq!(again.body, first.body);
    }
}
