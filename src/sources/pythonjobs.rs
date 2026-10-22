//! The Python Job Board: `GET {base}/jobs/feed/rss/`.
//!
//! The python.org jobs page publishes its latest postings as an RSS feed, no key. Each item's
//! description starts with the place the poster typed (`Remote, Worldwide`), then the posting as
//! HTML; the feed has no dates. Applications happen as the posting says (a company page, a form or
//! an email), so every job keeps its python.org page as `url` and the gate must treat `python.org`
//! as `listing_only`, like `news.ycombinator.com`. The source makes one request per run.

use async_trait::async_trait;
use reqwest::Client;

use super::Source;
use super::base::{failure, get_text, skipped_error};
use super::rss::{self, Item};
use crate::models::{FetchResult, Job};
use crate::text::html_to_text;

const DEFAULT_BASE_URL: &str = "https://www.python.org";

/// Legal-form words that end a company name after a comma (`ActivePrime, Inc.`): the comma before
/// them belongs to the name.
const LEGAL_FORMS: [&str; 6] = ["inc", "llc", "ltd", "corp", "gmbh", "co"];

pub struct PythonJobsSource {
    client: Client,
    base_url: String,
}

impl PythonJobsSource {
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
        let url = format!("{}/jobs/feed/rss/", self.base_url);
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
impl Source for PythonJobsSource {
    fn name(&self) -> &str {
        "pythonjobs"
    }

    async fn fetch(&self) -> FetchResult {
        let mut result = FetchResult::default();
        match self.fetch_feed().await {
            Ok((jobs, skipped)) => {
                result.jobs = jobs;
                if skipped > 0 {
                    result
                        .errors
                        .push(skipped_error("pythonjobs", "rss", skipped));
                }
            }
            Err(error) => result.errors.push(failure("pythonjobs", "rss", &error)),
        }
        result
    }
}

/// `"Python developer, Eleks"` is the role `Python developer` at `Eleks`: the company is what follows
/// the last comma, with a legal form (`Inc.`) taking the comma before it along.
fn split_company(full: &str) -> (String, String) {
    let full = full.trim();
    let Some((rest, last)) = full.rsplit_once(", ") else {
        return (String::new(), full.to_string());
    };
    let mut company = last.trim().to_string();
    let mut role = rest.trim();
    let form = company.trim_end_matches('.').to_lowercase();
    if LEGAL_FORMS.contains(&form.as_str())
        && let Some((before, name)) = role.rsplit_once(", ")
    {
        company = format!("{}, {company}", name.trim());
        role = before.trim();
    }
    if role.is_empty() || company.is_empty() {
        return (String::new(), full.to_string());
    }
    (company, role.to_string())
}

/// The jobs board holds remote and on-site postings. A posting is remote when its place or its
/// title says so ("Remote, Worldwide", "Software Engineer (Remote)"); the place is kept as typed
/// for the region filter. The feed has no id: it is the number in the posting's link.
fn parse_item(item: &Item) -> Option<Job> {
    let url = item.text("link");
    let external_id = url
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))?;
    let (company, title) = split_company(&item.text("title"));
    if title.is_empty() {
        return None;
    }
    let raw = item.text("description");
    // The place the poster typed is the first line, before the HTML of the posting.
    let (location, body) = match raw.split_once('\n') {
        Some((first, rest)) if !first.trim_start().starts_with('<') => {
            (first.trim().to_string(), rest)
        }
        _ => (String::new(), raw.as_str()),
    };
    let remote =
        title.to_lowercase().contains("remote") || location.to_lowercase().contains("remote");
    Some(Job {
        source: "pythonjobs".to_string(),
        external_id: external_id.to_string(),
        company,
        title,
        apply_url: url.clone(),
        url,
        location,
        remote,
        description: html_to_text(body),
        salary_min_monthly_usd: None,
        salary_max_monthly_usd: None,
        posted_at: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A trimmed real response from `python.org/jobs/feed/rss/`: two remote-open postings, one
    /// with a legal form in the company, a London-time-zone one, an on-site one and one remote in
    /// its title only.
    fn real_feed() -> &'static str {
        include_str!("../../tests/fixtures/sources/pythonjobs.xml")
    }

    fn job_at(index: usize) -> Job {
        let items = rss::items(real_feed()).unwrap();
        parse_item(&items[index]).unwrap()
    }

    async fn serve(response: ResponseTemplate) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/jobs/feed/rss/"))
            .respond_with(response)
            .mount(&server)
            .await;
        server
    }

    #[test]
    fn parses_a_real_job() {
        let job = job_at(0);

        assert_eq!(job.key(), "pythonjobs:8139");
        assert_eq!(job.company, "tem");
        assert_eq!(
            job.title,
            "Senior Staff Engineer - Origination & New Products"
        );
        assert_eq!(
            job.location,
            "Remote (UK / EU), Remote (UK / EU), Remote (UK / EU)"
        );
        assert!(job.remote);
        assert_eq!(job.url, "https://www.python.org/jobs/8139/");
        assert_eq!(job.apply_url, job.url);
        assert!(job.posted_at.is_none(), "the feed has no dates");
        assert!(
            job.description.contains("production-grade")
                || job
                    .description
                    .contains("hands-on individual-contributor role"),
            "{}",
            job.description
        );
        assert!(!job.description.contains('<'), "{}", job.description);
        assert!(
            !job.description.contains("Remote (UK"),
            "the place line is the location, not the description: {}",
            job.description
        );
    }

    #[test]
    fn a_legal_form_stays_in_the_company_name() {
        let job = job_at(2);

        assert_eq!(job.company, "ActivePrime, Inc.");
        assert_eq!(
            job.title,
            "SENIOR Back-End Python Engineer - FULLY REMOTE-WORLDWIDE"
        );
        assert_eq!(
            split_company(
                "Software Engineer, Full Stack (Python, Java, Rust), Gridnaut Recruiting"
            ),
            (
                "Gridnaut Recruiting".to_string(),
                "Software Engineer, Full Stack (Python, Java, Rust)".to_string()
            )
        );
        assert_eq!(
            split_company("Python Developer"),
            (String::new(), "Python Developer".to_string())
        );
    }

    #[test]
    fn a_posting_is_remote_when_its_place_or_its_title_says_so() {
        assert!(job_at(1).remote, "Remote, Worlwide, Worldwide");
        assert!(!job_at(4).remote, "Birmingham, United Kingdom");
        assert!(job_at(5).remote, "(Remote) in the title, a Florida place");
    }

    #[test]
    fn the_region_filter_reads_the_place_the_poster_typed() {
        use crate::discovery::region_ok;

        assert!(region_ok(&job_at(1)), "Remote, Worlwide, Worldwide");
        assert!(!region_ok(&job_at(0)), "Remote (UK / EU)");
        assert!(!region_ok(&job_at(3)), "within 2 hours of London timezone");
        assert!(!region_ok(&job_at(4)), "on-site in Birmingham");
        assert!(!region_ok(&job_at(5)), "Florida");
    }

    #[test]
    fn an_item_without_a_numbered_link_or_a_title_is_not_a_job() {
        let feed = "<rss><channel>\
            <item><title>Dev, Acme</title></item>\
            <item><link>https://www.python.org/jobs/12/</link></item>\
            <item><title>Dev, Acme</title><link>https://www.python.org/jobs/</link></item>\
            </channel></rss>";
        let items = rss::items(feed).unwrap();

        assert!(items.iter().all(|item| parse_item(item).is_none()));
    }

    #[tokio::test]
    async fn fetch_makes_one_request_and_reads_every_item() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/jobs/feed/rss/"))
            .respond_with(ResponseTemplate::new(200).set_body_string(real_feed()))
            .expect(1)
            .mount(&server)
            .await;

        let source = PythonJobsSource::new(Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.jobs.len(), 6);
        assert!(result.jobs.iter().all(|job| job.source == "pythonjobs"));
    }

    #[tokio::test]
    async fn fetch_records_a_block_page_an_http_error_and_skipped_items() {
        let blocked =
            serve(ResponseTemplate::new(200).set_body_string("<html>blocked</html>")).await;
        let result = PythonJobsSource::new(Client::new())
            .with_base_url(blocked.uri())
            .fetch()
            .await;
        assert!(result.jobs.is_empty());
        assert!(
            result.errors[0].starts_with("pythonjobs:rss: ")
                && result.errors[0].contains("not an RSS feed"),
            "{}",
            result.errors[0]
        );

        let failing = serve(ResponseTemplate::new(503)).await;
        let result = PythonJobsSource::new(Client::new())
            .with_base_url(failing.uri())
            .fetch()
            .await;
        assert!(
            result.errors[0].starts_with("pythonjobs:rss: "),
            "{}",
            result.errors[0]
        );

        let bare =
            serve(ResponseTemplate::new(200).set_body_string(
                "<rss><channel><item><title>No link</title></item></channel></rss>",
            ))
            .await;
        let result = PythonJobsSource::new(Client::new())
            .with_base_url(bare.uri())
            .fetch()
            .await;
        assert_eq!(
            result.errors,
            vec!["pythonjobs:rss: skipped 1 postings missing id, title or url".to_string()]
        );
    }
}
