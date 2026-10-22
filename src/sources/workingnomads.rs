//! Working Nomads remote jobs: `GET {base}/api/exposed_jobs/`.
//!
//! The site's own public JSON feed of its current listings (every category), no key. The source
//! makes exactly one request per run and keeps the configured categories. The `url` of each job is
//! a Working Nomads redirect (`/job/go/{id}/`) to the company's page, so the application happens
//! off-site: the job keeps that link as `url` and `apply_url`, and the gate must treat
//! `workingnomads.com` as `listing_only`, like `remotive.com`.

use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use super::Source;
use super::base::{failure, get_json, parse_iso, skipped_error, str_field};
use crate::models::{FetchResult, Job};
use crate::text::html_to_text;

const DEFAULT_BASE_URL: &str = "https://www.workingnomads.com";

/// The `[workingnomads]` table of `profile/sources.toml`; present (even empty) turns the source on.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkingNomadsConfig {
    /// Working Nomads categories kept (`Development`, `Administration`, ...); empty keeps every one.
    pub categories: Vec<String>,
}

impl Default for WorkingNomadsConfig {
    fn default() -> Self {
        Self {
            categories: vec!["Development".to_string()],
        }
    }
}

pub struct WorkingNomadsSource {
    client: Client,
    base_url: String,
    config: WorkingNomadsConfig,
}

impl WorkingNomadsSource {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            base_url: DEFAULT_BASE_URL.to_string(),
            config: WorkingNomadsConfig::default(),
        }
    }

    pub fn with_config(mut self, config: WorkingNomadsConfig) -> Self {
        self.config = config;
        self
    }

    /// Points the source at another server (tests use a wiremock server).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    fn keeps(&self, item: &Value) -> bool {
        let category = str_field(item, "category_name");
        self.config.categories.is_empty()
            || self
                .config
                .categories
                .iter()
                .any(|kept| kept.eq_ignore_ascii_case(category))
    }

    async fn fetch_feed(&self) -> anyhow::Result<(Vec<Job>, usize)> {
        let url = format!("{}/api/exposed_jobs/", self.base_url);
        let body = get_json(&self.client, &url, &[]).await?;
        let items = body.as_array().context("response is not a JSON array")?;
        let mut jobs = Vec::new();
        let mut skipped = 0;
        for item in items.iter().filter(|item| self.keeps(item)) {
            match parse_job(item) {
                Some(job) => jobs.push(job),
                None => skipped += 1,
            }
        }
        Ok((jobs, skipped))
    }
}

#[async_trait]
impl Source for WorkingNomadsSource {
    fn name(&self) -> &str {
        "workingnomads"
    }

    async fn fetch(&self) -> FetchResult {
        let mut result = FetchResult::default();
        match self.fetch_feed().await {
            Ok((jobs, skipped)) => {
                result.jobs = jobs;
                if skipped > 0 {
                    result
                        .errors
                        .push(skipped_error("workingnomads", "api", skipped));
                }
            }
            Err(error) => result.errors.push(failure("workingnomads", "api", &error)),
        }
        result
    }
}

/// Every Working Nomads job is remote; `location` is free text ("Worldwide", "Brazil", "Europe,
/// Latin America, USA") that the region filter reads as it is. The feed has no id: it is the
/// number in the redirect link, `/job/go/1878435/`.
fn parse_job(item: &Value) -> Option<Job> {
    let url = str_field(item, "url");
    let title = str_field(item, "title");
    let external_id = url
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))?;
    if title.is_empty() {
        return None;
    }
    let mut description = html_to_text(str_field(item, "description"));
    for (label, value) in [
        ("Category", str_field(item, "category_name")),
        ("Tags", str_field(item, "tags")),
    ] {
        if !value.is_empty() {
            description = format!("{description}\n{label}: {value}");
        }
    }
    Some(Job {
        source: "workingnomads".to_string(),
        external_id: external_id.to_string(),
        company: str_field(item, "company_name").to_string(),
        title: title.to_string(),
        url: url.to_string(),
        apply_url: url.to_string(),
        location: str_field(item, "location").trim().to_string(),
        remote: true,
        description,
        salary_min_monthly_usd: None,
        salary_max_monthly_usd: None,
        posted_at: parse_iso(str_field(item, "pub_date")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A trimmed real response from `workingnomads.com/api/exposed_jobs/`: four development jobs
    /// and one marketing job.
    fn real_payload() -> Value {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/sources/workingnomads.json"
        ))
        .unwrap()
    }

    #[test]
    fn parses_a_real_job() {
        let payload = real_payload();
        let job = parse_job(&payload[0]).unwrap();

        assert_eq!(job.key(), "workingnomads:1878435");
        assert_eq!(job.company, "Lemon.io");
        assert_eq!(job.title, "Head of Engineering");
        assert_eq!(job.location, "Europe, Latin America, USA");
        assert!(job.remote);
        assert_eq!(job.url, "https://www.workingnomads.com/job/go/1878435/");
        assert_eq!(job.apply_url, job.url);
        assert_eq!(
            job.posted_at,
            crate::sources::base::parse_iso("2026-09-20T20:29:25+00:00")
        );
        assert!(
            job.description.contains("About Lemon.io"),
            "{}",
            job.description
        );
        assert!(job.description.contains("Category: Development"));
        assert!(job.description.contains("Tags: architecture,startup,it"));
        assert!(!job.description.contains('<'), "{}", job.description);
        assert_eq!(
            (job.salary_min_monthly_usd, job.salary_max_monthly_usd),
            (None, None)
        );
    }

    #[test]
    fn the_region_filter_reads_the_location_text() {
        use crate::discovery::region_ok;

        let payload = real_payload();
        let job = |index: usize| parse_job(&payload[index]).unwrap();

        assert!(region_ok(&job(0)), "Europe, Latin America, USA");
        assert!(region_ok(&job(1)), "Brazil");
        assert!(region_ok(&job(2)), "Worldwide");
        assert!(
            !region_ok(&job(3)),
            "a CET time zone with 3 hours of slack does not reach Chile"
        );
    }

    #[test]
    fn a_job_without_a_numbered_link_title_or_url_is_not_a_job() {
        assert!(parse_job(&json!({"title": "Dev", "url": ""})).is_none());
        assert!(parse_job(&json!({"url": "https://www.workingnomads.com/job/go/1/"})).is_none());
        assert!(
            parse_job(&json!({"title": "Dev", "url": "https://www.workingnomads.com/"})).is_none()
        );
        assert!(parse_job(&json!({"title": "Dev", "url": "https://x.test/job/go/12/"})).is_some());
    }

    #[tokio::test]
    async fn fetch_makes_one_request_and_keeps_the_development_category() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/exposed_jobs/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(real_payload()))
            .expect(1)
            .mount(&server)
            .await;

        let source = WorkingNomadsSource::new(Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.jobs.len(), 4, "the marketing job is not kept");
        assert!(result.jobs.iter().all(|job| job.source == "workingnomads"));
    }

    #[tokio::test]
    async fn an_empty_category_list_keeps_every_category() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/exposed_jobs/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(real_payload()))
            .mount(&server)
            .await;

        let source = WorkingNomadsSource::new(Client::new())
            .with_base_url(server.uri())
            .with_config(WorkingNomadsConfig { categories: vec![] });
        let result = source.fetch().await;

        assert_eq!(result.jobs.len(), 5);
    }

    #[tokio::test]
    async fn fetch_records_skipped_postings_and_failures() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/exposed_jobs/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"title": "No link", "category_name": "Development"},
                {"title": "Ok", "category_name": "Development", "url": "https://www.workingnomads.com/job/go/7/"}
            ])))
            .mount(&server)
            .await;
        let result = WorkingNomadsSource::new(Client::new())
            .with_base_url(server.uri())
            .fetch()
            .await;
        assert_eq!(result.jobs.len(), 1);
        assert_eq!(
            result.errors,
            vec!["workingnomads:api: skipped 1 postings missing id, title or url".to_string()]
        );

        let blocked = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>blocked</html>"))
            .mount(&blocked)
            .await;
        let result = WorkingNomadsSource::new(Client::new())
            .with_base_url(blocked.uri())
            .fetch()
            .await;
        assert!(result.jobs.is_empty());
        assert!(
            result.errors[0].starts_with("workingnomads:api: "),
            "{}",
            result.errors[0]
        );
    }

    #[test]
    fn config_defaults_and_unknown_keys() {
        let cfg: WorkingNomadsConfig = toml::from_str("").unwrap();
        assert_eq!(cfg, WorkingNomadsConfig::default());
        assert_eq!(cfg.categories, ["Development"]);
        let cfg: WorkingNomadsConfig = toml::from_str("categories = []").unwrap();
        assert!(cfg.categories.is_empty());
        assert!(toml::from_str::<WorkingNomadsConfig>("category = \"x\"").is_err());
    }
}
