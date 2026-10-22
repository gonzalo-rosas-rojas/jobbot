//! Helpers shared by every source: lenient time parsing, lenient JSON access and one HTTP GET that
//! honors `Retry-After`.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde_json::Value;

/// Upper bound for one HTTP request. A slow board must not stall the whole run.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Wait between two requests to the same site, for the sources that read more than one page or feed.
pub const REQUEST_PAUSE: Duration = Duration::from_millis(500);

/// Parses an ISO 8601 / RFC 3339 timestamp (`Z` or an offset). A value without an offset is read as UTC.
/// Returns `None` for empty or unparseable text.
pub fn parse_iso(value: &str) -> Option<DateTime<Utc>> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Some(parsed.with_timezone(&Utc));
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some(naive.and_utc());
    }
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|naive| naive.and_utc())
}

/// Parses an RFC 2822 date such as `Sun, 04 Oct 2026 07:31:08 +0000` (the RSS `pubDate`).
/// Returns `None` for empty or unparseable text.
pub fn parse_rfc2822(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc2822(value.trim())
        .ok()
        .map(|parsed| parsed.with_timezone(&Utc))
}

/// Converts Unix seconds to a UTC timestamp. Returns `None` when the value is out of range.
pub fn from_epoch(value: i64) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(value, 0)
}

/// Reads a JSON timestamp that may be an ISO string or Unix seconds (Get on Board sends either).
pub fn parse_time(value: &Value) -> Option<DateTime<Utc>> {
    match value {
        Value::String(text) => parse_iso(text),
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|seconds| seconds as i64))
            .and_then(from_epoch),
        _ => None,
    }
}

/// The string at `key`, or `""` when the key is missing, null or not a string.
pub fn str_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

/// A posting id as text. Boards send ids as numbers (Greenhouse) or strings (Lever); null and `""` give `None`.
pub fn id_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// How a GET reacts when the host answers 429 (too many requests) or 503 (unavailable).
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// Retries after the first request (so at most `max_retries + 1` requests per address).
    pub max_retries: u32,
    /// First wait when the host gave no `Retry-After`; it doubles on each retry.
    pub base: Duration,
    /// Longest wait the program sits through; a longer `Retry-After` is not waited for.
    pub max_wait: Duration,
    /// How long the host is left alone after the program gives up and it named no time.
    pub default_cooldown: Duration,
}

impl RetryPolicy {
    pub const STANDARD: RetryPolicy = RetryPolicy {
        max_retries: 2,
        base: Duration::from_secs(1),
        max_wait: Duration::from_secs(30),
        default_cooldown: Duration::from_secs(300),
    };
}

/// The longest a host is left alone, whatever its `Retry-After` claims: a day.
const MAX_COOLDOWN: Duration = Duration::from_secs(24 * 60 * 60);

/// The host asked to be left alone, or the program already gave up on it a moment ago.
#[derive(Debug, thiserror::Error)]
#[error("{host} asked us to slow down; not asking again for {wait_secs} s")]
pub struct RateLimited {
    pub host: String,
    pub wait_secs: u64,
}

/// Hosts left alone until an instant.
#[derive(Default)]
struct Cooldowns(Mutex<HashMap<String, Instant>>);

impl Cooldowns {
    /// How much longer `key` is to be left alone, if at all.
    fn rest_of(&self, key: &str) -> Option<Duration> {
        let board = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        board
            .get(key)
            .and_then(|until| until.checked_duration_since(Instant::now()))
            .filter(|rest| !rest.is_zero())
    }

    fn start(&self, key: &str, wait: Duration) {
        let mut board = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // A wait too far ahead to be an `Instant` is as good as no end: leave the host alone for
        // the longest the board can say.
        let until = Instant::now()
            .checked_add(wait)
            .unwrap_or_else(|| Instant::now() + MAX_COOLDOWN);
        board.insert(key.to_string(), until);
    }
}

/// The board every source shares: a discovery run is one process, so a slug that earned a 429
/// protects the slugs behind it. Keeping it across runs (in SQLite) belongs to the retry decorator
/// of the resilience phase.
static SHARED_COOLDOWNS: LazyLock<Cooldowns> = LazyLock::new(Cooldowns::default);

fn host_key(url: &reqwest::Url) -> String {
    format!(
        "{}:{}",
        url.host_str().unwrap_or(""),
        url.port_or_known_default().unwrap_or(0)
    )
}

/// A stub or a local proxy: nobody's shared host, so there is nothing to protect and nothing to
/// remember between calls (the tests of every source talk to servers like this one).
fn is_local(url: &reqwest::Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// What a `Retry-After` value asks for: a number of seconds or an HTTP date (RFC 9110, 10.2.3). A
/// date already past is a zero wait. Anything else is no answer at all.
pub fn parse_retry_after(value: &str, now: DateTime<Utc>) -> Option<Duration> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let when = parse_rfc2822(value)?;
    Some((when - now).to_std().unwrap_or(Duration::ZERO))
}

/// How long to wait before retry number `attempt` (0-based): the server's own `Retry-After` when it
/// gave one, otherwise `base` doubling each time; plus up to 25 % of `jitter` (0.0 to 1.0, the caller
/// supplies the random number); never above `max_wait`.
pub fn backoff_delay(
    policy: &RetryPolicy,
    attempt: u32,
    retry_after: Option<Duration>,
    jitter: f64,
) -> Duration {
    let base = retry_after.unwrap_or_else(|| {
        policy
            .base
            .checked_mul(2u32.saturating_pow(attempt.min(10)))
            .unwrap_or(policy.max_wait)
    });
    let jitter = if jitter.is_finite() {
        jitter.clamp(0.0, 1.0)
    } else {
        0.0
    };
    // Capped before the jitter is applied: a `Duration::MAX` from the wire must not overflow it.
    base.min(policy.max_wait)
        .mul_f64(1.0 + 0.25 * jitter)
        .min(policy.max_wait)
}

fn jitter_unit() -> f64 {
    let mut bytes = [0u8; 4];
    if getrandom::fill(&mut bytes).is_err() {
        return 0.0;
    }
    f64::from(u32::from_le_bytes(bytes)) / f64::from(u32::MAX)
}

/// GET `url` with `query` appended and return the body as text.
/// Fails on a network error and on a non-2xx status. A 429 or 503 is retried with the host's own
/// `Retry-After` (or a doubling wait with jitter) up to `RetryPolicy::STANDARD`, then the host is
/// left alone for a while (`RateLimited`).
pub async fn get_text(
    client: &reqwest::Client,
    url: &str,
    query: &[(&str, &str)],
) -> anyhow::Result<String> {
    get_text_with(client, url, query, &RetryPolicy::STANDARD).await
}

/// `get_text` with an explicit retry policy.
pub async fn get_text_with(
    client: &reqwest::Client,
    url: &str,
    query: &[(&str, &str)],
    policy: &RetryPolicy,
) -> anyhow::Result<String> {
    let url = if query.is_empty() {
        reqwest::Url::parse(url)?
    } else {
        reqwest::Url::parse_with_params(url, query)?
    };
    if is_local(&url) {
        return fetch(&Cooldowns::default(), client, url, policy).await;
    }
    fetch(&SHARED_COOLDOWNS, client, url, policy).await
}

/// The GET itself, against the given board of cooled-down hosts.
async fn fetch(
    cooldowns: &Cooldowns,
    client: &reqwest::Client,
    url: reqwest::Url,
    policy: &RetryPolicy,
) -> anyhow::Result<String> {
    let host = host_key(&url);
    if let Some(rest) = cooldowns.rest_of(&host) {
        return Err(RateLimited {
            host,
            wait_secs: rest.as_secs().max(1),
        }
        .into());
    }
    let mut attempt = 0;
    loop {
        let response = client
            .get(url.clone())
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await?;
        let status = response.status().as_u16();
        if status != 429 && status != 503 {
            return Ok(response.error_for_status()?.text().await?);
        }
        let asked = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| parse_retry_after(value, Utc::now()));
        let waitable = asked.is_none_or(|wait| wait <= policy.max_wait);
        if attempt < policy.max_retries && waitable {
            tokio::time::sleep(backoff_delay(policy, attempt, asked, jitter_unit())).await;
            attempt += 1;
            continue;
        }
        // Whatever the host claims, it is left alone for a day at most.
        let rest = asked.unwrap_or(policy.default_cooldown).min(MAX_COOLDOWN);
        cooldowns.start(&host, rest);
        return Err(RateLimited {
            host,
            wait_secs: rest.as_secs(),
        }
        .into());
    }
}

/// GET `url` with `query` appended and parse the body as JSON.
/// Fails on a network error, on a non-2xx status, and on a body that is not JSON (for example an HTML block page).
pub async fn get_json(
    client: &reqwest::Client,
    url: &str,
    query: &[(&str, &str)],
) -> anyhow::Result<Value> {
    let body = get_text(client, url, query).await?;
    serde_json::from_str(&body).with_context(|| {
        let head: String = body.chars().take(40).collect();
        format!("response is not JSON (starts with {head:?})")
    })
}

/// Joins place names into one location text with `"; "`, in order. Blank parts and parts already
/// named are skipped.
pub fn join_places<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    let mut joined = String::new();
    let mut lower = String::new();
    for part in parts {
        let part = part.trim();
        let part_lower = part.to_lowercase();
        if part.is_empty() {
            continue;
        }
        // A long name is also "already named" when an earlier place contains it ("Canada" in
        // "Ontario, Canada"); short codes only count on an exact match, because "us" is inside "austin".
        let named = lower.split("; ").any(|place| place == part_lower)
            || (part_lower.len() > 3 && lower.contains(&part_lower));
        if named {
            continue;
        }
        if !joined.is_empty() {
            joined.push_str("; ");
            lower.push_str("; ");
        }
        joined.push_str(part);
        lower.push_str(&part_lower);
    }
    joined
}

/// The error line for a failed slug: `"<source>:<slug>: <reason>"`. `{:#}` prints the whole cause chain.
pub fn failure(source: &str, slug: &str, error: &anyhow::Error) -> String {
    format!("{source}:{slug}: {error:#}")
}

/// The error line for postings that were dropped because they lacked an id, a title or a URL.
pub fn skipped_error(source: &str, slug: &str, count: usize) -> String {
    format!("{source}:{slug}: skipped {count} postings missing id, title or url")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    #[test]
    fn parse_iso_accepts_offsets_zulu_and_naive() {
        let expected = Utc.with_ymd_and_hms(2026, 9, 30, 16, 0, 0).unwrap();
        assert_eq!(parse_iso("2026-09-30T12:00:00-04:00"), Some(expected));
        assert_eq!(parse_iso("2026-09-30T16:00:00Z"), Some(expected));
        assert_eq!(parse_iso("2026-09-30T16:00:00.000+00:00"), Some(expected));
        assert_eq!(parse_iso("2026-09-30T16:00:00"), Some(expected));
        assert_eq!(
            parse_iso("2026-09-30"),
            Some(Utc.with_ymd_and_hms(2026, 9, 30, 0, 0, 0).unwrap())
        );
    }

    #[test]
    fn parse_iso_rejects_empty_and_garbage() {
        assert_eq!(parse_iso(""), None);
        assert_eq!(parse_iso("   "), None);
        assert_eq!(parse_iso("yesterday"), None);
    }

    #[test]
    fn parse_rfc2822_reads_feed_dates_and_rejects_garbage() {
        let expected = Utc.with_ymd_and_hms(2026, 10, 4, 7, 31, 8).unwrap();
        assert_eq!(
            parse_rfc2822("Sun, 04 Oct 2026 07:31:08 +0000"),
            Some(expected)
        );
        assert_eq!(
            parse_rfc2822("  Sun, 04 Oct 2026 03:31:08 -0400 "),
            Some(expected)
        );
        assert_eq!(parse_rfc2822(""), None);
        assert_eq!(parse_rfc2822("2026-10-04T07:31:08Z"), None);
    }

    #[test]
    fn from_epoch_converts_seconds_and_rejects_out_of_range() {
        assert_eq!(
            from_epoch(1_759_320_000),
            Some(Utc.with_ymd_and_hms(2025, 10, 1, 12, 0, 0).unwrap())
        );
        assert_eq!(from_epoch(i64::MAX), None);
    }

    #[test]
    fn parse_time_reads_strings_and_numbers() {
        let expected = Utc.with_ymd_and_hms(2025, 10, 1, 12, 0, 0).unwrap();
        assert_eq!(parse_time(&json!(1_759_320_000)), Some(expected));
        assert_eq!(parse_time(&json!(1_759_320_000.0)), Some(expected));
        assert_eq!(parse_time(&json!("2025-10-01T12:00:00Z")), Some(expected));
        assert_eq!(parse_time(&json!(null)), None);
        assert_eq!(parse_time(&json!(["x"])), None);
    }

    #[test]
    fn join_places_skips_blanks_and_repeats() {
        assert_eq!(
            join_places([
                "Toronto", "Canada", "", "  ", "canada", "New York", "Toronto"
            ]),
            "Toronto; Canada; New York"
        );
        // A place already named inside an earlier one adds nothing ("Canada" is in "Ontario, Canada")...
        assert_eq!(
            join_places(["Ontario, Canada", "Canada"]),
            "Ontario, Canada"
        );
        // ...but a short code is not looked up inside other words: "US" is not in "Austin".
        assert_eq!(join_places(["Austin", "US"]), "Austin; US");
        assert_eq!(join_places(["", " "]), "");
    }

    #[test]
    fn id_string_accepts_numbers_and_strings_only() {
        assert_eq!(id_string(&json!(4461)), Some("4461".to_string()));
        assert_eq!(id_string(&json!("abc")), Some("abc".to_string()));
        assert_eq!(id_string(&json!("")), None);
        assert_eq!(id_string(&json!(null)), None);
        assert_eq!(id_string(&json!({"id": 1})), None);
    }

    #[test]
    fn str_field_is_empty_for_missing_null_or_non_string() {
        let value = json!({"a": "x", "b": null, "c": 3});
        assert_eq!(str_field(&value, "a"), "x");
        assert_eq!(str_field(&value, "b"), "");
        assert_eq!(str_field(&value, "c"), "");
        assert_eq!(str_field(&value, "missing"), "");
    }

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A policy with waits so short that a test never sleeps.
    fn quick() -> RetryPolicy {
        RetryPolicy {
            max_retries: 2,
            base: Duration::from_millis(1),
            max_wait: Duration::from_secs(30),
            default_cooldown: Duration::from_secs(30),
        }
    }

    /// `get_text_with` against a board of the test's own: the shared one is never touched.
    async fn fetch_on(board: &Cooldowns, url: &str) -> anyhow::Result<String> {
        fetch(
            board,
            &reqwest::Client::new(),
            reqwest::Url::parse(url).unwrap(),
            &quick(),
        )
        .await
    }

    #[test]
    fn retry_after_reads_seconds_and_dates_and_ignores_garbage() {
        let now = Utc.with_ymd_and_hms(2026, 10, 4, 7, 31, 0).unwrap();
        assert_eq!(
            parse_retry_after("120", now),
            Some(Duration::from_secs(120))
        );
        assert_eq!(parse_retry_after(" 0 ", now), Some(Duration::ZERO));
        assert_eq!(
            parse_retry_after("Sun, 04 Oct 2026 07:32:00 +0000", now),
            Some(Duration::from_secs(60))
        );
        assert_eq!(
            parse_retry_after("Sun, 04 Oct 2026 07:30:00 +0000", now),
            Some(Duration::ZERO),
            "a date already past means: go ahead"
        );
        for garbage in [
            "",
            "abc",
            "-5",
            "1.5",
            "tomorrow",
            "99999999999999999999999",
        ] {
            assert_eq!(parse_retry_after(garbage, now), None, "{garbage:?}");
        }
    }

    #[test]
    fn backoff_doubles_adds_bounded_jitter_and_never_exceeds_the_cap() {
        let policy = RetryPolicy::STANDARD;
        assert_eq!(backoff_delay(&policy, 0, None, 0.0), Duration::from_secs(1));
        assert_eq!(backoff_delay(&policy, 1, None, 0.0), Duration::from_secs(2));
        assert_eq!(backoff_delay(&policy, 2, None, 0.0), Duration::from_secs(4));
        assert_eq!(
            backoff_delay(&policy, 0, None, 1.0),
            Duration::from_millis(1250),
            "jitter adds at most 25 %"
        );
        assert_eq!(
            backoff_delay(&policy, 0, Some(Duration::from_secs(10)), 0.0),
            Duration::from_secs(10),
            "the server's own word wins over the guess"
        );
        assert_eq!(backoff_delay(&policy, 20, None, 1.0), policy.max_wait);
        assert_eq!(
            backoff_delay(&policy, 0, None, f64::NAN),
            Duration::from_secs(1),
            "a broken random number is no jitter, not a panic"
        );
    }

    #[test]
    fn an_absurd_wait_is_capped_instead_of_overflowing() {
        let policy = RetryPolicy::STANDARD;
        assert_eq!(
            backoff_delay(&policy, 0, Some(Duration::MAX), 1.0),
            policy.max_wait
        );
        let huge_base = RetryPolicy {
            base: Duration::MAX,
            ..policy
        };
        assert_eq!(backoff_delay(&huge_base, 10, None, 1.0), policy.max_wait);
    }

    #[tokio::test]
    async fn a_429_or_503_with_retry_after_is_retried_and_then_succeeds() {
        for status in [429, 503] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/feed"))
                .respond_with(ResponseTemplate::new(status).insert_header("Retry-After", "0"))
                .up_to_n_times(1)
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/feed"))
                .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
                .mount(&server)
                .await;
            let body = get_text_with(
                &reqwest::Client::new(),
                &format!("{}/feed", server.uri()),
                &[],
                &quick(),
            )
            .await
            .unwrap();
            assert_eq!(body, "ok", "status {status}");
            assert_eq!(server.received_requests().await.unwrap().len(), 2);
        }
    }

    #[tokio::test]
    async fn a_host_that_keeps_saying_429_is_left_alone_afterwards() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;
        let url = format!("{}/feed", server.uri());
        let board = Cooldowns::default();

        let err = fetch_on(&board, &url).await.unwrap_err();
        assert!(err.downcast_ref::<RateLimited>().is_some(), "{err:#}");
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1 + quick().max_retries as usize
        );

        // The host now rests: the next call does not even send a request.
        let again = fetch_on(&board, &url).await.unwrap_err();
        assert!(again.downcast_ref::<RateLimited>().is_some(), "{again:#}");
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1 + quick().max_retries as usize,
            "no new request while the host cools down"
        );
    }

    #[tokio::test]
    async fn a_local_address_is_never_put_on_the_shared_board() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;
        let url = format!("{}/feed", server.uri());
        let client = reqwest::Client::new();
        for round in 1..=2 {
            let err = get_text_with(&client, &url, &[], &quick())
                .await
                .unwrap_err();
            assert!(err.downcast_ref::<RateLimited>().is_some(), "{err:#}");
            assert_eq!(
                server.received_requests().await.unwrap().len(),
                round * (1 + quick().max_retries as usize),
                "round {round}: a local stub is asked again, nothing is remembered about it"
            );
        }
    }

    #[tokio::test]
    async fn a_retry_after_longer_than_the_cap_is_not_waited_for_or_retried() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "3600"))
            .mount(&server)
            .await;
        let url = format!("{}/feed", server.uri());
        let err = fetch_on(&Cooldowns::default(), &url).await.unwrap_err();
        let limited = err
            .downcast_ref::<RateLimited>()
            .expect("a RateLimited error");
        assert_eq!(limited.wait_secs, 3600);
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_retry_after_of_a_lifetime_cannot_panic_the_run() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(429).insert_header("Retry-After", "18446744073709551615"),
            )
            .mount(&server)
            .await;
        let url = format!("{}/feed", server.uri());
        let board = Cooldowns::default();
        let err = fetch_on(&board, &url).await.unwrap_err();
        let limited = err
            .downcast_ref::<RateLimited>()
            .expect("a RateLimited error");
        assert_eq!(
            limited.wait_secs,
            MAX_COOLDOWN.as_secs(),
            "the host rests for a day at most, whatever it claims"
        );
        let again = fetch_on(&board, &url).await.unwrap_err();
        assert!(again.downcast_ref::<RateLimited>().is_some(), "{again:#}");
    }

    #[tokio::test]
    async fn other_errors_are_not_retried_and_are_not_rate_limits() {
        for status in [404, 500] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(status))
                .mount(&server)
                .await;
            let err = get_text_with(
                &reqwest::Client::new(),
                &format!("{}/feed", server.uri()),
                &[],
                &quick(),
            )
            .await
            .unwrap_err();
            assert!(
                err.downcast_ref::<RateLimited>().is_none(),
                "{status}: {err:#}"
            );
            assert_eq!(
                server.received_requests().await.unwrap().len(),
                1,
                "{status}"
            );
        }
    }
}
