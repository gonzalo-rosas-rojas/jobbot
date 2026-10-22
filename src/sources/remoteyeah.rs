//! RemoteYeah feed: `GET {base}/rss.xml`.
//!
//! The all-jobs RSS feed is free and needs no account (the site says so on its RSS page). It
//! holds the latest 200 postings, and the category feeds are subsets of it, so the source makes
//! exactly one request per run. The apply button lives on the RemoteYeah page and leads to the
//! company, so every job keeps its RemoteYeah page as `url` and the gate must treat
//! `remoteyeah.com` as `listing_only`, like `remotive.com`.

use async_trait::async_trait;
use reqwest::Client;

use super::Source;
use super::base::{failure, get_text, parse_iso, skipped_error};
use super::rss::{self, Item};
use crate::models::{FetchResult, Job};
use crate::text::html_to_text;

const DEFAULT_BASE_URL: &str = "https://remoteyeah.com";

pub struct RemoteYeahSource {
    client: Client,
    base_url: String,
}

impl RemoteYeahSource {
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
        let url = format!("{}/rss.xml", self.base_url);
        let body = get_text(&self.client, &url, &[]).await?;
        let mut jobs = Vec::new();
        let mut skipped = 0;
        for item in rss::items(&body)? {
            match parse_item(&item) {
                Some(job) => jobs.push(job),
                None => skipped += 1,
            }
        }
        Ok((jobs, skipped))
    }
}

#[async_trait]
impl Source for RemoteYeahSource {
    fn name(&self) -> &str {
        "remoteyeah"
    }

    async fn fetch(&self) -> FetchResult {
        let mut result = FetchResult::default();
        match self.fetch_feed().await {
            Ok((jobs, skipped)) => {
                result.jobs = jobs;
                if skipped > 0 {
                    result
                        .errors
                        .push(skipped_error("remoteyeah", "rss", skipped));
                }
            }
            Err(error) => result.errors.push(failure("remoteyeah", "rss", &error)),
        }
        result
    }
}

/// `"Remote Staff Software Engineer at Cision"` is the role `Staff Software Engineer`: the feed
/// wraps every title in `Remote ... at {company}`, which would only hide the same role posted by
/// another source.
fn clean_title(raw: &str, company: &str) -> String {
    let title = raw.trim();
    let title = match title.get(..7) {
        Some(prefix) if prefix.eq_ignore_ascii_case("remote ") => title[7..].trim_start(),
        _ => title,
    };
    let suffix = format!(" at {company}");
    match title.strip_suffix(suffix.as_str()) {
        Some(role) if !company.is_empty() && !role.trim().is_empty() => role.trim().to_string(),
        _ => title.to_string(),
    }
}

/// Every RemoteYeah posting is remote. `location` is one country or region, or a comma-separated
/// list of them (`Latin America, Chile`), and is empty when the posting names none; the region
/// filter reads it as it is. The page link carries `utm_source` and `ref` tracking, which is cut.
fn parse_item(item: &Item) -> Option<Job> {
    let external_id = item.text("guid");
    let link = item.text("link");
    let url = link.split('?').next().unwrap_or("");
    let company = item.text("company");
    let title = clean_title(&item.text("title"), &company);
    if external_id.is_empty() || title.is_empty() || url.is_empty() {
        return None;
    }
    let mut description = html_to_text(&item.text("description"));
    let category = item.text("category");
    if !category.is_empty() {
        description = format!("{description}\nCategory: {category}");
    }
    Some(Job {
        source: "remoteyeah".to_string(),
        external_id,
        company,
        title,
        url: url.to_string(),
        apply_url: url.to_string(),
        location: item.text("location"),
        remote: true,
        description,
        salary_min_monthly_usd: None,
        salary_max_monthly_usd: None,
        posted_at: parse_iso(&item.text("pubDate")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A trimmed real response from `remoteyeah.com/rss.xml`: a Latin America posting, a single
    /// country, a worldwide one and one with no location.
    fn real_feed() -> &'static str {
        include_str!("../../tests/fixtures/sources/remoteyeah.xml")
    }

    fn job_at(index: usize) -> Job {
        let items = rss::items(real_feed()).unwrap();
        parse_item(&items[index]).unwrap()
    }

    async fn serve(response: ResponseTemplate) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rss.xml"))
            .respond_with(response)
            .mount(&server)
            .await;
        server
    }

    #[test]
    fn parses_a_real_job() {
        let job = job_at(0);

        assert_eq!(
            job.key(),
            "remoteyeah:software-architect-aws-nodejs-latin-america-remote-bluelight-consulting-6"
        );
        assert_eq!(job.company, "Bluelight Consulting");
        assert_eq!(
            job.title,
            "Software Architect (AWS, NodeJS) - Latin America, Remote"
        );
        assert_eq!(job.location, "Latin America, Chile");
        assert!(job.remote);
        assert_eq!(
            job.url,
            "https://remoteyeah.com/jobs/remote-software-architect-aws-nodejs-latin-america-remote-bluelight-consulting-6"
        );
        assert_eq!(job.apply_url, job.url);
        assert_eq!(
            job.posted_at,
            crate::sources::base::parse_iso("2026-10-04T20:01:20+00:00")
        );
        assert!(
            job.description.contains("Skills: JavaScript, TypeScript")
                && job
                    .description
                    .contains("Locations: Latin America (Remote), Chile (Remote)")
                && job.description.contains("Category: Software Architect"),
            "{}",
            job.description
        );
        assert!(!job.description.contains('<'), "{}", job.description);
    }

    #[test]
    fn the_remote_prefix_and_the_company_suffix_are_cut_from_the_title() {
        assert_eq!(job_at(1).title, "Staff Software Engineer");
        assert_eq!(job_at(2).title, "Online Data Analyst - Spanish (US)");
        // Only a suffix that names this company is cut, and a title that is nothing else stays.
        assert_eq!(
            clean_title("Remote Rust Engineer at Somewhere Else", "Acme"),
            "Rust Engineer at Somewhere Else"
        );
        assert_eq!(
            clean_title(" Remote Remote Cat at Cat ", "Cat"),
            "Remote Cat"
        );
        assert_eq!(clean_title("Rust Engineer", ""), "Rust Engineer");
        assert_eq!(clean_title("Remote", "Acme"), "Remote");
    }

    #[test]
    fn a_posting_with_no_location_is_kept_with_an_empty_one() {
        let job = job_at(3);

        assert_eq!(job.location, "");
        assert_eq!(job.company, "Carrot");
        assert_eq!(job.title, "Sr. Data Engineer");
    }

    #[test]
    fn the_region_filter_reads_the_countries_the_posting_names() {
        use crate::discovery::region_ok;

        assert!(region_ok(&job_at(0)), "Latin America, Chile");
        assert!(!region_ok(&job_at(1)), "Bulgaria");
        assert!(region_ok(&job_at(2)), "Worldwide");
        assert!(
            region_ok(&job_at(3)),
            "no location: the description decides"
        );
    }

    #[tokio::test]
    async fn fetch_makes_one_request_and_reads_every_item() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rss.xml"))
            .respond_with(ResponseTemplate::new(200).set_body_string(real_feed()))
            .expect(1)
            .mount(&server)
            .await;

        let source = RemoteYeahSource::new(Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.jobs.len(), 4);
        assert!(result.jobs.iter().all(|job| job.source == "remoteyeah"));
    }

    #[tokio::test]
    async fn fetch_records_a_block_page_and_an_http_error() {
        let server =
            serve(ResponseTemplate::new(200).set_body_string("<html>blocked</html>")).await;
        let result = RemoteYeahSource::new(Client::new())
            .with_base_url(server.uri())
            .fetch()
            .await;
        assert!(result.jobs.is_empty());
        assert!(
            result.errors[0].starts_with("remoteyeah:rss: ")
                && result.errors[0].contains("not an RSS feed"),
            "{}",
            result.errors[0]
        );

        let failing = serve(ResponseTemplate::new(503)).await;
        let result = RemoteYeahSource::new(Client::new())
            .with_base_url(failing.uri())
            .fetch()
            .await;
        assert!(result.jobs.is_empty());
        assert!(
            result.errors[0].starts_with("remoteyeah:rss: "),
            "{}",
            result.errors[0]
        );
    }

    #[tokio::test]
    async fn fetch_records_items_without_a_guid_title_or_link() {
        let body = "<rss><channel>\
            <item><title>Remote Dev at A</title><link>https://remoteyeah.com/jobs/a</link></item>\
            <item><guid>b</guid><link>https://remoteyeah.com/jobs/b</link></item>\
            <item><guid>c</guid><title>Remote Dev at C</title></item>\
            <item><guid>d</guid><title>Remote Dev at D</title><link>https://remoteyeah.com/jobs/d</link></item>\
            </channel></rss>";
        let server = serve(ResponseTemplate::new(200).set_body_string(body)).await;

        let result = RemoteYeahSource::new(Client::new())
            .with_base_url(server.uri())
            .fetch()
            .await;

        assert_eq!(result.jobs.len(), 1);
        assert_eq!(
            result.errors,
            vec!["remoteyeah:rss: skipped 3 postings missing id, title or url".to_string()]
        );
    }
}
