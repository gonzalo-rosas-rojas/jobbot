//! Atlassian careers: `GET {base}/endpoint/careers/listings`.
//!
//! The careers site loads its whole job list from this public JSON endpoint (no key, no account),
//! so the source makes exactly one request per run: about 360 postings, a fifth of them in
//! Australia. The application form lives on Atlassian's iCIMS portal, which the filler cannot
//! drive, so the gate treats `icims.com` as `listing_only` and Gonza applies from the link.
//!
//! A posting lists its places as `City - Country -   City, Postcode Country`, plus `Remote - Remote`
//! (or `Remote - India - Remote`) when the team can work from home. Atlassian hires only in the
//! countries where it has a legal entity, so a bare `Remote` says nothing about Chile: it is kept
//! as `Remote` next to the countries the posting names, and a posting with no place at all gets no
//! location (the region filter then closes it).

use std::collections::HashSet;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use super::Source;
use super::base::{failure, get_json, id_string, join_places, skipped_error, str_field};
use crate::models::{FetchResult, Job};
use crate::text::html_to_text;

const DEFAULT_BASE_URL: &str = "https://www.atlassian.com";

pub struct AtlassianSource {
    client: Client,
    base_url: String,
}

impl AtlassianSource {
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

    async fn fetch_listings(&self) -> anyhow::Result<(Vec<Job>, usize)> {
        let url = format!("{}/endpoint/careers/listings", self.base_url);
        let body = get_json(&self.client, &url, &[]).await?;
        let items = body
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("response is not a list of postings"))?;
        let mut seen = HashSet::new();
        let mut jobs = Vec::new();
        let mut skipped = 0;
        for item in items {
            match parse_job(item) {
                // The endpoint repeats a posting once per portal it is published on.
                Some(job) => {
                    if seen.insert(job.external_id.clone()) {
                        jobs.push(job);
                    }
                }
                None => skipped += 1,
            }
        }
        Ok((jobs, skipped))
    }
}

#[async_trait]
impl Source for AtlassianSource {
    fn name(&self) -> &str {
        "atlassian"
    }

    async fn fetch(&self) -> FetchResult {
        let mut result = FetchResult::default();
        match self.fetch_listings().await {
            Ok((jobs, skipped)) => {
                result.jobs = jobs;
                if skipped > 0 {
                    result
                        .errors
                        .push(skipped_error("atlassian", "listings", skipped));
                }
            }
            Err(error) => result.errors.push(failure("atlassian", "listings", &error)),
        }
        result
    }
}

/// One place as `Sydney, Australia`, or `Remote` / `Remote, India`. Anything that does not follow
/// the `A - B - C` shape is kept as it is.
fn clean_place(raw: &str) -> String {
    let parts: Vec<&str> = raw.split(" - ").map(str::trim).collect();
    let first = parts.first().copied().unwrap_or("");
    let country = parts
        .get(1)
        .copied()
        .unwrap_or("")
        .trim_end_matches(" or Remote");
    if first.eq_ignore_ascii_case("remote") {
        return if country.is_empty() || country.eq_ignore_ascii_case("remote") {
            "Remote".to_string()
        } else {
            format!("Remote, {country}")
        };
    }
    if country.is_empty() {
        first.to_string()
    } else {
        format!("{first}, {country}")
    }
}

fn is_remote_place(raw: &str) -> bool {
    raw.trim_start().to_lowercase().starts_with("remote")
}

fn parse_job(item: &Value) -> Option<Job> {
    let external_id = id_string(&item["id"])?;
    let title = str_field(item, "title").trim();
    let apply_url = str_field(item, "applyUrl");
    let page = str_field(&item["portalJobPost"], "portalUrl");
    let url = if page.is_empty() {
        apply_url.split('?').next().unwrap_or("")
    } else {
        page
    };
    if title.is_empty() || url.is_empty() {
        return None;
    }
    let places: Vec<&str> = item["locations"]
        .as_array()
        .map(|places| places.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let cleaned: Vec<String> = places.iter().map(|place| clean_place(place)).collect();
    let location = join_places(cleaned.iter().map(String::as_str));
    let mut description = ["overview", "responsibilities", "qualifications"]
        .iter()
        .map(|key| html_to_text(str_field(item, key)))
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    for (label, value) in [
        ("Category", str_field(item, "category").to_string()),
        ("Type", str_field(item, "type").to_string()),
    ] {
        if !value.is_empty() {
            description = format!("{description}\n{label}: {value}");
        }
    }
    Some(Job {
        source: "atlassian".to_string(),
        external_id,
        company: "Atlassian".to_string(),
        title: title.to_string(),
        url: url.to_string(),
        apply_url: if apply_url.is_empty() { url } else { apply_url }.to_string(),
        location,
        remote: places.iter().any(|place| is_remote_place(place)),
        description,
        salary_min_monthly_usd: None,
        salary_max_monthly_usd: None,
        posted_at: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Trimmed real postings from `www.atlassian.com/endpoint/careers/listings`: a Sydney backend
    /// engineer (also Auckland and remote), a security role in three Australian cities, a Seattle
    /// role that also lists Sydney, an Indian sales role with an Indian remote place, a Sydney sales
    /// role, a Seattle-only graduate role and a role with no place at all.
    fn real_payload() -> Value {
        serde_json::from_str(include_str!("../../tests/fixtures/sources/atlassian.json")).unwrap()
    }

    fn job_at(index: usize) -> Job {
        parse_job(&real_payload()[index]).unwrap()
    }

    #[test]
    fn parses_a_real_posting() {
        let job = job_at(0);

        assert_eq!(job.key(), "atlassian:25538");
        assert_eq!(job.company, "Atlassian");
        assert_eq!(job.title, "Backend Software Engineer");
        assert_eq!(
            job.location,
            "Sydney, Australia; Remote; Auckland, New Zealand"
        );
        assert!(job.remote);
        assert!(
            job.url
                .starts_with("https://careers-apac-atlassian.icims.com/jobs/25538/")
        );
        assert!(job.apply_url.ends_with("?mode=apply"), "{}", job.apply_url);
        assert_eq!(
            job.posted_at, None,
            "`updatedDate` moves when a posting is edited"
        );
        assert!(
            job.description.contains("Category: Engineering"),
            "{}",
            job.description
        );
        assert!(!job.description.contains("<p>"), "{}", job.description);
    }

    #[test]
    fn places_lose_their_postcodes_and_keep_their_countries() {
        assert_eq!(
            job_at(1).location,
            "Sydney, Australia; Remote; Melbourne, Australia; Brisbane, Australia"
        );
        assert_eq!(
            job_at(3).location,
            // The bare `Remote` is already named inside `Remote, India`.
            "Bengaluru, India; Remote, India"
        );
        assert_eq!(job_at(5).location, "Seattle, United States");
        assert_eq!(
            clean_place(
                "Mountain View - United States or Remote -   Mountain View, California 94041 United States"
            ),
            "Mountain View, United States"
        );
        assert_eq!(clean_place("Remote - Remote"), "Remote");
        assert_eq!(clean_place("Somewhere"), "Somewhere");
    }

    #[test]
    fn a_posting_with_no_place_has_no_location_and_is_not_remote() {
        let job = job_at(6);

        assert_eq!(job.location, "");
        assert!(!job.remote);
        assert!(
            job.description.contains("Type: Full-Time"),
            "{}",
            job.description
        );
    }

    #[test]
    fn the_region_filter_opens_australian_postings_and_closes_the_rest() {
        use crate::discovery::{region_ok, region_ok_with};
        let australia = ["australia".to_string(), "sydney".to_string()];

        assert!(
            !region_ok(&job_at(0)),
            "Australia is not open without the extra place"
        );
        assert!(
            region_ok_with(&job_at(0), &australia),
            "Sydney, Auckland, remote"
        );
        assert!(
            region_ok_with(&job_at(1), &australia),
            "three Australian cities"
        );
        assert!(
            region_ok_with(&job_at(4), &australia),
            "a Sydney sales role"
        );
        assert!(!region_ok_with(&job_at(3), &australia), "India");
        assert!(!region_ok_with(&job_at(5), &australia), "Seattle only");
        assert!(!region_ok_with(&job_at(6), &australia), "no place at all");
    }

    #[test]
    fn a_posting_without_title_or_link_is_not_a_job() {
        assert!(
            parse_job(&json!({"id": 1, "applyUrl": "https://x.icims.com/jobs/1/a/job"})).is_none()
        );
        assert!(parse_job(&json!({"id": 1, "title": "Engineer"})).is_none());
        assert!(
            parse_job(
                &json!({"title": "Engineer", "applyUrl": "https://x.icims.com/jobs/1/a/job"})
            )
            .is_none()
        );
    }

    #[test]
    fn the_page_link_comes_from_the_apply_link_when_the_portal_post_is_missing() {
        let job = parse_job(&json!({
            "id": 7, "title": "Engineer", "locations": ["Sydney - Australia -   Sydney,  2000 Australia"],
            "applyUrl": "https://careers-apac-atlassian.icims.com/jobs/7/engineer/job?mode=apply"
        }))
        .unwrap();

        assert_eq!(
            job.url,
            "https://careers-apac-atlassian.icims.com/jobs/7/engineer/job"
        );
        assert!(job.apply_url.ends_with("?mode=apply"));
    }

    fn source(server: &MockServer) -> AtlassianSource {
        AtlassianSource::new(Client::new()).with_base_url(server.uri())
    }

    #[tokio::test]
    async fn fetch_reads_the_one_endpoint_and_drops_a_repeated_posting() {
        let server = MockServer::start().await;
        let mut postings = real_payload();
        let copy = postings[0].clone();
        postings.as_array_mut().unwrap().push(copy);
        Mock::given(method("GET"))
            .and(path("/endpoint/careers/listings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(postings))
            .expect(1)
            .mount(&server)
            .await;

        let result = source(&server).fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(
            result.jobs.len(),
            7,
            "seven real postings, the repeat dropped"
        );
        assert!(result.jobs.iter().all(|job| job.source == "atlassian"));
    }

    #[tokio::test]
    async fn fetch_records_skipped_postings_html_and_a_status_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/endpoint/careers/listings"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!([{"id": 1, "title": "No link"}])),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/endpoint/careers/listings"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>blocked</html>"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/endpoint/careers/listings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"jobs": []})))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/endpoint/careers/listings"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let skipped = source(&server).fetch().await;
        assert!(skipped.jobs.is_empty());
        assert_eq!(
            skipped.errors,
            ["atlassian:listings: skipped 1 postings missing id, title or url"]
        );
        for what in ["html", "not a list", "status"] {
            let result = source(&server).fetch().await;
            assert!(result.jobs.is_empty(), "{what}");
            assert_eq!(result.errors.len(), 1, "{what}: {:?}", result.errors);
            assert!(
                result.errors[0].starts_with("atlassian:listings: "),
                "{what}: {}",
                result.errors[0]
            );
        }
    }
}
