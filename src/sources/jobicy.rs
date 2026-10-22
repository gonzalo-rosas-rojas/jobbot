//! Jobicy remote jobs: `GET {base}/api/v2/remote-jobs?count=N&geo=latam&industry=dev`.
//!
//! Free public API, no key. Jobicy delays its data by about 3 hours and asks for at most one
//! request per hour, so the source makes exactly one request per run. It also asks for a link
//! back, so every job keeps its Jobicy page as `url`; the application happens there, which is why
//! the gate must treat `jobicy.com` as `listing_only`, like `remotive.com`.

use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use super::Source;
use super::base::{failure, get_json, id_string, parse_time, skipped_error, str_field};
use super::pay::{monthly_usd_range, number};
use crate::models::{FetchResult, Job};
use crate::text::html_to_text;

const DEFAULT_BASE_URL: &str = "https://jobicy.com";

/// The `[jobicy]` table of `profile/sources.toml`; present (even empty) turns the source on.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JobicyConfig {
    /// Jobicy region slug: `latam`, `usa`, `emea`, ... (`anywhere` is not a filter value).
    pub geo: String,
    /// Jobicy industry slug, `dev` for software.
    pub industry: String,
    /// Jobs per request.
    pub count: u32,
}

impl Default for JobicyConfig {
    fn default() -> Self {
        Self {
            geo: "latam".to_string(),
            industry: "dev".to_string(),
            count: 50,
        }
    }
}

pub struct JobicySource {
    client: Client,
    base_url: String,
    config: JobicyConfig,
}

impl JobicySource {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            base_url: DEFAULT_BASE_URL.to_string(),
            config: JobicyConfig::default(),
        }
    }

    pub fn with_config(mut self, config: JobicyConfig) -> Self {
        self.config = config;
        self
    }

    /// Points the source at another server (tests use a wiremock server).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    async fn fetch_feed(&self) -> anyhow::Result<(Vec<Job>, usize)> {
        let url = format!("{}/api/v2/remote-jobs", self.base_url);
        let count = self.config.count.to_string();
        let mut params = vec![("count", count.as_str())];
        if !self.config.geo.is_empty() {
            params.push(("geo", self.config.geo.as_str()));
        }
        if !self.config.industry.is_empty() {
            params.push(("industry", self.config.industry.as_str()));
        }
        let body = get_json(&self.client, &url, &params).await?;
        let items = body["jobs"]
            .as_array()
            .context("response has no `jobs` array")?;
        let mut jobs = Vec::new();
        let mut skipped = 0;
        for item in items {
            match parse_job(item) {
                Some(job) => jobs.push(job),
                None => skipped += 1,
            }
        }
        Ok((jobs, skipped))
    }
}

#[async_trait]
impl Source for JobicySource {
    fn name(&self) -> &str {
        "jobicy"
    }

    async fn fetch(&self) -> FetchResult {
        let mut result = FetchResult::default();
        match self.fetch_feed().await {
            Ok((jobs, skipped)) => {
                result.jobs = jobs;
                if skipped > 0 {
                    result.errors.push(skipped_error("jobicy", "api", skipped));
                }
            }
            Err(error) => result.errors.push(failure("jobicy", "api", &error)),
        }
        result
    }
}

/// `jobGeo` is text such as `"Anywhere"` or `"Argentina,  Canada,  UK"` (uneven spacing); it is
/// read as a comma-separated list and written back with single spaces. An array also works.
fn geo(value: &Value) -> String {
    let parts: Vec<&str> = match value {
        Value::String(text) => text.split(',').collect(),
        Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    parts
        .iter()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A text list from a JSON array of strings (`jobType`), or the string itself.
fn list(value: &Value) -> String {
    match value {
        Value::String(text) => text.trim().to_string(),
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    }
}

/// Every Jobicy job is remote. Pay (`salaryMin`, `salaryMax`, `salaryCurrency`, `salaryPeriod`)
/// is optional and is only converted when it is in US dollars.
fn parse_job(item: &Value) -> Option<Job> {
    let external_id = id_string(&item["id"])?;
    let title = str_field(item, "jobTitle");
    let url = str_field(item, "url");
    if title.is_empty() || url.is_empty() {
        return None;
    }
    let body = match str_field(item, "jobDescription") {
        "" => str_field(item, "jobExcerpt"),
        full => full,
    };
    let mut description = html_to_text(body);
    for (label, value) in [
        ("Level", str_field(item, "jobLevel").to_string()),
        ("Type", list(&item["jobType"])),
    ] {
        if !value.is_empty() {
            description = format!("{description}\n{label}: {value}");
        }
    }
    let (salary_min, salary_max) = monthly_usd_range(
        number(&item["salaryMin"]),
        number(&item["salaryMax"]),
        str_field(item, "salaryCurrency"),
        str_field(item, "salaryPeriod"),
    );
    Some(Job {
        source: "jobicy".to_string(),
        external_id,
        company: str_field(item, "companyName").to_string(),
        title: title.to_string(),
        url: url.to_string(),
        apply_url: url.to_string(),
        location: geo(&item["jobGeo"]),
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

    /// A trimmed real response from `jobicy.com/api/v2/remote-jobs?count=20&geo=latam&industry=dev`.
    fn real_payload() -> Value {
        serde_json::from_str(include_str!("../../tests/fixtures/sources/jobicy.json")).unwrap()
    }

    #[test]
    fn parses_a_real_job() {
        let payload = real_payload();
        let job = parse_job(&payload["jobs"][0]).unwrap();

        assert_eq!(job.key(), "jobicy:154514");
        assert_eq!(job.company, "Canonical");
        assert_eq!(job.location, "Anywhere");
        assert!(job.remote);
        assert_eq!(
            job.url,
            "https://jobicy.com/jobs/154514-solutions-architect-for-automotive"
        );
        assert_eq!(job.apply_url, job.url);
        assert_eq!(
            job.posted_at,
            crate::sources::base::parse_iso("2026-10-04T05:45:40+00:00")
        );
        assert!(job.description.contains("Type: Full-Time"));
        assert_eq!(job.salary_min_monthly_usd, None);
    }

    #[test]
    fn pay_without_a_currency_is_not_converted() {
        // The real second job pays "4 hourly" with no currency: it must not become US$640 a month.
        let payload = real_payload();
        let job = parse_job(&payload["jobs"][1]).unwrap();

        assert_eq!(job.location, "Argentina");
        assert_eq!(job.salary_min_monthly_usd, None);
        assert_eq!(job.salary_max_monthly_usd, None);
    }

    #[test]
    fn usd_pay_is_converted_to_monthly() {
        let item = json!({
            "id": 1, "jobTitle": "Dev", "url": "https://jobicy.com/jobs/1-dev",
            "salaryMin": 55_000, "salaryMax": 100_000, "salaryCurrency": "USD", "salaryPeriod": "yearly"
        });
        let job = parse_job(&item).unwrap();

        assert_eq!(job.salary_min_monthly_usd.map(f64::round), Some(4_583.0));
        assert_eq!(job.salary_max_monthly_usd.map(f64::round), Some(8_333.0));
    }

    #[test]
    fn geo_collapses_uneven_spacing_and_reads_arrays() {
        assert_eq!(
            geo(&json!("Argentina,  Canada,  UK")),
            "Argentina, Canada, UK"
        );
        assert_eq!(geo(&json!("Anywhere")), "Anywhere");
        assert_eq!(geo(&json!(["LATAM", "USA"])), "LATAM, USA");
        assert_eq!(geo(&json!(null)), "");
    }

    #[test]
    fn a_job_without_id_title_or_url_is_not_a_job() {
        assert!(parse_job(&json!({"jobTitle": "Dev", "url": "https://x.test/1"})).is_none());
        assert!(parse_job(&json!({"id": 1, "url": "https://x.test/1"})).is_none());
        assert!(parse_job(&json!({"id": 1, "jobTitle": "Dev"})).is_none());
    }

    #[tokio::test]
    async fn fetch_makes_one_request_with_the_filters() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v2/remote-jobs"))
            .and(query_param("count", "50"))
            .and(query_param("geo", "latam"))
            .and(query_param("industry", "dev"))
            .respond_with(ResponseTemplate::new(200).set_body_json(real_payload()))
            .expect(1)
            .mount(&server)
            .await;

        let source = JobicySource::new(Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.jobs.len(), 2);
        assert!(result.jobs.iter().all(|job| job.source == "jobicy"));
    }

    #[tokio::test]
    async fn fetch_records_skipped_postings_and_http_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v2/remote-jobs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "jobs": [{"id": 1, "jobTitle": "Dev"}, {"id": 2, "jobTitle": "Ok", "url": "https://jobicy.com/jobs/2"}]
            })))
            .mount(&server)
            .await;

        let source = JobicySource::new(Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert_eq!(result.jobs.len(), 1);
        assert_eq!(result.errors.len(), 1);
        assert!(
            result.errors[0].starts_with("jobicy:api: skipped 1"),
            "{}",
            result.errors[0]
        );

        let failing = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&failing)
            .await;
        let result = JobicySource::new(Client::new())
            .with_base_url(failing.uri())
            .fetch()
            .await;
        assert!(result.jobs.is_empty());
        assert!(
            result.errors[0].starts_with("jobicy:api: "),
            "{}",
            result.errors[0]
        );
    }

    #[test]
    fn config_defaults_and_unknown_keys() {
        let cfg: JobicyConfig = toml::from_str("").unwrap();
        assert_eq!(cfg, JobicyConfig::default());
        assert_eq!(
            (cfg.geo.as_str(), cfg.industry.as_str(), cfg.count),
            ("latam", "dev", 50)
        );
        assert!(toml::from_str::<JobicyConfig>("regio = \"usa\"").is_err());
    }
}
