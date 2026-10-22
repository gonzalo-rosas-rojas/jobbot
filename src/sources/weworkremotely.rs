//! We Work Remotely: `GET {base}/categories/{category}.rss`.
//!
//! One public RSS feed per category, no key. Every posting is remote; `region` ("Anywhere in the
//! World", "North America Only") says where the company hires and is what the region filter
//! reads. The apply button lives on the WWR page and leads to the company, so every job keeps its
//! WWR page as `url`, and the gate must treat `weworkremotely.com` as `listing_only`, like
//! `remotive.com`.

use std::collections::HashSet;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use super::Source;
use super::base::{REQUEST_PAUSE, failure, get_text, join_places, parse_rfc2822, skipped_error};
use super::rss::{self, Item};
use crate::models::{FetchResult, Job};
use crate::text::html_to_text;

const DEFAULT_BASE_URL: &str = "https://weworkremotely.com";

/// The `[weworkremotely]` table of `profile/sources.toml`; present (even empty) turns the source on.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WeWorkRemotelyConfig {
    /// Category feeds read, one request each: the name in `categories/{name}.rss`.
    pub categories: Vec<String>,
}

impl Default for WeWorkRemotelyConfig {
    fn default() -> Self {
        Self {
            categories: [
                "remote-full-stack-programming-jobs",
                "remote-back-end-programming-jobs",
            ]
            .map(String::from)
            .to_vec(),
        }
    }
}

pub struct WeWorkRemotelySource {
    client: Client,
    base_url: String,
    config: WeWorkRemotelyConfig,
    pause: Duration,
}

impl WeWorkRemotelySource {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            base_url: DEFAULT_BASE_URL.to_string(),
            config: WeWorkRemotelyConfig::default(),
            pause: REQUEST_PAUSE,
        }
    }

    pub fn with_config(mut self, config: WeWorkRemotelyConfig) -> Self {
        self.config = config;
        self
    }

    /// Points the source at another server (tests use a wiremock server).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    /// Waits this long between two category requests (tests use none).
    pub fn with_pause(mut self, pause: Duration) -> Self {
        self.pause = pause;
        self
    }

    /// Reads one category feed. `seen` drops a job that two categories both list.
    async fn fetch_category(
        &self,
        category: &str,
        seen: &mut HashSet<String>,
        result: &mut FetchResult,
    ) -> anyhow::Result<()> {
        let url = format!("{}/categories/{category}.rss", self.base_url);
        let body = get_text(&self.client, &url, &[]).await?;
        let mut skipped = 0;
        for item in rss::items(&body)? {
            match parse_item(&item) {
                Some(job) => {
                    if seen.insert(job.external_id.clone()) {
                        result.jobs.push(job);
                    }
                }
                None => skipped += 1,
            }
        }
        if skipped > 0 {
            result
                .errors
                .push(skipped_error("weworkremotely", category, skipped));
        }
        Ok(())
    }
}

#[async_trait]
impl Source for WeWorkRemotelySource {
    fn name(&self) -> &str {
        "weworkremotely"
    }

    async fn fetch(&self) -> FetchResult {
        let mut result = FetchResult::default();
        let mut seen = HashSet::new();
        for (n, category) in self.config.categories.iter().enumerate() {
            if n > 0 {
                tokio::time::sleep(self.pause).await;
            }
            if let Err(error) = self.fetch_category(category, &mut seen, &mut result).await {
                result
                    .errors
                    .push(failure("weworkremotely", category, &error));
            }
        }
        result
    }
}

/// Every WWR job is remote. The title reads `Company: Role`; the location joins the region the
/// company hires from with the country and state it names, minus the flag emoji before the country.
fn parse_item(item: &Item) -> Option<Job> {
    let url = match item.text("link") {
        link if link.is_empty() => item.text("guid"),
        link => link,
    };
    let external_id = url.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let (company, title) = split_title(&item.text("title"));
    if external_id.is_empty() || title.is_empty() {
        return None;
    }
    let country = item.text("country");
    let state = item.text("state");
    let location = join_places([
        item.text("region").as_str(),
        country.trim_start_matches(|c: char| !c.is_alphanumeric()),
        state.as_str(),
    ]);
    let mut description = html_to_text(&item.text("description"));
    for (label, value) in [
        ("Type", item.text("type")),
        ("Category", item.text("category")),
        ("Skills", item.text("skills")),
    ] {
        if !value.is_empty() {
            description = format!("{description}\n{label}: {value}");
        }
    }
    Some(Job {
        source: "weworkremotely".to_string(),
        external_id: external_id.to_string(),
        company,
        title,
        apply_url: url.clone(),
        url,
        location,
        remote: true,
        description,
        salary_min_monthly_usd: None,
        salary_max_monthly_usd: None,
        posted_at: parse_rfc2822(&item.text("pubDate")),
    })
}

/// `"Stripe: Abuse Research Engineer"` is company `Stripe` and role `Abuse Research Engineer`; a
/// title without the `Company: ` prefix is all role.
fn split_title(full: &str) -> (String, String) {
    match full.split_once(": ") {
        Some((company, title)) if !company.trim().is_empty() && !title.trim().is_empty() => {
            (company.trim().to_string(), title.trim().to_string())
        }
        _ => (String::new(), full.trim().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A trimmed real response from `weworkremotely.com/categories/remote-full-stack-programming-jobs.rss`.
    fn real_feed() -> &'static str {
        include_str!("../../tests/fixtures/sources/weworkremotely.rss")
    }

    fn job_at(index: usize) -> Job {
        let feed = real_feed();
        let items = rss::items(feed).unwrap();
        parse_item(&items[index]).unwrap()
    }

    fn source(server: &MockServer, categories: &[&str]) -> WeWorkRemotelySource {
        WeWorkRemotelySource::new(Client::new())
            .with_base_url(server.uri())
            .with_config(WeWorkRemotelyConfig {
                categories: categories.iter().map(|name| name.to_string()).collect(),
            })
            .with_pause(Duration::ZERO)
    }

    async fn serve(server: &MockServer, category: &str, response: ResponseTemplate) {
        Mock::given(method("GET"))
            .and(path(format!("/categories/{category}.rss")))
            .respond_with(response)
            .mount(server)
            .await;
    }

    #[test]
    fn parses_a_real_job() {
        let job = job_at(0);

        assert_eq!(job.key(), "weworkremotely:stripe-abuse-research-engineer");
        assert_eq!(job.company, "Stripe");
        assert_eq!(job.title, "Abuse Research Engineer");
        assert_eq!(job.location, "Anywhere in the World");
        assert!(job.remote);
        assert_eq!(
            job.url,
            "https://weworkremotely.com/remote-jobs/stripe-abuse-research-engineer"
        );
        assert_eq!(job.apply_url, job.url);
        assert_eq!(
            job.posted_at,
            crate::sources::base::parse_iso("2026-10-04T07:31:08+00:00")
        );
        assert!(
            job.description.contains("Headquarters: Remote from the US")
                && job.description.contains("About Stripe"),
            "{}",
            job.description
        );
        assert!(job.description.contains("Type: Full-Time"));
        assert!(job.description.contains("Category: Full-Stack Programming"));
        assert!(!job.description.contains('<'), "{}", job.description);
        assert_eq!(
            (job.salary_min_monthly_usd, job.salary_max_monthly_usd),
            (None, None)
        );
    }

    #[test]
    fn the_location_joins_region_country_and_state_without_the_flag() {
        assert_eq!(
            job_at(1).location,
            "Anywhere in the World; United States of America"
        );
        assert_eq!(job_at(2).location, "North America Only");
        assert_eq!(job_at(3).location, "Anywhere in the World; Delaware");
    }

    #[test]
    fn the_region_filter_reads_the_region_the_company_hires_from() {
        use crate::discovery::region_ok;

        assert!(region_ok(&job_at(0)), "anywhere in the world");
        assert!(region_ok(&job_at(1)), "anywhere, with a US country tag");
        assert!(!region_ok(&job_at(2)), "North America only");
    }

    #[test]
    fn entities_in_the_title_are_decoded() {
        assert_eq!(
            job_at(2).title,
            "Senior Integration Engineer for Enterprise API & Integration Products"
        );
        assert_eq!(job_at(2).company, "Toptal");
    }

    #[test]
    fn a_title_without_a_company_prefix_stays_whole() {
        let feed = "<rss><channel><item><title>Rust Engineer</title>\
            <guid>https://weworkremotely.com/remote-jobs/rust-engineer</guid>\
            <link>https://weworkremotely.com/remote-jobs/rust-engineer</link></item></channel></rss>";
        let items = rss::items(feed).unwrap();

        let job = parse_item(&items[0]).unwrap();

        assert_eq!(
            (job.company.as_str(), job.title.as_str()),
            ("", "Rust Engineer")
        );
        assert_eq!(job.external_id, "rust-engineer");
        assert_eq!(job.location, "");
        assert!(job.posted_at.is_none());
    }

    #[test]
    fn an_item_without_a_title_or_a_link_is_not_a_job() {
        let feed = "<rss><channel>\
            <item><link>https://weworkremotely.com/remote-jobs/a</link></item>\
            <item><title>Acme: Dev</title></item>\
            </channel></rss>";
        let items = rss::items(feed).unwrap();

        assert!(items.iter().all(|item| parse_item(item).is_none()));
    }

    #[tokio::test]
    async fn fetch_reads_each_category_and_drops_repeats() {
        let server = MockServer::start().await;
        for category in ["full-stack", "programming"] {
            serve(
                &server,
                category,
                ResponseTemplate::new(200).set_body_string(real_feed()),
            )
            .await;
        }

        let result = source(&server, &["full-stack", "programming"])
            .fetch()
            .await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.jobs.len(), 4, "the same four jobs once, not twice");
        assert!(result.jobs.iter().all(|job| job.source == "weworkremotely"));
    }

    #[tokio::test]
    async fn fetch_records_a_failed_category_and_keeps_the_others() {
        let server = MockServer::start().await;
        serve(&server, "down", ResponseTemplate::new(503)).await;
        serve(
            &server,
            "blocked",
            ResponseTemplate::new(200).set_body_string("<html>blocked</html>"),
        )
        .await;
        serve(
            &server,
            "fine",
            ResponseTemplate::new(200).set_body_string(real_feed()),
        )
        .await;

        let result = source(&server, &["down", "blocked", "fine"]).fetch().await;

        assert_eq!(result.jobs.len(), 4);
        assert_eq!(result.errors.len(), 2, "{:?}", result.errors);
        assert!(
            result.errors[0].starts_with("weworkremotely:down: "),
            "{}",
            result.errors[0]
        );
        assert!(
            result.errors[1].starts_with("weworkremotely:blocked: ")
                && result.errors[1].contains("not an RSS feed"),
            "{}",
            result.errors[1]
        );
    }

    #[tokio::test]
    async fn fetch_records_skipped_items() {
        let server = MockServer::start().await;
        serve(
            &server,
            "x",
            ResponseTemplate::new(200).set_body_string(
                "<rss><channel><item><title>No link</title></item></channel></rss>",
            ),
        )
        .await;

        let result = source(&server, &["x"]).fetch().await;

        assert!(result.jobs.is_empty());
        assert_eq!(
            result.errors,
            vec!["weworkremotely:x: skipped 1 postings missing id, title or url".to_string()]
        );
    }

    #[test]
    fn config_defaults_and_unknown_keys() {
        let cfg: WeWorkRemotelyConfig = toml::from_str("").unwrap();
        assert_eq!(cfg, WeWorkRemotelyConfig::default());
        assert_eq!(
            cfg.categories,
            [
                "remote-full-stack-programming-jobs",
                "remote-back-end-programming-jobs"
            ]
        );
        let cfg: WeWorkRemotelyConfig =
            toml::from_str("categories = [\"remote-devops-sysadmin-jobs\"]").unwrap();
        assert_eq!(cfg.categories, ["remote-devops-sysadmin-jobs"]);
        assert!(toml::from_str::<WeWorkRemotelyConfig>("category = \"x\"").is_err());
    }
}
