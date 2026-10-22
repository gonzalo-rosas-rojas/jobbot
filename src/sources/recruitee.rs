//! Recruitee offers: `GET https://{company}.recruitee.com/api/offers/`.
//!
//! Public and keyless. Most companies publish their careers page on their own domain, but the
//! default address `https://{company}.recruitee.com/o/{slug}` always redirects there. The jobs use
//! the `recruitee.com` address so the gate can tell the ATS from the host, while the real page is
//! one redirect away. Times come as `"2026-10-02 13:38:06 UTC"`, which is not ISO 8601.

use anyhow::Context;
use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use reqwest::Client;
use serde_json::Value;

use super::Source;
use super::base::{failure, get_json, id_string, parse_iso, skipped_error, str_field};
use crate::models::{FetchResult, Job};
use crate::scoring::parse_salary_monthly_usd;
use crate::text::html_to_text;

pub struct RecruiteeSource {
    companies: Vec<String>,
    client: Client,
    /// `None` reads `https://{company}.recruitee.com`; tests point it at a server that serves
    /// `{base}/{company}/api/offers/`.
    base_url: Option<String>,
}

impl RecruiteeSource {
    pub fn new<I, S>(companies: I, client: Client) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            companies: companies.into_iter().map(Into::into).collect(),
            client,
            base_url: None,
        }
    }

    /// Points the source at another server (tests use a wiremock server).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into().trim_end_matches('/').to_string());
        self
    }

    fn offers_url(&self, company: &str) -> String {
        match &self.base_url {
            Some(base) => format!("{base}/{company}/api/offers/"),
            None => format!("https://{company}.recruitee.com/api/offers/"),
        }
    }

    async fn fetch_company(&self, company: &str) -> anyhow::Result<(Vec<Job>, usize)> {
        let body = get_json(&self.client, &self.offers_url(company), &[]).await?;
        let offers = body["offers"]
            .as_array()
            .context("response has no `offers` array")?;
        let now = Utc::now();
        let mut jobs = Vec::new();
        let mut skipped = 0;
        for offer in offers {
            if !is_open(offer, now) {
                continue;
            }
            match parse_offer(company, offer) {
                Some(job) => jobs.push(job),
                None => skipped += 1,
            }
        }
        Ok((jobs, skipped))
    }
}

#[async_trait]
impl Source for RecruiteeSource {
    fn name(&self) -> &str {
        "recruitee"
    }

    async fn fetch(&self) -> FetchResult {
        let mut result = FetchResult::default();
        for company in &self.companies {
            match self.fetch_company(company).await {
                Ok((jobs, skipped)) => {
                    result.jobs.extend(jobs);
                    if skipped > 0 {
                        result
                            .errors
                            .push(skipped_error("recruitee", company, skipped));
                    }
                }
                Err(error) => result.errors.push(failure("recruitee", company, &error)),
            }
        }
        result
    }
}

/// `"2026-10-02 13:38:06 UTC"` (Recruitee's format), or ISO 8601 as a fallback.
fn parse_recruitee_time(value: &str) -> Option<DateTime<Utc>> {
    let value = value.trim();
    NaiveDateTime::parse_from_str(value.trim_end_matches(" UTC"), "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|naive| naive.and_utc())
        .or_else(|| parse_iso(value))
}

/// An offer is open when it is published and has no `close_at` in the past.
fn is_open(offer: &Value, now: DateTime<Utc>) -> bool {
    let status = str_field(offer, "status");
    if !status.is_empty() && status != "published" {
        return false;
    }
    parse_recruitee_time(str_field(offer, "close_at")).is_none_or(|closes| closes > now)
}

fn parse_offer(company: &str, offer: &Value) -> Option<Job> {
    let external_id = id_string(&offer["id"])?;
    let title = str_field(offer, "title").trim();
    let slug = str_field(offer, "slug");
    if title.is_empty() || slug.is_empty() {
        return None;
    }
    let url = format!("https://{company}.recruitee.com/o/{slug}");
    let apply_url = format!("{url}/c/new");
    let location = match str_field(offer, "location") {
        "" => [str_field(offer, "city"), str_field(offer, "country")]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        location => location.to_string(),
    };
    let description = format!(
        "{}\n{}",
        html_to_text(str_field(offer, "description")),
        html_to_text(str_field(offer, "requirements"))
    )
    .trim()
    .to_string();
    let (salary_min, salary_max) = parse_salary_monthly_usd(&description);
    let posted_at = parse_recruitee_time(str_field(offer, "published_at"))
        .or_else(|| parse_recruitee_time(str_field(offer, "created_at")));
    let company_name = match str_field(offer, "company_name") {
        "" => company,
        name => name,
    };
    Some(Job {
        source: "recruitee".to_string(),
        external_id,
        company: company_name.to_string(),
        title: title.to_string(),
        url,
        apply_url,
        location,
        remote: offer["remote"].as_bool().unwrap_or(false),
        description,
        salary_min_monthly_usd: salary_min,
        salary_max_monthly_usd: salary_max,
        posted_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use reqwest::Client;
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// One real offer from `bunq.recruitee.com/api/offers/` (a few fields trimmed).
    fn real_payload() -> Value {
        serde_json::from_str(include_str!("../../tests/fixtures/sources/recruitee.json")).unwrap()
    }

    #[test]
    fn parses_a_real_offer() {
        let payload = real_payload();
        let job = parse_offer("bunq", &payload["offers"][0]).unwrap();

        assert_eq!(job.key(), "recruitee:2768887");
        assert_eq!(job.company, "bunq");
        assert_eq!(job.title, "Cards Experience Guide");
        assert_eq!(
            job.url,
            "https://bunq.recruitee.com/o/cards-experience-guide-2"
        );
        assert_eq!(
            job.apply_url,
            "https://bunq.recruitee.com/o/cards-experience-guide-2/c/new"
        );
        assert_eq!(job.location, "Amsterdam, Noord-Holland, Netherlands");
        assert!(!job.remote);
        assert_eq!(
            job.posted_at,
            Some(Utc.with_ymd_and_hms(2026, 10, 2, 13, 38, 6).unwrap())
        );
        assert!(!job.description.is_empty());
    }

    #[test]
    fn parses_recruitee_times_and_falls_back_to_iso() {
        assert_eq!(
            parse_recruitee_time("2026-10-02 13:38:06 UTC"),
            Some(Utc.with_ymd_and_hms(2026, 10, 2, 13, 38, 6).unwrap())
        );
        assert_eq!(
            parse_recruitee_time("2026-10-02T13:38:06Z"),
            Some(Utc.with_ymd_and_hms(2026, 10, 2, 13, 38, 6).unwrap())
        );
        assert_eq!(parse_recruitee_time(""), None);
        assert_eq!(parse_recruitee_time("soon"), None);
    }

    #[test]
    fn only_published_offers_without_a_past_close_date_are_open() {
        let now = Utc.with_ymd_and_hms(2026, 10, 4, 0, 0, 0).unwrap();
        assert!(is_open(
            &json!({"status": "published", "close_at": null}),
            now
        ));
        assert!(is_open(
            &json!({"close_at": "2026-12-01 00:00:00 UTC"}),
            now
        ));
        assert!(!is_open(&json!({"status": "draft"}), now));
        assert!(!is_open(
            &json!({"status": "published", "close_at": "2026-09-01 00:00:00 UTC"}),
            now
        ));
    }

    #[test]
    fn an_offer_without_id_title_or_slug_is_not_a_job() {
        assert!(parse_offer("a", &json!({"title": "Dev", "slug": "dev"})).is_none());
        assert!(parse_offer("a", &json!({"id": 1, "slug": "dev"})).is_none());
        assert!(parse_offer("a", &json!({"id": 1, "title": "Dev"})).is_none());
    }

    #[tokio::test]
    async fn fetch_reads_a_company_and_drops_closed_offers() {
        let server = MockServer::start().await;
        let mut payload = real_payload();
        payload["offers"].as_array_mut().unwrap().push(json!({
            "id": 2, "title": "Closed", "slug": "closed", "status": "published",
            "close_at": "2020-01-01 00:00:00 UTC"
        }));
        Mock::given(method("GET"))
            .and(path("/bunq/api/offers/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(payload))
            .mount(&server)
            .await;

        let source = RecruiteeSource::new(["bunq"], Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.jobs.len(), 1);
        assert_eq!(result.jobs[0].title, "Cards Experience Guide");
    }

    #[tokio::test]
    async fn fetch_records_a_missing_company_and_skipped_offers() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gone/api/offers/"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": "not found"})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/acme/api/offers/"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"offers": [{"id": 1, "title": "No slug"}]})),
            )
            .mount(&server)
            .await;

        let source =
            RecruiteeSource::new(["gone", "acme"], Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.jobs.is_empty());
        assert_eq!(result.errors.len(), 2);
        assert!(
            result.errors[0].starts_with("recruitee:gone: "),
            "{}",
            result.errors[0]
        );
        assert!(
            result.errors[1].starts_with("recruitee:acme: skipped 1"),
            "{}",
            result.errors[1]
        );
    }
}
