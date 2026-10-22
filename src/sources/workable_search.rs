//! Workable's cross-company job search: `GET {base}/api/v1/jobs?query=..&location=..&workplace=remote`.
//!
//! This is the public search behind jobs.workable.com (the site invites indexing and its
//! robots.txt leaves the API open), no key. A page holds 20 jobs and `nextPageToken` asks for the
//! next one; the last page has none. `location=Chile` returns the jobs that list Chile among their
//! places, which is how a company says it hires someone who lives there. It is a different source
//! from `workable` (one company's own board): every job here keeps its jobs.workable.com page as
//! `url`, and the application happens on the Workable form behind it, which the form filler does
//! not drive yet, so the gate treats `workable.com` as `listing_only`.

use std::collections::HashSet;
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use super::Source;
use super::base::{
    REQUEST_PAUSE, failure, get_json, id_string, join_places, parse_iso, skipped_error, str_field,
};
use crate::models::{FetchResult, Job};
use crate::text::html_to_text;

const DEFAULT_BASE_URL: &str = "https://jobs.workable.com";

/// The `[workable_search]` table of `profile/sources.toml`; present (even empty) turns the source on.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkableSearchConfig {
    /// One search per query.
    pub queries: Vec<String>,
    /// A country the jobs must list (`Chile`); empty means no place filter.
    pub location: String,
    /// Pages of 20 jobs read per query.
    pub max_pages: u32,
}

impl Default for WorkableSearchConfig {
    fn default() -> Self {
        Self {
            queries: [
                "software engineer",
                "full stack developer",
                "ai engineer",
                "backend developer",
            ]
            .map(String::from)
            .to_vec(),
            location: "Chile".to_string(),
            max_pages: 2,
        }
    }
}

pub struct WorkableSearchSource {
    client: Client,
    base_url: String,
    config: WorkableSearchConfig,
    pause: Duration,
}

impl WorkableSearchSource {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            base_url: DEFAULT_BASE_URL.to_string(),
            config: WorkableSearchConfig::default(),
            pause: REQUEST_PAUSE,
        }
    }

    pub fn with_config(mut self, config: WorkableSearchConfig) -> Self {
        self.config = config;
        self
    }

    /// Points the source at another server (tests use a wiremock server).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    /// Waits this long between two requests (tests use none).
    pub fn with_pause(mut self, pause: Duration) -> Self {
        self.pause = pause;
        self
    }

    /// Follows the pages of one query. Jobs go straight into `result`, so when a later page fails
    /// the earlier pages are kept and only the error is returned. `seen` drops a job that two
    /// queries both return; `requests` counts every request of the run, to pause between them.
    async fn fetch_query(
        &self,
        query: &str,
        seen: &mut HashSet<String>,
        requests: &mut u32,
        result: &mut FetchResult,
    ) -> anyhow::Result<()> {
        let url = format!("{}/api/v1/jobs", self.base_url);
        let mut token = String::new();
        for _ in 0..self.config.max_pages {
            if *requests > 0 {
                tokio::time::sleep(self.pause).await;
            }
            *requests += 1;
            let mut params = vec![("query", query), ("workplace", "remote")];
            if !self.config.location.is_empty() {
                params.push(("location", self.config.location.as_str()));
            }
            if !token.is_empty() {
                params.push(("pageToken", token.as_str()));
            }
            let body = get_json(&self.client, &url, &params).await?;
            let items = body["jobs"]
                .as_array()
                .context("response has no `jobs` array")?;
            let mut skipped = 0;
            for item in items {
                match parse_job(item) {
                    Some(job) => {
                        if seen.insert(job.external_id.clone()) {
                            result.jobs.push(job);
                        }
                    }
                    None => skipped += 1,
                }
            }
            if skipped > 0 {
                result
                    .errors
                    .push(skipped_error("workable_search", query, skipped));
            }
            token = str_field(&body, "nextPageToken").to_string();
            if token.is_empty() {
                break;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Source for WorkableSearchSource {
    fn name(&self) -> &str {
        "workable_search"
    }

    async fn fetch(&self) -> FetchResult {
        let mut result = FetchResult::default();
        let mut seen = HashSet::new();
        let mut requests = 0;
        for query in &self.config.queries {
            if let Err(error) = self
                .fetch_query(query, &mut seen, &mut requests, &mut result)
                .await
            {
                result
                    .errors
                    .push(failure("workable_search", query, &error));
            }
        }
        result
    }
}

/// The text of an HTML section, or `""` when it is missing.
fn section(item: &Value, key: &str) -> String {
    html_to_text(str_field(item, key))
}

/// A job is remote when its `workplace` says so. `locations` lists the places it is open to, with
/// `TELECOMMUTE` marking the remote option itself ("Chile", "Santiago, Chile"); a job without that
/// list falls back to the country and city of `location`. The full text is the description, then
/// the requirements and the benefits the page shows below it.
fn parse_job(item: &Value) -> Option<Job> {
    let external_id = id_string(&item["id"])?;
    let title = str_field(item, "title");
    let url = str_field(item, "url");
    if title.is_empty() || url.is_empty() {
        return None;
    }
    let listed: Vec<&str> = item["locations"]
        .as_array()
        .map(|places| {
            places
                .iter()
                .filter_map(Value::as_str)
                .filter(|place| *place != "TELECOMMUTE")
                .collect()
        })
        .unwrap_or_default();
    let location = if listed.is_empty() {
        let place = &item["location"];
        join_places([
            str_field(place, "city"),
            str_field(place, "subregion"),
            str_field(place, "countryName"),
        ])
    } else {
        join_places(listed)
    };
    let mut description = ["description", "requirementsSection", "benefitsSection"]
        .iter()
        .map(|key| section(item, key))
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    for (label, value) in [
        ("Department", str_field(item, "department")),
        ("Type", str_field(item, "employmentType")),
    ] {
        if !value.is_empty() {
            description = format!("{description}\n{label}: {value}");
        }
    }
    Some(Job {
        source: "workable_search".to_string(),
        external_id,
        company: str_field(&item["company"], "title").to_string(),
        title: title.to_string(),
        url: url.to_string(),
        apply_url: url.to_string(),
        location,
        remote: str_field(item, "workplace") == "remote",
        description,
        salary_min_monthly_usd: None,
        salary_max_monthly_usd: None,
        posted_at: parse_iso(str_field(item, "created")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A trimmed real response from `jobs.workable.com/api/v1/jobs?query=software engineer&location=Chile&workplace=remote`.
    fn page_one() -> Value {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/sources/workable_search.json"
        ))
        .unwrap()
    }

    /// The trimmed second (last) page of the same search: no `nextPageToken`.
    fn page_two() -> Value {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/sources/workable_search_page2.json"
        ))
        .unwrap()
    }

    fn config(queries: &[&str], max_pages: u32) -> WorkableSearchConfig {
        WorkableSearchConfig {
            queries: queries.iter().map(|query| query.to_string()).collect(),
            max_pages,
            ..WorkableSearchConfig::default()
        }
    }

    fn source(server: &MockServer, config: WorkableSearchConfig) -> WorkableSearchSource {
        WorkableSearchSource::new(Client::new())
            .with_base_url(server.uri())
            .with_config(config)
            .with_pause(Duration::ZERO)
    }

    fn token() -> String {
        page_one()["nextPageToken"].as_str().unwrap().to_string()
    }

    #[test]
    fn parses_a_real_job() {
        let payload = page_one();
        let job = parse_job(&payload["jobs"][0]).unwrap();

        assert_eq!(
            job.key(),
            "workable_search:2c507af9-3a33-4d77-a8cc-7babaffbc705"
        );
        assert_eq!(job.company, "Enroute");
        assert_eq!(
            job.title,
            "Software Engineer (Python/Typescript (API integrations))"
        );
        assert_eq!(job.location, "Chile");
        assert!(job.remote);
        assert_eq!(
            job.url,
            "https://jobs.workable.com/view/6toeS2gWbTnuz4KdnKCtRP/remote-software-engineer-(python%2Ftypescript-(api-integrations))-in-chile-at-enroute"
        );
        assert_eq!(job.apply_url, job.url);
        assert_eq!(
            job.posted_at,
            crate::sources::base::parse_iso("2026-09-22T16:28:39.737Z")
        );
        for expected in [
            "We love technology",
            "Must-Haves",
            "Monetary compensation",
            "Department: Rocket",
            "Type: Full-time",
        ] {
            assert!(
                job.description.contains(expected),
                "{expected:?} missing from {}",
                job.description
            );
        }
        assert!(!job.description.contains('<'), "{}", job.description);
        assert_eq!(
            (job.salary_min_monthly_usd, job.salary_max_monthly_usd),
            (None, None)
        );
    }

    #[test]
    fn the_location_is_the_places_the_job_lists_without_the_remote_marker() {
        let one = page_one();
        let two = page_two();

        assert_eq!(parse_job(&one["jobs"][1]).unwrap().location, "Chile");
        assert_eq!(
            parse_job(&one["jobs"][2]).unwrap().location,
            "Santiago, Chile"
        );
        assert_eq!(
            parse_job(&two["jobs"][0]).unwrap().location,
            "Santiago, Santiago Metropolitan Region, Chile"
        );
        // Without `locations`, the country and city of `location` stand in.
        let bare = json!({
            "id": "x", "title": "Dev", "url": "https://jobs.workable.com/view/x/dev",
            "workplace": "remote",
            "location": {"city": "Lima", "subregion": null, "countryName": "Peru"}
        });
        assert_eq!(parse_job(&bare).unwrap().location, "Lima; Peru");
    }

    #[test]
    fn only_the_remote_workplace_is_remote() {
        let mut item = json!({
            "id": "x", "title": "Dev", "url": "https://jobs.workable.com/view/x/dev",
            "locations": ["Santiago, Chile"], "workplace": "hybrid"
        });
        assert!(!parse_job(&item).unwrap().remote);
        item["workplace"] = json!("remote");
        assert!(parse_job(&item).unwrap().remote);
    }

    #[test]
    fn the_region_filter_reads_the_places_the_job_lists() {
        use crate::discovery::region_ok;

        let payload = page_one();
        assert!(region_ok(&parse_job(&payload["jobs"][0]).unwrap()), "Chile");
        let us = json!({
            "id": "x", "title": "Dev", "url": "https://jobs.workable.com/view/x/dev",
            "locations": ["TELECOMMUTE", "United States"], "workplace": "remote"
        });
        assert!(!region_ok(&parse_job(&us).unwrap()), "a US-only job");
    }

    #[test]
    fn a_job_without_id_title_or_url_is_not_a_job() {
        assert!(
            parse_job(&json!({"title": "Dev", "url": "https://jobs.workable.com/view/x"}))
                .is_none()
        );
        assert!(
            parse_job(&json!({"id": "x", "url": "https://jobs.workable.com/view/x"})).is_none()
        );
        assert!(parse_job(&json!({"id": "x", "title": "Dev"})).is_none());
    }

    #[tokio::test]
    async fn fetch_follows_the_page_token_with_the_filters() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/jobs"))
            .and(query_param("query", "software engineer"))
            .and(query_param("location", "Chile"))
            .and(query_param("workplace", "remote"))
            .and(query_param_is_missing("pageToken"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_one()))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/jobs"))
            .and(query_param("pageToken", token()))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_two()))
            .expect(1)
            .mount(&server)
            .await;

        let result = source(&server, config(&["software engineer"], 5))
            .fetch()
            .await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(
            result.jobs.len(),
            5,
            "three jobs, then two, and no third request"
        );
        assert!(
            result
                .jobs
                .iter()
                .all(|job| job.source == "workable_search")
        );
    }

    #[tokio::test]
    async fn fetch_stops_at_max_pages_and_drops_repeats_across_queries() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/jobs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_one()))
            .expect(2)
            .mount(&server)
            .await;

        let result = source(&server, config(&["a", "b"], 1)).fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(
            result.jobs.len(),
            3,
            "one page per query, and the same three jobs once"
        );
    }

    #[tokio::test]
    async fn an_empty_location_sends_no_place_filter() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/jobs"))
            .and(query_param_is_missing("location"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_two()))
            .expect(1)
            .mount(&server)
            .await;
        let mut cfg = config(&["x"], 1);
        cfg.location = String::new();

        let result = source(&server, cfg).fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.jobs.len(), 2);
    }

    #[tokio::test]
    async fn fetch_records_a_failed_query_and_keeps_the_others() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/jobs"))
            .and(query_param("query", "limited"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/jobs"))
            .and(query_param("query", "fine"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_two()))
            .mount(&server)
            .await;

        let result = source(&server, config(&["limited", "fine"], 1))
            .fetch()
            .await;

        assert_eq!(result.jobs.len(), 2);
        assert_eq!(result.errors.len(), 1);
        assert!(
            result.errors[0].starts_with("workable_search:limited: "),
            "{}",
            result.errors[0]
        );
    }

    #[tokio::test]
    async fn fetch_records_skipped_postings_and_a_body_without_jobs() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/jobs"))
            .and(query_param("query", "bare"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jobs": [{"id": "1", "title": "No url"}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/jobs"))
            .and(query_param("query", "html"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"title": "Workable"})))
            .mount(&server)
            .await;

        let result = source(&server, config(&["bare", "html"], 1)).fetch().await;

        assert!(result.jobs.is_empty());
        assert_eq!(result.errors.len(), 2, "{:?}", result.errors);
        assert_eq!(
            result.errors[0],
            "workable_search:bare: skipped 1 postings missing id, title or url"
        );
        assert!(
            result.errors[1].starts_with("workable_search:html: "),
            "{}",
            result.errors[1]
        );
    }

    #[test]
    fn config_defaults_and_unknown_keys() {
        let cfg: WorkableSearchConfig = toml::from_str("").unwrap();
        assert_eq!(cfg, WorkableSearchConfig::default());
        assert_eq!(cfg.location, "Chile");
        assert_eq!(cfg.max_pages, 2);
        let cfg: WorkableSearchConfig =
            toml::from_str("queries = [\"rust\"]\nlocation = \"Peru\"").unwrap();
        assert_eq!(
            (cfg.queries, cfg.location.as_str()),
            (vec!["rust".to_string()], "Peru")
        );
        assert!(toml::from_str::<WorkableSearchConfig>("locaton = \"Chile\"").is_err());
    }
}
