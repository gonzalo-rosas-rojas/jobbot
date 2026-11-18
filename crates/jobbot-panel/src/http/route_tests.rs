//! Tests of the GET routes that `route` answers with what another module builds: the explorer of
//! postings (`jobs.rs`) and the snapshot.

use super::support_tests::{guard, json_of, request};
use super::*;
use crate::fixtures::{add, panel};
use jobbot_domain::state::State;

/// An authenticated `GET` of an API route.
fn api_get(p: &Panel, target: &str) -> Reply {
    handle(p, &guard(), &request("GET", target, ""))
}

#[test]
fn the_explorer_pages_and_filters_the_jobs() {
    let (p, _d) = panel();
    for n in 0..3u32 {
        add(&p, &n.to_string(), 70.0 + f64::from(n), &[State::Prepared]);
    }
    add(&p, "9", 99.0, &[State::Prepared, State::NeedsGonza]);
    let before = p.tracker.counts().unwrap();
    let get = |target: &str| api_get(&p, target);

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
    assert_eq!(p.tracker.counts().unwrap(), before, "it only reads");
}

#[test]
fn the_explorer_refuses_queries_it_does_not_understand() {
    let (p, _d) = panel();
    let before = p.tracker.counts().unwrap();
    for bad in [
        "/api/jobs?state=nope",
        "/api/jobs?min_score=500",
        "/api/jobs?page=0",
        "/api/jobs?pagina=2",
    ] {
        let reply = api_get(&p, bad);
        assert_eq!(reply.status, 400, "{bad}");
        assert_eq!(json_of(&reply)["ok"], false, "{bad}");
    }
    assert_eq!(
        p.tracker.counts().unwrap(),
        before,
        "a refused query writes nothing"
    );
}

#[test]
fn the_snapshot_carries_today_and_unknown_task_routes_are_404() {
    let (p, _d) = panel();
    p.tracker
        .add_task(
            &jobbot_app::ports::store::NewTask {
                job_key: None,
                kind: "action_required".into(),
                text: "Responder".into(),
                due_at: None,
                source_message_id: None,
                link: None,
            },
            *jobbot_testkit::NOW,
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
