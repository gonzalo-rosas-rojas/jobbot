//! Lever postings: `GET {base}/v0/postings/{company}?mode=json`.

use crate::http::Http;
use anyhow::Context;
use async_trait::async_trait;
use serde_json::Value;

use super::Source;
use super::base::{
    LEVER_API_URL, from_epoch, get_json, id_string, or_if_empty, parse_all, record, str_field,
};
use jobbot_domain::models::{FetchResult, Job};
use jobbot_domain::scoring::parse_salary_monthly_usd;

const SOURCE_NAME: &str = "lever";

pub struct LeverSource {
    companies: Vec<String>,
    http: Http,
    base_url: String,
}

impl LeverSource {
    pub fn new<I, S>(companies: I, http: impl Into<Http>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            companies: companies.into_iter().map(Into::into).collect(),
            http: http.into(),
            base_url: LEVER_API_URL.to_string(),
        }
    }

    /// Points the source at another server (tests use a wiremock server).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    async fn fetch_company(&self, company: &str) -> anyhow::Result<(Vec<Job>, usize)> {
        let url = format!("{}/v0/postings/{company}", self.base_url);
        let body = get_json(&self.http, &url, &[("mode", "json")]).await?;
        let postings = body.as_array().context("response is not a JSON array")?;
        Ok(parse_all(postings, |posting| {
            parse_posting(company, posting)
        }))
    }
}

#[async_trait]
impl Source for LeverSource {
    fn name(&self) -> &str {
        SOURCE_NAME
    }

    async fn fetch(&self) -> FetchResult {
        let mut result = FetchResult::default();
        for company in &self.companies {
            record(
                &mut result,
                SOURCE_NAME,
                company,
                self.fetch_company(company).await,
            );
        }
        result
    }
}

/// Lever has no company display name in the postings feed, so the slug is the company.
fn parse_posting(company: &str, posting: &Value) -> Option<Job> {
    let external_id = id_string(&posting["id"])?;
    let title = str_field(posting, "text");
    let url = str_field(posting, "hostedUrl");
    if title.is_empty() || url.is_empty() {
        return None;
    }
    let apply_url = or_if_empty(str_field(posting, "applyUrl"), url);
    let location = posting["categories"]["location"].as_str().unwrap_or("");
    let remote = str_field(posting, "workplaceType").eq_ignore_ascii_case("remote")
        || location.to_lowercase().contains("remote");
    let description = format!(
        "{}\n{}",
        str_field(posting, "descriptionPlain"),
        str_field(posting, "additionalPlain")
    )
    .trim()
    .to_string();
    let (salary_min, salary_max) = parse_salary_monthly_usd(&description);
    // `createdAt` is in milliseconds.
    let posted_at = posting["createdAt"]
        .as_f64()
        .filter(|millis| *millis > 0.0)
        .and_then(|millis| from_epoch((millis / 1000.0) as i64));
    Some(Job {
        source: SOURCE_NAME.to_string(),
        external_id,
        company: company.to_string(),
        title: title.to_string(),
        url: url.to_string(),
        apply_url: apply_url.to_string(),
        location: location.to_string(),
        remote,
        description,
        salary_min_monthly_usd: salary_min,
        salary_max_monthly_usd: salary_max,
        posted_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn payload() -> Value {
        json!([{
            "id": "abc",
            "text": "AI Engineer",
            "hostedUrl": "https://jobs.lever.co/acme/abc",
            "applyUrl": "https://jobs.lever.co/acme/abc/apply",
            "categories": {"location": "Remote", "commitment": "Full-time", "team": "Eng"},
            "workplaceType": "remote",
            "descriptionPlain": "Build agents. $90K - $130K",
            "additionalPlain": "Benefits included",
            "createdAt": 1759320000000_i64
        }])
    }

    fn source_for(company: &str, base_url: String) -> LeverSource {
        LeverSource::new([company], reqwest::Client::new()).with_base_url(base_url)
    }

    #[tokio::test]
    async fn lever_parses_posting() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v0/postings/acme"))
            .and(query_param("mode", "json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(payload()))
            .mount(&server)
            .await;

        let result = source_for("acme", server.uri()).fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let job = &result.jobs[0];
        assert_eq!(job.key(), "lever:abc");
        assert_eq!(job.company, "acme");
        assert!(job.remote);
        assert!(job.apply_url.ends_with("/apply"));
        assert_eq!(job.salary_min_monthly_usd.map(f64::round), Some(7500.0));
        assert!(job.description.contains("Benefits included"));
        assert_eq!(
            job.posted_at,
            crate::sources::base::from_epoch(1_759_320_000)
        );
    }

    #[test]
    fn the_milliseconds_of_created_at_are_cut_to_the_second_and_zero_or_less_is_no_date() {
        use chrono::{TimeZone, Utc};
        let posted = |created: Value| {
            let mut posting = payload()[0].clone();
            posting["createdAt"] = created;
            parse_posting("acme", &posting).unwrap().posted_at
        };
        let noon = Utc.with_ymd_and_hms(2025, 10, 1, 12, 0, 0).unwrap();
        assert_eq!(posted(json!(1_759_320_000_000_i64)), Some(noon));
        assert_eq!(posted(json!(1_759_320_000_999_i64)), Some(noon));
        assert_eq!(posted(json!(1_759_320_000_500.5)), Some(noon));
        assert_eq!(
            posted(json!(1_759_320_001_000_i64)),
            Some(noon + chrono::TimeDelta::seconds(1))
        );
        assert_eq!(
            posted(json!(999)),
            Some(Utc.with_ymd_and_hms(1970, 1, 1, 0, 0, 0).unwrap())
        );
        for none in [json!(0), json!(-1_000), json!("1759320000000"), Value::Null] {
            assert_eq!(posted(none.clone()), None, "{none}");
        }
    }

    #[tokio::test]
    async fn lever_object_instead_of_array_is_recorded() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v0/postings/gone"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"ok": false, "error": "Document not found"})),
            )
            .mount(&server)
            .await;

        let result = source_for("gone", server.uri()).fetch().await;

        assert!(result.jobs.is_empty());
        assert_eq!(result.errors.len(), 1);
        assert!(
            result.errors[0].to_string().starts_with("lever:gone: "),
            "{}",
            result.errors[0]
        );
    }

    #[tokio::test]
    async fn lever_unreachable_server_is_recorded_not_raised() {
        // A listener that accepts a connection and closes it without answering. The port stays
        // owned by this test, so a parallel test's MockServer can never be assigned it: dropping a
        // MockServer and reusing its closed port was a race (another test's server could answer).
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let uri = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let _ = listener.incoming().take(1).count();
        });

        let result = source_for("acme", uri).fetch().await;

        assert!(result.jobs.is_empty());
        assert_eq!(result.errors.len(), 1);
        assert!(
            result.errors[0].to_string().starts_with("lever:acme: "),
            "{}",
            result.errors[0]
        );
    }
}
