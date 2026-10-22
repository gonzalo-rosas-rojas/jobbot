//! 4 Day Week jobs: `GET {base}/api/v2/jobs?q=..&work_arrangement=remote&category=..&page=N`.
//!
//! The site documents it at 4dayweek.io/developers as a free, public, no-auth JSON API (60
//! requests a minute per IP) and asks for a link back, so every job keeps its 4 Day Week page as
//! `url`. The apply button on that page leads to the company, which is why the gate must treat
//! `4dayweek.io` as `listing_only`, like `remotive.com`. A page holds at most 100 jobs and
//! `has_more` says whether another follows.

use std::collections::HashSet;
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use super::Source;
use super::base::{
    REQUEST_PAUSE, failure, get_json, id_string, join_places, parse_iso, skipped_error, str_field,
};
use super::pay::{monthly_usd_range, number};
use crate::models::{FetchResult, Job};

const DEFAULT_BASE_URL: &str = "https://4dayweek.io";

/// The `[fourdayweek]` table of `profile/sources.toml`; present (even empty) turns the source on.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FourDayWeekConfig {
    /// One full-text search per query.
    pub queries: Vec<String>,
    /// 4 Day Week category (`engineering`); empty means every category.
    pub category: String,
    /// Only postings newer than this many days (the API accepts up to 365).
    pub max_age_days: u32,
    /// Pages read per query.
    pub max_pages: u32,
    /// Jobs per page (the API serves at most 100).
    pub per_page: u32,
}

impl Default for FourDayWeekConfig {
    fn default() -> Self {
        Self {
            queries: ["software engineer", "full stack", "ai engineer"]
                .map(String::from)
                .to_vec(),
            category: "engineering".to_string(),
            max_age_days: 30,
            max_pages: 2,
            per_page: 50,
        }
    }
}

pub struct FourDayWeekSource {
    client: Client,
    base_url: String,
    config: FourDayWeekConfig,
    pause: Duration,
}

impl FourDayWeekSource {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            base_url: DEFAULT_BASE_URL.to_string(),
            config: FourDayWeekConfig::default(),
            pause: REQUEST_PAUSE,
        }
    }

    pub fn with_config(mut self, config: FourDayWeekConfig) -> Self {
        self.config = config;
        self
    }

    /// Points the source at another server (tests use a wiremock server).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    /// Waits this long between two requests (tests use none).
    pub fn with_pause(mut self, pause: Duration) -> Self {
        self.pause = pause;
        self
    }

    /// Follows the pages of one query. Jobs go straight into `result`, so when a later page fails
    /// the earlier pages are kept and only the error is returned. `seen` drops a job that two
    /// queries both return; `requests` counts every request of the run, to pause between them.
    async fn fetch_query(
        &self,
        query: &str,
        seen: &mut HashSet<String>,
        requests: &mut u32,
        result: &mut FetchResult,
    ) -> anyhow::Result<()> {
        let url = format!("{}/api/v2/jobs", self.base_url);
        let limit = self.config.per_page.to_string();
        let posted_after = self.config.max_age_days.to_string();
        for page in 1..=self.config.max_pages {
            if *requests > 0 {
                tokio::time::sleep(self.pause).await;
            }
            *requests += 1;
            let page_text = page.to_string();
            let mut params = vec![
                ("q", query),
                ("work_arrangement", "remote"),
                ("posted_after", posted_after.as_str()),
                ("limit", limit.as_str()),
                ("page", page_text.as_str()),
            ];
            if !self.config.category.is_empty() {
                params.push(("category", self.config.category.as_str()));
            }
            let body = get_json(&self.client, &url, &params).await?;
            let items = body["data"]
                .as_array()
                .context("response has no `data` array")?;
            let mut skipped = 0;
            for item in items {
                match parse_job(item) {
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
                    .push(skipped_error("fourdayweek", query, skipped));
            }
            if !body["has_more"].as_bool().unwrap_or(false) {
                break;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Source for FourDayWeekSource {
    fn name(&self) -> &str {
        "fourdayweek"
    }

    async fn fetch(&self) -> FetchResult {
        let mut result = FetchResult::default();
        let mut seen = HashSet::new();
        let mut requests = 0;
        for query in &self.config.queries {
            if let Err(error) = self
                .fetch_query(query, &mut seen, &mut requests, &mut result)
                .await
            {
                result.errors.push(failure("fourdayweek", query, &error));
            }
        }
        result
    }
}

/// The names in a JSON array of `{name, slug}` objects.
fn names(item: &Value, key: &str) -> Vec<String> {
    item[key]
        .as_array()
        .map(|values| {
            values
                .iter()
                .map(|value| str_field(value, "name"))
                .filter(|name| !name.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// A job is remote when its `work_arrangement` says so. Its place is the countries it lists, with
/// `Worldwide` in front when the company hires worldwide. Pay comes in cents (a US$208,000 Coinbase
/// salary arrives as `20800000`), as a currency and a period; only US dollars are converted.
fn parse_job(item: &Value) -> Option<Job> {
    let external_id = id_string(&item["id"])?;
    let title = str_field(item, "title");
    let url = str_field(item, "url");
    if title.is_empty() || url.is_empty() {
        return None;
    }
    let company = &item["company"];
    let worldwide = if company["hires_worldwide"].as_bool().unwrap_or(false) {
        "Worldwide"
    } else {
        ""
    };
    let countries = item["locations"]
        .as_array()
        .map(|places| places.iter().map(|place| str_field(place, "country")))
        .into_iter()
        .flatten();
    let location = join_places(std::iter::once(worldwide).chain(countries));
    let mut skills: Vec<String> = Vec::new();
    for skill in ["skills", "stack", "tools"]
        .iter()
        .flat_map(|key| names(item, key))
    {
        if !skills.contains(&skill) {
            skills.push(skill);
        }
    }
    let mut description = str_field(item, "description").trim().to_string();
    for (label, value) in [
        ("Level", str_field(item, "level").to_string()),
        ("Schedule", str_field(item, "schedule_type").to_string()),
        ("Contract", str_field(item, "contract_type").to_string()),
        ("Skills", skills.join(", ")),
    ] {
        if !value.is_empty() {
            description = format!("{description}\n{label}: {value}");
        }
    }
    let cents = |key: &str| number(&item[key]).map(|amount| amount / 100.0);
    let (salary_min, salary_max) = monthly_usd_range(
        cents("salary_min"),
        cents("salary_max"),
        str_field(item, "salary_currency"),
        str_field(item, "salary_period"),
    );
    Some(Job {
        source: "fourdayweek".to_string(),
        external_id,
        company: str_field(company, "name").to_string(),
        title: title.to_string(),
        url: url.to_string(),
        apply_url: url.to_string(),
        location,
        remote: str_field(item, "work_arrangement") == "remote",
        description,
        salary_min_monthly_usd: salary_min,
        salary_max_monthly_usd: salary_max,
        posted_at: parse_iso(str_field(item, "posted_at")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Client;
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A trimmed real response from `4dayweek.io/api/v2/jobs?work_arrangement=remote&category=engineering&limit=25`:
    /// a US/Canada/Brazil job paid per year, a US one paid per hour, a Canadian one paid in CAD
    /// and a Portuguese one with no pay.
    fn real_payload() -> Value {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/sources/fourdayweek.json"
        ))
        .unwrap()
    }

    fn job_at(index: usize) -> Job {
        parse_job(&real_payload()["data"][index]).unwrap()
    }

    fn config(queries: &[&str], max_pages: u32) -> FourDayWeekConfig {
        FourDayWeekConfig {
            queries: queries.iter().map(|query| query.to_string()).collect(),
            max_pages,
            ..FourDayWeekConfig::default()
        }
    }

    fn source(server: &MockServer, config: FourDayWeekConfig) -> FourDayWeekSource {
        FourDayWeekSource::new(Client::new())
            .with_base_url(server.uri())
            .with_config(config)
            .with_pause(Duration::ZERO)
    }

    /// `count` distinct minimal jobs, `has_more` as given.
    fn page(count: usize, offset: usize, has_more: bool) -> Value {
        let data: Vec<Value> = (offset..offset + count)
            .map(|n| {
                json!({
                    "id": format!("id-{n}"), "title": format!("Engineer {n}"),
                    "url": format!("https://4dayweek.io/job/engineer-{n}"),
                    "work_arrangement": "remote",
                    "locations": [{"country": "Brazil"}],
                    "company": {"name": "Acme"}
                })
            })
            .collect();
        json!({"data": data, "has_more": has_more, "limit": 50, "page": 1, "total": 99})
    }

    #[test]
    fn parses_a_real_job() {
        let job = job_at(0);

        assert_eq!(
            job.key(),
            "fourdayweek:01a10457-36fb-77cf-9e93-7fa2924a6560"
        );
        assert_eq!(job.company, "Ledgebrook");
        assert_eq!(job.title, "Full Stack Engineer - Finance");
        assert_eq!(job.location, "United States; Canada; Brazil");
        assert!(job.remote);
        assert_eq!(
            job.url,
            "https://4dayweek.io/job/full-stack-engineer-finance-at-ledgebrook-f2cc121f"
        );
        assert_eq!(job.apply_url, job.url);
        assert_eq!(
            job.posted_at,
            crate::sources::base::parse_iso("2026-10-04T00:36:27Z")
        );
        for expected in [
            "Ledgebrook is an InsurTech MGA",
            "Level: mid",
            "Schedule: unlimited_pto",
            "Contract: permanent",
            "Skills: Agile, Communication, Data Modeling, Testability, Angular, Bootstrap, CSS",
        ] {
            assert!(
                job.description.contains(expected),
                "{expected:?} missing from {}",
                job.description
            );
        }
    }

    #[test]
    fn pay_in_cents_becomes_monthly_us_dollars_and_other_currencies_are_not_guessed() {
        // US$100,000 to 150,000 a year.
        let yearly = job_at(0);
        assert_eq!(yearly.salary_min_monthly_usd.map(f64::round), Some(8_333.0));
        assert_eq!(
            yearly.salary_max_monthly_usd.map(f64::round),
            Some(12_500.0)
        );
        // US$30 to 100 an hour.
        let hourly = job_at(1);
        assert_eq!(hourly.salary_min_monthly_usd, Some(4_800.0));
        assert_eq!(hourly.salary_max_monthly_usd, Some(16_000.0));
        // CAD is left alone, and no pay is no pay.
        assert_eq!(
            (
                job_at(2).salary_min_monthly_usd,
                job_at(2).salary_max_monthly_usd
            ),
            (None, None)
        );
        assert_eq!(
            (
                job_at(3).salary_min_monthly_usd,
                job_at(3).salary_max_monthly_usd
            ),
            (None, None)
        );
    }

    #[test]
    fn the_region_filter_reads_the_countries_the_job_lists() {
        use crate::discovery::region_ok;

        assert!(region_ok(&job_at(0)), "United States, Canada and Brazil");
        assert!(!region_ok(&job_at(1)), "United States");
        assert!(!region_ok(&job_at(3)), "Portugal");
    }

    #[test]
    fn a_company_that_hires_worldwide_is_open_to_the_world() {
        let item = json!({
            "id": "x", "title": "Dev", "url": "https://4dayweek.io/job/x",
            "work_arrangement": "remote",
            "locations": [{"country": "United States"}],
            "company": {"name": "Acme", "hires_worldwide": true}
        });

        assert_eq!(
            parse_job(&item).unwrap().location,
            "Worldwide; United States"
        );
    }

    #[test]
    fn only_the_remote_arrangement_is_remote() {
        let mut item = json!({
            "id": "x", "title": "Dev", "url": "https://4dayweek.io/job/x",
            "work_arrangement": "hybrid", "locations": [{"country": "Chile"}]
        });
        assert!(!parse_job(&item).unwrap().remote);
        item["work_arrangement"] = json!("remote");
        assert!(parse_job(&item).unwrap().remote);
    }

    #[test]
    fn a_job_without_id_title_or_url_is_not_a_job() {
        assert!(parse_job(&json!({"title": "Dev", "url": "https://4dayweek.io/job/x"})).is_none());
        assert!(parse_job(&json!({"id": "x", "url": "https://4dayweek.io/job/x"})).is_none());
        assert!(parse_job(&json!({"id": "x", "title": "Dev"})).is_none());
    }

    #[tokio::test]
    async fn fetch_sends_the_filters_and_follows_has_more() {
        let server = MockServer::start().await;
        for (page_number, body) in [("1", page(2, 0, true)), ("2", page(1, 2, false))] {
            Mock::given(method("GET"))
                .and(path("/api/v2/jobs"))
                .and(query_param("q", "software engineer"))
                .and(query_param("work_arrangement", "remote"))
                .and(query_param("category", "engineering"))
                .and(query_param("posted_after", "30"))
                .and(query_param("limit", "50"))
                .and(query_param("page", page_number))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .expect(1)
                .mount(&server)
                .await;
        }

        let result = source(&server, config(&["software engineer"], 5))
            .fetch()
            .await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(
            result.jobs.len(),
            3,
            "two jobs, then one, and no third page"
        );
        assert!(result.jobs.iter().all(|job| job.source == "fourdayweek"));
    }

    #[tokio::test]
    async fn fetch_stops_at_max_pages_and_drops_repeats_across_queries() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v2/jobs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page(2, 0, true)))
            .expect(2)
            .mount(&server)
            .await;

        let result = source(&server, config(&["a", "b"], 1)).fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(
            result.jobs.len(),
            2,
            "one page per query, the same two jobs once"
        );
    }

    #[tokio::test]
    async fn an_empty_category_sends_no_category_filter() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v2/jobs"))
            .and(wiremock::matchers::query_param_is_missing("category"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page(1, 0, false)))
            .expect(1)
            .mount(&server)
            .await;
        let mut cfg = config(&["x"], 1);
        cfg.category = String::new();

        let result = source(&server, cfg).fetch().await;

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.jobs.len(), 1);
    }

    #[tokio::test]
    async fn fetch_records_a_rate_limit_and_keeps_the_other_queries() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v2/jobs"))
            .and(query_param("q", "limited"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v2/jobs"))
            .and(query_param("q", "fine"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page(2, 0, false)))
            .mount(&server)
            .await;

        let result = source(&server, config(&["limited", "fine"], 1))
            .fetch()
            .await;

        assert_eq!(result.jobs.len(), 2);
        assert_eq!(result.errors.len(), 1);
        assert!(
            result.errors[0].starts_with("fourdayweek:limited: "),
            "{}",
            result.errors[0]
        );
    }

    #[tokio::test]
    async fn fetch_records_skipped_postings_and_a_body_without_data() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v2/jobs"))
            .and(query_param("q", "bare"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"id": "1", "title": "No url"}], "has_more": false
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v2/jobs"))
            .and(query_param("q", "html"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>blocked</html>"))
            .mount(&server)
            .await;

        let result = source(&server, config(&["bare", "html"], 1)).fetch().await;

        assert!(result.jobs.is_empty());
        assert_eq!(result.errors.len(), 2, "{:?}", result.errors);
        assert_eq!(
            result.errors[0],
            "fourdayweek:bare: skipped 1 postings missing id, title or url"
        );
        assert!(
            result.errors[1].starts_with("fourdayweek:html: "),
            "{}",
            result.errors[1]
        );
    }

    #[test]
    fn config_defaults_and_unknown_keys() {
        let cfg: FourDayWeekConfig = toml::from_str("").unwrap();
        assert_eq!(cfg, FourDayWeekConfig::default());
        assert_eq!(
            (
                cfg.category.as_str(),
                cfg.max_age_days,
                cfg.max_pages,
                cfg.per_page
            ),
            ("engineering", 30, 2, 50)
        );
        let cfg: FourDayWeekConfig =
            toml::from_str("queries = [\"rust\"]\nmax_age_days = 7").unwrap();
        assert_eq!(
            (cfg.queries, cfg.max_age_days),
            (vec!["rust".to_string()], 7)
        );
        assert!(toml::from_str::<FourDayWeekConfig>("categry = \"x\"").is_err());
    }
}
