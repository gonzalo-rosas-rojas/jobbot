//! Tests of the guards walked over every route: Host, Origin, token, JSON content type and size, the
//! 404 and 405 of what is not a route, and the security headers of every reply.

use super::support_tests::{
    LAUNCH, approved, guard, json_of, panel_whose_clock_can_panic, request, text, with, without,
};
use super::*;
use crate::fixtures::{add, capture, panel};
use jobbot_domain::state::State;

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
        let early = refuse_early(&p, &g, &req)
            .unwrap_or_else(|| panic!("{} {} is refused from its head", req.method, req.target));
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
    capture(&p, &key);
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
    // A route that is not in the lists is not there, whatever the `match` in `route` says.
    assert_eq!(
        handle(&p, &guard(), &request("POST", "/api/not-a-route", "{}")).status,
        404
    );
}

#[test]
fn a_post_must_be_json_so_a_hostile_form_cannot_send_one() {
    let (p, _d) = panel();
    let key = add(&p, "1", 80.0, &[State::Prepared]);
    capture(&p, &key);
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
    capture(&p, &key);
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
fn a_panic_in_a_handler_is_a_500_and_the_next_request_is_answered() {
    use std::sync::atomic::Ordering;
    let (p, _d, calls, panic_at) = panel_whose_clock_can_panic();
    panic_at.store(calls.load(Ordering::SeqCst), Ordering::SeqCst);
    let reply = handle(&p, &guard(), &request("GET", "/api/snapshot", ""));
    assert_eq!(reply.status, 500);
    assert_eq!(json_of(&reply)["message"], "Error interno", "generic");
    panic_at.store(usize::MAX, Ordering::SeqCst);
    let next = handle(&p, &guard(), &request("GET", "/api/snapshot", ""));
    assert_eq!(next.status, 200, "{}", text(&next));
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
