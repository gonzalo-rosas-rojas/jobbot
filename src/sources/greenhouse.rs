//! Greenhouse boards: `GET {base}/v1/boards/{board}/jobs?content=true`.

use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use super::Source;
use super::base::{failure, get_json, id_string, join_places, parse_iso, skipped_error, str_field};
use crate::models::{FetchResult, Job};
use crate::scoring::parse_salary_monthly_usd;
use crate::text::{host_of, html_to_text};

const DEFAULT_BASE_URL: &str = "https://boards-api.greenhouse.io";

pub struct GreenhouseSource {
    boards: Vec<String>,
    client: Client,
    base_url: String,
}

impl GreenhouseSource {
    pub fn new<I, S>(boards: I, client: Client) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            boards: boards.into_iter().map(Into::into).collect(),
            client,
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    /// Points the source at another server (tests use a wiremock server).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    /// One board: the parsed jobs and how many postings were skipped for missing fields.
    async fn fetch_board(&self, board: &str) -> anyhow::Result<(Vec<Job>, usize)> {
        let url = format!("{}/v1/boards/{board}/jobs", self.base_url);
        let body = get_json(&self.client, &url, &[("content", "true")]).await?;
        let postings = body["jobs"]
            .as_array()
            .context("response has no `jobs` array")?;
        let mut jobs = Vec::new();
        let mut skipped = 0;
        for posting in postings {
            match parse_posting(board, posting) {
                Some(job) => jobs.push(job),
                None => skipped += 1,
            }
        }
        Ok((jobs, skipped))
    }
}

#[async_trait]
impl Source for GreenhouseSource {
    fn name(&self) -> &str {
        "greenhouse"
    }

    async fn fetch(&self) -> FetchResult {
        let mut result = FetchResult::default();
        for board in &self.boards {
            match self.fetch_board(board).await {
                Ok((jobs, skipped)) => {
                    result.jobs.extend(jobs);
                    if skipped > 0 {
                        result
                            .errors
                            .push(skipped_error("greenhouse", board, skipped));
                    }
                }
                Err(error) => result.errors.push(failure("greenhouse", board, &error)),
            }
        }
        result
    }
}

/// The page the bot fills. Many companies point `absolute_url` at their own site, where the form
/// is an iframe the filler cannot scan. Greenhouse's hosted `/{board}/jobs/{id}` page is no way
/// out for them (it redirects back to the company site); the page the iframe itself loads, the
/// embed form, opens on its own and is the plain form.
fn apply_url_for(board: &str, external_id: &str, absolute_url: &str) -> String {
    let host = host_of(absolute_url);
    if host == "greenhouse.io" || host.ends_with(".greenhouse.io") {
        absolute_url.to_string()
    } else {
        format!("https://job-boards.greenhouse.io/embed/job_app?for={board}&token={external_id}")
    }
}

/// The public location plus the names of the remote offices. A plain "Remote" posting often says
/// where it is open only through its office ("Remote US", "Remote Canada"); a physical office is
/// the team's home, not a statement about who may apply, so it is left out.
fn location_text(posting: &Value) -> String {
    let public = posting["location"]["name"].as_str().unwrap_or("");
    let offices = posting["offices"].as_array().into_iter().flatten();
    let remote_offices = offices
        .map(|office| str_field(office, "name"))
        .filter(|name| name.to_lowercase().contains("remote"));
    join_places(std::iter::once(public).chain(remote_offices))
}

/// Whether the location and title say the job is not tied to an office. Many boards never use the
/// word "remote": Canonical's read "Home based - Worldwide" or "Home Based - Americas". Those are
/// remote; whether Chile is included is the region filter's call, not this flag's.
fn says_remote(text: &str) -> bool {
    let text = text.to_lowercase();
    [
        "remote",
        "home based",
        "home-based",
        "work from home",
        "worldwide",
        "anywhere",
    ]
    .iter()
    .any(|word| text.contains(word))
}

/// Greenhouse sends `content` HTML-escaped (`&lt;p&gt;`): decode the entities first, then strip the tags.
///
/// `posted_at` is `first_published`, not `updated_at`: the latter moves with every edit, which
/// made every posting of a board look recent to the staleness rule.
fn parse_posting(board: &str, posting: &Value) -> Option<Job> {
    let external_id = id_string(&posting["id"])?;
    let title = str_field(posting, "title");
    let url = str_field(posting, "absolute_url");
    if title.is_empty() || url.is_empty() {
        return None;
    }
    let location = location_text(posting);
    let location = location.as_str();
    let company = match str_field(posting, "company_name") {
        "" => board,
        name => name,
    };
    let description = html_to_text(&html_escape::decode_html_entities(str_field(
        posting, "content",
    )));
    let (salary_min, salary_max) = parse_salary_monthly_usd(&description);
    let apply_url = apply_url_for(board, &external_id, url);
    Some(Job {
        source: "greenhouse".to_string(),
        external_id,
        company: company.to_string(),
        title: title.to_string(),
        url: url.to_string(),
        apply_url,
        location: location.to_string(),
        remote: says_remote(&format!("{location} {title}")),
        description,
        salary_min_monthly_usd: salary_min,
        salary_max_monthly_usd: salary_max,
        posted_at: parse_iso(str_field(posting, "first_published"))
            .or_else(|| parse_iso(str_field(posting, "updated_at"))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn posting_at(location: &str) -> Value {
        json!({
            "id": 1,
            "title": "Software Engineer",
            "absolute_url": "https://job-boards.greenhouse.io/canonical/jobs/1",
            "location": {"name": location},
            "content": "",
            "first_published": "2026-10-01T00:00:00-04:00"
        })
    }

    #[test]
    fn a_home_based_posting_is_remote_without_saying_the_word() {
        // Canonical's board: about 250 of its 310 postings read "Home based - Worldwide" or
        // "Home Based - Americas" and never say "remote". They are remote, and the region filter,
        // not a keyword, must decide whether Chile is included.
        for (location, open_to_chile) in [
            ("Home based - Worldwide", true),
            ("Home Based - Americas", true),
            ("Worldwide", true),
            ("Work from home - EMEA", false),
            ("Home based - US", false),
        ] {
            let job = parse_posting("canonical", &posting_at(location)).unwrap();
            assert!(job.remote, "{location}");
            assert_eq!(
                crate::discovery::region_ok(&job),
                open_to_chile,
                "{location}"
            );
        }
        // A physical office is still not remote.
        let office = parse_posting("canonical", &posting_at("London")).unwrap();
        assert!(!office.remote);
    }

    fn payload() -> Value {
        json!({
            "jobs": [{
                "id": 4461,
                "title": "AI Engineer",
                "absolute_url": "https://job-boards.greenhouse.io/acme/jobs/4461",
                "company_name": "Acme",
                "location": {"name": "Remote - LATAM"},
                "updated_at": "2026-09-30T12:00:00-04:00",
                "content": "&lt;p&gt;Pay: $120,000 - $160,000&lt;/p&gt;&lt;ul&gt;&lt;li&gt;Python&lt;/li&gt;&lt;/ul&gt;"
            }],
            "meta": {"total": 1}
        })
    }

    async fn mount_board(server: &MockServer, board: &str, response: ResponseTemplate) {
        Mock::given(method("GET"))
            .and(path(format!("/v1/boards/{board}/jobs")))
            .and(query_param("content", "true"))
            .respond_with(response)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn greenhouse_parses_escaped_html_and_salary() {
        let server = MockServer::start().await;
        mount_board(
            &server,
            "acme",
            ResponseTemplate::new(200).set_body_json(payload()),
        )
        .await;

        let source = GreenhouseSource::new(["acme"], Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let job = &result.jobs[0];
        assert_eq!(job.key(), "greenhouse:4461");
        assert_eq!(job.company, "Acme");
        assert!(job.remote);
        assert!(job.description.contains("Python"));
        assert!(
            !job.description.contains('<'),
            "tags must be stripped: {}",
            job.description
        );
        assert_eq!(job.salary_min_monthly_usd.map(f64::round), Some(10000.0));
        assert!(job.posted_at.is_some());
    }

    #[test]
    fn posted_at_is_the_first_publication_not_the_last_edit() {
        // Real Mozilla data (2026-10-04): `updated_at` moves whenever the posting is touched, so
        // every posting looked under 45 days old (0 of 90 stale on Mozilla, 0 of 887 on
        // Databricks) while `first_published` showed 15 and 634 of them were older.
        let posting = json!({
            "id": 1,
            "title": "Staff Data Scientist, Ads",
            "absolute_url": "https://job-boards.greenhouse.io/mozilla/jobs/1",
            "location": {"name": "Remote"},
            "first_published": "2025-08-04T09:00:00-04:00",
            "updated_at": "2026-09-29T09:00:00-04:00",
            "content": ""
        });
        let job = parse_posting("mozilla", &posting).unwrap();
        assert_eq!(job.posted_at, parse_iso("2025-08-04T09:00:00-04:00"));

        // Without `first_published` (null, empty or not a date) the last edit is the best date there is.
        for missing in [json!(null), json!(""), json!("not a date")] {
            let mut posting = posting.clone();
            posting["first_published"] = missing;
            let job = parse_posting("mozilla", &posting).unwrap();
            assert_eq!(job.posted_at, parse_iso("2026-09-29T09:00:00-04:00"));
        }
    }

    #[test]
    fn a_remote_office_says_where_a_plain_remote_posting_is_open() {
        // Real Mozilla data: the public location is just "Remote" while the office is "Remote US".
        let posting = |location: &str, offices: Value| {
            json!({
                "id": 2,
                "title": "Engineer",
                "absolute_url": "https://job-boards.greenhouse.io/mozilla/jobs/2",
                "location": {"name": location},
                "offices": offices,
                "content": ""
            })
        };
        let office = |name: &str| json!({"id": 1, "name": name, "location": null});

        let job =
            parse_posting("mozilla", &posting("Remote", json!([office("Remote US")]))).unwrap();
        assert_eq!(job.location, "Remote; Remote US");

        // An office that is already in the location, a blank remote office and a physical
        // headquarters office add nothing: only remote geography is evidence of who may apply.
        let job = parse_posting(
            "mozilla",
            &posting(
                "Remote Canada; Remote UK",
                json!([
                    office("Remote Canada"),
                    office("Remote "),
                    office("San Francisco")
                ]),
            ),
        )
        .unwrap();
        assert_eq!(job.location, "Remote Canada; Remote UK");
    }

    #[test]
    fn a_form_embedded_in_the_company_site_applies_through_the_hosted_board() {
        let embedded = json!({
            "id": 7882009002u64,
            "title": "Staff Engineer",
            "absolute_url": "https://databricks.com/company/careers/open-positions/job?gh_jid=7882009002",
            "location": {"name": "Remote"},
            "content": ""
        });

        let job = parse_posting("databricks", &embedded).unwrap();

        // The page people read stays the company's; the form the bot fills is Greenhouse's own.
        assert_eq!(
            job.url,
            "https://databricks.com/company/careers/open-positions/job?gh_jid=7882009002"
        );
        // Not `/databricks/jobs/…`: that page redirects back to the company site, while the page
        // the embed's iframe loads is the plain form.
        assert_eq!(
            job.apply_url,
            "https://job-boards.greenhouse.io/embed/job_app?for=databricks&token=7882009002"
        );
    }

    #[test]
    fn a_hosted_greenhouse_page_is_used_as_it_is() {
        for url in [
            "https://job-boards.greenhouse.io/acme/jobs/4461",
            "https://boards.greenhouse.io/acme/jobs/4461",
        ] {
            let posting = json!({
                "id": 4461,
                "title": "AI Engineer",
                "absolute_url": url,
                "location": {"name": "Remote"},
                "content": ""
            });
            let job = parse_posting("acme", &posting).unwrap();
            assert_eq!(job.apply_url, url);
        }
    }

    #[tokio::test]
    async fn greenhouse_error_isolated() {
        let server = MockServer::start().await;
        mount_board(
            &server,
            "bad",
            ResponseTemplate::new(404).set_body_json(json!({"error": "not found"})),
        )
        .await;
        mount_board(
            &server,
            "acme",
            ResponseTemplate::new(200).set_body_json(payload()),
        )
        .await;

        let source =
            GreenhouseSource::new(["bad", "acme"], Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert_eq!(result.jobs.len(), 1);
        assert_eq!(result.errors.len(), 1);
        assert!(
            result.errors[0].starts_with("greenhouse:bad"),
            "{}",
            result.errors[0]
        );
        assert!(result.errors[0].contains("404"), "{}", result.errors[0]);
    }

    #[tokio::test]
    async fn greenhouse_posting_with_missing_fields_is_skipped_not_fatal() {
        let server = MockServer::start().await;
        let body = json!({"jobs": [
            {"id": 1, "absolute_url": "https://example.com/1"},
            {"id": 2, "title": "No url"},
            {"title": "No id", "absolute_url": "https://example.com/3"},
            {"id": 4, "title": "Bare minimum", "absolute_url": "https://example.com/4",
             "location": null, "content": null, "updated_at": null}
        ]});
        mount_board(
            &server,
            "acme",
            ResponseTemplate::new(200).set_body_json(body),
        )
        .await;

        let source = GreenhouseSource::new(["acme"], Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert_eq!(result.jobs.len(), 1);
        let job = &result.jobs[0];
        assert_eq!(
            (job.title.as_str(), job.company.as_str()),
            ("Bare minimum", "acme")
        );
        assert_eq!((job.location.as_str(), job.description.as_str()), ("", ""));
        assert!(!job.remote && job.posted_at.is_none());
        assert_eq!(
            result.errors,
            vec!["greenhouse:acme: skipped 3 postings missing id, title or url".to_string()]
        );
    }

    #[tokio::test]
    async fn greenhouse_body_without_jobs_array_is_recorded() {
        let server = MockServer::start().await;
        mount_board(
            &server,
            "acme",
            ResponseTemplate::new(200).set_body_json(json!({"meta": {}})),
        )
        .await;

        let source = GreenhouseSource::new(["acme"], Client::new()).with_base_url(server.uri());
        let result = source.fetch().await;

        assert!(result.jobs.is_empty());
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].starts_with("greenhouse:acme: "));
        assert!(result.errors[0].contains("jobs"));
    }
}
