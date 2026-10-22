//! SmartRecruiters postings: `GET {base}/v1/companies/{company}/postings?locationType=REMOTE`.
//!
//! Public and keyless. The listing has no description and no public page, so the newest remote
//! postings of each company are read one by one (`GET .../postings/{id}`), at most
//! `MAX_DETAILS` per company per run: big employers list hundreds of postings. Only remote
//! postings are read; the server does the filtering. The company slug is SmartRecruiters'
//! identifier (`BoschGroup`), not the display name.

use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use super::Source;
use super::base::{failure, get_json, id_string, parse_iso, skipped_error, str_field};
use crate::models::{FetchResult, Job};
use crate::scoring::parse_salary_monthly_usd;
use crate::text::html_to_text;

const DEFAULT_BASE_URL: &str = "https://api.smartrecruiters.com";
/// The API serves at most 100 postings per page.
const PAGE_SIZE: usize = 100;
/// Listing pages read per company.
const MAX_PAGES: u32 = 3;
/// Postings per company whose detail is read each run (newest first).
const MAX_DETAILS: usize = 25;
/// The sections of a posting's ad, in reading order.
const SECTIONS: [&str; 4] = [
    "companyDescription",
    "jobDescription",
    "qualifications",
    "additionalInformation",
];

pub struct SmartRecruitersSource {
    companies: Vec<String>,
    client: Client,
    base_url: String,
}

impl SmartRecruitersSource {
    pub fn new<I, S>(companies: I, client: Client) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            companies: companies.into_iter().map(Into::into).collect(),
            client,
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    /// Points the source at another server (tests use a wiremock server).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    /// The remote postings of one company, newest first.
    async fn remote_postings(&self, company: &str) -> anyhow::Result<Vec<Value>> {
        let url = format!("{}/v1/companies/{company}/postings", self.base_url);
        let limit = PAGE_SIZE.to_string();
        let mut postings: Vec<Value> = Vec::new();
        for page in 0..MAX_PAGES {
            let offset = (page as usize * PAGE_SIZE).to_string();
            let query = [
                ("limit", limit.as_str()),
                ("offset", offset.as_str()),
                ("locationType", "REMOTE"),
            ];
            let body = get_json(&self.client, &url, &query).await?;
            let content = body["content"]
                .as_array()
                .context("response has no `content` array")?;
            postings.extend(content.iter().cloned());
            let total = body["totalFound"].as_u64().unwrap_or(0) as usize;
            if content.len() < PAGE_SIZE || postings.len() >= total {
                break;
            }
        }
        // ISO 8601 timestamps in one format sort as text.
        postings.sort_by(|a, b| str_field(b, "releasedDate").cmp(str_field(a, "releasedDate")));
        Ok(postings)
    }

    async fn detail(&self, company: &str, id: &str) -> anyhow::Result<Value> {
        let url = format!("{}/v1/companies/{company}/postings/{id}", self.base_url);
        get_json(&self.client, &url, &[]).await
    }

    async fn fetch_company(&self, company: &str, result: &mut FetchResult) -> anyhow::Result<()> {
        let postings = self.remote_postings(company).await?;
        let mut skipped = 0;
        for item in postings.iter().take(MAX_DETAILS) {
            let Some(id) = id_string(&item["id"]) else {
                skipped += 1;
                continue;
            };
            // A posting whose detail cannot be read still counts, without a description.
            let detail = match self.detail(company, &id).await {
                Ok(detail) => Some(detail),
                Err(error) => {
                    let error = error.context(format!("posting {id}"));
                    result
                        .errors
                        .push(failure("smartrecruiters", company, &error));
                    None
                }
            };
            if detail
                .as_ref()
                .is_some_and(|detail| detail["active"].as_bool() == Some(false))
            {
                continue;
            }
            match parse_posting(company, item, detail.as_ref()) {
                Some(job) => result.jobs.push(job),
                None => skipped += 1,
            }
        }
        if skipped > 0 {
            result
                .errors
                .push(skipped_error("smartrecruiters", company, skipped));
        }
        Ok(())
    }
}

#[async_trait]
impl Source for SmartRecruitersSource {
    fn name(&self) -> &str {
        "smartrecruiters"
    }

    async fn fetch(&self) -> FetchResult {
        let mut result = FetchResult::default();
        for company in &self.companies {
            if let Err(error) = self.fetch_company(company, &mut result).await {
                result
                    .errors
                    .push(failure("smartrecruiters", company, &error));
            }
        }
        result
    }
}

/// One posting from its listing entry and, when it could be read, its detail (description, public
/// page and apply link). Without the detail the public page is built from the ids.
fn parse_posting(company: &str, item: &Value, detail: Option<&Value>) -> Option<Job> {
    let external_id = id_string(&item["id"])?;
    let title = str_field(item, "name").trim();
    if title.is_empty() {
        return None;
    }
    let identifier = match str_field(&item["company"], "identifier") {
        "" => company,
        identifier => identifier,
    };
    let page = format!("https://jobs.smartrecruiters.com/{identifier}/{external_id}");
    let url = detail
        .map(|detail| str_field(detail, "postingUrl"))
        .filter(|url| !url.is_empty())
        .unwrap_or(&page);
    let apply_url = detail
        .map(|detail| str_field(detail, "applyUrl"))
        .filter(|url| !url.is_empty())
        .unwrap_or(url);
    let sections = detail.map_or(&Value::Null, |detail| &detail["jobAd"]["sections"]);
    let description = SECTIONS
        .iter()
        .filter_map(|key| sections[*key]["text"].as_str())
        .map(html_to_text)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let location = &item["location"];
    let (salary_min, salary_max) = parse_salary_monthly_usd(&description);
    Some(Job {
        source: "smartrecruiters".to_string(),
        external_id,
        company: match str_field(&item["company"], "name") {
            "" => company.to_string(),
            name => name.to_string(),
        },
        title: title.to_string(),
        url: url.to_string(),
        apply_url: apply_url.to_string(),
        location: str_field(location, "fullLocation").to_string(),
        remote: location["remote"].as_bool().unwrap_or(false),
        description,
        salary_min_monthly_usd: salary_min,
        salary_max_monthly_usd: salary_max,
        posted_at: parse_iso(str_field(item, "releasedDate")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path, path_regex, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Two real remote postings of `BoschGroup` (the listing) and the detail of the first.
    fn real_listing() -> Value {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/sources/smartrecruiters.json"
        ))
        .unwrap()
    }

    fn real_detail() -> Value {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/sources/smartrecruiters_detail.json"
        ))
        .unwrap()
    }

    #[test]
    fn parses_a_real_posting_with_its_detail() {
        let listing = real_listing();
        let detail = real_detail();
        let job = parse_posting("BoschGroup", &listing["content"][0], Some(&detail)).unwrap();

        assert_eq!(job.key(), "smartrecruiters:744000153279439");
        assert_eq!(job.company, "Bosch Group");
        assert_eq!(
            job.title,
            "ANALISTA MARKETING PRODUTO JR ou PL -  36087".trim()
        );
        assert_eq!(job.location, "Campinas, SP, Brazil");
        assert!(job.remote);
        assert!(
            job.url
                .starts_with("https://jobs.smartrecruiters.com/BoschGroup/744000153279439"),
            "{}",
            job.url
        );
        assert!(job.apply_url.starts_with(&job.url));
        assert_eq!(
            job.posted_at,
            crate::sources::base::parse_iso("2026-10-02T22:35:26.714Z")
        );
        assert!(!job.description.is_empty());
    }

    #[test]
    fn a_posting_without_its_detail_has_no_description_but_keeps_a_public_page() {
        let listing = real_listing();
        let job = parse_posting("BoschGroup", &listing["content"][1], None).unwrap();

        assert_eq!(job.description, "");
        assert_eq!(
            job.url,
            "https://jobs.smartrecruiters.com/BoschGroup/744000153258680"
        );
        assert_eq!(job.apply_url, job.url);
        assert!(job.remote);
    }

    #[test]
    fn a_posting_without_id_or_title_is_not_a_job() {
        let detail = real_detail();
        assert!(parse_posting("A", &json!({"name": "Dev"}), Some(&detail)).is_none());
        assert!(parse_posting("A", &json!({"id": "1", "name": " "}), Some(&detail)).is_none());
    }

    fn listing_of(count: usize) -> Value {
        // Newest first would be the server's order; the source must sort anyway.
        let content: Vec<Value> = (0..count)
            .map(|n| {
                json!({
                    "id": format!("{}", 1000 + n),
                    "name": format!("Engineer {n}"),
                    "releasedDate": format!("2026-09-{:02}T10:00:00.000Z", 1 + n % 28),
                    "company": {"identifier": "Acme", "name": "Acme Corp"},
                    "location": {"fullLocation": "Remote, Chile", "remote": true}
                })
            })
            .collect();
        json!({"offset": 0, "limit": 100, "totalFound": count, "content": content})
    }

    fn detail_of(id: &str) -> Value {
        json!({
            "id": id, "active": true,
            "postingUrl": format!("https://jobs.smartrecruiters.com/Acme/{id}-engineer"),
            "applyUrl": format!("https://jobs.smartrecruiters.com/Acme/{id}-engineer?oga=true"),
            "jobAd": {"sections": {"jobDescription": {"title": "Job", "text": "<p>Build things.</p>"}}}
        })
    }

    #[tokio::test]
    async fn fetch_asks_only_for_remote_postings_and_reads_their_details() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/companies/BoschGroup/postings"))
            .and(query_param("locationType", "REMOTE"))
            .and(query_param("limit", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_json(real_listing()))
            .mount(&server)
            .await;
        for id in ["744000153279439", "744000153258680"] {
            let mut detail = real_detail();
            detail["id"] = json!(id);
            Mock::given(method("GET"))
                .and(path(format!("/v1/companies/BoschGroup/postings/{id}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(detail))
                .mount(&server)
                .await;
        }

        let source =
            SmartRecruitersSource::new(["BoschGroup"], Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.jobs.len(), 2);
        assert!(result.jobs.iter().all(|job| job.remote));
    }

    #[tokio::test]
    async fn fetch_reads_only_the_newest_postings_up_to_the_cap() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(listing_of(40)))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/v1/companies/Acme/postings/\d+$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(detail_of("1")))
            .expect(MAX_DETAILS as u64)
            .mount(&server)
            .await;

        let source =
            SmartRecruitersSource::new(["Acme"], Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.jobs.len(), MAX_DETAILS);
        let dates: Vec<_> = result.jobs.iter().map(|job| job.posted_at).collect();
        let mut newest_first = dates.clone();
        newest_first.sort_by(|a, b| b.cmp(a));
        assert_eq!(dates, newest_first, "postings come newest first");
    }

    #[tokio::test]
    async fn fetch_records_a_failed_detail_and_skips_inactive_postings() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(listing_of(3)))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings/1000"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let mut inactive = detail_of("1001");
        inactive["active"] = json!(false);
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings/1001"))
            .respond_with(ResponseTemplate::new(200).set_body_json(inactive))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings/1002"))
            .respond_with(ResponseTemplate::new(200).set_body_json(detail_of("1002")))
            .mount(&server)
            .await;

        let source =
            SmartRecruitersSource::new(["Acme"], Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        let keys: Vec<String> = result.jobs.iter().map(Job::key).collect();
        // Newest first: 1002 has its detail, 1001 is inactive and dropped, and 1000 (detail 404)
        // still counts from its listing entry, without a description.
        assert_eq!(keys, ["smartrecruiters:1002", "smartrecruiters:1000"]);
        assert_eq!(result.jobs[1].description, "");
        assert!(
            result
                .errors
                .iter()
                .any(|line| line.starts_with("smartrecruiters:Acme: posting 1000")),
            "{:?}",
            result.errors
        );
    }

    #[tokio::test]
    async fn fetch_records_a_missing_company_and_keeps_the_others() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/companies/Gone/postings"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(listing_of(1)))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings/1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(detail_of("1000")))
            .mount(&server)
            .await;

        let source =
            SmartRecruitersSource::new(["Gone", "Acme"], Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert_eq!(result.jobs.len(), 1);
        assert_eq!(result.errors.len(), 1);
        assert!(
            result.errors[0].starts_with("smartrecruiters:Gone: "),
            "{}",
            result.errors[0]
        );
    }
}
