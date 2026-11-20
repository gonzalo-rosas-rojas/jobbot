//! Tests of the POST commands as the page sends them: one key or a batch, the refusals in Spanish and
//! the settings, tasks, records and imports they change.

use super::commands::MAX_BATCH;
use super::support_tests::{api_post, approved, encode, guard, json_of, request, text, without};
use super::*;
use crate::fixtures::{add, capture, panel, task};
use jobbot_domain::state::State;

#[test]
fn awkward_keys_survive_queries_and_bodies() {
    let (p, _d) = panel();
    for id in ["acme/42", "a b&c+d%e", "ñandú", "q?x=1#frag", "x\"y\\z"] {
        let key = add(&p, id, 80.0, &[State::Prepared]);
        capture(&p, &key);
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
fn a_refused_state_change_is_409_in_spanish_and_changes_nothing() {
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
        let b =
            serde_json::json!({ "key": key, "state": state, "note": "martes 19:00" }).to_string();
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
}

#[test]
fn a_body_that_is_not_the_expected_json_is_a_400() {
    let (p, _d) = panel();
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
fn stop_and_resume_toggle_the_file_and_the_snapshot_shows_it() {
    let (p, _d) = panel();
    let stopped = |p: &Panel| {
        json_of(&handle(p, &guard(), &request("GET", "/api/snapshot", "")))["controls"]["stopped"]
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
fn a_task_can_be_added_from_the_page_with_its_date_posting_and_link() {
    let (p, _d) = panel();
    let key = add(&p, "1", 80.0, &[State::NeedsGonza]);
    let post = |body: Value| post_value(&p, "/api/task/add", body);

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
    let post = |body: Value| post_value(&p, "/api/record", body);

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
    let submitted = p.tracker.list_by_state(State::Submitted).unwrap();
    assert_eq!(submitted.len(), 1, "only the recorded one is sent");
    let key = submitted[0].job_key.clone();
    assert!(
        message.contains(&key),
        "the message names the key: {message}"
    );
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
    assert_eq!(row.submitted_at, Some(*jobbot_testkit::NOW));

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
fn an_application_sent_by_hand_can_be_recorded_from_the_page_with_its_origin() {
    let (p, _d) = panel();
    let reply = post_value(
        &p,
        "/api/record",
        serde_json::json!({
            "company": "Acme",
            "title": "Backend Engineer",
            "url": "https://jobs.example.com/acme/42",
            "origin": "agency",
        }),
    );
    assert_eq!(reply.status, 200, "{}", text(&reply));
    let key = p.tracker.list_by_state(State::Submitted).unwrap()[0]
        .job_key
        .clone();
    assert_eq!(
        p.tracker.origin_of(&key).unwrap(),
        Some(jobbot_domain::origin::Origin::Agency)
    );
}

#[test]
fn a_record_from_the_page_with_an_unknown_origin_is_a_409() {
    let (p, _d) = panel();
    let reply = post_value(
        &p,
        "/api/record",
        serde_json::json!({
            "company": "Acme",
            "title": "Backend Engineer",
            "url": "https://jobs.example.com/acme/42",
            "origin": "amigo",
        }),
    );
    assert_eq!(reply.status, 409, "{}", text(&reply));
    assert!(
        json_of(&reply)["message"]
            .as_str()
            .unwrap()
            .starts_with("Origen no válido: amigo."),
        "{}",
        text(&reply)
    );
    assert!(
        p.tracker.counts().unwrap().is_empty(),
        "nothing was written"
    );
}

/// An authenticated `POST` of a JSON value to an API route.
fn post_value(p: &Panel, route: &str, body: Value) -> Reply {
    api_post(p, route, &body.to_string())
}

/// Two prepared applications with a captured form, the keys a batch test works on.
fn two_prepared_with_form(p: &Panel) -> (String, String) {
    let a = add(p, "a", 80.0, &[State::Prepared]);
    capture(p, &a);
    let b = add(p, "b", 70.0, &[State::Prepared]);
    capture(p, &b);
    (a, b)
}

#[test]
fn approve_takes_a_batch_and_reports_each_key() {
    let (p, _d) = panel();
    let (a, b) = two_prepared_with_form(&p);
    let post = |route: &str, body: Value| post_value(&p, route, body);

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
}

#[test]
fn a_malformed_batch_is_a_400_and_changes_nothing() {
    let (p, _d) = panel();
    let (a, b) = two_prepared_with_form(&p);
    let states = || [&a, &b].map(|key| p.tracker.get_application(key).unwrap().unwrap().state);
    let before = states();
    for body in [
        serde_json::json!({ "keys": [] }),
        serde_json::json!({ "key": a, "keys": [b] }),
        serde_json::json!({}),
        serde_json::json!({ "keys": vec!["x"; MAX_BATCH + 1] }),
    ] {
        assert_eq!(
            post_value(&p, "/api/confirm", body.clone()).status,
            400,
            "{body}"
        );
    }
    assert_eq!(states(), before, "none of them moved an application");
}

#[test]
fn resolve_and_confirm_take_batches() {
    let (p, _d) = panel();
    let c = add(&p, "c", 60.0, &[State::Prepared, State::NeedsGonza]);
    let d = add(&p, "d", 60.0, &[State::Prepared, State::NeedsGonza]);
    let resolved = post_value(&p, "/api/resolve", serde_json::json!({ "keys": [c, d] }));
    assert_eq!(resolved.status, 200, "{}", text(&resolved));
    let e = add(&p, "e", 60.0, &[State::Prepared, State::NeedsGonza]);
    let confirmed = post_value(&p, "/api/confirm", serde_json::json!({ "keys": [e] }));
    assert_eq!(confirmed.status, 200, "{}", text(&confirmed));
    let state = |key: &str| p.tracker.get_application(key).unwrap().unwrap().state;
    assert_eq!(
        (state(&c), state(&d), state(&e)),
        (State::Scored, State::Scored, State::Submitted)
    );
}

/// An application parked for its region, as the gate leaves it.
fn parked_for_region(p: &Panel, name: &str) -> String {
    let key = add(p, name, 60.0, &[]);
    p.tracker
        .transition(
            &key,
            State::NeedsGonza,
            Some(jobbot_domain::gate::OUT_OF_REGION_NOTE),
            *jobbot_domain::fixtures::NOW,
        )
        .unwrap();
    key
}

#[test]
fn a_batch_resolve_refuses_the_region_card_and_resolves_the_others() {
    let (p, _d) = panel();
    let c = add(&p, "c", 60.0, &[State::Prepared, State::NeedsGonza]);
    let region = parked_for_region(&p, "r");
    let d = add(&p, "d", 60.0, &[State::Prepared, State::NeedsGonza]);

    let reply = post_value(
        &p,
        "/api/resolve",
        serde_json::json!({ "keys": [c, region, d] }),
    );

    assert_eq!(reply.status, 409, "{}", text(&reply));
    let body = json_of(&reply);
    assert_eq!(body["message"], "2 de 3 con éxito");
    assert_eq!(body["results"][1]["ok"], false);
    assert!(
        body["results"][1]["message"]
            .as_str()
            .unwrap()
            .contains("«Enviar igual»")
    );
    let row = |key: &str| p.tracker.get_application(key).unwrap().unwrap();
    assert_eq!(row(&c).state, State::Scored);
    assert_eq!(row(&d).state, State::Scored);
    assert_eq!(row(&region).state, State::NeedsGonza);
    assert!(!p.tracker.region_override(&region).unwrap());
}

#[test]
fn send_anyway_takes_one_region_card_back_with_its_override_and_never_a_batch() {
    let (p, _d) = panel();
    let region = parked_for_region(&p, "r");
    let other = parked_for_region(&p, "s");

    let batch = post_value(
        &p,
        "/api/send_anyway",
        serde_json::json!({ "keys": [region, other] }),
    );
    assert_eq!(batch.status, 400, "{}", text(&batch));
    assert!(!p.tracker.region_override(&region).unwrap());

    let one = post_value(&p, "/api/send_anyway", serde_json::json!({ "key": region }));
    assert_eq!(one.status, 200, "{}", text(&one));
    assert_eq!(
        json_of(&one)["message"],
        "Vuelve a preparación: prepare le hará el PDF y se enviará sin la revisión de región."
    );
    assert!(p.tracker.region_override(&region).unwrap());
    assert!(!p.tracker.region_override(&other).unwrap());
}

#[test]
fn send_anyway_on_a_card_with_another_reason_is_a_plain_resolve() {
    let (p, _d) = panel();
    let c = add(&p, "c", 60.0, &[State::Prepared, State::NeedsGonza]);

    let reply = post_value(&p, "/api/send_anyway", serde_json::json!({ "key": c }));

    assert_eq!(reply.status, 200, "{}", text(&reply));
    assert_eq!(
        p.tracker.get_application(&c).unwrap().unwrap().state,
        State::Scored
    );
    assert!(!p.tracker.region_override(&c).unwrap());
}

#[test]
fn mark_takes_a_batch_and_keeps_a_note() {
    let (p, _d) = panel();
    let (a, _b) = two_prepared_with_form(&p);
    let state = |key: &str| p.tracker.get_application(key).unwrap().unwrap().state;
    let sent_a = add(&p, "f", 60.0, &[State::Prepared, State::Submitted]);
    let sent_b = add(&p, "g", 60.0, &[State::Prepared, State::Submitted]);
    let marked = post_value(
        &p,
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
        post_value(&p, "/api/mark", serde_json::json!({ "state": "rejected" })).status,
        400
    );
}

#[test]
fn a_mark_is_undone_from_the_panel_while_the_card_offers_it() {
    let (p, _d) = panel();
    let key = add(&p, "1", 80.0, &[State::Prepared, State::Submitted]);
    let post = |route: &str, body: Value| post_value(&p, route, body);
    let detail = || {
        let path = format!("/api/job?key={}", encode(&key));
        json_of(&handle(&p, &guard(), &request("GET", &path, "")))["card"].clone()
    };
    let on_board = || {
        let snap = json_of(&handle(&p, &guard(), &request("GET", "/api/snapshot", "")));
        snap["board"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|column| column["cards"].as_array().unwrap().clone())
            .find(|card| card["key"] == key.as_str())
            .unwrap()
    };
    assert_eq!(
        detail()["undo"],
        Value::Null,
        "nothing to undo before a mark"
    );

    let marked = post(
        "/api/mark",
        serde_json::json!({ "key": key, "state": "rejected" }),
    );
    assert_eq!(marked.status, 200, "{}", text(&marked));
    let offered = serde_json::json!({
        "from": "submitted",
        "to": "rejected",
        "at": "2026-10-01T12:00:00Z",
        "until": "2026-10-01T12:10:00Z",
    });
    assert_eq!(detail()["undo"], offered);
    assert_eq!(on_board()["undo"], offered);

    let undone = post("/api/mark/undo", serde_json::json!({ "key": key }));
    assert_eq!(undone.status, 200, "{}", text(&undone));
    assert_eq!(
        json_of(&undone)["message"],
        format!("Deshecho: {key} vuelve a submitted")
    );
    assert_eq!(detail()["state"], "submitted");
    assert_eq!(on_board()["undo"], Value::Null);

    let again = post("/api/mark/undo", serde_json::json!({ "key": key }));
    assert_eq!(again.status, 409);
    assert_eq!(
        json_of(&again)["message"],
        "No hay una marca reciente que deshacer"
    );
    assert_eq!(post("/api/mark/undo", serde_json::json!({})).status, 400);
}

#[test]
fn snooze_takes_a_card_out_of_the_queues_and_unsnooze_brings_it_back() {
    let (p, _d) = panel();
    let key = add(&p, "1", 80.0, &[State::NeedsGonza]);
    let snapshot = || json_of(&handle(&p, &guard(), &request("GET", "/api/snapshot", "")));
    let post = |route: &str, body: Value| post_value(&p, route, body);
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
    let post = |body: &str| api_post(&p, "/api/inbox/import", body);

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
    for task_text in ["Responder a Clara", "Grabar el video"] {
        task(&p, task_text, None, None);
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

/// The smallest `criteria.toml` the panel accepts, with the threshold from which it queues.
const CRITERIA_QUEUING_FROM_60: &str = r#"
floor_monthly_usd = 1
target_monthly_usd = [2, 3]
apply_threshold = 60
daily_cap = 1
daily_cap_until_reviewed = 1
company_cooldown_days = 1
first_n_reviewed = 1

[weights]
fit = 50
compensation = 10
stability = 10
learning = 10
interview_style = 10
other = 10
"#;

#[test]
fn settings_keep_the_auto_approval_threshold_and_refuse_what_is_not_valid() {
    // The profile this test writes queues from 60: its own number, not the repo's.
    let (mut p, d) = panel();
    let profile = d.path().join("profile-under-test");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("criteria.toml"), CRITERIA_QUEUING_FROM_60).unwrap();
    p.paths.profile_dir = profile;
    let snapshot_value = || {
        json_of(&handle(&p, &guard(), &request("GET", "/api/snapshot", "")))["controls"]
            ["auto_approve_min"]
            .clone()
    };
    assert!(snapshot_value().is_null());

    let ok = api_post(&p, "/api/settings", r#"{"auto_approve_min": 90}"#);
    assert_eq!(ok.status, 200);
    let body = json_of(&ok);
    assert_eq!(body["ok"], true);
    assert_eq!(body["auto_approve_min"], 90);
    assert!(body["message"].as_str().unwrap().contains("90"));
    assert_eq!(snapshot_value(), 90, "the snapshot shows what was kept");

    // Out of range is a refusal of the action: 409, nothing changes, and it says the range.
    for value in ["59", "101", "0", "-1"] {
        let refused = api_post(
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
        let reply = api_post(&p, "/api/settings", bad);
        assert_eq!(reply.status, 400, "{bad}");
        assert_eq!(json_of(&reply)["ok"], false, "{bad}");
    }
    assert_eq!(snapshot_value(), 90, "none of those changed anything");

    // null turns it off, twice is fine, and the whole range edge is accepted.
    for _ in 0..2 {
        let off = api_post(&p, "/api/settings", r#"{"auto_approve_min": null}"#);
        assert_eq!(off.status, 200);
        assert!(json_of(&off)["auto_approve_min"].is_null());
    }
    assert!(snapshot_value().is_null());
    for edge in [60, 100] {
        let reply = api_post(
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
    let refused = api_post(&p, "/api/settings", r#"{"auto_approve_min": 90}"#);
    assert_eq!(refused.status, 409);
    assert!(
        json_of(&refused)["message"]
            .as_str()
            .unwrap()
            .contains("criteria.toml")
    );
    assert_eq!(
        api_post(&p, "/api/settings", r#"{"auto_approve_min": null}"#).status,
        200
    );
}

#[test]
fn the_date_the_page_sends_is_read_in_any_rfc_3339_shape_and_stored_as_the_same_utc_second() {
    let sent_at = jobbot_testkit::utc_at(2026, 10, 1, 9, 0, 0);
    for text_sent in [
        "2026-10-01T09:00:00Z",
        "2026-10-01T09:00:00+00:00",
        "2026-10-01T06:00:00-03:00",
        "2026-10-01T14:30:00+05:30",
        " 2026-10-01T09:00:00Z\n",
        "2026-10-01T09:00:00.987654321Z",
    ] {
        // A panel for each text: the one application it holds is the one this text recorded.
        let (p, _d) = panel();
        let reply = post_value(
            &p,
            "/api/record",
            serde_json::json!({
                "company": "Acme",
                "title": "Backend Engineer",
                "url": "https://jobs.example.com/acme/1",
                "at": text_sent,
            }),
        );
        assert_eq!(reply.status, 200, "{text_sent:?}: {}", text(&reply));
        let submitted = p.tracker.list_by_state(State::Submitted).unwrap();
        assert_eq!(submitted.len(), 1, "{text_sent:?}");
        assert_eq!(
            submitted[0].submitted_at,
            Some(sent_at),
            "{text_sent:?} is stored to the second"
        );
    }
}

#[test]
fn a_date_in_the_future_is_refused_with_the_moment_written_as_utc_with_a_plus_zero_offset() {
    let (p, _d) = panel();
    let refused = |at: &str| {
        let reply = post_value(
            &p,
            "/api/record",
            serde_json::json!({
                "company": "Acme", "title": "Backend", "url": "https://jobs.example.com/a", "at": at,
            }),
        );
        assert_eq!(reply.status, 409, "{at}");
        json_of(&reply)["message"].as_str().unwrap().to_string()
    };
    // The clock is 2026-10-01T12:00:00Z and the page may be five minutes ahead of it.
    assert_eq!(
        refused("2026-10-01T13:00:00Z"),
        "La fecha de envío está en el futuro (2026-10-01T13:00:00+00:00): una postulación no se registra antes de enviarse"
    );
    assert_eq!(
        refused("2026-10-01T10:00:00.5-04:00"),
        "La fecha de envío está en el futuro (2026-10-01T14:00:00.500+00:00): una postulación no se registra antes de enviarse",
        "the offset is gone and a fraction shows its three digits"
    );
}

#[test]
fn a_snooze_to_monday_ends_at_midnight_in_santiago_and_the_page_gets_it_in_utc() {
    let (p, _d) = panel();
    let key = add(&p, "1", 80.0, &[State::Prepared]);
    let reply = post_value(
        &p,
        "/api/snooze",
        serde_json::json!({ "key": key, "until": "monday" }),
    );
    assert_eq!(reply.status, 200, "{}", text(&reply));
    // NOW is Thursday 2026-10-01 12:00Z; the next Monday starts at 00:00 Santiago, UTC-3 in October.
    assert_eq!(
        json_of(&reply)["message"],
        format!("Pospuesta hasta 2026-10-05 00:00: {key}")
    );
    let snapshot = json_of(&handle(&p, &guard(), &request("GET", "/api/snapshot", "")));
    assert_eq!(
        snapshot["snoozed"]["cards"][0]["snoozed_until"],
        "2026-10-05T03:00:00Z"
    );
}
