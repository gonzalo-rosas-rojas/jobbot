//! SmartRecruiters postings: `GET {base}/v1/companies/{company}/postings?locationType=REMOTE`.
//!
//! Public and keyless. The listing has no description and no public page, so the newest remote
//! postings of each company are read one by one (`GET .../postings/{id}`), at most
//! `MAX_DETAILS` per company per run: big employers list hundreds of postings. Remote postings
//! are read, the server doing the filtering. The company slug is SmartRecruiters'
//! identifier (`BoschGroup`), not the display name.
//!
//! `onsite_countries` (ISO alpha-2 codes, `au`) are listed again with `country={code}` and no
//! remote filter, so on-site and hybrid postings there are read too: Gonza would move to
//! Australia. Both listings are merged by posting id before the details are read: postings whose
//! title matches `criteria.extra_open_title_words` go first (a big employer's newest postings are
//! mostly other fields), then the rest, each group newest first. Relevant postings the cap leaves
//! unread are reported in `errors`.

use crate::http::Http;
use anyhow::Context;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashSet;

use super::Source;
use super::base::{failure, get_json, id_string, parse_iso, skipped_error, str_field};
use jobbot_domain::models::{FetchError, FetchResult, Job};
use jobbot_domain::region::title_matches_extra_words;
use jobbot_domain::scoring::parse_salary_monthly_usd;
use jobbot_domain::text::html_to_text;

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
    /// ISO alpha-2 codes (lowercase) whose on-site and hybrid postings are read too.
    onsite_countries: Vec<String>,
    /// `criteria.extra_open_title_words`: postings whose title matches are read first (see
    /// `jobbot_domain::region::title_matches_extra_words`). Empty: every title is relevant.
    title_words: Vec<String>,
    http: Http,
    base_url: String,
}

impl SmartRecruitersSource {
    pub fn new<I, S>(companies: I, http: impl Into<Http>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            companies: companies.into_iter().map(Into::into).collect(),
            onsite_countries: Vec::new(),
            title_words: Vec::new(),
            http: http.into(),
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    /// Points the source at another server (tests use a wiremock server).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    /// Also reads every posting (on-site and hybrid included) in these countries, given as ISO
    /// alpha-2 codes (`au`); blank codes are ignored.
    pub fn with_onsite_countries<I, S>(mut self, countries: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.onsite_countries = countries
            .into_iter()
            .map(|code| code.as_ref().trim().to_ascii_lowercase())
            .filter(|code| !code.is_empty())
            .collect();
        self
    }

    /// The title words (`criteria.extra_open_title_words`, `!` vetoes included) that decide which
    /// postings are read first when a company has more than `MAX_DETAILS`.
    pub fn with_title_words<I, S>(mut self, words: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.title_words = words.into_iter().map(Into::into).collect();
        self
    }

    /// One listing of a company, `filter` being its extra query parameter (`locationType=REMOTE`
    /// or `country={code}`), in the server's order.
    async fn listing(&self, company: &str, filter: (&str, &str)) -> anyhow::Result<Vec<Value>> {
        let url = format!("{}/v1/companies/{company}/postings", self.base_url);
        let limit = PAGE_SIZE.to_string();
        let mut postings: Vec<Value> = Vec::new();
        for page in 0..MAX_PAGES {
            let offset = (page as usize * PAGE_SIZE).to_string();
            let query = [
                ("limit", limit.as_str()),
                ("offset", offset.as_str()),
                filter,
            ];
            let body = get_json(&self.http, &url, &query).await?;
            let content = body["content"]
                .as_array()
                .context("response has no `content` array")?;
            postings.extend(content.iter().cloned());
            let total = body["totalFound"].as_u64().unwrap_or(0) as usize;
            if content.len() < PAGE_SIZE || postings.len() >= total {
                break;
            }
        }
        Ok(postings)
    }

    /// The remote postings of one company and its postings in `onsite_countries`, each posting
    /// once: those with a relevant title first, then the rest, each group newest first, and how
    /// many are relevant. A country listing that fails is recorded and the others are kept; the
    /// remote listing failing fails the company.
    async fn postings(
        &self,
        company: &str,
        result: &mut FetchResult,
    ) -> anyhow::Result<(Vec<Value>, usize)> {
        let mut postings = self.listing(company, ("locationType", "REMOTE")).await?;
        for country in &self.onsite_countries {
            match self.listing(company, ("country", country)).await {
                Ok(listing) => postings.extend(listing),
                Err(error) => {
                    let error = error.context(format!("country {country}"));
                    result
                        .errors
                        .push(failure("smartrecruiters", company, &error));
                }
            }
        }
        // A posting without an id is kept: it is counted as skipped later.
        let mut seen = HashSet::new();
        postings.retain(|item| id_string(&item["id"]).is_none_or(|id| seen.insert(id)));
        // ISO 8601 timestamps in one format sort as text.
        postings.sort_by(|a, b| str_field(b, "releasedDate").cmp(str_field(a, "releasedDate")));
        // Stable: relevant titles move to the front and keep their newest-first order.
        let relevant = |item: &Value| {
            title_matches_extra_words(str_field(item, "name").trim(), &self.title_words)
        };
        let (mut first, rest): (Vec<Value>, Vec<Value>) = postings.into_iter().partition(relevant);
        let relevant_count = first.len();
        first.extend(rest);
        Ok((first, relevant_count))
    }

    async fn detail(&self, company: &str, id: &str) -> anyhow::Result<Value> {
        let url = format!("{}/v1/companies/{company}/postings/{id}", self.base_url);
        get_json(&self.http, &url, &[]).await
    }

    async fn fetch_company(&self, company: &str, result: &mut FetchResult) -> anyhow::Result<()> {
        let (postings, relevant) = self.postings(company, result).await?;
        // With no title words every title is relevant and nothing is worth reporting.
        if !self.title_words.is_empty() && relevant > MAX_DETAILS {
            result.errors.push(FetchError::failed(
                "smartrecruiters",
                company,
                format!(
                    "{} postings with a relevant title were not read (only {MAX_DETAILS} details per run)",
                    relevant - MAX_DETAILS
                ),
            ));
        }
        let mut skipped = 0;
        for item in postings.iter().take(MAX_DETAILS) {
            let Some(id) = id_string(&item["id"]) else {
                skipped += 1;
                continue;
            };
            // A posting whose detail cannot be read is not emitted: discovery would store it
            // without a description and later runs never fill that in. It is not stored, so the
            // next run reads it again.
            let detail = match self.detail(company, &id).await {
                Ok(detail) => detail,
                Err(error) => {
                    let error = error.context(format!("posting {id}"));
                    result
                        .errors
                        .push(failure("smartrecruiters", company, &error));
                    continue;
                }
            };
            if detail["active"].as_bool() == Some(false) {
                continue;
            }
            match parse_posting(company, item, Some(&detail)) {
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
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path, path_regex, query_param, query_param_is_missing};
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

        let source = SmartRecruitersSource::new(["BoschGroup"], reqwest::Client::new())
            .with_base_url(server.uri());
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

        let source = SmartRecruitersSource::new(["Acme"], reqwest::Client::new())
            .with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.jobs.len(), MAX_DETAILS);
        let dates: Vec<_> = result.jobs.iter().map(|job| job.posted_at).collect();
        let mut newest_first = dates.clone();
        newest_first.sort_by(|a, b| b.cmp(a));
        assert_eq!(dates, newest_first, "postings come newest first");
    }

    #[tokio::test]
    async fn fetch_records_a_failed_detail_without_emitting_it_and_skips_inactive_postings() {
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

        let source = SmartRecruitersSource::new(["Acme"], reqwest::Client::new())
            .with_base_url(server.uri());
        let result = source.fetch().await;

        let keys: Vec<String> = result.jobs.iter().map(Job::key).collect();
        // Newest first: 1002 has its detail, 1001 is inactive and dropped, and 1000 (detail 404)
        // is not emitted at all: stored without a description it would never get one.
        assert_eq!(keys, ["smartrecruiters:1002"]);
        assert!(!result.jobs[0].description.is_empty());
        assert!(
            result.errors.iter().any(|line| line
                .to_string()
                .starts_with("smartrecruiters:Acme: posting 1000")),
            "{:?}",
            result.errors
        );
    }

    #[tokio::test]
    async fn a_posting_whose_detail_fails_with_a_server_error_is_left_for_the_next_run() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(listing_of(2)))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings/1000"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings/1001"))
            .respond_with(ResponseTemplate::new(200).set_body_json(detail_of("1001")))
            .mount(&server)
            .await;

        let source = SmartRecruitersSource::new(["Acme"], reqwest::Client::new())
            .with_base_url(server.uri());
        let result = source.fetch().await;

        let keys: Vec<String> = result.jobs.iter().map(Job::key).collect();
        assert_eq!(keys, ["smartrecruiters:1001"]);
        assert_eq!(result.jobs[0].description, "Build things.");
        assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
        assert!(
            result.errors[0]
                .to_string()
                .starts_with("smartrecruiters:Acme: posting 1000"),
            "{}",
            result.errors[0]
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

        let source = SmartRecruitersSource::new(["Gone", "Acme"], reqwest::Client::new())
            .with_base_url(server.uri());
        let result = source.fetch().await;

        assert_eq!(result.jobs.len(), 1);
        assert_eq!(result.errors.len(), 1);
        assert!(
            result.errors[0]
                .to_string()
                .starts_with("smartrecruiters:Gone: "),
            "{}",
            result.errors[0]
        );
    }

    /// A listing with these ids, all dated in September (newest is the last id).
    fn listing_with(ids: &[&str], remote: bool) -> Value {
        let content: Vec<Value> = ids
            .iter()
            .enumerate()
            .map(|(n, id)| {
                json!({
                    "id": id,
                    "name": format!("Engineer {id}"),
                    "releasedDate": format!("2026-09-{:02}T10:00:00.000Z", 1 + n),
                    "company": {"identifier": "Acme", "name": "Acme Corp"},
                    "location": {"fullLocation": "Sydney, NSW, Australia", "remote": remote}
                })
            })
            .collect();
        json!({"offset": 0, "limit": 100, "totalFound": ids.len(), "content": content})
    }

    #[tokio::test]
    async fn fetch_also_reads_the_onsite_postings_of_each_country_once_per_id() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings"))
            .and(query_param("locationType", "REMOTE"))
            .and(query_param_is_missing("country"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(listing_with(&["1000", "1001"], true)),
            )
            .expect(1)
            .mount(&server)
            .await;
        // The country listing has no remote filter, and repeats 1001 (a remote posting in Australia).
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings"))
            .and(query_param("country", "au"))
            .and(query_param("limit", "100"))
            .and(query_param_is_missing("locationType"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(listing_with(&["1001", "2000"], false)),
            )
            .expect(1)
            .mount(&server)
            .await;
        for id in ["1000", "1001", "2000"] {
            Mock::given(method("GET"))
                .and(path(format!("/v1/companies/Acme/postings/{id}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(detail_of(id)))
                .expect(1)
                .mount(&server)
                .await;
        }

        let source = SmartRecruitersSource::new(["Acme"], reqwest::Client::new())
            .with_onsite_countries([" AU "])
            .with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let mut keys: Vec<String> = result.jobs.iter().map(Job::key).collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "smartrecruiters:1000",
                "smartrecruiters:1001",
                "smartrecruiters:2000"
            ]
        );
        let onsite = result
            .jobs
            .iter()
            .find(|job| job.external_id == "2000")
            .unwrap();
        assert!(!onsite.remote);
        assert_eq!(onsite.location, "Sydney, NSW, Australia");
    }

    #[tokio::test]
    async fn fetch_keeps_the_remote_postings_when_a_country_listing_fails() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings"))
            .and(query_param("locationType", "REMOTE"))
            .respond_with(ResponseTemplate::new(200).set_body_json(listing_with(&["1000"], true)))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings"))
            .and(query_param("country", "au"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings/1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(detail_of("1000")))
            .mount(&server)
            .await;

        let source = SmartRecruitersSource::new(["Acme"], reqwest::Client::new())
            .with_onsite_countries(["au"])
            .with_base_url(server.uri());
        let result = source.fetch().await;

        assert_eq!(result.jobs.len(), 1);
        assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
        assert!(
            result.errors[0]
                .to_string()
                .starts_with("smartrecruiters:Acme: country au"),
            "{}",
            result.errors[0]
        );
    }

    /// A country listing of `nurses` newer "Registered Nurse" postings (ids 1000..) and `engineers`
    /// older "Software Engineer" ones (ids 2000..). The remote listing is empty.
    async fn mount_nurses_and_engineers(server: &MockServer, nurses: usize, engineers: usize) {
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings"))
            .and(query_param("locationType", "REMOTE"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!({"offset": 0, "limit": 100, "totalFound": 0, "content": []}),
                ),
            )
            .mount(server)
            .await;
        let mut content: Vec<Value> = Vec::new();
        for (count, base, name, month) in [
            (nurses, 1000, "Registered Nurse", "10"),
            (engineers, 2000, "Software Engineer", "08"),
        ] {
            for n in 0..count {
                content.push(json!({
                    "id": format!("{}", base + n), "name": name,
                    "releasedDate": format!("2026-{month}-{:02}T10:00:00.000Z", 1 + n % 28),
                    "company": {"identifier": "Acme", "name": "Acme Corp"},
                    "location": {"fullLocation": "Sydney, NSW, Australia", "remote": false}
                }));
            }
        }
        let total = content.len();
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings"))
            .and(query_param("country", "au"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"offset": 0, "limit": 100, "totalFound": total, "content": content}),
            ))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/v1/companies/Acme/postings/\d+$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(detail_of("1")))
            .mount(server)
            .await;
    }

    fn acme_with_title_words(server: &MockServer, words: &[&str]) -> SmartRecruitersSource {
        SmartRecruitersSource::new(["Acme"], reqwest::Client::new())
            .with_onsite_countries(["au"])
            .with_title_words(words.iter().copied())
            .with_base_url(server.uri())
    }

    #[tokio::test]
    async fn relevant_titles_are_read_before_newer_ones_of_other_fields() {
        let server = MockServer::start().await;
        mount_nurses_and_engineers(&server, 30, 10).await;

        let result = acme_with_title_words(&server, &["engineer"]).fetch().await;

        let engineers = result
            .jobs
            .iter()
            .filter(|job| job.title == "Software Engineer")
            .count();
        assert_eq!(engineers, 10, "{:?}", result.errors);
        assert_eq!(result.jobs.len(), MAX_DETAILS);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
    }

    #[tokio::test]
    async fn relevant_postings_left_unread_by_the_cap_are_reported() {
        let server = MockServer::start().await;
        mount_nurses_and_engineers(&server, 0, 60).await;

        let result = acme_with_title_words(&server, &["engineer"]).fetch().await;

        assert_eq!(result.jobs.len(), MAX_DETAILS);
        assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
        let line = result.errors[0].to_string();
        assert!(
            line.starts_with("smartrecruiters:Acme: 35 postings"),
            "{line}"
        );
    }

    #[tokio::test]
    async fn without_title_words_the_newest_are_read_whatever_their_field() {
        let server = MockServer::start().await;
        mount_nurses_and_engineers(&server, 30, 10).await;

        let result = acme_with_title_words(&server, &[]).fetch().await;

        assert_eq!(result.jobs.len(), MAX_DETAILS);
        assert!(
            result
                .jobs
                .iter()
                .all(|job| job.title == "Registered Nurse")
        );
        assert!(result.errors.is_empty(), "{:?}", result.errors);
    }

    #[tokio::test]
    async fn fetch_without_onsite_countries_asks_for_no_country_listing() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings"))
            .and(query_param("locationType", "REMOTE"))
            .respond_with(ResponseTemplate::new(200).set_body_json(listing_with(&["1000"], true)))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings"))
            .and(query_param_is_missing("locationType"))
            .respond_with(ResponseTemplate::new(200).set_body_json(listing_with(&["2000"], false)))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/companies/Acme/postings/1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(detail_of("1000")))
            .mount(&server)
            .await;

        let source = SmartRecruitersSource::new(["Acme"], reqwest::Client::new())
            .with_onsite_countries(Vec::<String>::new())
            .with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let keys: Vec<String> = result.jobs.iter().map(Job::key).collect();
        assert_eq!(keys, ["smartrecruiters:1000"]);
    }
}
