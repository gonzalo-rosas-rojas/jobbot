//! Himalayas remote jobs: `GET {base}/jobs/api/search?q=..&country=..&sort=recent&page=N`.
//!
//! Free public API, no key. The data refreshes every 24 hours, a page holds at most 20 jobs and
//! going too fast gets HTTP 429, so the source asks for few queries and few pages. Himalayas wants
//! a link back to himalayas.app, so every job keeps its Himalayas page as `url`. That page is
//! where the application happens (the API has no company apply URL): the gate must treat
//! `himalayas.app` as `listing_only`, like `remotive.com`.

use std::collections::HashSet;

use anyhow::Context;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use super::Source;
use super::base::{failure, get_json, parse_time, skipped_error, str_field};
use super::pay::{monthly_usd_range, number};
use crate::models::{FetchResult, Job};
use crate::text::html_to_text;

const DEFAULT_BASE_URL: &str = "https://himalayas.app";
/// The API serves at most 20 jobs per page.
const PAGE_SIZE: usize = 20;

/// The `[himalayas]` table of `profile/sources.toml`; present (even empty) turns the source on.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HimalayasConfig {
    /// ISO country the candidate lives in; Himalayas returns the jobs open to it and worldwide ones.
    pub country: String,
    /// One search per query, newest first.
    pub queries: Vec<String>,
    /// Pages of 20 jobs read per query.
    pub max_pages: u32,
    /// Himalayas values (`Full Time`, `Contractor`, ...); empty means every type.
    pub employment_types: Vec<String>,
}

impl Default for HimalayasConfig {
    fn default() -> Self {
        Self {
            country: "CL".to_string(),
            queries: ["ai engineer", "full stack engineer", "software engineer"]
                .map(String::from)
                .to_vec(),
            max_pages: 2,
            employment_types: Vec::new(),
        }
    }
}

pub struct HimalayasSource {
    client: Client,
    base_url: String,
    config: HimalayasConfig,
}

impl HimalayasSource {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            base_url: DEFAULT_BASE_URL.to_string(),
            config: HimalayasConfig::default(),
        }
    }

    pub fn with_config(mut self, config: HimalayasConfig) -> Self {
        self.config = config;
        self
    }

    /// Points the source at another server (tests use a wiremock server).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    /// Follows the pages of one query. Jobs go straight into `result`, so when a later page fails
    /// the earlier pages are kept and only the error is returned. `seen` drops a job that two
    /// queries both return.
    async fn fetch_query(
        &self,
        query: &str,
        seen: &mut HashSet<String>,
        result: &mut FetchResult,
    ) -> anyhow::Result<()> {
        let url = format!("{}/jobs/api/search", self.base_url);
        let employment = self.config.employment_types.join(",");
        let now = Utc::now();
        for page in 1..=self.config.max_pages {
            let page_text = page.to_string();
            let mut params = vec![
                ("q", query),
                ("sort", "recent"),
                ("page", page_text.as_str()),
            ];
            if !self.config.country.is_empty() {
                params.push(("country", self.config.country.as_str()));
            }
            if !employment.is_empty() {
                params.push(("employment_type", employment.as_str()));
            }
            let body = get_json(&self.client, &url, &params).await?;
            let items = body["jobs"]
                .as_array()
                .context("response has no `jobs` array")?;
            let mut skipped = 0;
            for item in items {
                if is_expired(item, now) {
                    continue;
                }
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
                    .push(skipped_error("himalayas", query, skipped));
            }
            let total = body["totalCount"].as_u64().unwrap_or(0);
            if items.len() < PAGE_SIZE || (total > 0 && u64::from(page) * PAGE_SIZE as u64 >= total)
            {
                break;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Source for HimalayasSource {
    fn name(&self) -> &str {
        "himalayas"
    }

    async fn fetch(&self) -> FetchResult {
        let mut result = FetchResult::default();
        let mut seen = HashSet::new();
        for query in &self.config.queries {
            if let Err(error) = self.fetch_query(query, &mut seen, &mut result).await {
                result.errors.push(failure("himalayas", query, &error));
            }
        }
        result
    }
}

/// True when the posting carries an `expiryDate` (Unix seconds) that already passed.
fn is_expired(item: &Value, now: DateTime<Utc>) -> bool {
    parse_time(&item["expiryDate"]).is_some_and(|expiry| expiry < now)
}

/// A comma-separated list from a JSON array of strings.
fn joined(item: &Value, key: &str) -> String {
    item[key]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

/// Every Himalayas job is remote. `locationRestrictions` lists the countries it is open to; an
/// empty list means no restriction. Pay arrives as numbers with a currency and a period.
fn parse_job(item: &Value) -> Option<Job> {
    let url = match str_field(item, "applicationLink") {
        "" => str_field(item, "guid"),
        link => link,
    };
    let title = str_field(item, "title");
    if title.is_empty() || url.is_empty() {
        return None;
    }
    let page_slug = url.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let external_id = match str_field(item, "companySlug") {
        "" => page_slug.to_string(),
        company_slug => format!("{company_slug}-{page_slug}"),
    };
    let restrictions = joined(item, "locationRestrictions");
    let location = if restrictions.is_empty() {
        "Worldwide".to_string()
    } else {
        restrictions
    };
    let mut description = html_to_text(str_field(item, "description"));
    for (label, value) in [
        (
            "Employment type",
            str_field(item, "employmentType").to_string(),
        ),
        ("Seniority", joined(item, "seniority")),
    ] {
        if !value.is_empty() {
            description = format!("{description}\n{label}: {value}");
        }
    }
    let (salary_min, salary_max) = monthly_usd_range(
        number(&item["minSalary"]),
        number(&item["maxSalary"]),
        str_field(item, "currency"),
        str_field(item, "salaryPeriod"),
    );
    Some(Job {
        source: "himalayas".to_string(),
        external_id,
        company: str_field(item, "companyName").to_string(),
        title: title.to_string(),
        url: url.to_string(),
        apply_url: url.to_string(),
        location,
        remote: true,
        description,
        salary_min_monthly_usd: salary_min,
        salary_max_monthly_usd: salary_max,
        posted_at: parse_time(&item["pubDate"]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A trimmed real response from `himalayas.app/jobs/api/search?country=CL&q=ai engineer`.
    /// The real `expiryDate` values are pushed far into the future so the tests do not start
    /// failing the day the captured postings expire.
    fn real_payload() -> Value {
        let mut payload: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/sources/himalayas.json"))
                .unwrap();
        for job in payload["jobs"].as_array_mut().unwrap() {
            job["expiryDate"] = json!(4_000_000_000_i64);
        }
        payload
    }

    fn config(queries: &[&str], max_pages: u32) -> HimalayasConfig {
        HimalayasConfig {
            queries: queries.iter().map(|query| query.to_string()).collect(),
            max_pages,
            ..HimalayasConfig::default()
        }
    }

    /// `count` distinct jobs shaped like the real ones.
    fn jobs(count: usize, offset: usize) -> Vec<Value> {
        (offset..offset + count)
            .map(|n| {
                json!({
                    "title": format!("Engineer {n}"),
                    "companyName": "Acme",
                    "companySlug": "acme",
                    "applicationLink": format!("https://himalayas.app/companies/acme/jobs/engineer-{n}"),
                    "guid": format!("https://himalayas.app/companies/acme/jobs/engineer-{n}"),
                    "locationRestrictions": [],
                    "pubDate": 1_790_000_000,
                })
            })
            .collect()
    }

    #[test]
    fn parses_a_real_job() {
        let payload = real_payload();
        let job = parse_job(&payload["jobs"][0]).unwrap();

        assert_eq!(job.key(), "himalayas:lingo-dev-senior-content-engineer");
        assert_eq!(job.company, "Lingo.dev");
        assert!(job.remote);
        assert_eq!(
            job.url,
            "https://himalayas.app/companies/lingo-dev/jobs/senior-content-engineer"
        );
        assert_eq!(job.apply_url, job.url);
        assert_eq!(job.location, "Worldwide");
        assert_eq!(job.salary_min_monthly_usd, Some(5_000.0));
        assert_eq!(job.salary_max_monthly_usd, Some(7_500.0));
        assert_eq!(
            job.posted_at,
            crate::sources::base::from_epoch(1_789_539_355)
        );
        assert!(job.description.contains("Employment type:"));
        assert!(job.description.contains("Seniority: Mid-level, Senior"));
    }

    #[test]
    fn restrictions_become_the_location() {
        let item = json!({
            "title": "Dev", "applicationLink": "https://himalayas.app/companies/a/jobs/dev",
            "locationRestrictions": ["Chile", "Argentina"]
        });
        assert_eq!(parse_job(&item).unwrap().location, "Chile, Argentina");
    }

    #[test]
    fn a_job_without_title_or_link_is_not_a_job() {
        assert!(parse_job(&json!({"title": "", "applicationLink": "https://x.test/a"})).is_none());
        assert!(parse_job(&json!({"title": "Dev"})).is_none());
    }

    #[test]
    fn only_a_past_expiry_date_expires_a_job() {
        let now = DateTime::from_timestamp(1_790_000_000, 0).unwrap();
        assert!(is_expired(&json!({"expiryDate": 1_700_000_000}), now));
        assert!(!is_expired(&json!({"expiryDate": 1_800_000_000}), now));
        assert!(!is_expired(&json!({}), now));
        assert!(!is_expired(&json!({"expiryDate": null}), now));
    }

    #[tokio::test]
    async fn fetch_reads_the_search_endpoint() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/jobs/api/search"))
            .and(query_param("q", "ai engineer"))
            .and(query_param("country", "CL"))
            .and(query_param("sort", "recent"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(real_payload()))
            .mount(&server)
            .await;

        let source = HimalayasSource::new(Client::new())
            .with_base_url(server.uri())
            .with_config(config(&["ai engineer"], 3));
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.jobs.len(), 2, "a short page ends the walk");
        assert!(result.jobs.iter().all(|job| job.source == "himalayas"));
    }

    #[tokio::test]
    async fn fetch_sends_employment_types_only_when_set() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/jobs/api/search"))
            .and(query_param("employment_type", "Contractor,Full Time"))
            .respond_with(ResponseTemplate::new(200).set_body_json(real_payload()))
            .mount(&server)
            .await;
        let mut cfg = config(&["x"], 1);
        cfg.employment_types = vec!["Contractor".to_string(), "Full Time".to_string()];

        let source = HimalayasSource::new(Client::new())
            .with_base_url(server.uri())
            .with_config(cfg);
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.jobs.len(), 2);
    }

    #[tokio::test]
    async fn fetch_follows_full_pages_and_drops_duplicates_across_queries() {
        let server = MockServer::start().await;
        for (page, body) in [
            ("1", json!({"jobs": jobs(PAGE_SIZE, 0), "totalCount": 25})),
            ("2", json!({"jobs": jobs(5, PAGE_SIZE), "totalCount": 25})),
        ] {
            Mock::given(method("GET"))
                .and(path("/jobs/api/search"))
                .and(query_param("page", page))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
        }

        let source = HimalayasSource::new(Client::new())
            .with_base_url(server.uri())
            .with_config(config(&["first", "second"], 5));
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(
            result.jobs.len(),
            25,
            "25 jobs once, not twice, and page 3 is never asked for"
        );
    }

    #[tokio::test]
    async fn fetch_drops_expired_jobs_without_an_error() {
        let server = MockServer::start().await;
        let mut stale = jobs(1, 0);
        stale[0]["expiryDate"] = json!(1_000_000_000);
        Mock::given(method("GET"))
            .and(path("/jobs/api/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"jobs": stale})))
            .mount(&server)
            .await;

        let source = HimalayasSource::new(Client::new())
            .with_base_url(server.uri())
            .with_config(config(&["x"], 1));
        let result = source.fetch().await;

        assert!(result.jobs.is_empty());
        assert!(result.errors.is_empty(), "{:?}", result.errors);
    }

    #[tokio::test]
    async fn fetch_records_a_rate_limit_and_keeps_the_other_queries() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/jobs/api/search"))
            .and(query_param("q", "limited"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/jobs/api/search"))
            .and(query_param("q", "fine"))
            .respond_with(ResponseTemplate::new(200).set_body_json(real_payload()))
            .mount(&server)
            .await;

        let source = HimalayasSource::new(Client::new())
            .with_base_url(server.uri())
            .with_config(config(&["limited", "fine"], 1));
        let result = source.fetch().await;

        assert_eq!(result.jobs.len(), 2);
        assert_eq!(result.errors.len(), 1);
        assert!(
            result.errors[0].starts_with("himalayas:limited: "),
            "{}",
            result.errors[0]
        );
    }

    #[test]
    fn config_defaults_and_unknown_keys() {
        let cfg: HimalayasConfig = toml::from_str("").unwrap();
        assert_eq!(cfg, HimalayasConfig::default());
        assert_eq!(cfg.country, "CL");
        let cfg: HimalayasConfig =
            toml::from_str("queries = [\"rust\"]\nemployment_types = [\"Contractor\"]").unwrap();
        assert_eq!(cfg.queries, ["rust"]);
        assert_eq!(cfg.max_pages, 2);
        assert!(toml::from_str::<HimalayasConfig>("contry = \"CL\"").is_err());
    }
}
