//! The browser's only way out. The filler's and the inspector's Chromium are launched with this
//! proxy and with their own name lookups switched off, so every connection a page makes (the page
//! itself, a redirect, a popup, a frame, a fetch, a form, a WebSocket) arrives here as
//! `CONNECT host:443`. The proxy resolves the name once, refuses it if any answer is not on the
//! public internet (`navigation::is_public_ip`), and connects to the exact address it judged. A name
//! that answers a public address to a check and a private one to the browser (DNS rebinding) gets
//! no second question, and a page cannot reach the router or this machine however it asks.

use std::collections::VecDeque;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use jobbot_domain::form::SubmitResult;
use jobbot_domain::navigation::{ConnectHost, NavRefusal, connect_host, is_public_ip};
use jobbot_domain::outbound::plain_line;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::timeout;

/// The only port a page may reach: jobbot opens https pages only, and wss shares the port.
const HTTPS_PORT: u16 = 443;
/// The longest request head the proxy reads; a CONNECT head is a few hundred bytes.
const HEAD_LIMIT: usize = 8 * 1024;
/// How long the browser has to send its request head, the name lookup to answer and the site to
/// take the connection.
const HEAD_DEADLINE: Duration = Duration::from_secs(10);
const LOOKUP_DEADLINE: Duration = Duration::from_secs(10);
const CONNECT_DEADLINE: Duration = Duration::from_secs(10);
/// How long a connection attempt runs alone before the next judged address is tried too (RFC
/// 8305's connection attempt delay).
const NEXT_ATTEMPT_AFTER: Duration = Duration::from_millis(250);
/// The pause after a failed accept, so a machine out of sockets is not asked again in a hot loop.
const ACCEPT_PAUSE: Duration = Duration::from_millis(50);
/// How many different refusals a session keeps (a page can ask for thousands).
const REFUSALS_KEPT: usize = 100;
/// How many refusals a session prints a notice for before it says it stops.
const NOTICES_PER_SESSION: usize = 20;

/// Turns a name into its addresses. The program uses the system's resolver; the tests write the
/// answers.
#[async_trait]
pub(crate) trait Resolve: Send + Sync {
    async fn lookup(&self, name: &str) -> io::Result<Vec<IpAddr>>;
}

/// The system's resolver, asked once per CONNECT.
pub(crate) struct SystemResolver;

#[async_trait]
impl Resolve for SystemResolver {
    async fn lookup(&self, name: &str) -> io::Result<Vec<IpAddr>> {
        Ok(tokio::net::lookup_host((name, HTTPS_PORT))
            .await?
            .map(|addr| addr.ip())
            .collect())
    }
}

/// A connection the proxy did not make: `blocked` when the address check refused it (answered
/// `403`), otherwise the site could not be reached (answered `502`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub host: String,
    pub why: String,
    pub blocked: bool,
}

impl Refusal {
    fn blocked(host: &str, why: impl Into<String>) -> Refusal {
        Refusal {
            host: host.to_string(),
            why: why.into(),
            blocked: true,
        }
    }

    fn unreachable(host: &str, why: impl Into<String>) -> Refusal {
        Refusal {
            host: host.to_string(),
            why: why.into(),
            blocked: false,
        }
    }
}

/// The refusals of one session, each kept once, the newest `REFUSALS_KEPT`, numbered in the order
/// they last happened.
#[derive(Debug, Clone, Default)]
struct RefusalLog(Arc<Mutex<Numbered>>);

#[derive(Debug, Default)]
struct Numbered {
    next: u64,
    kept: VecDeque<(u64, Refusal)>,
    /// How many refusals got a notice this session.
    noticed: usize,
}

/// What a refusal is worth on stderr.
#[derive(Debug, PartialEq, Eq)]
enum Notice {
    /// The address check refused this host for this reason for the first time.
    Line,
    /// As `Line`, but the session has printed `NOTICES_PER_SESSION`: one last line says so.
    Last,
    Quiet,
}

impl RefusalLog {
    /// Keeps `refusal` as the newest one, and says whether it is worth a notice: only a refusal
    /// of the address check (a site that is down is not news), only once per host and reason,
    /// and only `NOTICES_PER_SESSION` of them.
    fn record(&self, refusal: Refusal) -> Notice {
        let mut log = self.0.lock().unwrap();
        let before = log.kept.len();
        log.kept.retain(|(_, kept)| kept != &refusal);
        let new = log.kept.len() == before;
        if log.kept.len() == REFUSALS_KEPT {
            log.kept.pop_front();
        }
        let number = log.next;
        log.next += 1;
        let blocked = refusal.blocked;
        log.kept.push_back((number, refusal));
        if !new || !blocked {
            return Notice::Quiet;
        }
        log.noticed += 1;
        match log.noticed {
            n if n <= NOTICES_PER_SESSION => Notice::Line,
            n if n == NOTICES_PER_SESSION + 1 => Notice::Last,
            _ => Notice::Quiet,
        }
    }

    /// The number the next refusal gets: what `since` counts from.
    fn mark(&self) -> u64 {
        self.0.lock().unwrap().next
    }

    /// The kept refusals that happened at or after `mark`, oldest first.
    fn since(&self, mark: u64) -> Vec<Refusal> {
        let log = self.0.lock().unwrap();
        log.kept
            .iter()
            .filter(|(number, _)| *number >= mark)
            .map(|(_, refusal)| refusal.clone())
            .collect()
    }
}

/// The proxy of one browser session, listening on `127.0.0.1` from `start` until it is dropped
/// (which also cuts the tunnels it holds open).
pub(crate) struct Egress {
    addr: SocketAddr,
    log: RefusalLog,
    accept: JoinHandle<()>,
}

impl Egress {
    pub(crate) async fn start(resolver: Arc<dyn Resolve>) -> io::Result<Egress> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        let log = RefusalLog::default();
        let accept = tokio::spawn(accept(listener, resolver, log.clone()));
        Ok(Egress { addr, log, accept })
    }

    /// The proxy address to launch Chromium with.
    pub(crate) fn server(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Taken before a page is filled, so `explain_tunnel_failure` reads only what that fill was
    /// refused.
    pub(crate) fn mark(&self) -> u64 {
        self.log.mark()
    }

    /// What the proxy refused at or after `mark`, oldest first.
    pub(crate) fn refusals_since(&self, mark: u64) -> Vec<Refusal> {
        self.log.since(mark)
    }
}

/// The reason Gonza reads when the page could not open because the proxy refused it. Chromium only
/// says `ERR_TUNNEL_CONNECTION_FAILED`; the proxy knows which host and why. The refusal for the
/// host the error names is preferred, otherwise the newest one since `mark` (a redirect fails with
/// the first address in the message, not the one refused). Any other result is returned as it is.
pub(crate) fn explain_tunnel_failure(
    result: SubmitResult,
    egress: &Egress,
    mark: u64,
) -> SubmitResult {
    let Some(reason) = result.reason.as_deref() else {
        return result;
    };
    if !reason.starts_with("page_error:") || !reason.contains("ERR_TUNNEL_CONNECTION_FAILED") {
        return result;
    }
    // `net::ERR_…` parses as a URL too; the address is the first word with a host.
    let named = reason.split_whitespace().find_map(|word| {
        url::Url::parse(word)
            .ok()
            .and_then(|url| url.host_str().map(str::to_string))
    });
    let fresh = egress.refusals_since(mark);
    let refusal = named
        .and_then(|host| fresh.iter().rev().find(|refusal| refusal.host == host))
        .or_else(|| fresh.last());
    let Some(refusal) = refusal else {
        return result;
    };
    let reason = if refusal.blocked {
        format!(
            "unsafe_url: the address check refused {} ({})",
            refusal.host, refusal.why
        )
    } else {
        format!(
            "page_error: {} could not be reached ({})",
            refusal.host, refusal.why
        )
    };
    SubmitResult {
        reason: Some(reason),
        ..result
    }
}

impl Drop for Egress {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

async fn accept(listener: TcpListener, resolver: Arc<dyn Resolve>, log: RefusalLog) {
    // Dropped with this task when the session ends, which aborts every tunnel still open.
    let mut connections = JoinSet::new();
    loop {
        while connections.try_join_next().is_some() {}
        match listener.accept().await {
            Ok((client, _)) => {
                connections.spawn(serve(client, Arc::clone(&resolver), log.clone()));
            }
            Err(_) => tokio::time::sleep(ACCEPT_PAUSE).await,
        }
    }
}

async fn serve(mut client: TcpStream, resolver: Arc<dyn Resolve>, log: RefusalLog) {
    let (head, rest) = match read_head(&mut client, HEAD_DEADLINE).await {
        Head::Complete { text, rest } => (text, rest),
        Head::TooLong => {
            let refusal = Refusal::blocked("", "the request head is longer than 8 KiB");
            refuse(&mut client, &log, refusal).await;
            return;
        }
        Head::Closed => return,
    };
    let opened = match parse_connect(&head) {
        Ok((host, port)) => match decide(&host, port, &*resolver).await {
            Ok(addrs) => tunnel(&mut client, &host, &addrs, &rest).await,
            Err(refusal) => Err(refusal),
        },
        Err(refusal) => Err(refusal),
    };
    if let Err(refusal) = opened {
        refuse(&mut client, &log, refusal).await;
    }
}

/// What reading a request head gave.
#[derive(Debug)]
enum Head {
    /// The head up to its blank line, and the bytes that came after it.
    Complete {
        text: String,
        rest: Vec<u8>,
    },
    TooLong,
    /// The browser closed, failed or took longer than the deadline.
    Closed,
}

async fn read_head<R: AsyncRead + Unpin>(stream: &mut R, deadline: Duration) -> Head {
    let read = async {
        let mut buf = Vec::with_capacity(1024);
        let mut chunk = [0u8; 1024];
        loop {
            if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let rest = buf.split_off(end + 4);
                let text = String::from_utf8_lossy(&buf).into_owned();
                return Head::Complete { text, rest };
            }
            if buf.len() > HEAD_LIMIT {
                return Head::TooLong;
            }
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return Head::Closed,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        }
    };
    timeout(deadline, read).await.unwrap_or(Head::Closed)
}

/// The host and port of a `CONNECT host:port HTTP/1.x` head. Any other request is refused, naming
/// only the host it was for (a plain-http link's path may carry a token).
fn parse_connect(head: &str) -> Result<(String, u16), Refusal> {
    let line = head.lines().next().unwrap_or_default();
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Refusal::blocked("", "not an HTTP request"));
    };
    if !version.starts_with("HTTP/1.") {
        return Err(Refusal::blocked("", "not an HTTP/1 request"));
    }
    if method != "CONNECT" {
        let host = url::Url::parse(target)
            .ok()
            .and_then(|url| url.host_str().map(str::to_string))
            .unwrap_or_default();
        return Err(Refusal::blocked(
            &host,
            format!("only CONNECT to port 443 goes through, not {method}"),
        ));
    }
    let Some((host, port)) = target.rsplit_once(':') else {
        return Err(Refusal::blocked(target, "the CONNECT target has no port"));
    };
    match port.parse::<u16>() {
        Ok(number) if port.bytes().all(|b| b.is_ascii_digit()) => Ok((host.to_string(), number)),
        _ => Err(Refusal::blocked(host, format!("`{port}` is not a port"))),
    }
}

/// The addresses a CONNECT to `host:port` may open, or why it may not. An address is judged as it
/// is written; a name is looked up once and refused if any answer is not public.
async fn decide(host: &str, port: u16, resolver: &dyn Resolve) -> Result<Vec<SocketAddr>, Refusal> {
    if port != HTTPS_PORT {
        return Err(Refusal::blocked(
            host,
            format!("port {port}: only 443 goes through"),
        ));
    }
    let name = match connect_host(host) {
        Ok(ConnectHost::Ip(ip)) => return Ok(vec![SocketAddr::new(ip, port)]),
        Ok(ConnectHost::Name(name)) => name,
        Err(refusal @ NavRefusal::PrivateHost(_)) => {
            return Err(Refusal::blocked(host, refusal.to_string()));
        }
        Err(_) => {
            return Err(Refusal::blocked(host, "not a host name or an address"));
        }
    };
    let answers = match timeout(LOOKUP_DEADLINE, resolver.lookup(&name)).await {
        Ok(Ok(answers)) if !answers.is_empty() => answers,
        Ok(Ok(_)) => return Err(Refusal::unreachable(host, "the name has no address")),
        Ok(Err(error)) => {
            return Err(Refusal::unreachable(
                host,
                format!("the name lookup failed: {error}"),
            ));
        }
        Err(_) => {
            return Err(Refusal::unreachable(
                host,
                "the name lookup did not answer in 10 s",
            ));
        }
    };
    let local: Vec<String> = answers
        .iter()
        .filter(|ip| !is_public_ip(**ip))
        .map(ToString::to_string)
        .collect();
    if !local.is_empty() {
        return Err(Refusal::blocked(
            host,
            format!(
                "the name answered {}, a local or private address",
                local.join(", ")
            ),
        ));
    }
    Ok(answers
        .into_iter()
        .map(|ip| SocketAddr::new(ip, port))
        .collect())
}

/// The judged addresses in the order they are tried: the families alternate, starting with the
/// resolver's first answer (RFC 8305, section 4).
fn attempt_order(addrs: &[SocketAddr]) -> Vec<SocketAddr> {
    let Some(first) = addrs.first() else {
        return Vec::new();
    };
    let (mut lead, mut other): (Vec<SocketAddr>, Vec<SocketAddr>) = addrs
        .iter()
        .partition(|addr| addr.is_ipv4() == first.is_ipv4());
    lead.reverse();
    other.reverse();
    let mut order = Vec::with_capacity(addrs.len());
    while !lead.is_empty() || !other.is_empty() {
        order.extend(lead.pop());
        order.extend(other.pop());
    }
    order
}

/// The first connection one of `addrs` takes (Happy Eyeballs, RFC 8305): an attempt starts every
/// `NEXT_ATTEMPT_AFTER`, or as soon as the one before it fails, and the first that connects wins;
/// the others are dropped. An address family that does not route costs that delay, not the whole
/// deadline. `connect` is the system's connect outside the tests.
async fn first_connection<F, Fut>(addrs: &[SocketAddr], connect: F) -> io::Result<TcpStream>
where
    F: Fn(SocketAddr) -> Fut,
    Fut: Future<Output = io::Result<TcpStream>> + Send + 'static,
{
    let mut waiting = attempt_order(addrs).into_iter().peekable();
    let mut attempts = JoinSet::new();
    let mut last_error = io::Error::new(io::ErrorKind::NotFound, "no address to connect to");
    if let Some(addr) = waiting.next() {
        attempts.spawn(connect(addr));
    }
    loop {
        let finished = if waiting.peek().is_some() {
            match timeout(NEXT_ATTEMPT_AFTER, attempts.join_next()).await {
                Ok(finished) => finished,
                Err(_) => {
                    attempts.extend(waiting.next().map(&connect));
                    continue;
                }
            }
        } else {
            attempts.join_next().await
        };
        match finished {
            Some(Ok(Ok(stream))) => return Ok(stream),
            Some(Ok(Err(error))) => last_error = error,
            Some(Err(error)) => last_error = io::Error::other(error),
            None => return Err(last_error),
        }
        attempts.extend(waiting.next().map(&connect));
    }
}

/// Connects to `addrs` (the checked answers, never looked up again), tells the browser the tunnel
/// is open, hands the site the bytes that came after the head, and then carries bytes both ways
/// until either side closes. Only a failure to connect is a refusal.
async fn tunnel(
    client: &mut TcpStream,
    host: &str,
    addrs: &[SocketAddr],
    rest: &[u8],
) -> Result<(), Refusal> {
    let connecting = first_connection(addrs, TcpStream::connect);
    let mut site = match timeout(CONNECT_DEADLINE, connecting).await {
        Ok(Ok(site)) => site,
        Ok(Err(error)) => {
            return Err(Refusal::unreachable(
                host,
                format!("could not connect: {error}"),
            ));
        }
        Err(_) => {
            return Err(Refusal::unreachable(
                host,
                "the site did not take the connection in 10 s",
            ));
        }
    };
    let opened = async {
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
        site.write_all(rest).await?;
        tokio::io::copy_bidirectional(client, &mut site).await
    };
    let _ = opened.await;
    Ok(())
}

/// The `aviso:` line for a refusal of the address check, on one line and cleaned: the host and
/// the reason come from the page.
fn notice_line(refusal: &Refusal) -> String {
    format!(
        "aviso: el navegador quiso conectarse a {} y la revisión de direcciones lo impidió ({})",
        plain_line(&refusal.host),
        plain_line(&refusal.why)
    )
}

/// Keeps `refusal`, prints the notice it is worth, and answers the browser. A notice is the only
/// trace of a page that reached for this machine or the home network from a frame or a script:
/// the posting's result only shows what happened to the page itself.
async fn refuse(client: &mut TcpStream, log: &RefusalLog, refusal: Refusal) {
    let status = if refusal.blocked {
        "403 Forbidden"
    } else {
        "502 Bad Gateway"
    };
    match log.record(refusal.clone()) {
        Notice::Line => eprintln!("{}", notice_line(&refusal)),
        Notice::Last => {
            eprintln!("{}", notice_line(&refusal));
            eprintln!(
                "aviso: el navegador siguió intentando conectarse a direcciones bloqueadas; \
                 no se muestran más en esta sesión"
            );
        }
        Notice::Quiet => {}
    }
    let answer = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let _ = client.write_all(answer.as_bytes()).await;
    let _ = client.shutdown().await;
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use jobbot_domain::form::SubmitStatus;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::time::timeout;

    use super::*;

    type Answer = dyn Fn(&str, usize) -> io::Result<Vec<IpAddr>> + Send + Sync;

    /// A resolver whose answers the test writes, `answer(name, call)` for the `call`-th lookup
    /// (from 0), and that counts every lookup.
    pub(crate) struct FakeResolver {
        answer: Box<Answer>,
        asked: AtomicUsize,
    }

    impl FakeResolver {
        pub(crate) fn new(
            answer: impl Fn(&str, usize) -> io::Result<Vec<IpAddr>> + Send + Sync + 'static,
        ) -> FakeResolver {
            FakeResolver {
                answer: Box::new(answer),
                asked: AtomicUsize::new(0),
            }
        }

        /// Each name answers its list every time; any other name is not found.
        pub(crate) fn fixed(names: &[(&str, &[&str])]) -> FakeResolver {
            let names: Vec<(String, Vec<IpAddr>)> = names
                .iter()
                .map(|(name, answers)| (name.to_string(), ips(answers)))
                .collect();
            FakeResolver::new(move |asked, _| {
                names
                    .iter()
                    .find(|(name, _)| name == asked)
                    .map(|(_, answers)| answers.clone())
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such name"))
            })
        }

        fn asked(&self) -> usize {
            self.asked.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl Resolve for FakeResolver {
        async fn lookup(&self, name: &str) -> io::Result<Vec<IpAddr>> {
            let call = self.asked.fetch_add(1, Ordering::SeqCst);
            (self.answer)(name, call)
        }
    }

    fn ips(list: &[&str]) -> Vec<IpAddr> {
        list.iter().map(|ip| ip.parse().unwrap()).collect()
    }

    fn addrs(list: &[&str]) -> Vec<SocketAddr> {
        list.iter().map(|addr| addr.parse().unwrap()).collect()
    }

    /// The newest refusal the proxy kept for `host`.
    fn refusal_for(egress: &Egress, host: &str) -> Option<Refusal> {
        egress
            .refusals_since(0)
            .into_iter()
            .rev()
            .find(|refusal| refusal.host == host)
    }

    /// Sends `request` to the proxy and returns what it answers before it closes.
    async fn ask(egress: &Egress, request: &[u8]) -> String {
        let mut stream = TcpStream::connect(egress.addr).await.unwrap();
        stream.write_all(request).await.unwrap();
        let mut answer = Vec::new();
        let _ = timeout(Duration::from_secs(5), stream.read_to_end(&mut answer)).await;
        String::from_utf8_lossy(&answer).into_owned()
    }

    #[test]
    fn a_connect_line_gives_its_host_and_port() {
        assert_eq!(
            parse_connect("CONNECT jobs.example:443 HTTP/1.1\r\nHost: jobs.example:443\r\n\r\n"),
            Ok(("jobs.example".into(), 443))
        );
        assert_eq!(
            parse_connect("CONNECT [2606:2800::1]:443 HTTP/1.1\r\n\r\n"),
            Ok(("[2606:2800::1]".into(), 443))
        );
        for head in [
            "CONNECT jobs.example HTTP/1.1\r\n\r\n",
            "CONNECT jobs.example:443x HTTP/1.1\r\n\r\n",
            "CONNECT jobs.example:+443 HTTP/1.1\r\n\r\n",
            "CONNECT jobs.example:99999 HTTP/1.1\r\n\r\n",
            "CONNECT jobs.example:443 SPDY/3\r\n\r\n",
            "\u{1}\u{2} garbage",
            "",
        ] {
            assert!(parse_connect(head).unwrap_err().blocked, "{head:?}");
        }
    }

    #[test]
    fn a_plain_request_is_refused_naming_only_its_host() {
        let refusal =
            parse_connect("GET http://192.168.1.1/admin?token=abc HTTP/1.1\r\n\r\n").unwrap_err();
        assert!(refusal.blocked);
        assert_eq!(refusal.host, "192.168.1.1");
        assert!(refusal.why.contains("GET"), "{}", refusal.why);
        assert!(
            !refusal.why.contains("token"),
            "a link's path never reaches the log"
        );
    }

    #[tokio::test]
    async fn a_name_whose_answers_are_all_public_is_connected_to_those_answers() {
        let resolver = FakeResolver::fixed(&[
            ("jobs.example", &["93.184.216.34"]),
            (
                "dual.example",
                &["93.184.216.34", "2606:2800:220:1:248:1893:25c8:1946"],
            ),
        ]);
        assert_eq!(
            decide("jobs.example", 443, &resolver).await,
            Ok(addrs(&["93.184.216.34:443"]))
        );
        assert_eq!(
            decide("dual.example", 443, &resolver).await,
            Ok(addrs(&[
                "93.184.216.34:443",
                "[2606:2800:220:1:248:1893:25c8:1946]:443"
            ]))
        );
        assert_eq!(resolver.asked(), 2, "one lookup per CONNECT");
    }

    #[tokio::test]
    async fn one_private_answer_refuses_the_name() {
        let resolver =
            FakeResolver::fixed(&[("rebind.example", &["93.184.216.34", "192.168.1.1"])]);
        let refusal = decide("rebind.example", 443, &resolver).await.unwrap_err();
        assert!(refusal.blocked);
        assert_eq!(refusal.host, "rebind.example");
        assert!(refusal.why.contains("192.168.1.1"), "{}", refusal.why);
        assert!(
            !refusal.why.contains("93.184.216.34"),
            "it names what broke the rule: {}",
            refusal.why
        );
    }

    #[tokio::test]
    async fn a_name_that_changes_its_answer_is_asked_once() {
        let resolver = FakeResolver::new(|_, call| {
            Ok(ips(if call == 0 {
                &["93.184.216.34"]
            } else {
                &["127.0.0.1"]
            }))
        });
        assert_eq!(
            decide("rebind.example", 443, &resolver).await,
            Ok(addrs(&["93.184.216.34:443"]))
        );
        assert_eq!(
            resolver.asked(),
            1,
            "the address connected to is the one judged"
        );
    }

    #[tokio::test]
    async fn a_name_with_no_answer_is_unreachable_not_blocked() {
        let resolver = FakeResolver::fixed(&[("empty.example", &[])]);
        for host in ["empty.example", "missing.example"] {
            let refusal = decide(host, 443, &resolver).await.unwrap_err();
            assert!(!refusal.blocked, "{host}: {}", refusal.why);
            assert_eq!(refusal.host, host);
        }
    }

    #[tokio::test]
    async fn local_hosts_and_other_ports_are_refused_without_a_lookup() {
        let resolver = FakeResolver::fixed(&[("jobs.example", &["93.184.216.34"])]);
        for (host, port) in [
            ("127.0.0.1", 443),
            ("[::1]", 443),
            ("[::ffff:192.168.1.1]", 443),
            ("192.168.1.1", 443),
            ("169.254.169.254", 443),
            ("localhost", 443),
            ("router.lan", 443),
            ("jobs.example", 80),
            ("93.184.216.34", 8443),
            ("jobs example", 443),
        ] {
            let refusal = decide(host, port, &resolver).await.unwrap_err();
            assert!(refusal.blocked, "{host}:{port}: {}", refusal.why);
        }
        assert_eq!(resolver.asked(), 0);
    }

    #[tokio::test]
    async fn the_system_resolver_answers_a_name_with_its_addresses() {
        let answers = SystemResolver.lookup("localhost").await.unwrap();
        assert!(
            answers.iter().any(IpAddr::is_loopback),
            "localhost answers a loopback address: {answers:?}"
        );
    }

    #[tokio::test]
    async fn a_public_address_is_connected_to_as_written() {
        let resolver = FakeResolver::fixed(&[]);
        assert_eq!(
            decide("93.184.216.34", 443, &resolver).await,
            Ok(addrs(&["93.184.216.34:443"]))
        );
        assert_eq!(resolver.asked(), 0);
    }

    #[tokio::test]
    async fn a_head_is_read_up_to_its_blank_line_and_no_further_than_8_kib() {
        let mut complete: &[u8] = b"CONNECT jobs.example:443 HTTP/1.1\r\n\r\n\x16\x03\x01";
        match read_head(&mut complete, Duration::from_secs(1)).await {
            Head::Complete { text, rest } => {
                assert_eq!(text, "CONNECT jobs.example:443 HTTP/1.1\r\n\r\n");
                assert_eq!(rest, b"\x16\x03\x01", "bytes after the head are kept");
            }
            other => panic!("expected a complete head, got {other:?}"),
        }
        let endless = vec![b'a'; 9 * 1024];
        assert!(matches!(
            read_head(&mut endless.as_slice(), Duration::from_secs(1)).await,
            Head::TooLong
        ));
        let mut cut: &[u8] = b"CONNECT jobs.example:443 HTTP/1.1\r\n";
        assert!(matches!(
            read_head(&mut cut, Duration::from_secs(1)).await,
            Head::Closed
        ));
    }

    #[tokio::test]
    async fn a_head_that_never_ends_is_given_up_on() {
        let (mut proxy_side, mut browser_side) = tokio::io::duplex(64);
        browser_side.write_all(b"CONNECT jobs.ex").await.unwrap();
        let read = timeout(
            Duration::from_secs(5),
            read_head(&mut proxy_side, Duration::from_millis(50)),
        )
        .await
        .expect("the deadline ends the read");
        assert!(matches!(read, Head::Closed));
        drop(browser_side);
    }

    #[tokio::test]
    async fn a_tunnel_carries_bytes_both_ways_once_it_is_established() {
        let echo = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let echo_addr = echo.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = echo.accept().await.unwrap();
            let (mut read, mut write) = stream.split();
            let _ = tokio::io::copy(&mut read, &mut write).await;
        });
        let front = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let mut browser = TcpStream::connect(front.local_addr().unwrap())
            .await
            .unwrap();
        let (mut client, _) = front.accept().await.unwrap();
        tokio::spawn(
            async move { tunnel(&mut client, "echo.example", &[echo_addr], b"early ").await },
        );

        browser.write_all(b"hello").await.unwrap();
        let expected = "HTTP/1.1 200 Connection Established\r\n\r\nearly hello";
        let mut got = vec![0; expected.len()];
        timeout(Duration::from_secs(5), browser.read_exact(&mut got))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&got), expected);
    }

    #[test]
    fn the_judged_addresses_are_tried_alternating_their_families() {
        assert_eq!(
            attempt_order(&addrs(&[
                "[2606:2800::1]:443",
                "[2606:2800::2]:443",
                "93.184.216.34:443",
                "93.184.216.35:443",
            ])),
            addrs(&[
                "[2606:2800::1]:443",
                "93.184.216.34:443",
                "[2606:2800::2]:443",
                "93.184.216.35:443",
            ])
        );
        let v4_first = addrs(&["93.184.216.34:443", "[2606:2800::1]:443"]);
        assert_eq!(attempt_order(&v4_first), v4_first);
        let one_family = addrs(&["93.184.216.34:443", "93.184.216.35:443"]);
        assert_eq!(attempt_order(&one_family), one_family);
    }

    #[tokio::test]
    async fn an_address_that_never_answers_does_not_hold_up_the_next_one() {
        // A network whose IPv6 does not route: the first answer never connects.
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let reachable = listener.local_addr().unwrap();
        let silent: SocketAddr = "[2606:2800::1]:443".parse().unwrap();
        let started = std::time::Instant::now();
        let candidates = [silent, reachable];
        let connecting = first_connection(&candidates, move |addr| async move {
            if addr == silent {
                return std::future::pending().await;
            }
            TcpStream::connect(addr).await
        });
        let stream = timeout(Duration::from_secs(5), connecting)
            .await
            .expect("the next address is tried while the first hangs")
            .unwrap();
        assert_eq!(stream.peer_addr().unwrap(), reachable);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn when_every_address_fails_the_last_failure_is_returned() {
        let error = first_connection(
            &addrs(&["93.184.216.34:443", "[2606:2800::1]:443"]),
            |_| async { Err(io::Error::new(io::ErrorKind::ConnectionRefused, "refused")) },
        )
        .await
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::ConnectionRefused);
    }

    #[tokio::test]
    async fn a_tunnel_to_nothing_is_unreachable() {
        let gone = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let gone_addr = gone.local_addr().unwrap();
        drop(gone);
        let front = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let _browser = TcpStream::connect(front.local_addr().unwrap())
            .await
            .unwrap();
        let (mut client, _) = front.accept().await.unwrap();
        let refusal = tunnel(&mut client, "gone.example", &[gone_addr], b"")
            .await
            .unwrap_err();
        assert!(!refusal.blocked, "{}", refusal.why);
        assert_eq!(refusal.host, "gone.example");
    }

    #[tokio::test]
    async fn the_proxy_answers_403_to_anything_but_a_connect_it_allows_and_keeps_the_refusal() {
        let egress = Egress::start(Arc::new(FakeResolver::fixed(&[])))
            .await
            .unwrap();
        assert!(egress.server().starts_with("http://127.0.0.1:"));

        let answer = ask(
            &egress,
            b"GET http://jobs.example/ HTTP/1.1\r\nHost: jobs.example\r\n\r\n",
        )
        .await;
        assert!(answer.starts_with("HTTP/1.1 403 "), "{answer}");

        let answer = ask(
            &egress,
            b"CONNECT 127.0.0.1:443 HTTP/1.1\r\nHost: 127.0.0.1:443\r\n\r\n",
        )
        .await;
        assert!(answer.starts_with("HTTP/1.1 403 "), "{answer}");
        let refusal = refusal_for(&egress, "127.0.0.1").expect("the refusal is kept");
        assert!(refusal.blocked);
        assert!(refusal.why.contains("local or private"), "{}", refusal.why);
        assert_eq!(egress.refusals_since(0).len(), 2);
    }

    #[tokio::test]
    async fn a_name_that_cannot_be_reached_answers_502() {
        let egress = Egress::start(Arc::new(FakeResolver::fixed(&[])))
            .await
            .unwrap();
        let answer = ask(&egress, b"CONNECT missing.example:443 HTTP/1.1\r\n\r\n").await;
        assert!(answer.starts_with("HTTP/1.1 502 "), "{answer}");
        assert!(!refusal_for(&egress, "missing.example").unwrap().blocked);
    }

    #[tokio::test]
    async fn a_stopped_proxy_takes_no_more_connections() {
        let egress = Egress::start(Arc::new(FakeResolver::fixed(&[])))
            .await
            .unwrap();
        let addr = egress.addr;
        drop(egress);
        for _ in 0..50 {
            if TcpStream::connect(addr).await.is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the proxy still takes connections after it was dropped");
    }

    #[test]
    fn the_refusal_log_keeps_each_refusal_once_and_only_the_newest() {
        let log = RefusalLog::default();
        let refusal = |host: String| Refusal {
            host,
            why: "local".into(),
            blocked: true,
        };
        assert_eq!(log.record(refusal("a".into())), Notice::Line);
        assert_eq!(log.record(refusal("a".into())), Notice::Quiet);
        for n in 0..REFUSALS_KEPT + 5 {
            log.record(refusal(format!("h{n}")));
        }
        let kept = log.since(0);
        assert_eq!(kept.len(), REFUSALS_KEPT);
        assert_eq!(kept[0].host, "h5");
        assert_eq!(
            kept[REFUSALS_KEPT - 1].host,
            format!("h{}", REFUSALS_KEPT + 4)
        );

        let mark = log.mark();
        assert!(log.since(mark).is_empty());
        assert_eq!(log.record(refusal("h7".into())), Notice::Quiet);
        assert_eq!(
            log.since(mark),
            vec![refusal("h7".into())],
            "a repeated refusal counts as a fresh one"
        );
        assert_eq!(log.since(0).len(), REFUSALS_KEPT, "and is still kept once");
    }

    #[test]
    fn only_new_refusals_of_the_address_check_are_worth_a_notice_and_only_so_many() {
        let log = RefusalLog::default();
        let blocked = |host: &str| Refusal::blocked(host, "local");
        assert_eq!(
            log.record(Refusal::unreachable("gone.example", "no address")),
            Notice::Quiet,
            "a site that is down is not news"
        );
        assert_eq!(log.record(blocked("h0")), Notice::Line);
        assert_eq!(
            log.record(blocked("h0")),
            Notice::Quiet,
            "once per host and reason"
        );
        for n in 1..NOTICES_PER_SESSION {
            assert_eq!(log.record(blocked(&format!("h{n}"))), Notice::Line);
        }
        assert_eq!(log.record(blocked("one more")), Notice::Last);
        assert_eq!(log.record(blocked("and another")), Notice::Quiet);
    }

    #[test]
    fn a_notice_says_what_was_refused_on_one_clean_line() {
        let refusal = Refusal::blocked(
            "evil\u{1b}[31m.example",
            "the name answered 192.168.1.1,\na local or private address",
        );
        let line = notice_line(&refusal);
        assert!(
            line.starts_with("aviso: el navegador quiso conectarse a evil"),
            "{line}"
        );
        assert!(!line.contains('\u{1b}') && !line.contains('\n'), "{line:?}");
        assert!(
            line.ends_with("(the name answered 192.168.1.1, a local or private address)"),
            "{line}"
        );
    }

    fn page_error(reason: &str) -> SubmitResult {
        SubmitResult {
            status: SubmitStatus::NeedsGonza,
            reason: Some(reason.to_string()),
            screenshot: None,
        }
    }

    #[tokio::test]
    async fn a_tunnel_failure_is_explained_from_the_refusals_since_the_fill_began() {
        let egress = Egress::start(Arc::new(FakeResolver::fixed(&[(
            "rebind.example",
            &["192.168.1.1"],
        )])))
        .await
        .unwrap();
        let failed = |url: &str| {
            page_error(&format!(
                "page_error: Protocol error (Page.goto): net::ERR_TUNNEL_CONNECTION_FAILED at {url}"
            ))
        };
        let since = egress.mark();
        ask(&egress, b"CONNECT rebind.example:443 HTTP/1.1\r\n\r\n").await;
        ask(&egress, b"CONNECT gone.example:443 HTTP/1.1\r\n\r\n").await;

        let blocked =
            explain_tunnel_failure(failed("https://rebind.example/apply"), &egress, since);
        assert_eq!(blocked.status, SubmitStatus::NeedsGonza);
        assert_eq!(
            blocked.reason.as_deref(),
            Some(
                "unsafe_url: the address check refused rebind.example (the name answered \
                 192.168.1.1, a local or private address)"
            )
        );
        let unreachable =
            explain_tunnel_failure(failed("https://gone.example/apply"), &egress, since);
        let reason = unreachable.reason.unwrap();
        assert!(
            reason.starts_with("page_error: gone.example could not be reached ("),
            "{reason}"
        );
        let redirected =
            explain_tunnel_failure(failed("https://first.example/apply"), &egress, since);
        assert!(
            redirected.reason.unwrap().contains("gone.example"),
            "a redirect's refused host is the newest refusal of the fill"
        );

        let later = egress.mark();
        let earlier_page = failed("https://rebind.example/apply");
        assert_eq!(
            explain_tunnel_failure(earlier_page.clone(), &egress, later).reason,
            earlier_page.reason,
            "a refusal from before the fill explains nothing"
        );
        let other = page_error("page_error: HTTP 403");
        assert_eq!(
            explain_tunnel_failure(other.clone(), &egress, since).reason,
            other.reason
        );
    }
}

#[cfg(test)]
mod chromium_tests;
