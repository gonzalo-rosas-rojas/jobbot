//! What the tests of `http` and its children share: a request to the panel as the page would
//! send it, the guard it is answered under and the readers of a reply.

use super::*;

const PORT: u16 = 4321;
pub(super) const TOKEN: &str = "tok-0123456789abcdef";
pub(super) const LAUNCH: &str = "f00ba4f00ba4f00b";

pub(super) fn guard() -> Guard {
    Guard {
        port: PORT,
        token: TOKEN.to_string(),
        launch_id: LAUNCH.to_string(),
    }
}

/// A well-formed request: right Host, right token, JSON content type.
pub(super) fn request(method: &str, target: &str, body: &str) -> Request {
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

pub(super) fn without(mut req: Request, name: &str) -> Request {
    req.headers.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
    req
}

pub(super) fn with(req: Request, name: &str, value: &str) -> Request {
    let mut req = without(req, name);
    req.headers.push((name.to_string(), value.to_string()));
    req
}

pub(super) fn text(reply: &Reply) -> String {
    String::from_utf8(reply.body.clone()).unwrap()
}

pub(super) fn json_of(reply: &Reply) -> Value {
    serde_json::from_slice(&reply.body).unwrap()
}

pub(super) fn approved(p: &Panel, key: &str) -> bool {
    p.tracker.get_application(key).unwrap().unwrap().approved
}

pub(super) fn encode(key: &str) -> String {
    url::form_urlencoded::byte_serialize(key.as_bytes()).collect()
}

/// A panel whose clock panics at the call numbered `*panic_at` (counting from 0), and
/// never while `panic_at` is `usize::MAX`: the way to make a handler panic from outside.
pub(super) fn panel_whose_clock_can_panic() -> (
    Panel,
    tempfile::TempDir,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let dir = tempfile::tempdir().unwrap();
    let mut paths = jobbot_app::paths::Paths::new(dir.path());
    paths.profile_dir = jobbot_testkit::real_paths().profile_dir;
    let calls = Arc::new(AtomicUsize::new(0));
    let panic_at = Arc::new(AtomicUsize::new(usize::MAX));
    let clock = {
        let (calls, panic_at) = (calls.clone(), panic_at.clone());
        Box::new(move || {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            if n == panic_at.load(Ordering::SeqCst) {
                panic!("the clock broke");
            }
            *jobbot_testkit::NOW
        })
    };
    let p = Panel::new(
        paths,
        jobbot_testkit::Tracker::in_memory().unwrap(),
        jobbot_testkit::PrepStore::in_memory().unwrap(),
        clock,
    );
    (p, dir, calls, panic_at)
}
