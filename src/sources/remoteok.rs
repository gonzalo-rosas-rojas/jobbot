//! RemoteOK feed: `GET {base}/api`.
//!
//! RemoteOK's terms require linking back to the listing and naming RemoteOK as the source:
//! every job keeps its RemoteOK `url`, and the digest shows the source name.

use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use super::Source;
use super::base::{failure, from_epoch, get_json, id_string, parse_iso, skipped_error, str_field};
use crate::models::{FetchResult, Job};
use crate::text::html_to_text;

const DEFAULT_BASE_URL: &str = "https://remoteok.com";

pub struct RemoteOkSource {
    client: Client,
    base_url: String,
}

impl RemoteOkSource {
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
        let url = format!("{}/api", self.base_url);
        let body = get_json(&self.client, &url, &[]).await?;
        let items = body.as_array().context("response is not a JSON array")?;
        let mut jobs = Vec::new();
        let mut skipped = 0;
        for item in items {
            // The first element is the legal notice `{last_updated, legal}`: it has no id.
            let Some(external_id) = id_string(&item["id"]) else {
                continue;
            };
            match parse_listing(&self.base_url, &external_id, item) {
                Some(job) => jobs.push(job),
                None => skipped += 1,
            }
        }
        Ok((jobs, skipped))
    }
}

#[async_trait]
impl Source for RemoteOkSource {
    fn name(&self) -> &str {
        "remoteok"
    }

    async fn fetch(&self) -> FetchResult {
        let mut result = FetchResult::default();
        match self.fetch_feed().await {
            Ok((jobs, skipped)) => {
                result.jobs = jobs;
                if skipped > 0 {
                    result
                        .errors
                        .push(skipped_error("remoteok", "api", skipped));
                }
            }
            Err(error) => result.errors.push(failure("remoteok", "api", &error)),
        }
        result
    }
}

/// Salaries are annual USD (0 means unknown): they become monthly. A listing without `url`
/// still links back to RemoteOK through its id.
fn parse_listing(base_url: &str, external_id: &str, item: &Value) -> Option<Job> {
    let title = str_field(item, "position");
    if title.is_empty() {
        return None;
    }
    let url = match str_field(item, "url") {
        "" => format!("{base_url}/remote-jobs/{external_id}"),
        url => url.to_string(),
    };
    let apply_url = match str_field(item, "apply_url") {
        "" => url.clone(),
        apply_url => apply_url.to_string(),
    };
    let tags: Vec<&str> = item["tags"]
        .as_array()
        .map(|tags| tags.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut description = html_to_text(str_field(item, "description"));
    if !tags.is_empty() {
        description = format!("{description}\nTags: {}", tags.join(", "))
            .trim()
            .to_string();
    }
    let monthly = |key: &str| {
        item[key]
            .as_f64()
            .filter(|annual| *annual > 0.0)
            .map(|annual| annual / 12.0)
    };
    Some(Job {
        source: "remoteok".to_string(),
        external_id: external_id.to_string(),
        company: str_field(item, "company").to_string(),
        title: title.to_string(),
        url,
        apply_url,
        location: str_field(item, "location").to_string(),
        remote: true,
        description,
        salary_min_monthly_usd: monthly("salary_min"),
        salary_max_monthly_usd: monthly("salary_max"),
        posted_at: item["epoch"]
            .as_i64()
            .and_then(from_epoch)
            .or_else(|| parse_iso(str_field(item, "date"))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn payload() -> Value {
        json!([
            {"last_updated": 1, "legal": "terms"},
            {
                "id": "111",
                "position": "Senior AI Engineer",
                "company": "Foo",
                "url": "https://remoteok.com/remote-jobs/111",
                "apply_url": "https://remoteok.com/l/111",
                "location": "Worldwide",
                "tags": ["ai", "python"],
                "salary_min": 96000,
                "salary_max": 144000,
                "description": "<p>Build things</p>",
                "epoch": 1759320000
            }
        ])
    }

    async fn serve_api(response: ResponseTemplate) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api"))
            .respond_with(response)
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn remoteok_skips_legal_element_and_converts_annual_salary() {
        let server = serve_api(ResponseTemplate::new(200).set_body_json(payload())).await;

        let source = RemoteOkSource::new(Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.jobs.len(), 1);
        let job = &result.jobs[0];
        assert_eq!(job.key(), "remoteok:111");
        assert!(job.remote);
        assert_eq!(job.salary_min_monthly_usd, Some(8000.0));
        assert_eq!(job.salary_max_monthly_usd, Some(12000.0));
        assert!(
            job.description.contains("Tags: ai, python"),
            "{}",
            job.description
        );
        assert!(job.url.starts_with("https://remoteok.com/"));
        assert!(job.posted_at.is_some());
    }

    #[tokio::test]
    async fn remoteok_malformed_body_is_recorded() {
        let server =
            serve_api(ResponseTemplate::new(200).set_body_string("<html>blocked</html>")).await;

        let source = RemoteOkSource::new(Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.jobs.is_empty());
        assert_eq!(result.errors.len(), 1);
        assert!(
            result.errors[0].starts_with("remoteok"),
            "{}",
            result.errors[0]
        );
    }

    #[tokio::test]
    async fn remoteok_missing_fields_keep_the_link_back_to_remoteok() {
        let body = json!([
            {"last_updated": 1, "legal": "terms"},
            {"id": 222, "position": "Bare Role", "salary_min": 0, "salary_max": null},
            {"id": 333}
        ]);
        let server = serve_api(ResponseTemplate::new(200).set_body_json(body)).await;

        let source = RemoteOkSource::new(Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert_eq!(result.jobs.len(), 1);
        let job = &result.jobs[0];
        assert_eq!(job.url, format!("{}/remote-jobs/222", server.uri()));
        assert_eq!(job.apply_url, job.url);
        assert_eq!(
            (job.salary_min_monthly_usd, job.salary_max_monthly_usd),
            (None, None)
        );
        assert_eq!((job.company.as_str(), job.description.as_str()), ("", ""));
        assert_eq!(
            result.errors,
            vec!["remoteok:api: skipped 1 postings missing id, title or url".to_string()]
        );
    }
}
