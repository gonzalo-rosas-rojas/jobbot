//! The socket side of the panel: binds 127.0.0.1, turns `tiny_http` requests into `http::Request`
//! values, and writes the `Reply` back. One thread, one request at a time, on purpose: a single
//! person, local calls of a few milliseconds, and the prep store's connection is not shareable.
//!
//! That one thread must never wait for a client. The head of a request is judged before its body
//! is read (a request that will be refused anyway costs nothing), and the body of a POST is read
//! by a short-lived thread of its own that the serving thread waits for only up to a deadline:
//! tiny_http reads the body from the socket on the thread that asks for it and offers no timeout,
//! so a client that sends half a body and stops would otherwise freeze the panel for good.

use std::io::Read;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Result, anyhow};
use tiny_http::{Header, Response, Server};

use super::Panel;
use super::http::{
    Guard, Reply, Request, body_limit, handle, random_hex, refuse_early, with_common_headers,
};

/// How the panel treats the body of a POST.
#[derive(Clone, Copy, Debug)]
pub struct BodyPolicy {
    /// The most the serving thread waits for a body to arrive whole once the head has been
    /// accepted. A body of a few hundred bytes from the page takes microseconds.
    pub deadline: Duration,
    /// The most body-reading threads alive at once. A client that stalls leaves its thread
    /// waiting on the socket (a blocked read cannot be cancelled), so they are counted: past this
    /// number a POST is turned away at once instead of starting one more.
    pub max_readers: usize,
}

impl Default for BodyPolicy {
    fn default() -> Self {
        BodyPolicy {
            deadline: Duration::from_secs(5),
            max_readers: 4,
        }
    }
}

pub struct PanelServer {
    addr: SocketAddr,
    token: String,
    launch_id: String,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl PanelServer {
    /// Binds `127.0.0.1:port` (0: any free port) and serves from a thread of its own.
    pub fn start(panel: Panel, port: u16) -> Result<PanelServer> {
        PanelServer::start_with(panel, port, BodyPolicy::default())
    }

    /// `start` with its own `BodyPolicy` (the tests use a short deadline).
    pub fn start_with(panel: Panel, port: u16, policy: BodyPolicy) -> Result<PanelServer> {
        let server = Server::http(("127.0.0.1", port))
            .map_err(|e| anyhow!("no pude abrir el puerto {port} en 127.0.0.1: {e}"))?;
        let addr = server
            .server_addr()
            .to_ip()
            .ok_or_else(|| anyhow!("el servidor del panel no tiene dirección IP"))?;
        let token = random_hex(16)?;
        // Not a secret, only this launch's cache key for the assets: shorter, and never the token.
        let launch_id = random_hex(8)?;
        let guard = Guard {
            port: addr.port(),
            token: token.clone(),
            launch_id: launch_id.clone(),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let thread = std::thread::Builder::new()
            .name("panel".to_string())
            .spawn({
                let stop = stop.clone();
                move || serve(&server, &panel, &guard, policy, &stop)
            })?;
        Ok(PanelServer {
            addr,
            token,
            launch_id,
            stop,
            thread: Some(thread),
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// The segment under which this launch serves its assets (`/a/<launch-id>/<name>`).
    pub fn launch_id(&self) -> &str {
        &self.launch_id
    }

    /// The address to open: the token travels in the query, and the page moves it out of the
    /// address bar as soon as it has read it.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/?t={}", self.addr.port(), self.token)
    }

    /// Blocks until the serving thread ends, which in practice means until the process is killed.
    pub fn wait(mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    /// Stops the loop and waits for the thread, which closes the socket.
    pub fn shutdown(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for PanelServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn serve(server: &Server, panel: &Panel, guard: &Guard, policy: BodyPolicy, stop: &AtomicBool) {
    let readers = Arc::new(AtomicUsize::new(0));
    while !stop.load(Ordering::Relaxed) {
        match server.recv_timeout(Duration::from_millis(200)) {
            Ok(Some(request)) => answer(panel, guard, policy, &readers, request),
            Ok(None) => {}
            Err(_) => break,
        }
    }
}

/// What came of reading a body.
enum BodyRead {
    /// All of it, and the request to answer.
    Whole(tiny_http::Request, Vec<u8>),
    /// More than the route allows (declared, or found while reading).
    TooBig(tiny_http::Request),
    /// Too many readers are stuck already.
    Busy(tiny_http::Request),
    /// The client went quiet or left: the thread that was reading owns the request and ends it.
    Late,
}

/// What the reading thread hands back.
enum Done {
    Whole(Vec<u8>),
    TooBig,
    /// The connection broke: there is nobody to answer.
    Broken,
}

/// Reads the body of `request` on a thread of its own and waits for it at most `policy.deadline`.
/// A body that declares more than `limit` is not read at all; one that does not declare its size
/// is read up to one byte past the limit, which is enough to tell.
fn read_body(
    request: tiny_http::Request,
    limit: usize,
    policy: BodyPolicy,
    readers: &Arc<AtomicUsize>,
) -> BodyRead {
    if request.body_length().is_some_and(|n| n > limit) {
        return BodyRead::TooBig(request);
    }
    if readers.fetch_add(1, Ordering::SeqCst) >= policy.max_readers {
        readers.fetch_sub(1, Ordering::SeqCst);
        return BodyRead::Busy(request);
    }
    let (sender, receiver) = mpsc::channel::<(tiny_http::Request, Done)>();
    let alive = readers.clone();
    let spawned = std::thread::Builder::new()
        .name("panel-body".to_string())
        .spawn(move || {
            let mut request = request;
            let mut body = Vec::new();
            let done = match request
                .as_reader()
                .take(limit as u64 + 1)
                .read_to_end(&mut body)
            {
                Ok(_) if body.len() <= limit => Done::Whole(body),
                Ok(_) => Done::TooBig,
                Err(_) => Done::Broken,
            };
            alive.fetch_sub(1, Ordering::SeqCst);
            // Nobody waits any more (the deadline passed): the answer is this thread's to give.
            if let Err(mpsc::SendError((request, done))) = sender.send((request, done))
                && !matches!(done, Done::Broken)
            {
                send_reply(request, late_reply());
            }
        });
    if spawned.is_err() {
        // The request went into the closure and is gone; tiny_http answers 500 for it.
        readers.fetch_sub(1, Ordering::SeqCst);
        return BodyRead::Late;
    }
    match receiver.recv_timeout(policy.deadline) {
        Ok((request, Done::Whole(body))) => BodyRead::Whole(request, body),
        Ok((request, Done::TooBig)) => BodyRead::TooBig(request),
        Ok((_, Done::Broken)) | Err(_) => BodyRead::Late,
    }
}

fn late_reply() -> Reply {
    with_common_headers(Reply::message(
        408,
        "El cuerpo de la petición no llegó a tiempo",
    ))
}

fn answer(
    panel: &Panel,
    guard: &Guard,
    policy: BodyPolicy,
    readers: &Arc<AtomicUsize>,
    request: tiny_http::Request,
) {
    let method = request.method().as_str().to_string();
    let target = request.url().to_string();
    let headers: Vec<(String, String)> = request
        .headers()
        .iter()
        .map(|h| (h.field.to_string(), h.value.to_string()))
        .collect();
    let head = Request {
        method,
        target,
        headers,
        body: Vec::new(),
    };
    // What the head alone can refuse (Host, token, Origin, route, method, content type) is
    // refused before a byte of the body is read.
    if let Some(refusal) = refuse_early(panel, guard, &head) {
        send_reply(request, refusal);
        return;
    }
    if head.method != "POST" {
        send_reply(request, handle(panel, guard, &head));
        return;
    }
    match read_body(request, body_limit(&head.target), policy, readers) {
        BodyRead::Whole(request, body) => {
            let reply = handle(panel, guard, &Request { body, ..head });
            send_reply(request, reply);
        }
        BodyRead::TooBig(request) => send_reply(
            request,
            with_common_headers(Reply::message(413, "Cuerpo demasiado grande")),
        ),
        BodyRead::Busy(request) => send_reply(
            request,
            with_common_headers(Reply::message(
                503,
                "Hay peticiones a medias sin terminar: intenta de nuevo en un momento",
            )),
        ),
        BodyRead::Late => {}
    }
}

fn send_reply(request: tiny_http::Request, reply: Reply) {
    let mut response = Response::from_data(reply.body).with_status_code(reply.status);
    if let Ok(header) = Header::from_bytes(&b"Content-Type"[..], reply.content_type.as_bytes()) {
        response.add_header(header);
    }
    for (name, value) in &reply.headers {
        if let Ok(header) = Header::from_bytes(name.as_bytes(), value.as_bytes()) {
            response.add_header(header);
        }
    }
    let _ = request.respond(response);
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    use super::*;
    use crate::paths::Paths;
    use crate::testing::{NOW, make_job_with};
    use crate::tracker::{State, Tracker};

    struct Wire {
        status: u16,
        headers: Vec<(String, String)>,
        body: String,
    }

    impl Wire {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        }
    }

    /// One raw HTTP/1.1 request on a fresh connection; the server closes it after the reply.
    fn send(addr: SocketAddr, request: &str) -> Wire {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut raw = Vec::new();
        let _ = stream.read_to_end(&mut raw);
        let text = String::from_utf8_lossy(&raw).into_owned();
        let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
        let mut lines = head.lines();
        let status = lines
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| panic!("no status line in {text:?}"));
        let headers = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
            .collect();
        Wire {
            status,
            headers,
            body: body.to_string(),
        }
    }

    fn raw(method: &str, target: &str, host: &str, extra: &[(&str, &str)], body: &str) -> String {
        let mut text =
            format!("{method} {target} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
        for (name, value) in extra {
            text.push_str(&format!("{name}: {value}\r\n"));
        }
        text.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
        text
    }

    struct Env {
        _dir: tempfile::TempDir,
        paths: Paths,
        server: PanelServer,
    }

    impl Env {
        /// A server over a real database in a temp folder holding two prepared applications.
        fn start() -> Env {
            Env::start_with(BodyPolicy::default())
        }

        fn start_with(policy: BodyPolicy) -> Env {
            let dir = tempfile::tempdir().unwrap();
            let paths = Paths::new(dir.path());
            let panel = Panel::open(paths.clone(), Box::new(|| *NOW)).unwrap();
            for id in ["1", "2"] {
                let job = make_job_with(|j| j.external_id = id.into());
                panel
                    .tracker
                    .add_job(&job, 80.0, &Default::default(), *NOW)
                    .unwrap();
                panel.tracker.create_application(&job.key(), *NOW).unwrap();
                panel
                    .tracker
                    .transition(&job.key(), State::Prepared, None, *NOW)
                    .unwrap();
            }
            let server = PanelServer::start_with(panel, 0, policy).unwrap();
            Env {
                _dir: dir,
                paths,
                server,
            }
        }

        fn host(&self) -> String {
            format!("127.0.0.1:{}", self.server.addr().port())
        }

        fn json_post(&self, target: &str, body: &str, extra: &[(&str, &str)]) -> Wire {
            let mut headers = vec![
                ("X-Panel-Token", self.server.token()),
                ("Content-Type", "application/json"),
            ];
            headers.extend_from_slice(extra);
            send(
                self.server.addr(),
                &raw("POST", target, &self.host(), &headers, body),
            )
        }

        fn approved(&self, key: &str) -> bool {
            Tracker::open(&self.paths.data_dir.join("jobbot.db"))
                .unwrap()
                .get_application(key)
                .unwrap()
                .unwrap()
                .approved
        }
    }

    #[test]
    fn the_server_listens_on_loopback_only_with_a_fresh_token() {
        let a = Env::start();
        let b = Env::start();
        assert!(a.server.addr().ip().is_loopback());
        assert_ne!(a.server.addr().port(), 0);
        assert_eq!(a.server.token().len(), 32);
        assert_ne!(a.server.token(), b.server.token());
        assert_eq!(
            a.server.url(),
            format!(
                "http://127.0.0.1:{}/?t={}",
                a.server.addr().port(),
                a.server.token()
            )
        );
    }

    #[test]
    fn the_page_needs_no_token_but_the_api_does() {
        let env = Env::start();
        let addr = env.server.addr();
        let page = send(addr, &raw("GET", "/", &env.host(), &[], ""));
        assert_eq!(page.status, 200);
        assert!(
            page.header("content-type")
                .unwrap()
                .starts_with("text/html")
        );
        assert!(page.header("content-security-policy").is_some());
        let no_token = send(addr, &raw("GET", "/api/snapshot", &env.host(), &[], ""));
        assert_eq!(no_token.status, 401);
        let token = env.server.token().to_string();
        let ok = send(
            addr,
            &raw(
                "GET",
                "/api/snapshot",
                &env.host(),
                &[("X-Panel-Token", token.as_str())],
                "",
            ),
        );
        assert_eq!(ok.status, 200);
        let json: serde_json::Value = serde_json::from_str(&ok.body).unwrap();
        assert_eq!(json["board"].as_array().unwrap().len(), 12);
    }

    #[test]
    fn an_asset_travels_the_wire_under_the_launch_id_and_a_wrong_id_is_404() {
        let env = Env::start();
        let launch = env.server.launch_id().to_string();
        assert_eq!(launch.len(), 16);
        assert_ne!(
            launch,
            env.server.token(),
            "the cache key is not the secret"
        );
        let ok = send(
            env.server.addr(),
            &raw(
                "GET",
                &format!("/a/{launch}/inter-400.woff2"),
                &env.host(),
                &[],
                "",
            ),
        );
        assert_eq!(ok.status, 200);
        assert_eq!(ok.header("content-type"), Some("font/woff2"));
        assert_eq!(
            ok.header("cache-control"),
            Some("public, max-age=31536000, immutable")
        );
        let wrong = send(
            env.server.addr(),
            &raw(
                "GET",
                "/a/0000000000000000/inter-400.woff2",
                &env.host(),
                &[],
                "",
            ),
        );
        assert_eq!(wrong.status, 404);
        let page = send(env.server.addr(), &raw("GET", "/", &env.host(), &[], ""));
        assert_eq!(page.header("x-frame-options"), Some("DENY"));
    }

    #[test]
    fn a_font_travels_the_wire_whole_as_woff2_and_only_under_this_launch() {
        let env = Env::start();
        let launch = env.server.launch_id().to_string();
        // The bytes are read lossily by `send`, so the length on the wire is what proves them whole.
        let sizes = [
            (
                "barlow-condensed-700.woff2",
                include_bytes!("assets/barlow-condensed-700.woff2").len(),
            ),
            (
                "geist-mono-400.woff2",
                include_bytes!("assets/geist-mono-400.woff2").len(),
            ),
            (
                "instrument-serif-400italic.woff2",
                include_bytes!("assets/instrument-serif-400italic.woff2").len(),
            ),
            (
                "inter-400.woff2",
                include_bytes!("assets/inter-400.woff2").len(),
            ),
            (
                "inter-600.woff2",
                include_bytes!("assets/inter-600.woff2").len(),
            ),
        ];
        for (name, size) in sizes {
            let ok = send(
                env.server.addr(),
                &raw("GET", &format!("/a/{launch}/{name}"), &env.host(), &[], ""),
            );
            assert_eq!(ok.status, 200, "{name}");
            assert_eq!(ok.header("content-type"), Some("font/woff2"), "{name}");
            assert_eq!(
                ok.header("cache-control"),
                Some("public, max-age=31536000, immutable"),
                "{name}"
            );
            assert_eq!(
                ok.header("content-length").map(str::to_string),
                Some(size.to_string()),
                "{name}"
            );
            let wrong = send(
                env.server.addr(),
                &raw(
                    "GET",
                    &format!("/a/0000000000000000/{name}"),
                    &env.host(),
                    &[],
                    "",
                ),
            );
            assert_eq!(wrong.status, 404, "{name}");
        }
    }

    #[test]
    fn a_rebinding_host_is_refused_on_the_wire() {
        let env = Env::start();
        let port = env.server.addr().port();
        for host in [
            format!("evil.example:{port}"),
            "127.0.0.1:1".to_string(),
            format!("[::1]:{port}"),
            format!("evil.localhost:{port}"),
            format!("localhost.:{port}"),
        ] {
            let reply = send(env.server.addr(), &raw("GET", "/", &host, &[], ""));
            assert_eq!(reply.status, 403, "{host}");
        }
    }

    #[test]
    fn the_panel_is_not_reachable_over_ipv6_loopback() {
        let env = Env::start();
        let port = env.server.addr().port();
        assert!(env.server.addr().is_ipv4(), "bound to 127.0.0.1 only");
        let v6 = std::net::SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port));
        assert!(
            std::net::TcpStream::connect_timeout(&v6, std::time::Duration::from_millis(500))
                .is_err(),
            "nothing listens on [::1]:{port}"
        );
    }

    #[test]
    fn an_approval_lands_in_sqlite_and_a_hostile_post_does_not() {
        let env = Env::start();
        let own = format!("http://{}", env.host());
        let ok = env.json_post(
            "/api/approve",
            r#"{"key":"test:1"}"#,
            &[("Origin", own.as_str())],
        );
        assert_eq!(ok.status, 200, "{}", ok.body);
        assert!(
            env.approved("test:1"),
            "what the page marks is in SQLite when the reply arrives"
        );

        let token = env.server.token().to_string();
        let form = send(
            env.server.addr(),
            &raw(
                "POST",
                "/api/approve",
                &env.host(),
                &[
                    ("X-Panel-Token", token.as_str()),
                    ("Content-Type", "text/plain"),
                ],
                r#"{"key":"test:2"}"#,
            ),
        );
        assert_eq!(form.status, 415);
        let foreign = env.json_post(
            "/api/approve",
            r#"{"key":"test:2"}"#,
            &[("Origin", "https://evil.example")],
        );
        assert_eq!(foreign.status, 403);
        assert!(
            !env.approved("test:2"),
            "neither hostile request changed anything"
        );
    }

    #[test]
    fn a_declared_huge_body_is_refused_without_reading_it_and_the_server_keeps_serving() {
        let env = Env::start();
        let token = env.server.token().to_string();
        let head = format!(
            "POST /api/approve HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nX-Panel-Token: {token}\r\n\
             Content-Type: application/json\r\nContent-Length: 10000000\r\n\r\n{{}}",
            env.host()
        );
        assert_eq!(send(env.server.addr(), &head).status, 413);
        let after = send(
            env.server.addr(),
            &raw(
                "GET",
                "/api/snapshot",
                &env.host(),
                &[("X-Panel-Token", token.as_str())],
                "",
            ),
        );
        assert_eq!(after.status, 200);
    }

    // tiny_http keeps a request back until a body of up to 1024 bytes has arrived whole, on a
    // thread of its own, so only a bigger declared body can hold the serving thread: these tests
    // declare 5000 bytes (the limit of a route is 16 KiB).
    const DECLARED: usize = 5000;

    /// The status the server answers to `head` alone (it promised a body that never comes), or 0
    /// when it only waits: what a panel that reads the body first would do.
    fn status_without_a_body(addr: SocketAddr, head: &str) -> u16 {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream.write_all(head.as_bytes()).unwrap();
        status_of(&mut stream)
    }

    /// The status line of whatever the server writes on `stream`; 0 when nothing comes in time.
    fn status_of(stream: &mut TcpStream) -> u16 {
        let mut seen = Vec::new();
        let mut chunk = [0u8; 512];
        while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => seen.extend_from_slice(&chunk[..n]),
            }
        }
        String::from_utf8_lossy(&seen)
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }

    #[test]
    fn a_request_the_head_already_refuses_is_answered_without_waiting_for_its_body() {
        let env = Env::start();
        let token = env.server.token().to_string();
        let host = env.host();
        let json = "Content-Type: application/json\r\n";
        let good = format!("X-Panel-Token: {token}\r\n");
        let post = |target: &str, extra: &str| {
            format!(
                "POST {target} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n{extra}\
                 Content-Length: {DECLARED}\r\n\r\n"
            )
        };
        for (what, head, want) in [
            ("no token", post("/api/approve", json), 401),
            (
                "wrong host",
                post("/api/approve", &format!("{good}{json}")).replace(&host, "evil.example:1"),
                403,
            ),
            (
                "foreign origin",
                post(
                    "/api/approve",
                    &format!("{good}{json}Origin: https://evil.example\r\n"),
                ),
                403,
            ),
            (
                "not json",
                post(
                    "/api/approve",
                    &format!("{good}Content-Type: text/plain\r\n"),
                ),
                415,
            ),
            (
                "no such route",
                post("/api/nothing", &format!("{good}{json}")),
                404,
            ),
        ] {
            assert_eq!(
                status_without_a_body(env.server.addr(), &head),
                want,
                "{what}"
            );
        }
    }

    #[test]
    fn a_post_that_stops_halfway_does_not_freeze_the_panel_and_is_told_so_when_it_resumes() {
        let env = Env::start_with(BodyPolicy {
            deadline: Duration::from_millis(300),
            max_readers: 4,
        });
        let token = env.server.token().to_string();
        let mut stalled = TcpStream::connect(env.server.addr()).unwrap();
        stalled
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        // Right head, right token, a body of 5000 bytes of which 2000 arrive.
        let sent = format!("{{\"key\":\"{}", "x".repeat(1992));
        stalled
            .write_all(
                format!(
                    "POST /api/approve HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\
                     X-Panel-Token: {token}\r\nContent-Type: application/json\r\n\
                     Content-Length: {DECLARED}\r\n\r\n{sent}",
                    env.host()
                )
                .as_bytes(),
            )
            .unwrap();

        // Meanwhile somebody else is served, in a few seconds at most.
        let started = std::time::Instant::now();
        std::thread::sleep(Duration::from_millis(100));
        let other = send(
            env.server.addr(),
            &raw(
                "GET",
                "/api/snapshot",
                &env.host(),
                &[("X-Panel-Token", token.as_str())],
                "",
            ),
        );
        assert_eq!(other.status, 200);
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "served after {:?}",
            started.elapsed()
        );

        // It comes back only after the deadline, and finds out: nothing was done for it.
        std::thread::sleep(Duration::from_millis(600).saturating_sub(started.elapsed()));
        stalled
            .write_all(&vec![b' '; DECLARED - sent.len()])
            .unwrap();
        assert_eq!(status_of(&mut stalled), 408);
        assert!(!env.approved("test:1") && !env.approved("test:2"));
    }

    #[test]
    fn stalled_readers_are_capped_and_the_panel_serves_again_when_they_end() {
        let env = Env::start_with(BodyPolicy {
            deadline: Duration::from_millis(200),
            max_readers: 1,
        });
        let token = env.server.token().to_string();
        let mut stalled = TcpStream::connect(env.server.addr()).unwrap();
        stalled
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stalled
            .write_all(
                format!(
                    "POST /api/approve HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\
                     X-Panel-Token: {token}\r\nContent-Type: application/json\r\n\
                     Content-Length: {DECLARED}\r\n\r\n{{\"key\":\"{}",
                    env.host(),
                    "x".repeat(1992)
                )
                .as_bytes(),
            )
            .unwrap();
        // Past its deadline the first one still holds its place: the next POST is turned away
        // at once instead of starting one more thread that could wait for ever.
        std::thread::sleep(Duration::from_millis(500));
        let turned_away = env.json_post("/api/approve", r#"{"key":"test:1"}"#, &[]);
        assert_eq!(turned_away.status, 503, "{}", turned_away.body);
        assert!(!env.approved("test:1"));
        // It ends (the client closes), the place is free again.
        drop(stalled);
        std::thread::sleep(Duration::from_millis(300));
        let served = env.json_post("/api/approve", r#"{"key":"test:1"}"#, &[]);
        assert_eq!(served.status, 200, "{}", served.body);
        assert!(env.approved("test:1"));
    }

    #[test]
    fn shutdown_stops_the_thread_and_frees_the_port() {
        let env = Env::start();
        let addr = env.server.addr();
        env.server.shutdown();
        assert!(
            TcpStream::connect(addr).is_err(),
            "nothing listens after shutdown"
        );
    }

    #[test]
    fn a_port_in_use_is_a_clear_error() {
        let env = Env::start();
        let dir = tempfile::tempdir().unwrap();
        let panel = Panel::open(Paths::new(dir.path()), Box::new(|| *NOW)).unwrap();
        let err = PanelServer::start(panel, env.server.addr().port())
            .err()
            .expect("the port is taken");
        assert!(
            err.to_string().contains("no pude abrir el puerto"),
            "{err:#}"
        );
    }
}
