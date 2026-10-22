//! Remotive feed: `GET {base}/api/remote-jobs?category=software-dev`.
//!
//! Remotive asks for a link back to the listing, so every job keeps its Remotive `url`. The apply
//! button lives on that page (the API has no company apply URL), which is why the gate treats
//! `remotive.com` as `listing_only`: these postings feed discovery and the digest, never the browser.

use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use super::Source;
use super::base::{failure, get_json, id_string, parse_iso, skipped_error, str_field};
use crate::models::{FetchResult, Job};
use crate::scoring::parse_salary_monthly_usd;
use crate::text::html_to_text;

const DEFAULT_BASE_URL: &str = "https://remotive.com";
const CATEGORY: &str = "software-dev";

pub struct RemotiveSource {
    client: Client,
    base_url: String,
}

impl RemotiveSource {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    /// Points the source at another server (tests use a wiremock server).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    async fn fetch_feed(&self) -> anyhow::Result<(Vec<Job>, usize)> {
        let url = format!("{}/api/remote-jobs", self.base_url);
        let body = get_json(&self.client, &url, &[("category", CATEGORY)]).await?;
        let items = body["jobs"]
            .as_array()
            .context("response has no `jobs` array")?;
        let mut jobs = Vec::new();
        let mut skipped = 0;
        for item in items {
            match parse_listing(item) {
                Some(job) => jobs.push(job),
                None => skipped += 1,
            }
        }
        Ok((jobs, skipped))
    }
}

#[async_trait]
impl Source for RemotiveSource {
    fn name(&self) -> &str {
        "remotive"
    }

    async fn fetch(&self) -> FetchResult {
        let mut result = FetchResult::default();
        match self.fetch_feed().await {
            Ok((jobs, skipped)) => {
                result.jobs = jobs;
                if skipped > 0 {
                    result
                        .errors
                        .push(skipped_error("remotive", "api", skipped));
                }
            }
            Err(error) => result.errors.push(failure("remotive", "api", &error)),
        }
        result
    }
}

/// Every Remotive listing is remote; `candidate_required_location` ("Worldwide", "USA", "LATAM",
/// "Europe", ...) is what the region filter reads. Pay is free text, so it goes through the same
/// parser as posting descriptions.
fn parse_listing(item: &Value) -> Option<Job> {
    let external_id = id_string(&item["id"])?;
    let title = str_field(item, "title");
    let url = str_field(item, "url");
    if title.is_empty() || url.is_empty() {
        return None;
    }
    let (salary_min, salary_max) = parse_salary_monthly_usd(str_field(item, "salary"));
    let mut description = html_to_text(str_field(item, "description"));
    let tags: Vec<&str> = item["tags"]
        .as_array()
        .map(|tags| tags.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if !tags.is_empty() {
        description = format!("{description}\nTags: {}", tags.join(", "));
    }
    Some(Job {
        source: "remotive".to_string(),
        external_id,
        company: str_field(item, "company_name").to_string(),
        title: title.to_string(),
        url: url.to_string(),
        apply_url: url.to_string(),
        location: str_field(item, "candidate_required_location").to_string(),
        remote: true,
        description,
        salary_min_monthly_usd: salary_min,
        salary_max_monthly_usd: salary_max,
        posted_at: parse_iso(str_field(item, "publication_date")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn payload() -> Value {
        json!({
            "0-legal-notice": "Remotive API Legal Notice",
            "job-count": 2,
            "jobs": [
                {
                    "id": 1917000,
                    "url": "https://remotive.com/remote-jobs/software-dev/senior-ai-engineer-1917000",
                    "title": "Senior AI Engineer",
                    "company_name": "Foo Labs",
                    "category": "Software Development",
                    "tags": ["python", "llm"],
                    "job_type": "full_time",
                    "publication_date": "2026-10-01T09:30:00",
                    "candidate_required_location": "Worldwide",
                    "salary": "$120,000 - $150,000",
                    "description": "<p>Build <b>agents</b>.</p>"
                },
                {
                    "id": 1917001,
                    "url": "https://remotive.com/remote-jobs/software-dev/backend-1917001",
                    "title": "Backend Engineer",
                    "company_name": "Bar",
                    "tags": [],
                    "publication_date": "",
                    "candidate_required_location": "USA",
                    "salary": "",
                    "description": ""
                }
            ]
        })
    }

    async fn serve(response: ResponseTemplate) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/remote-jobs"))
            .and(query_param("category", "software-dev"))
            .respond_with(response)
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn remotive_parses_listings_salary_text_and_location() {
        let server = serve(ResponseTemplate::new(200).set_body_json(payload())).await;

        let source = RemotiveSource::new(Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.jobs.len(), 2);
        let job = &result.jobs[0];
        assert_eq!(job.key(), "remotive:1917000");
        assert_eq!(job.company, "Foo Labs");
        assert!(job.remote);
        assert_eq!(job.location, "Worldwide");
        assert_eq!(job.apply_url, job.url);
        assert!(
            job.description.contains("Build agents")
                && job.description.contains("Tags: python, llm"),
            "{}",
            job.description
        );
        assert_eq!(job.salary_min_monthly_usd, Some(10_000.0));
        assert_eq!(job.salary_max_monthly_usd, Some(12_500.0));
        assert!(job.posted_at.is_some());
        let bare = &result.jobs[1];
        assert_eq!(bare.location, "USA");
        assert_eq!(
            (bare.salary_min_monthly_usd, bare.salary_max_monthly_usd),
            (None, None)
        );
        assert!(bare.posted_at.is_none());
    }

    #[tokio::test]
    async fn remotive_non_json_body_is_recorded() {
        let server =
            serve(ResponseTemplate::new(200).set_body_string("<html>blocked</html>")).await;

        let result = RemotiveSource::new(Client::new())
            .with_base_url(server.uri())
            .fetch()
            .await;

        assert!(result.jobs.is_empty());
        assert_eq!(result.errors.len(), 1);
        assert!(
            result.errors[0].starts_with("remotive:api:"),
            "{}",
            result.errors[0]
        );
    }

    #[tokio::test]
    async fn remotive_listings_without_id_title_or_url_are_skipped() {
        let body = json!({"jobs": [{"id": 1, "title": "No url"}, {"url": "https://remotive.com/x", "title": "No id"}, {"id": 3, "url": "https://remotive.com/y"}]});
        let server = serve(ResponseTemplate::new(200).set_body_json(body)).await;

        let result = RemotiveSource::new(Client::new())
            .with_base_url(server.uri())
            .fetch()
            .await;

        assert!(result.jobs.is_empty());
        assert_eq!(
            result.errors,
            vec!["remotive:api: skipped 3 postings missing id, title or url".to_string()]
        );
    }
}
