//! Hacker News "Ask HN: Who is hiring?" through the Algolia API.
//!
//! `GET {base}/api/v1/search_by_date?tags=story,author_whoishiring` finds the monthly threads and
//! `GET {base}/api/v1/items/{id}` returns one thread with its top-level comments. Every top-level
//! comment is one posting, written by the employer, usually as `Company | Role | Location | ...`.
//! Applications happen by email or on the company site named in the comment, so each job points at
//! its HN comment and the gate must treat `news.ycombinator.com` as `listing_only`: these postings
//! feed discovery and the digest, never the browser.

use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use super::Source;
use super::base::{failure, get_json, id_string, parse_time, str_field};
use crate::models::{FetchResult, Job};
use crate::scoring::parse_salary_monthly_usd;
use crate::text::html_to_text;

const DEFAULT_BASE_URL: &str = "https://hn.algolia.com";

/// Words that mark a header segment as a job title rather than a company, place or stack.
const ROLE_WORDS: [&str; 17] = [
    "engineer",
    "developer",
    "designer",
    "manager",
    "scientist",
    "architect",
    "analyst",
    "researcher",
    "lead",
    "director",
    "sre",
    "devops",
    "founder",
    "specialist",
    "consultant",
    "head of",
    "cto",
];

/// The `[hackernews]` table of `profile/sources.toml`; present (even empty) turns the source on.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HackerNewsConfig {
    /// How many of the latest monthly threads to read.
    pub months: u32,
    /// Keep only postings that say they are remote.
    pub remote_only: bool,
}

impl Default for HackerNewsConfig {
    fn default() -> Self {
        Self {
            months: 1,
            remote_only: true,
        }
    }
}

pub struct HackerNewsSource {
    client: Client,
    base_url: String,
    config: HackerNewsConfig,
}

impl HackerNewsSource {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            base_url: DEFAULT_BASE_URL.to_string(),
            config: HackerNewsConfig::default(),
        }
    }

    pub fn with_config(mut self, config: HackerNewsConfig) -> Self {
        self.config = config;
        self
    }

    /// Points the source at another server (tests use a wiremock server).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    /// The ids of the latest "Who is hiring?" threads, newest first. The same account also posts
    /// "Who wants to be hired?" and "Freelancer?" threads, which are not postings.
    async fn latest_threads(&self) -> anyhow::Result<Vec<String>> {
        let url = format!("{}/api/v1/search_by_date", self.base_url);
        let query = [("tags", "story,author_whoishiring"), ("hitsPerPage", "8")];
        let body = get_json(&self.client, &url, &query).await?;
        let hits = body["hits"]
            .as_array()
            .context("response has no `hits` array")?;
        Ok(hits
            .iter()
            .filter(|hit| {
                str_field(hit, "title")
                    .to_lowercase()
                    .contains("who is hiring")
            })
            .filter_map(|hit| id_string(&hit["objectID"]))
            .take(self.config.months as usize)
            .collect())
    }

    async fn fetch_thread(&self, id: &str, result: &mut FetchResult) -> anyhow::Result<()> {
        let url = format!("{}/api/v1/items/{id}", self.base_url);
        let thread = get_json(&self.client, &url, &[]).await?;
        let comments = thread["children"]
            .as_array()
            .context("thread has no `children` array")?;
        for comment in comments {
            // Deleted and dead comments have no text; comments without a header are not postings.
            if let Some(job) = parse_comment(comment)
                && (!self.config.remote_only || job.remote)
            {
                result.jobs.push(job);
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Source for HackerNewsSource {
    fn name(&self) -> &str {
        "hackernews"
    }

    async fn fetch(&self) -> FetchResult {
        let mut result = FetchResult::default();
        let threads = match self.latest_threads().await {
            Ok(threads) => threads,
            Err(error) => {
                result.errors.push(failure("hackernews", "threads", &error));
                return result;
            }
        };
        for id in threads {
            if let Err(error) = self.fetch_thread(&id, &mut result).await {
                result.errors.push(failure("hackernews", &id, &error));
            }
        }
        result
    }
}

/// Splits a header line into its segments: on `|` when there is one, else on dashes.
fn split_header(header: &str) -> Vec<String> {
    let parts: Vec<&str> = if header.contains('|') {
        header.split('|').collect()
    } else if header.contains(['\u{2013}', '\u{2014}']) {
        header.split(['\u{2013}', '\u{2014}']).collect()
    } else {
        header.split(" - ").collect()
    };
    parts
        .iter()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

/// `"PrairieLearn (Remote US)"` -> `("PrairieLearn", Some("Remote US"))`.
fn split_company(segment: &str) -> (String, Option<String>) {
    if let Some(open) = segment.rfind('(')
        && segment.ends_with(')')
        && open > 0
    {
        let company = segment[..open].trim().to_string();
        let note = segment[open + 1..segment.len() - 1].trim().to_string();
        return (company, (!note.is_empty()).then_some(note));
    }
    (segment.to_string(), None)
}

/// Header segments that describe pay, a link or the kind of contract are not a place.
fn is_place_segment(segment: &str) -> bool {
    const NOT_PLACE: [&str; 9] = [
        "http",
        "$",
        "@",
        "equity",
        "salary",
        "full-time",
        "full time",
        "part-time",
        "contract",
    ];
    let lower = segment.to_lowercase();
    segment.len() <= 60 && !NOT_PLACE.iter().any(|word| lower.contains(word))
}

fn has_role_word(segment: &str) -> bool {
    let lower = segment.to_lowercase();
    ROLE_WORDS.iter().any(|word| lower.contains(word))
}

/// One top-level comment as a job, or `None` when it has no text or no `Company | Role` header.
fn parse_comment(comment: &Value) -> Option<Job> {
    let external_id = id_string(&comment["id"])?;
    let text = str_field(comment, "text");
    if text.is_empty() {
        return None;
    }
    // Algolia keeps the header in the first paragraph; later paragraphs are separated by `<p>`.
    let head_html = text.split_once("<p>").map_or(text, |(head, _)| head);
    let header = html_to_text(head_html)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    let segments = split_header(&header);
    if segments.len() < 2 {
        return None;
    }
    let (company, note) = split_company(&segments[0]);
    if company.is_empty() {
        return None;
    }
    let title_index = segments
        .iter()
        .skip(1)
        .position(|segment| has_role_word(segment))
        .map(|index| index + 1);
    let title = match title_index {
        Some(index) => segments[index].clone(),
        None => format!("Open roles at {company}"),
    };
    let mut place: Vec<String> = note.into_iter().collect();
    place.extend(
        segments
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(index, segment)| Some(*index) != title_index && is_place_segment(segment))
            .map(|(_, segment)| segment.clone()),
    );
    let description = html_to_text(text);
    let header_lower = header.to_lowercase();
    let onsite = header_lower.contains("onsite") || header_lower.contains("on-site");
    let remote = header_lower.contains("remote")
        || (!onsite && description.to_lowercase().contains("remote"));
    let (salary_min, salary_max) = parse_salary_monthly_usd(&description);
    let url = format!("https://news.ycombinator.com/item?id={external_id}");
    Some(Job {
        source: "hackernews".to_string(),
        external_id,
        company,
        title,
        url: url.clone(),
        apply_url: url,
        location: place.join(", "),
        remote,
        description,
        salary_min_monthly_usd: salary_min,
        salary_max_monthly_usd: salary_max,
        posted_at: parse_time(&comment["created_at"]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Three real top-level comments of the October 2026 thread, as Algolia returns them.
    fn real_thread() -> Value {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/sources/hn_whoishiring.json"
        ))
        .unwrap()
    }

    fn stories() -> Value {
        json!({"hits": [
            {"objectID": "300", "title": "Ask HN: Who wants to be hired? (October 2026)"},
            {"objectID": "200", "title": "Ask HN: Who is hiring? (October 2026)"},
            {"objectID": "100", "title": "Ask HN: Who is hiring? (September 2026)"}
        ]})
    }

    #[test]
    fn parses_a_remote_posting_with_dashes_and_a_company_note() {
        let thread = real_thread();
        let job = parse_comment(&thread["children"][0]).unwrap();

        assert_eq!(job.key(), "hackernews:49922584");
        assert_eq!(job.company, "PrairieLearn");
        assert_eq!(job.title, "Full-Stack Software Engineer");
        assert!(job.location.starts_with("Remote US"), "{}", job.location);
        assert!(job.remote);
        assert_eq!(job.url, "https://news.ycombinator.com/item?id=49922584");
        assert_eq!(job.apply_url, job.url);
        assert!(job.description.contains("https://www.prairielearn.com"));
        assert_eq!(
            job.posted_at,
            crate::sources::base::parse_iso("2026-10-01T15:02:37.000Z")
        );
    }

    #[test]
    fn an_onsite_posting_is_not_remote_and_a_header_without_role_gets_a_generic_title() {
        let thread = real_thread();
        let job = parse_comment(&thread["children"][1]).unwrap();

        assert_eq!(job.company, "Shepherd");
        assert_eq!(job.title, "Open roles at Shepherd");
        assert!(!job.remote);
        assert!(job.location.contains("ONSITE"), "{}", job.location);
    }

    #[test]
    fn parses_a_pipe_separated_remote_posting() {
        let thread = real_thread();
        let job = parse_comment(&thread["children"][2]).unwrap();

        assert_eq!(job.company, "We The Flywheel");
        assert_eq!(job.title, "Production Designer");
        assert!(job.remote);
        assert!(job.location.contains("REMOTE"), "{}", job.location);
    }

    #[test]
    fn the_location_leaves_out_pay_links_and_contract_kind() {
        let comment = json!({
            "id": 6, "created_at": "2026-10-02T00:00:00.000Z",
            "text": "Acme | Backend Engineer | REMOTE (US) | Full-time | $150k - $180k | https://acme.test/jobs<p>Details."
        });
        let job = parse_comment(&comment).unwrap();

        assert_eq!(job.location, "REMOTE (US)");
        assert!(
            job.description.contains("$150k"),
            "the text keeps everything"
        );
    }

    #[test]
    fn a_comment_without_a_structured_header_is_not_a_posting() {
        let reply = json!({"id": 9, "text": "Is this still open?", "created_at": "2026-10-02T00:00:00.000Z"});
        assert!(parse_comment(&reply).is_none());
        assert!(parse_comment(&json!({"id": 9, "text": null})).is_none());
        assert!(parse_comment(&json!({"text": "Acme | Engineer"})).is_none());
    }

    #[test]
    fn a_pay_range_in_the_text_is_read() {
        let comment = json!({
            "id": 5, "created_at": "2026-10-02T00:00:00.000Z",
            "text": "Acme | Backend Engineer | REMOTE<p>We pay $90K - $130K a year."
        });
        let job = parse_comment(&comment).unwrap();

        assert_eq!(job.salary_min_monthly_usd.map(f64::round), Some(7_500.0));
    }

    #[tokio::test]
    async fn fetch_reads_the_latest_hiring_thread_and_keeps_remote_postings() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/search_by_date"))
            .and(query_param("tags", "story,author_whoishiring"))
            .respond_with(ResponseTemplate::new(200).set_body_json(stories()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/items/200"))
            .respond_with(ResponseTemplate::new(200).set_body_json(real_thread()))
            .expect(1)
            .mount(&server)
            .await;

        let source = HackerNewsSource::new(Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let companies: Vec<&str> = result.jobs.iter().map(|job| job.company.as_str()).collect();
        assert_eq!(companies, ["PrairieLearn", "We The Flywheel"]);
    }

    #[tokio::test]
    async fn fetch_can_keep_onsite_postings_and_read_two_months() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/search_by_date"))
            .respond_with(ResponseTemplate::new(200).set_body_json(stories()))
            .mount(&server)
            .await;
        for id in ["200", "100"] {
            Mock::given(method("GET"))
                .and(path(format!("/api/v1/items/{id}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(real_thread()))
                .expect(1)
                .mount(&server)
                .await;
        }

        let source = HackerNewsSource::new(Client::new())
            .with_base_url(server.uri())
            .with_config(HackerNewsConfig {
                months: 2,
                remote_only: false,
            });
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.jobs.len(), 6);
    }

    #[tokio::test]
    async fn fetch_records_a_failed_thread_and_a_failed_search() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/search_by_date"))
            .respond_with(ResponseTemplate::new(200).set_body_json(stories()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/items/200"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let result = HackerNewsSource::new(Client::new())
            .with_base_url(server.uri())
            .fetch()
            .await;
        assert!(result.jobs.is_empty());
        assert!(
            result.errors[0].starts_with("hackernews:200: "),
            "{}",
            result.errors[0]
        );

        let down = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&down)
            .await;
        let result = HackerNewsSource::new(Client::new())
            .with_base_url(down.uri())
            .fetch()
            .await;
        assert!(
            result.errors[0].starts_with("hackernews:threads: "),
            "{}",
            result.errors[0]
        );
    }

    #[test]
    fn config_defaults_and_unknown_keys() {
        let cfg: HackerNewsConfig = toml::from_str("").unwrap();
        assert_eq!(cfg, HackerNewsConfig::default());
        assert!(cfg.remote_only && cfg.months == 1);
        assert!(toml::from_str::<HackerNewsConfig>("month = 2").is_err());
    }
}
