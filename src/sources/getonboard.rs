//! Get on Board (Chile and LatAm): `GET {base}/api/v0/categories/{category}/jobs?per_page=N&page=P&expand=["company"]`.
//!
//! Salaries are read as US$ per month. They may be net rather than gross, which only makes the
//! floor check more conservative.

use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use super::Source;
use super::base::{failure, get_json, id_string, parse_time, skipped_error, str_field};
use crate::models::{FetchResult, Job};
use crate::text::html_to_text;

const DEFAULT_BASE_URL: &str = "https://www.getonbrd.com";

/// The attribute fields that make up a posting's description, in reading order.
const DESCRIPTION_KEYS: [&str; 6] = [
    "description_headline",
    "description",
    "functions",
    "desirable",
    "benefits",
    "perks",
];

pub struct GetOnBoardSource {
    client: Client,
    base_url: String,
    categories: Vec<String>,
    max_pages: u32,
    per_page: u32,
}

impl GetOnBoardSource {
    /// Defaults: category `programming`, at most 3 pages of 50 postings.
    pub fn new(client: Client) -> Self {
        Self {
            client,
            base_url: DEFAULT_BASE_URL.to_string(),
            categories: vec!["programming".to_string()],
            max_pages: 3,
            per_page: 50,
        }
    }

    /// Points the source at another server (tests use a wiremock server).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    pub fn with_categories<I, S>(mut self, categories: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.categories = categories.into_iter().map(Into::into).collect();
        self
    }

    /// Upper bound on pages requested per category (0 requests nothing).
    pub fn with_max_pages(mut self, max_pages: u32) -> Self {
        self.max_pages = max_pages;
        self
    }

    pub fn with_per_page(mut self, per_page: u32) -> Self {
        self.per_page = per_page;
        self
    }

    /// Follows the pages of one category. Jobs go straight into `result`, so when a later page fails
    /// the earlier pages are kept and only the error is returned.
    async fn fetch_category(&self, category: &str, result: &mut FetchResult) -> anyhow::Result<()> {
        let url = format!("{}/api/v0/categories/{category}/jobs", self.base_url);
        let per_page = self.per_page.to_string();
        for page in 1..=self.max_pages {
            let page_text = page.to_string();
            let query = [
                ("per_page", per_page.as_str()),
                ("page", page_text.as_str()),
                ("expand", r#"["company"]"#),
            ];
            let body = get_json(&self.client, &url, &query).await?;
            let items = body["data"]
                .as_array()
                .context("response has no `data` array")?;
            let mut skipped = 0;
            for item in items {
                match parse_posting(item) {
                    Some(job) => result.jobs.push(job),
                    None => skipped += 1,
                }
            }
            if skipped > 0 {
                result
                    .errors
                    .push(skipped_error("getonboard", category, skipped));
            }
            let total_pages = body["meta"]["total_pages"]
                .as_u64()
                .unwrap_or(u64::from(page));
            if u64::from(page) >= total_pages {
                break;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Source for GetOnBoardSource {
    fn name(&self) -> &str {
        "getonboard"
    }

    async fn fetch(&self) -> FetchResult {
        let mut result = FetchResult::default();
        for category in &self.categories {
            if let Err(error) = self.fetch_category(category, &mut result).await {
                result.errors.push(failure("getonboard", category, &error));
            }
        }
        result
    }
}

fn parse_posting(item: &Value) -> Option<Job> {
    let external_id = id_string(&item["id"])?;
    let attributes = &item["attributes"];
    let title = str_field(attributes, "title");
    let url = str_field(&item["links"], "public_url");
    if title.is_empty() || url.is_empty() {
        return None;
    }
    let company = attributes["company"]["data"]["attributes"]["name"]
        .as_str()
        .unwrap_or("");
    let remote = attributes["remote"].as_bool().unwrap_or(false)
        || str_field(attributes, "remote_modality") == "fully_remote";
    // "Remote" shows up in `countries` as a pseudo-country on every fully remote or remote-local
    // posting; the remote marker below already says it.
    let countries: Vec<&str> = attributes["countries"]
        .as_array()
        .map(|countries| countries.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let countries: Vec<&str> = countries
        .into_iter()
        .filter(|country| !country.eq_ignore_ascii_case("remote"))
        .collect();
    let mut location = countries.join(", ");
    if remote {
        location = format!("{location} Remote {}", str_field(attributes, "remote_zone"))
            .trim()
            .to_string();
    }
    let html = DESCRIPTION_KEYS
        .iter()
        .filter_map(|key| attributes[*key].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let salary = |key: &str| attributes[key].as_f64().filter(|amount| *amount > 0.0);
    Some(Job {
        source: "getonboard".to_string(),
        external_id,
        company: company.to_string(),
        title: title.to_string(),
        url: url.to_string(),
        apply_url: url.to_string(),
        location,
        remote,
        description: html_to_text(&html),
        salary_min_monthly_usd: salary("min_salary"),
        salary_max_monthly_usd: salary("max_salary"),
        posted_at: parse_time(&attributes["published_at"]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    const JOBS_PATH: &str = "/api/v0/categories/programming/jobs";

    fn page_body(page: u32, total_pages: u32) -> Value {
        json!({
            "data": [{
                "id": format!("dev-{page}"),
                "type": "job",
                "links": {"public_url": format!("https://www.getonbrd.com/jobs/dev-{page}")},
                "attributes": {
                    "title": format!("Developer {page}"),
                    "remote": false,
                    "remote_modality": "hybrid",
                    "remote_zone": null,
                    "countries": ["Chile"],
                    "min_salary": 1900,
                    "max_salary": 2500,
                    "description": "<p>Join us</p>",
                    "functions": "<p>Build UI</p>",
                    "company": {"data": {"attributes": {"name": "TCIT"}}},
                    "published_at": 1759320000
                }
            }],
            "meta": {"page": page, "per_page": 50, "total_pages": total_pages}
        })
    }

    /// Answers every request for `JOBS_PATH` with the page body that matches its `page` parameter.
    async fn serve_pages(total_pages: u32) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(JOBS_PATH))
            .respond_with(move |request: &Request| {
                let page = request
                    .url
                    .query_pairs()
                    .find(|(key, _)| key == "page")
                    .and_then(|(_, value)| value.parse().ok())
                    .unwrap_or(1);
                ResponseTemplate::new(200).set_body_json(page_body(page, total_pages))
            })
            .mount(&server)
            .await;
        server
    }

    async fn requested_pages(server: &MockServer) -> Vec<String> {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter_map(|request| {
                request
                    .url
                    .query_pairs()
                    .find(|(key, _)| key == "page")
                    .map(|(_, value)| value.into_owned())
            })
            .collect()
    }

    #[tokio::test]
    async fn getonboard_parses_job_and_follows_pages() {
        let server = serve_pages(2).await;

        let source = GetOnBoardSource::new(Client::new())
            .with_max_pages(5)
            .with_base_url(server.uri());
        let result = source.fetch().await;

        assert_eq!(requested_pages(&server).await, ["1", "2"]);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.jobs.len(), 2);
        let job = &result.jobs[0];
        assert_eq!(job.company, "TCIT");
        assert_eq!(job.location, "Chile");
        assert!(!job.remote);
        assert_eq!(job.salary_min_monthly_usd, Some(1900.0));
        assert!(job.description.contains("Build UI"));
        assert!(job.posted_at.is_some());
    }

    #[tokio::test]
    async fn getonboard_sends_company_expansion_and_page_size() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(JOBS_PATH))
            .and(query_param("per_page", "20"))
            .and(query_param("expand", r#"["company"]"#))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_body(1, 1)))
            .expect(1)
            .mount(&server)
            .await;

        let source = GetOnBoardSource::new(Client::new())
            .with_per_page(20)
            .with_base_url(server.uri());
        let result = source.fetch().await;

        assert_eq!(result.jobs.len(), 1);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
    }

    #[tokio::test]
    async fn getonboard_stops_at_max_pages() {
        let server = serve_pages(9).await;

        let source = GetOnBoardSource::new(Client::new())
            .with_max_pages(2)
            .with_base_url(server.uri());
        let result = source.fetch().await;

        assert_eq!(requested_pages(&server).await, ["1", "2"]);
        assert_eq!(result.jobs.len(), 2);
    }

    #[tokio::test]
    async fn getonboard_remote_posting_gets_remote_zone_in_location() {
        let server = MockServer::start().await;
        let mut body = page_body(1, 1);
        body["data"][0]["attributes"]["remote"] = json!(true);
        body["data"][0]["attributes"]["remote_zone"] = json!("latam");
        Mock::given(method("GET"))
            .and(path(JOBS_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let source = GetOnBoardSource::new(Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.jobs[0].remote);
        assert_eq!(result.jobs[0].location, "Chile Remote latam");
    }

    #[tokio::test]
    async fn getonboard_pseudo_country_remote_is_not_repeated_in_the_location() {
        // Live data (2026-10-04): every fully remote or remote-local posting lists the
        // pseudo-country "Remote", which made the location read "Remote Remote".
        let server = MockServer::start().await;
        let mut body = page_body(1, 1);
        body["data"][0]["attributes"]["remote"] = json!(true);
        body["data"][0]["attributes"]["remote_modality"] = json!("fully_remote");
        body["data"][0]["attributes"]["countries"] = json!(["Remote"]);
        Mock::given(method("GET"))
            .and(path(JOBS_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let source = GetOnBoardSource::new(Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.jobs[0].remote);
        assert_eq!(result.jobs[0].location, "Remote");
    }

    #[tokio::test]
    async fn getonboard_page_failure_keeps_earlier_pages() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(JOBS_PATH))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_body(1, 3)))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(JOBS_PATH))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let source = GetOnBoardSource::new(Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert_eq!(result.jobs.len(), 1);
        assert_eq!(result.errors.len(), 1);
        assert!(
            result.errors[0].starts_with("getonboard:programming: "),
            "{}",
            result.errors[0]
        );
        assert!(result.errors[0].contains("500"), "{}", result.errors[0]);
    }
}
