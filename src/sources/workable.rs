//! Workable postings: `GET {base}/api/v1/widget/accounts/{account}?details=true`.
//!
//! The widget endpoint of `apply.workable.com` is public and needs no key. Its job pages are
//! `apply.workable.com/j/{shortcode}`, with no company in the URL, so the account slug is only
//! known from the configuration. Workable allows about 10 requests per 10 seconds; the source
//! asks for one account at a time, in order.

use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use super::Source;
use super::base::{failure, get_json, id_string, parse_time, skipped_error, str_field};
use crate::models::{FetchResult, Job};
use crate::scoring::parse_salary_monthly_usd;
use crate::text::html_to_text;

const DEFAULT_BASE_URL: &str = "https://apply.workable.com";

pub struct WorkableSource {
    accounts: Vec<String>,
    client: Client,
    base_url: String,
}

impl WorkableSource {
    pub fn new<I, S>(accounts: I, client: Client) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            accounts: accounts.into_iter().map(Into::into).collect(),
            client,
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    /// Points the source at another server (tests use a wiremock server).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    async fn fetch_account(&self, account: &str) -> anyhow::Result<(Vec<Job>, usize)> {
        let url = format!("{}/api/v1/widget/accounts/{account}", self.base_url);
        let body = get_json(&self.client, &url, &[("details", "true")]).await?;
        let postings = body["jobs"]
            .as_array()
            .context("response has no `jobs` array")?;
        let company = match str_field(&body, "name") {
            "" => account,
            name => name,
        };
        let mut jobs = Vec::new();
        let mut skipped = 0;
        for posting in postings {
            match parse_posting(company, posting) {
                Some(job) => jobs.push(job),
                None => skipped += 1,
            }
        }
        Ok((jobs, skipped))
    }
}

#[async_trait]
impl Source for WorkableSource {
    fn name(&self) -> &str {
        "workable"
    }

    async fn fetch(&self) -> FetchResult {
        let mut result = FetchResult::default();
        for account in &self.accounts {
            match self.fetch_account(account).await {
                Ok((jobs, skipped)) => {
                    result.jobs.extend(jobs);
                    if skipped > 0 {
                        result
                            .errors
                            .push(skipped_error("workable", account, skipped));
                    }
                }
                Err(error) => result.errors.push(failure("workable", account, &error)),
            }
        }
        result
    }
}

/// `published_on` is a plain date (`2026-07-30`); `created_at` is the fallback. `telecommuting`
/// marks a remote posting; `country` and `city` are the office the posting is attached to.
fn parse_posting(company: &str, posting: &Value) -> Option<Job> {
    let external_id = id_string(&posting["shortcode"])?;
    let title = str_field(posting, "title").trim();
    let url = str_field(posting, "url");
    if title.is_empty() || url.is_empty() {
        return None;
    }
    let apply_url = match str_field(posting, "application_url") {
        "" => url,
        apply_url => apply_url,
    };
    let location = [str_field(posting, "city"), str_field(posting, "country")]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    let description = html_to_text(str_field(posting, "description"));
    let (salary_min, salary_max) = parse_salary_monthly_usd(&description);
    let posted_at =
        parse_time(&posting["published_on"]).or_else(|| parse_time(&posting["created_at"]));
    Some(Job {
        source: "workable".to_string(),
        external_id,
        company: company.to_string(),
        title: title.to_string(),
        url: url.to_string(),
        apply_url: apply_url.to_string(),
        location,
        remote: posting["telecommuting"].as_bool().unwrap_or(false),
        description,
        salary_min_monthly_usd: salary_min,
        salary_max_monthly_usd: salary_max,
        posted_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A trimmed real response from `apply.workable.com/api/v1/widget/accounts/huggingface`.
    fn real_payload() -> Value {
        serde_json::from_str(include_str!("../../tests/fixtures/sources/workable.json")).unwrap()
    }

    #[test]
    fn parses_a_real_posting() {
        let payload = real_payload();
        let job = parse_posting("Hugging Face", &payload["jobs"][0]).unwrap();

        assert_eq!(job.key(), "workable:F4C096B22E");
        assert_eq!(job.company, "Hugging Face");
        assert_eq!(
            job.title,
            "Low-level Senior Software Engineer, Xet Storage - EMEA Remote"
        );
        assert_eq!(job.url, "https://apply.workable.com/j/F4C096B22E");
        assert_eq!(
            job.apply_url,
            "https://apply.workable.com/j/F4C096B22E/apply"
        );
        assert_eq!(job.location, "Paris, France");
        assert!(job.remote);
        assert_eq!(job.posted_at, crate::sources::base::parse_iso("2026-07-30"));
        assert!(job.description.contains("Hugging Face"));
    }

    #[test]
    fn a_posting_without_shortcode_title_or_url_is_not_a_job() {
        assert!(
            parse_posting("A", &json!({"title": "Dev", "url": "https://x.test/j/1"})).is_none()
        );
        assert!(
            parse_posting("A", &json!({"shortcode": "1", "url": "https://x.test/j/1"})).is_none()
        );
        assert!(parse_posting("A", &json!({"shortcode": "1", "title": "Dev"})).is_none());
    }

    #[tokio::test]
    async fn fetch_reads_one_account_and_names_the_company() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/widget/accounts/huggingface"))
            .and(query_param("details", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(real_payload()))
            .mount(&server)
            .await;

        let source =
            WorkableSource::new(["huggingface"], Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.jobs.len(), 1);
        assert_eq!(result.jobs[0].company, "Hugging Face");
    }

    #[tokio::test]
    async fn fetch_records_a_missing_account_and_keeps_the_others() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/widget/accounts/gone"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/widget/accounts/huggingface"))
            .respond_with(ResponseTemplate::new(200).set_body_json(real_payload()))
            .mount(&server)
            .await;

        let source =
            WorkableSource::new(["gone", "huggingface"], Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert_eq!(result.jobs.len(), 1);
        assert_eq!(result.errors.len(), 1);
        assert!(
            result.errors[0].starts_with("workable:gone: "),
            "{}",
            result.errors[0]
        );
    }

    #[tokio::test]
    async fn fetch_records_skipped_postings() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/widget/accounts/acme"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "name": "Acme",
                "jobs": [{"title": "No shortcode", "url": "https://apply.workable.com/j/1"}]
            })))
            .mount(&server)
            .await;

        let source = WorkableSource::new(["acme"], Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.jobs.is_empty());
        assert!(
            result.errors[0].starts_with("workable:acme: skipped 1"),
            "{}",
            result.errors[0]
        );
    }
}
