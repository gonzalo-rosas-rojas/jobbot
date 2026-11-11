//! One GET, built as layers that each add one behaviour. From the
//! outside in: `Deadline` (after the run's time for the boards, no new request leaves),
//! `HostCooldown` (a host that kept refusing is left alone for the time it asked),
//! `Cached` (a page kept from an earlier run is asked for with its validator, and a
//! 304 reuses it), `Retrying` (a 429 or 503 is asked again after the host's own `Retry-After`, or a
//! doubling wait with jitter) and `Network` (one reqwest client, one timeout). Every source holds
//! an `Http`, a cheap clone of one stack: one client and one board of resting hosts serve the run.
//! With an `HttpMemory` (the tracker) the board and the pages outlive the run: the next hourly run
//! respects the pause a host asked of this one.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{TimeDelta, Utc};
use jobbot_app::ports::store::{HttpMemory, StoredPage};

use crate::sources::base::{
    MAX_COOLDOWN, REQUEST_TIMEOUT, RateLimited, RetryPolicy, backoff_delay, jitter_unit,
    parse_retry_after,
};

/// What one GET asks for.
#[derive(Debug, Clone)]
pub struct Request {
    pub url: reqwest::Url,
    /// Sent as `If-None-Match`: the `ETag` of the page kept for this URL.
    pub if_none_match: Option<String>,
    /// Sent as `If-Modified-Since`: the `Last-Modified` of the page kept for this URL.
    pub if_modified_since: Option<String>,
}

impl Request {
    /// A plain GET of `url`, without validators.
    pub fn get(url: reqwest::Url) -> Request {
        Request {
            url,
            if_none_match: None,
            if_modified_since: None,
        }
    }
}

/// What came back: a page (2xx), a 304 (not modified, to a request with validators) or a refusal
/// (429, 503). Any other status is an error of the `Network` layer, with reqwest's own message.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reply {
    pub status: u16,
    /// The raw `Retry-After` header, when the host sent one.
    pub retry_after: Option<String>,
    /// The page's `ETag` header.
    pub etag: Option<String>,
    /// The page's `Last-Modified` header.
    pub last_modified: Option<String>,
    pub body: String,
}

impl Reply {
    /// A 429 (too many requests) or a 503 (unavailable): the host asks to be left alone a while.
    pub fn is_refusal(&self) -> bool {
        self.status == 429 || self.status == 503
    }

    /// How long the host asked to be left alone, read now.
    fn asked_wait(&self) -> Option<Duration> {
        self.retry_after
            .as_deref()
            .and_then(|value| parse_retry_after(value, Utc::now()))
    }
}

/// One layer of the stack, or the network under it.
#[async_trait]
pub trait Fetcher: Send + Sync {
    async fn get(&self, request: &Request) -> anyhow::Result<Reply>;
}

/// The run's time for the boards is over: no new request leaves.
#[derive(Debug, thiserror::Error)]
#[error("the run's time for the job boards is over; {host} was not asked")]
pub struct DeadlinePassed {
    pub host: String,
}

/// The bottom of the stack: one GET with reqwest, bounded by `REQUEST_TIMEOUT`.
pub struct Network {
    client: reqwest::Client,
}

impl Network {
    pub fn new(client: reqwest::Client) -> Network {
        Network { client }
    }
}

#[async_trait]
impl Fetcher for Network {
    async fn get(&self, request: &Request) -> anyhow::Result<Reply> {
        let mut get = self
            .client
            .get(request.url.clone())
            .timeout(REQUEST_TIMEOUT);
        if let Some(etag) = &request.if_none_match {
            get = get.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        if let Some(since) = &request.if_modified_since {
            get = get.header(reqwest::header::IF_MODIFIED_SINCE, since);
        }
        let response = get.send().await?;
        let status = response.status().as_u16();
        let header = |name| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string)
        };
        let mut reply = Reply {
            status,
            retry_after: header(reqwest::header::RETRY_AFTER),
            etag: header(reqwest::header::ETAG),
            last_modified: header(reqwest::header::LAST_MODIFIED),
            body: String::new(),
        };
        if status == 304 || status == 429 || status == 503 {
            return Ok(reply);
        }
        // Any other failure keeps reqwest's own message (status and address).
        reply.body = response.error_for_status()?.text().await?;
        Ok(reply)
    }
}

/// Asks again when the host refuses, as long as the policy allows; the last refusal goes up.
pub struct Retrying<F> {
    inner: F,
    policy: RetryPolicy,
}

impl<F> Retrying<F> {
    pub fn new(inner: F, policy: RetryPolicy) -> Retrying<F> {
        Retrying { inner, policy }
    }
}

#[async_trait]
impl<F: Fetcher> Fetcher for Retrying<F> {
    async fn get(&self, request: &Request) -> anyhow::Result<Reply> {
        let mut attempt = 0;
        loop {
            let reply = self.inner.get(request).await?;
            if !reply.is_refusal() {
                return Ok(reply);
            }
            let asked = reply.asked_wait();
            let waitable = asked.is_none_or(|wait| wait <= self.policy.max_wait);
            if attempt >= self.policy.max_retries || !waitable {
                return Ok(reply);
            }
            tokio::time::sleep(backoff_delay(&self.policy, attempt, asked, jitter_unit())).await;
            attempt += 1;
        }
    }
}

/// The largest page `Cached` keeps: a bigger one is fetched whole every run.
pub const MAX_KEPT_PAGE: usize = 5 * 1024 * 1024;

/// Asks for a page kept from an earlier request with the validators it came with
/// (`If-None-Match`, `If-Modified-Since`); a 304 (not modified) is answered with the kept body.
/// Only pages that carry a validator and are at most `MAX_KEPT_PAGE` are kept. The memory failing
/// never fails a GET: the page is then asked for and served as if nothing was kept.
pub struct Cached<F> {
    inner: F,
    memory: Arc<dyn HttpMemory>,
}

impl<F> Cached<F> {
    pub fn new(inner: F, memory: Arc<dyn HttpMemory>) -> Cached<F> {
        Cached { inner, memory }
    }
}

#[async_trait]
impl<F: Fetcher> Fetcher for Cached<F> {
    async fn get(&self, request: &Request) -> anyhow::Result<Reply> {
        let url = request.url.as_str();
        let kept = self
            .memory
            .recall_page(url, Utc::now())
            .unwrap_or_else(|e| {
                eprintln!("aviso: no se pudo leer la copia guardada de {url}: {e}");
                None
            });
        let mut asked = request.clone();
        if let Some(page) = &kept {
            asked.if_none_match = page.etag.clone();
            asked.if_modified_since = page.last_modified.clone();
        }
        let reply = self.inner.get(&asked).await?;
        if reply.status == 304 {
            let Some(page) = kept else {
                anyhow::bail!("{url} answered 304 (not modified) to a request without validators");
            };
            return Ok(Reply {
                status: 200,
                etag: page.etag,
                last_modified: page.last_modified,
                body: page.body,
                ..reply
            });
        }
        let has_validator = reply.etag.is_some() || reply.last_modified.is_some();
        if (200..300).contains(&reply.status) && has_validator && reply.body.len() <= MAX_KEPT_PAGE
        {
            let page = StoredPage {
                etag: reply.etag.clone(),
                last_modified: reply.last_modified.clone(),
                body: reply.body.clone(),
            };
            if let Err(e) = self.memory.store_page(url, &page, Utc::now()) {
                eprintln!("aviso: no se pudo guardar la copia de {url}: {e}");
            }
        }
        Ok(reply)
    }
}

/// Hosts left alone until an instant. One board belongs to one `Http` and its clones; with a
/// memory, it starts from the rests an earlier run left and every new rest is written to it.
#[derive(Default)]
pub struct Cooldowns {
    board: Mutex<HashMap<String, Instant>>,
    memory: Option<Arc<dyn HttpMemory>>,
}

impl Cooldowns {
    /// A board that starts from the rests `memory` holds and keeps its new rests there. A memory
    /// that cannot be read leaves the board empty: the run then only knows its own refusals.
    pub fn remembered(memory: Arc<dyn HttpMemory>) -> Cooldowns {
        let now = Utc::now();
        let rests = memory.resting_hosts(now).unwrap_or_else(|e| {
            eprintln!("aviso: no se pudieron leer las pausas de los sitios: {e}");
            Vec::new()
        });
        let board = rests
            .into_iter()
            .filter_map(|(host, until)| {
                // A rest is never longer than the board's longest, whatever was stored.
                let rest = (until - now).to_std().ok()?.min(MAX_COOLDOWN);
                Some((host, Instant::now() + rest))
            })
            .collect();
        Cooldowns {
            board: Mutex::new(board),
            memory: Some(memory),
        }
    }

    /// How much longer `host` is to be left alone, if at all.
    fn rest_of(&self, host: &str) -> Option<Duration> {
        let board = self
            .board
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        board
            .get(host)
            .and_then(|until| until.checked_duration_since(Instant::now()))
            .filter(|rest| !rest.is_zero())
    }

    fn start(&self, host: &str, wait: Duration) {
        let mut board = self
            .board
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // A wait too far ahead to be an `Instant` is as good as no end: the longest the board says.
        let until = Instant::now()
            .checked_add(wait)
            .unwrap_or_else(|| Instant::now() + MAX_COOLDOWN);
        board.insert(host.to_string(), until);
        drop(board);
        if let Some(memory) = &self.memory {
            let rest = TimeDelta::from_std(wait.min(MAX_COOLDOWN)).unwrap_or(TimeDelta::MAX);
            if let Err(e) = memory.rest_host(host, Utc::now() + rest) {
                eprintln!("aviso: no se pudo guardar la pausa de {host}: {e}");
            }
        }
    }
}

/// Leaves alone a host that refused even after the retries, for the time it asked (a day at most)
/// or the policy's default; a request to a resting host is refused here without leaving.
pub struct HostCooldown<F> {
    inner: F,
    board: Arc<Cooldowns>,
    default_cooldown: Duration,
}

impl<F> HostCooldown<F> {
    pub fn new(inner: F, board: Arc<Cooldowns>, default_cooldown: Duration) -> HostCooldown<F> {
        HostCooldown {
            inner,
            board,
            default_cooldown,
        }
    }
}

#[async_trait]
impl<F: Fetcher> Fetcher for HostCooldown<F> {
    async fn get(&self, request: &Request) -> anyhow::Result<Reply> {
        let host = host_key(&request.url);
        if let Some(rest) = self.board.rest_of(&host) {
            let wait_secs = rest.as_secs().max(1);
            return Err(RateLimited { host, wait_secs }.into());
        }
        let reply = self.inner.get(request).await?;
        if !reply.is_refusal() {
            return Ok(reply);
        }
        // Whatever the host claims, it is left alone for a day at most.
        let rest = reply
            .asked_wait()
            .unwrap_or(self.default_cooldown)
            .min(MAX_COOLDOWN);
        self.board.start(&host, rest);
        Err(RateLimited {
            host,
            wait_secs: rest.as_secs(),
        }
        .into())
    }
}

/// Refuses every request once the run's time for the boards is over.
pub struct Deadline<F> {
    inner: F,
    at: Option<Instant>,
}

impl<F> Deadline<F> {
    pub fn new(inner: F, at: Option<Instant>) -> Deadline<F> {
        Deadline { inner, at }
    }
}

#[async_trait]
impl<F: Fetcher> Fetcher for Deadline<F> {
    async fn get(&self, request: &Request) -> anyhow::Result<Reply> {
        if self.at.is_some_and(|at| Instant::now() >= at) {
            let host = host_key(&request.url);
            return Err(DeadlinePassed { host }.into());
        }
        self.inner.get(request).await
    }
}

/// `host:port` of a URL: what a cooldown is kept for.
pub(crate) fn host_key(url: &reqwest::Url) -> String {
    format!(
        "{}:{}",
        url.host_str().unwrap_or(""),
        url.port_or_known_default().unwrap_or(0)
    )
}

/// The handle every source holds: one stack of layers over one client. Cloning shares the stack,
/// so the clones also share the board of resting hosts.
#[derive(Clone)]
pub struct Http {
    fetcher: Arc<dyn Fetcher>,
}

impl Http {
    /// The standard stack over `client`: `RetryPolicy::STANDARD`, a board of its own, no deadline.
    pub fn new(client: reqwest::Client) -> Http {
        Http::over(Network::new(client), RetryPolicy::STANDARD, None)
    }

    /// The standard stack over `client` that sends nothing after `deadline` (the run goes on with
    /// what came back before it), with the rests and the pages an earlier run left in `memory`, and
    /// this run's added to it.
    pub fn remembering(
        client: reqwest::Client,
        deadline: Instant,
        memory: Arc<dyn HttpMemory>,
    ) -> Http {
        Http::over_with_memory(
            Network::new(client),
            RetryPolicy::STANDARD,
            Some(deadline),
            memory,
        )
    }

    /// The layers over any bottom fetcher (the network, or a test's script), with `policy` and an
    /// optional deadline; the board of resting hosts is the run's own and no page is kept.
    pub fn over(
        bottom: impl Fetcher + 'static,
        policy: RetryPolicy,
        deadline: Option<Instant>,
    ) -> Http {
        let stack = Deadline::new(
            HostCooldown::new(
                Retrying::new(bottom, policy),
                Arc::new(Cooldowns::default()),
                policy.default_cooldown,
            ),
            deadline,
        );
        Http {
            fetcher: Arc::new(stack),
        }
    }

    /// `over`, with the board and the kept pages held in `memory` as well.
    pub fn over_with_memory(
        bottom: impl Fetcher + 'static,
        policy: RetryPolicy,
        deadline: Option<Instant>,
        memory: Arc<dyn HttpMemory>,
    ) -> Http {
        let board = Arc::new(Cooldowns::remembered(memory.clone()));
        let stack = Deadline::new(
            HostCooldown::new(
                Cached::new(Retrying::new(bottom, policy), memory),
                board,
                policy.default_cooldown,
            ),
            deadline,
        );
        Http {
            fetcher: Arc::new(stack),
        }
    }

    /// GET `url` with `query` appended and return the body as text. Fails on a network error, a
    /// status that is neither a page nor a refusal, a host that refused even after the retries or
    /// is resting (`RateLimited`), and after the deadline (`DeadlinePassed`).
    pub async fn get_text(&self, url: &str, query: &[(&str, &str)]) -> anyhow::Result<String> {
        let url = if query.is_empty() {
            reqwest::Url::parse(url)?
        } else {
            reqwest::Url::parse_with_params(url, query)?
        };
        Ok(self.fetcher.get(&Request::get(url)).await?.body)
    }
}

impl From<reqwest::Client> for Http {
    fn from(client: reqwest::Client) -> Http {
        Http::new(client)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use chrono::{DateTime, TimeDelta};
    use jobbot_app::ports::store::{StoreError, StoreResult};

    use super::*;

    /// A bottom fetcher that plays a script: one reply per request, in order (the last one
    /// repeats), and remembers every request it got.
    #[derive(Clone, Default)]
    struct Script {
        replies: Arc<Mutex<VecDeque<Reply>>>,
        asked: Arc<Mutex<Vec<Request>>>,
    }

    impl Script {
        fn of(replies: &[Reply]) -> Script {
            Script {
                replies: Arc::new(Mutex::new(replies.iter().cloned().collect())),
                asked: Arc::default(),
            }
        }

        fn asked(&self) -> Vec<String> {
            let asked = self.asked.lock().unwrap();
            asked
                .iter()
                .map(|request| request.url.to_string())
                .collect()
        }

        /// The validators (`If-None-Match`, `If-Modified-Since`) each request carried.
        fn validators(&self) -> Vec<(Option<String>, Option<String>)> {
            let asked = self.asked.lock().unwrap();
            asked
                .iter()
                .map(|request| {
                    (
                        request.if_none_match.clone(),
                        request.if_modified_since.clone(),
                    )
                })
                .collect()
        }
    }

    #[async_trait]
    impl Fetcher for Script {
        async fn get(&self, request: &Request) -> anyhow::Result<Reply> {
            self.asked.lock().unwrap().push(request.clone());
            let mut replies = self.replies.lock().unwrap();
            let reply = if replies.len() > 1 {
                replies.pop_front()
            } else {
                replies.front().cloned()
            };
            Ok(reply.expect("a scripted reply"))
        }
    }

    fn page(body: &str) -> Reply {
        Reply {
            status: 200,
            body: body.into(),
            ..Reply::default()
        }
    }

    fn refusal(status: u16, retry_after: Option<&str>) -> Reply {
        Reply {
            status,
            retry_after: retry_after.map(str::to_string),
            ..Reply::default()
        }
    }

    /// Waits so short that a test never sleeps.
    fn quick() -> RetryPolicy {
        RetryPolicy {
            max_retries: 2,
            base: Duration::from_millis(1),
            max_wait: Duration::from_secs(30),
            default_cooldown: Duration::from_secs(30),
        }
    }

    fn request(url: &str) -> Request {
        Request::get(reqwest::Url::parse(url).unwrap())
    }

    fn rate_limited(err: &anyhow::Error) -> &RateLimited {
        err.downcast_ref::<RateLimited>()
            .unwrap_or_else(|| panic!("a RateLimited error, got {err:#}"))
    }

    #[tokio::test]
    async fn a_refusal_with_a_short_retry_after_is_asked_again_until_the_page_comes() {
        for status in [429, 503] {
            let script = Script::of(&[refusal(status, Some("0")), page("ok")]);
            let retrying = Retrying::new(script.clone(), quick());
            let reply = retrying.get(&request("https://a.test/feed")).await.unwrap();
            assert_eq!(reply, page("ok"), "status {status}");
            assert_eq!(script.asked().len(), 2, "status {status}");
        }
    }

    #[tokio::test]
    async fn retrying_stops_at_the_policy_and_passes_the_last_refusal_up() {
        let script = Script::of(&[refusal(429, None)]);
        let reply = Retrying::new(script.clone(), quick())
            .get(&request("https://a.test/feed"))
            .await
            .unwrap();
        assert_eq!(reply.status, 429);
        assert_eq!(script.asked().len(), 1 + quick().max_retries as usize);
    }

    #[tokio::test]
    async fn a_retry_after_beyond_the_longest_wait_is_not_waited_for() {
        let script = Script::of(&[refusal(429, Some("3600")), page("never")]);
        let reply = Retrying::new(script.clone(), quick())
            .get(&request("https://a.test/feed"))
            .await
            .unwrap();
        assert_eq!(reply.status, 429);
        assert_eq!(script.asked().len(), 1);
    }

    #[tokio::test]
    async fn pages_and_other_answers_are_not_retried() {
        let script = Script::of(&[page("first"), page("second")]);
        let reply = Retrying::new(script.clone(), quick())
            .get(&request("https://a.test/feed"))
            .await
            .unwrap();
        assert_eq!(reply, page("first"));
        assert_eq!(script.asked().len(), 1);
    }

    #[tokio::test]
    async fn a_host_that_refused_is_left_alone_and_the_others_are_not() {
        let script = Script::of(&[refusal(429, Some("120")), page("b")]);
        let cooldown = HostCooldown::new(script.clone(), Arc::default(), quick().default_cooldown);

        let err = cooldown
            .get(&request("https://a.test/1"))
            .await
            .unwrap_err();
        assert_eq!(rate_limited(&err).wait_secs, 120, "the time the host asked");
        let again = cooldown
            .get(&request("https://a.test/2"))
            .await
            .unwrap_err();
        assert!(rate_limited(&again).wait_secs >= 1);
        assert_eq!(
            script.asked(),
            ["https://a.test/1"],
            "nothing left for the resting host"
        );

        let other = cooldown.get(&request("https://b.test/1")).await.unwrap();
        assert_eq!(other, page("b"));
    }

    #[tokio::test]
    async fn a_refusal_without_a_time_rests_the_default_and_a_lifetime_rests_a_day() {
        let script = Script::of(&[refusal(503, None)]);
        let cooldown = HostCooldown::new(script, Arc::default(), Duration::from_secs(300));
        let err = cooldown
            .get(&request("https://a.test/1"))
            .await
            .unwrap_err();
        assert_eq!(rate_limited(&err).wait_secs, 300);

        let script = Script::of(&[refusal(429, Some("18446744073709551615"))]);
        let cooldown = HostCooldown::new(script, Arc::default(), Duration::from_secs(300));
        let err = cooldown
            .get(&request("https://a.test/1"))
            .await
            .unwrap_err();
        assert_eq!(rate_limited(&err).wait_secs, MAX_COOLDOWN.as_secs());
    }

    #[tokio::test]
    async fn after_the_deadline_no_request_leaves() {
        let script = Script::of(&[page("ok")]);
        let past = Deadline::new(script.clone(), Some(Instant::now()));
        let err = past.get(&request("https://a.test/1")).await.unwrap_err();
        assert!(err.downcast_ref::<DeadlinePassed>().is_some(), "{err:#}");
        assert!(script.asked().is_empty());

        let later = Instant::now() + Duration::from_secs(60);
        let future = Deadline::new(script.clone(), Some(later));
        assert_eq!(
            future.get(&request("https://a.test/1")).await.unwrap(),
            page("ok")
        );
        let none = Deadline::new(script.clone(), None);
        assert_eq!(
            none.get(&request("https://a.test/1")).await.unwrap(),
            page("ok")
        );
    }

    #[tokio::test]
    async fn the_clones_of_one_http_share_its_board_and_another_http_has_its_own() {
        let script = Script::of(&[refusal(429, Some("120"))]);
        let http = Http::over(script.clone(), quick(), None);
        let clone = http.clone();
        assert!(http.get_text("https://a.test/1", &[]).await.is_err());
        let asked = script.asked().len();
        let err = clone.get_text("https://a.test/2", &[]).await.unwrap_err();
        rate_limited(&err);
        assert_eq!(
            script.asked().len(),
            asked,
            "the clone knows the host is resting"
        );

        let other = Http::over(script.clone(), quick(), None);
        assert!(other.get_text("https://a.test/3", &[]).await.is_err());
        assert!(
            script.asked().len() > asked,
            "a separate Http asks for itself"
        );
    }

    #[tokio::test]
    async fn the_query_is_appended_to_the_url() {
        let script = Script::of(&[page("ok")]);
        let http = Http::over(script.clone(), quick(), None);
        let body = http
            .get_text("https://a.test/feed", &[("page", "2"), ("q", "rust dev")])
            .await
            .unwrap();
        assert_eq!(body, "ok");
        assert_eq!(script.asked(), ["https://a.test/feed?page=2&q=rust+dev"]);
    }

    /// The tracker's part, held in memory; `broken` makes every call fail.
    #[derive(Default)]
    struct Memory {
        rests: Mutex<HashMap<String, DateTime<Utc>>>,
        pages: Mutex<HashMap<String, StoredPage>>,
        broken: bool,
    }

    impl Memory {
        fn broken() -> Arc<Memory> {
            Arc::new(Memory {
                broken: true,
                ..Memory::default()
            })
        }

        fn check(&self) -> StoreResult<()> {
            if self.broken {
                return Err(StoreError::Backend("the disk is gone".into()));
            }
            Ok(())
        }

        fn rest_of(&self, host: &str) -> Option<DateTime<Utc>> {
            self.rests.lock().unwrap().get(host).copied()
        }

        fn page(&self, url: &str) -> Option<StoredPage> {
            self.pages.lock().unwrap().get(url).cloned()
        }
    }

    impl HttpMemory for Memory {
        fn resting_hosts(&self, now: DateTime<Utc>) -> StoreResult<Vec<(String, DateTime<Utc>)>> {
            self.check()?;
            let rests = self.rests.lock().unwrap();
            Ok(rests
                .iter()
                .filter(|(_, until)| **until > now)
                .map(|(host, until)| (host.clone(), *until))
                .collect())
        }
        fn rest_host(&self, host: &str, until: DateTime<Utc>) -> StoreResult<()> {
            self.check()?;
            self.rests.lock().unwrap().insert(host.to_string(), until);
            Ok(())
        }
        fn recall_page(&self, url: &str, _now: DateTime<Utc>) -> StoreResult<Option<StoredPage>> {
            self.check()?;
            Ok(self.page(url))
        }
        fn store_page(&self, url: &str, page: &StoredPage, _now: DateTime<Utc>) -> StoreResult<()> {
            self.check()?;
            self.pages
                .lock()
                .unwrap()
                .insert(url.to_string(), page.clone());
            Ok(())
        }
    }

    fn validated(body: &str, etag: Option<&str>, last_modified: Option<&str>) -> Reply {
        Reply {
            etag: etag.map(str::to_string),
            last_modified: last_modified.map(str::to_string),
            ..page(body)
        }
    }

    fn not_modified() -> Reply {
        Reply {
            status: 304,
            ..Reply::default()
        }
    }

    #[tokio::test]
    async fn a_host_resting_since_an_earlier_run_is_not_asked() {
        let memory = Arc::new(Memory::default());
        memory
            .rest_host("a.test:443", Utc::now() + TimeDelta::seconds(120))
            .unwrap();
        let script = Script::of(&[page("ok")]);
        let http = Http::over_with_memory(script.clone(), quick(), None, memory);

        let err = http.get_text("https://a.test/1", &[]).await.unwrap_err();
        let wait = rate_limited(&err).wait_secs;
        assert!((115..=120).contains(&wait), "{wait}");
        assert!(script.asked().is_empty());
        assert_eq!(http.get_text("https://b.test/1", &[]).await.unwrap(), "ok");
    }

    #[tokio::test]
    async fn a_refusal_is_remembered_for_the_next_run() {
        let memory = Arc::new(Memory::default());
        let script = Script::of(&[refusal(429, Some("120")), page("ok")]);
        let first_run = Http::over_with_memory(
            script.clone(),
            RetryPolicy {
                max_retries: 0,
                ..quick()
            },
            None,
            memory.clone(),
        );
        assert!(first_run.get_text("https://a.test/1", &[]).await.is_err());
        let until = memory.rest_of("a.test:443").expect("the rest is stored");
        let left = (until - Utc::now()).num_seconds();
        assert!((115..=120).contains(&left), "{left}");

        let next_run = Http::over_with_memory(script.clone(), quick(), None, memory);
        let err = next_run
            .get_text("https://a.test/2", &[])
            .await
            .unwrap_err();
        rate_limited(&err);
        assert_eq!(script.asked(), ["https://a.test/1"]);
    }

    #[tokio::test]
    async fn an_unchanged_page_is_asked_for_with_its_validators_and_comes_from_the_store() {
        let memory = Arc::new(Memory::default());
        let lm = "Wed, 21 Oct 2026 07:28:00 GMT";
        let script = Script::of(&[validated("jobs", Some("\"v1\""), Some(lm)), not_modified()]);
        let first_run = Http::over_with_memory(script.clone(), quick(), None, memory.clone());
        assert_eq!(
            first_run
                .get_text("https://a.test/feed", &[])
                .await
                .unwrap(),
            "jobs"
        );
        let next_run = Http::over_with_memory(script.clone(), quick(), None, memory);
        assert_eq!(
            next_run.get_text("https://a.test/feed", &[]).await.unwrap(),
            "jobs",
            "a 304 answers with the kept body"
        );
        assert_eq!(
            script.validators(),
            [
                (None, None),
                (Some("\"v1\"".to_string()), Some(lm.to_string()))
            ]
        );
    }

    #[tokio::test]
    async fn a_changed_page_replaces_the_kept_one() {
        let memory = Arc::new(Memory::default());
        let script = Script::of(&[
            validated("old", None, Some("Mon, 19 Oct 2026 07:28:00 GMT")),
            validated("new", None, Some("Fri, 23 Oct 2026 07:28:00 GMT")),
        ]);
        let http = Http::over_with_memory(script.clone(), quick(), None, memory.clone());
        http.get_text("https://a.test/feed", &[]).await.unwrap();
        assert_eq!(
            http.get_text("https://a.test/feed", &[]).await.unwrap(),
            "new"
        );
        assert_eq!(memory.page("https://a.test/feed").unwrap().body, "new");
        assert_eq!(
            script.validators()[1],
            (None, Some("Mon, 19 Oct 2026 07:28:00 GMT".to_string())),
            "only the validator the page came with is sent"
        );
    }

    #[tokio::test]
    async fn only_pages_with_a_validator_and_at_most_5_mb_are_kept() {
        let memory = Arc::new(Memory::default());
        let huge = "x".repeat(MAX_KEPT_PAGE + 1);
        let script = Script::of(&[
            page("no validator"),
            validated(&huge, Some("\"big\""), None),
            validated(&"y".repeat(MAX_KEPT_PAGE), Some("\"fits\""), None),
        ]);
        let http = Http::over_with_memory(script, quick(), None, memory.clone());
        for path in ["plain", "huge", "fits"] {
            http.get_text(&format!("https://a.test/{path}"), &[])
                .await
                .unwrap();
        }
        assert!(memory.page("https://a.test/plain").is_none());
        assert!(memory.page("https://a.test/huge").is_none());
        assert!(memory.page("https://a.test/fits").is_some());
    }

    #[tokio::test]
    async fn a_304_to_a_request_without_validators_is_an_error() {
        let script = Script::of(&[not_modified()]);
        let http = Http::over_with_memory(script, quick(), None, Arc::new(Memory::default()));
        let err = http.get_text("https://a.test/feed", &[]).await.unwrap_err();
        assert!(err.to_string().contains("304"), "{err:#}");
    }

    #[tokio::test]
    async fn a_failing_memory_never_fails_a_get() {
        let script = Script::of(&[
            validated("jobs", Some("\"v1\""), None),
            refusal(429, Some("120")),
        ]);
        let http = Http::over_with_memory(
            script.clone(),
            RetryPolicy {
                max_retries: 0,
                ..quick()
            },
            None,
            Memory::broken(),
        );
        assert_eq!(
            http.get_text("https://a.test/feed", &[]).await.unwrap(),
            "jobs"
        );
        let err = http.get_text("https://a.test/2", &[]).await.unwrap_err();
        rate_limited(&err);
        let err = http.get_text("https://a.test/3", &[]).await.unwrap_err();
        rate_limited(&err);
        assert_eq!(
            script.asked().len(),
            2,
            "the run's own board still holds the rest"
        );
    }

    #[tokio::test]
    async fn the_network_sends_the_validators_and_reads_a_304() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/feed"))
            .and(header("If-None-Match", "\"v1\""))
            .respond_with(ResponseTemplate::new(304))
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/feed"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("ETag", "\"v1\"")
                    .insert_header("Last-Modified", "Wed, 21 Oct 2026 07:28:00 GMT")
                    .set_body_string("jobs"),
            )
            .mount(&server)
            .await;
        let memory = Arc::new(Memory::default());
        let url = format!("{}/feed", server.uri());
        let first_run = Http::remembering(
            reqwest::Client::new(),
            Instant::now() + Duration::from_secs(60),
            memory.clone(),
        );
        assert_eq!(first_run.get_text(&url, &[]).await.unwrap(), "jobs");
        let kept = memory.page(&url).expect("the page is kept");
        assert_eq!(kept.etag.as_deref(), Some("\"v1\""));
        assert_eq!(
            kept.last_modified.as_deref(),
            Some("Wed, 21 Oct 2026 07:28:00 GMT")
        );

        let next_run = Http::remembering(
            reqwest::Client::new(),
            Instant::now() + Duration::from_secs(60),
            memory,
        );
        assert_eq!(next_run.get_text(&url, &[]).await.unwrap(), "jobs");
        let asked = server.received_requests().await.unwrap();
        assert_eq!(asked.len(), 2);
        assert_eq!(
            asked[1]
                .headers
                .get("If-Modified-Since")
                .and_then(|value| value.to_str().ok()),
            Some("Wed, 21 Oct 2026 07:28:00 GMT")
        );
    }
}
