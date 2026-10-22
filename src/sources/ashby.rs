//! Ashby job boards: `GET {base}/posting-api/job-board/{org}?includeCompensation=true`.

use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use super::Source;
use super::base::{failure, get_json, id_string, join_places, parse_iso, skipped_error, str_field};
use crate::models::{FetchResult, Job};
use crate::scoring::parse_salary_monthly_usd;

const DEFAULT_BASE_URL: &str = "https://api.ashbyhq.com";

pub struct AshbySource {
    orgs: Vec<String>,
    client: Client,
    base_url: String,
}

impl AshbySource {
    pub fn new<I, S>(orgs: I, client: Client) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            orgs: orgs.into_iter().map(Into::into).collect(),
            client,
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    /// Points the source at another server (tests use a wiremock server).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    async fn fetch_org(&self, org: &str) -> anyhow::Result<(Vec<Job>, usize)> {
        let url = format!("{}/posting-api/job-board/{org}", self.base_url);
        let body = get_json(&self.client, &url, &[("includeCompensation", "true")]).await?;
        let postings = body["jobs"]
            .as_array()
            .context("response has no `jobs` array")?;
        let mut jobs = Vec::new();
        let mut skipped = 0;
        for posting in postings {
            // Postings with `isListed: false` are not public: skip them without counting them as damaged.
            if posting["isListed"].as_bool() == Some(false) {
                continue;
            }
            match parse_posting(org, posting) {
                Some(job) => jobs.push(job),
                None => skipped += 1,
            }
        }
        Ok((jobs, skipped))
    }
}

#[async_trait]
impl Source for AshbySource {
    fn name(&self) -> &str {
        "ashby"
    }

    async fn fetch(&self) -> FetchResult {
        let mut result = FetchResult::default();
        for org in &self.orgs {
            match self.fetch_org(org).await {
                Ok((jobs, skipped)) => {
                    result.jobs.extend(jobs);
                    if skipped > 0 {
                        result.errors.push(skipped_error("ashby", org, skipped));
                    }
                }
                Err(error) => result.errors.push(failure("ashby", org, &error)),
            }
        }
        result
    }
}

/// Every place the posting is open in: its location, the country of its address and the secondary
/// locations. The primary location alone hides most of it ("Toronto" for a job open in Canada and
/// the US, "Ontario - Remote" for one that never names Canada).
fn location_text(posting: &Value) -> String {
    let country = &posting["address"]["postalAddress"]["addressCountry"];
    let secondary = posting["secondaryLocations"]
        .as_array()
        .into_iter()
        .flatten();
    join_places(
        [
            str_field(posting, "location"),
            country.as_str().unwrap_or(""),
        ]
        .into_iter()
        .chain(secondary.map(|place| str_field(place, "location"))),
    )
}

/// Whether the job can be done from home. `isRemote` cannot say: Ashby sets it for every posting
/// that is not on-site, hybrid ones included. `workplaceType` is the field to trust; older
/// postings without it fall back to `isRemote` and the location text.
fn is_remote(posting: &Value, location: &str) -> bool {
    match str_field(posting, "workplaceType").to_lowercase().as_str() {
        "remote" => true,
        "hybrid" | "onsite" | "on-site" => false,
        _ => {
            posting["isRemote"].as_bool().unwrap_or(false)
                || location.to_lowercase().contains("remote")
        }
    }
}

/// Pay comes from the compensation summary when present, else from the description text.
fn parse_posting(org: &str, posting: &Value) -> Option<Job> {
    let external_id = id_string(&posting["id"])?;
    let title = str_field(posting, "title");
    let url = str_field(posting, "jobUrl");
    if title.is_empty() || url.is_empty() {
        return None;
    }
    let apply_url = match str_field(posting, "applyUrl") {
        "" => url,
        apply_url => apply_url,
    };
    let description = str_field(posting, "descriptionPlain");
    let compensation = &posting["compensation"];
    let summary = [
        "scrapeableCompensationSalarySummary",
        "compensationTierSummary",
    ]
    .into_iter()
    .map(|key| str_field(compensation, key))
    .find(|summary| !summary.is_empty())
    .unwrap_or("");
    let (mut salary_min, mut salary_max) = parse_salary_monthly_usd(summary);
    if salary_min.is_none() {
        (salary_min, salary_max) = parse_salary_monthly_usd(description);
    }
    let location = location_text(posting);
    let location = location.as_str();
    let remote = is_remote(posting, location);
    Some(Job {
        source: "ashby".to_string(),
        external_id,
        company: org.to_string(),
        title: title.to_string(),
        url: url.to_string(),
        apply_url: apply_url.to_string(),
        location: location.to_string(),
        remote,
        description: description.to_string(),
        salary_min_monthly_usd: salary_min,
        salary_max_monthly_usd: salary_max,
        posted_at: parse_iso(str_field(posting, "publishedAt")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Job;
    use reqwest::Client;
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn payload() -> Value {
        json!({
            "apiVersion": "1",
            "jobs": [
                {
                    "id": "a1",
                    "title": "Applied AI Engineer",
                    "location": "Remote - Americas",
                    "isRemote": true,
                    "workplaceType": "Remote",
                    "jobUrl": "https://jobs.ashbyhq.com/acme/a1",
                    "applyUrl": "https://jobs.ashbyhq.com/acme/a1/application",
                    "descriptionPlain": "Build agents.",
                    "compensation": {"scrapeableCompensationSalarySummary": "$120K - $160K"},
                    "publishedAt": "2026-09-30T10:00:00.000+00:00",
                    "isListed": true
                },
                {"id": "a2", "title": "Hidden", "location": "X", "jobUrl": "https://x", "isListed": false}
            ]
        })
    }

    async fn mount_org(server: &MockServer, org: &str, response: ResponseTemplate) {
        Mock::given(method("GET"))
            .and(path(format!("/posting-api/job-board/{org}")))
            .and(query_param("includeCompensation", "true"))
            .respond_with(response)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn ashby_parses_compensation_and_skips_unlisted() {
        let server = MockServer::start().await;
        mount_org(
            &server,
            "acme",
            ResponseTemplate::new(200).set_body_json(payload()),
        )
        .await;

        let source = AshbySource::new(["acme"], Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let keys: Vec<String> = result.jobs.iter().map(Job::key).collect();
        assert_eq!(keys, ["ashby:a1"]);
        let job = &result.jobs[0];
        assert!(job.remote);
        assert_eq!(job.salary_min_monthly_usd.map(f64::round), Some(10000.0));
        assert!(job.apply_url.ends_with("/application"));
        assert!(job.posted_at.is_some());
    }

    #[tokio::test]
    async fn ashby_falls_back_to_description_when_there_is_no_summary() {
        let server = MockServer::start().await;
        let body = json!({"jobs": [{
            "id": "a3", "title": "Backend Engineer", "location": "Santiago, Chile",
            "jobUrl": "https://jobs.ashbyhq.com/acme/a3",
            "descriptionPlain": "Compensation: $90K - $130K per year."
        }]});
        mount_org(
            &server,
            "acme",
            ResponseTemplate::new(200).set_body_json(body),
        )
        .await;

        let source = AshbySource::new(["acme"], Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        let job = &result.jobs[0];
        assert!(!job.remote);
        assert_eq!(job.apply_url, job.url);
        assert_eq!(job.salary_min_monthly_usd.map(f64::round), Some(7500.0));
    }

    /// A posting shaped like the live API: only the fields these tests vary are filled.
    fn posting(location: &str, workplace_type: Option<&str>, is_remote: Option<bool>) -> Value {
        let mut posting = json!({
            "id": "p1",
            "title": "Software Engineer",
            "location": location,
            "jobUrl": "https://jobs.ashbyhq.com/acme/p1",
            "descriptionPlain": "",
        });
        if let Some(workplace_type) = workplace_type {
            posting["workplaceType"] = json!(workplace_type);
        }
        if let Some(is_remote) = is_remote {
            posting["isRemote"] = json!(is_remote);
        }
        posting
    }

    #[test]
    fn a_hybrid_posting_is_not_remote_even_though_ashby_flags_it_remote() {
        // Real data (2026-10-04): `isRemote` is true for every posting that is not on-site, hybrid
        // ones included (66 of 66 hybrid Replit postings, 518 of 518 at OpenAI), so it cannot
        // tell "work from anywhere" from "three days a week in San Francisco".
        let remote = |location, workplace_type, is_remote| {
            parse_posting("acme", &posting(location, workplace_type, is_remote))
                .unwrap()
                .remote
        };
        assert!(!remote("Foster City, CA", Some("Hybrid"), Some(true)));
        assert!(!remote("San Francisco", Some("OnSite"), Some(false)));
        assert!(remote("Remote - Americas", Some("Remote"), Some(true)));
        // `workplaceType` is the better field when both are there, even against a location that
        // says "Remote".
        assert!(!remote("US - Remote", Some("OnSite"), Some(false)));
        // Older postings carry no `workplaceType`: the other signals still work.
        assert!(remote("Anywhere", None, Some(true)));
        assert!(remote("Remote - Americas", None, None));
        assert!(!remote("San Francisco", None, None));
    }

    #[test]
    fn the_location_lists_every_place_the_posting_is_open_in() {
        // Cohere's Toronto posting (live data): the primary location alone hid that it is open
        // only in Canada and the US.
        let mut cohere = posting("Toronto", Some("Remote"), Some(true));
        cohere["address"] = json!({"postalAddress": {
            "addressRegion": "Ontario", "addressCountry": "Canada", "addressLocality": "Toronto"
        }});
        cohere["secondaryLocations"] = json!([
            {"location": "Canada"},
            {"location": "United States"},
            {"location": "New York"},
            {"location": "Montreal"}
        ]);
        assert_eq!(
            parse_posting("cohere", &cohere).unwrap().location,
            "Toronto; Canada; United States; New York; Montreal"
        );

        // The country alone also says it: "Ontario - Remote" never names Canada.
        let mut openai = posting("Ontario - Remote", Some("Remote"), Some(true));
        openai["address"] = json!({"postalAddress": {"addressCountry": "Canada"}});
        assert_eq!(
            parse_posting("openai", &openai).unwrap().location,
            "Ontario - Remote; Canada"
        );

        // Nothing extra: the location stays as it was.
        let plain = posting("Santiago, Chile", None, None);
        assert_eq!(
            parse_posting("acme", &plain).unwrap().location,
            "Santiago, Chile"
        );
    }

    #[tokio::test]
    async fn ashby_error_isolated() {
        let server = MockServer::start().await;
        mount_org(&server, "gone", ResponseTemplate::new(404)).await;
        mount_org(
            &server,
            "acme",
            ResponseTemplate::new(200).set_body_json(payload()),
        )
        .await;

        let source = AshbySource::new(["gone", "acme"], Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert_eq!(result.jobs.len(), 1);
        assert_eq!(result.errors.len(), 1);
        assert!(
            result.errors[0].starts_with("ashby:gone: "),
            "{}",
            result.errors[0]
        );
    }
}
