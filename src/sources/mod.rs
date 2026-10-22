//! Job sources. Each source turns one public job API into normalized `Job` values.

pub mod ashby;
pub mod atlassian;
pub mod base;
pub mod fourdayweek;
pub mod getonboard;
pub mod greenhouse;
pub mod himalayas;
pub mod hn;
pub mod jobicy;
pub mod lever;
pub mod pay;
pub mod pythonjobs;
pub mod recruitee;
pub mod remoteok;
pub mod remoteyeah;
pub mod remotive;
pub mod rss;
pub mod smartrecruiters;
pub mod weworkremotely;
pub mod workable;
pub mod workable_search;
pub mod workingnomads;

use std::path::Path;

use anyhow::Context;
use async_trait::async_trait;
use serde::Deserialize;

use crate::models::FetchResult;

pub use ashby::AshbySource;
pub use atlassian::AtlassianSource;
pub use fourdayweek::{FourDayWeekConfig, FourDayWeekSource};
pub use getonboard::GetOnBoardSource;
pub use greenhouse::GreenhouseSource;
pub use himalayas::{HimalayasConfig, HimalayasSource};
pub use hn::{HackerNewsConfig, HackerNewsSource};
pub use jobicy::{JobicyConfig, JobicySource};
pub use lever::LeverSource;
pub use pythonjobs::PythonJobsSource;
pub use recruitee::RecruiteeSource;
pub use remoteok::RemoteOkSource;
pub use remoteyeah::RemoteYeahSource;
pub use remotive::RemotiveSource;
pub use smartrecruiters::SmartRecruitersSource;
pub use weworkremotely::{WeWorkRemotelyConfig, WeWorkRemotelySource};
pub use workable::WorkableSource;
pub use workable_search::{WorkableSearchConfig, WorkableSearchSource};
pub use workingnomads::{WorkingNomadsConfig, WorkingNomadsSource};

/// A place that publishes job postings (a Greenhouse board, the RemoteOK feed, ...).
///
/// `fetch` never panics and never returns an error. Every failure (HTTP status, HTML instead of
/// JSON, network down) becomes one line `"<source>:<slug>: <reason>"` in `FetchResult::errors`,
/// so a broken board cannot stop the other boards or the other sources.
#[async_trait]
pub trait Source: Send + Sync {
    /// Short stable name used in job keys and error lines, for example `"greenhouse"`.
    fn name(&self) -> &str;
    async fn fetch(&self) -> FetchResult;
}

/// The parsed `profile/sources.toml`: which boards and feeds to read. A missing key means
/// "none"; a misspelled key is an error, so a typo cannot silently drop a source.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SourcesConfig {
    pub greenhouse: Vec<String>,
    pub lever: Vec<String>,
    pub ashby: Vec<String>,
    pub remoteok: bool,
    /// Remote listings across many companies; applications happen from the link (`listing_only`).
    pub remotive: bool,
    /// Present (even as an empty `[getonboard]` table) turns the source on.
    pub getonboard: Option<GetOnBoardConfig>,
    /// Remote jobs open to Chile or to the world; applications happen on the Himalayas page.
    pub himalayas: Option<HimalayasConfig>,
    /// Remote jobs by region; applications happen on the Jobicy page.
    pub jobicy: Option<JobicyConfig>,
    /// The monthly "Ask HN: Who is hiring?" threads; applications happen as each comment says.
    pub hackernews: Option<HackerNewsConfig>,
    /// Workable account slugs (`apply.workable.com/{slug}`).
    pub workable: Vec<String>,
    /// Recruitee company slugs (`{slug}.recruitee.com`).
    pub recruitee: Vec<String>,
    /// SmartRecruiters company identifiers (`jobs.smartrecruiters.com/{identifier}`); only their
    /// remote postings are read.
    pub smartrecruiters: Vec<String>,
    /// Remote jobs from the We Work Remotely category feeds; applications happen on the WWR page.
    pub weworkremotely: Option<WeWorkRemotelyConfig>,
    /// The RemoteYeah all-jobs feed; applications happen on the RemoteYeah page.
    pub remoteyeah: bool,
    /// Remote jobs from the Working Nomads feed; applications happen on the company's page.
    pub workingnomads: Option<WorkingNomadsConfig>,
    /// Workable's cross-company search for remote jobs that list a country (Chile); applications
    /// happen on the Workable form behind the jobs.workable.com page.
    pub workable_search: Option<WorkableSearchConfig>,
    /// Remote engineering jobs of 4-day-week companies; applications happen on the 4 Day Week page.
    pub fourdayweek: Option<FourDayWeekConfig>,
    /// The python.org job board feed; applications happen as each posting says.
    pub pythonjobs: bool,
    /// Atlassian's own careers list (a fifth of it in Australia); applications happen on its iCIMS
    /// portal, from the link.
    pub atlassian: bool,
}

/// The `[getonboard]` table of `profile/sources.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GetOnBoardConfig {
    pub categories: Vec<String>,
    pub max_pages: u32,
    pub per_page: u32,
}

impl Default for GetOnBoardConfig {
    fn default() -> Self {
        Self {
            categories: vec!["programming".to_string()],
            max_pages: 3,
            per_page: 50,
        }
    }
}

pub fn load_sources_config(path: &Path) -> anyhow::Result<SourcesConfig> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("invalid {}", path.display()))
}

/// One source per configured kind, in a fixed order: greenhouse, lever, ashby, remoteok, remotive,
/// getonboard, himalayas, jobicy, hackernews, workable, recruitee, smartrecruiters, weworkremotely,
/// remoteyeah, workingnomads, workable_search, fourdayweek, pythonjobs, atlassian. `reqwest::Client` is a cheap
/// handle to a shared connection pool, so every source gets a clone.
pub fn build_sources(cfg: &SourcesConfig, client: &reqwest::Client) -> Vec<Box<dyn Source>> {
    let mut sources: Vec<Box<dyn Source>> = Vec::new();
    if !cfg.greenhouse.is_empty() {
        sources.push(Box::new(GreenhouseSource::new(
            cfg.greenhouse.clone(),
            client.clone(),
        )));
    }
    if !cfg.lever.is_empty() {
        sources.push(Box::new(LeverSource::new(
            cfg.lever.clone(),
            client.clone(),
        )));
    }
    if !cfg.ashby.is_empty() {
        sources.push(Box::new(AshbySource::new(
            cfg.ashby.clone(),
            client.clone(),
        )));
    }
    if cfg.remoteok {
        sources.push(Box::new(RemoteOkSource::new(client.clone())));
    }
    if cfg.remotive {
        sources.push(Box::new(RemotiveSource::new(client.clone())));
    }
    if let Some(getonboard) = &cfg.getonboard {
        sources.push(Box::new(
            GetOnBoardSource::new(client.clone())
                .with_categories(getonboard.categories.clone())
                .with_max_pages(getonboard.max_pages)
                .with_per_page(getonboard.per_page),
        ));
    }
    if let Some(himalayas) = &cfg.himalayas {
        sources.push(Box::new(
            HimalayasSource::new(client.clone()).with_config(himalayas.clone()),
        ));
    }
    if let Some(jobicy) = &cfg.jobicy {
        sources.push(Box::new(
            JobicySource::new(client.clone()).with_config(jobicy.clone()),
        ));
    }
    if let Some(hackernews) = &cfg.hackernews {
        sources.push(Box::new(
            HackerNewsSource::new(client.clone()).with_config(hackernews.clone()),
        ));
    }
    if !cfg.workable.is_empty() {
        sources.push(Box::new(WorkableSource::new(
            cfg.workable.clone(),
            client.clone(),
        )));
    }
    if !cfg.recruitee.is_empty() {
        sources.push(Box::new(RecruiteeSource::new(
            cfg.recruitee.clone(),
            client.clone(),
        )));
    }
    if !cfg.smartrecruiters.is_empty() {
        sources.push(Box::new(SmartRecruitersSource::new(
            cfg.smartrecruiters.clone(),
            client.clone(),
        )));
    }
    if let Some(weworkremotely) = &cfg.weworkremotely {
        sources.push(Box::new(
            WeWorkRemotelySource::new(client.clone()).with_config(weworkremotely.clone()),
        ));
    }
    if cfg.remoteyeah {
        sources.push(Box::new(RemoteYeahSource::new(client.clone())));
    }
    if let Some(workingnomads) = &cfg.workingnomads {
        sources.push(Box::new(
            WorkingNomadsSource::new(client.clone()).with_config(workingnomads.clone()),
        ));
    }
    if let Some(workable_search) = &cfg.workable_search {
        sources.push(Box::new(
            WorkableSearchSource::new(client.clone()).with_config(workable_search.clone()),
        ));
    }
    if let Some(fourdayweek) = &cfg.fourdayweek {
        sources.push(Box::new(
            FourDayWeekSource::new(client.clone()).with_config(fourdayweek.clone()),
        ));
    }
    if cfg.pythonjobs {
        sources.push(Box::new(PythonJobsSource::new(client.clone())));
    }
    if cfg.atlassian {
        sources.push(Box::new(AtlassianSource::new(client.clone())));
    }
    sources
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(cfg: &SourcesConfig) -> Vec<String> {
        build_sources(cfg, &reqwest::Client::new())
            .iter()
            .map(|source| source.name().to_string())
            .collect()
    }

    #[test]
    fn build_sources_from_config() {
        let cfg: SourcesConfig = toml::from_str(
            r#"
            greenhouse = ["a"]
            lever = ["b"]
            ashby = ["c"]
            remoteok = true
            remotive = true

            [getonboard]
            categories = ["programming"]
            max_pages = 2
            "#,
        )
        .unwrap();
        assert_eq!(
            names(&cfg),
            [
                "greenhouse",
                "lever",
                "ashby",
                "remoteok",
                "remotive",
                "getonboard"
            ]
        );
        assert_eq!(cfg.getonboard.as_ref().unwrap().max_pages, 2);
        assert_eq!(cfg.getonboard.as_ref().unwrap().per_page, 50);
    }

    #[test]
    fn build_sources_from_the_new_kinds() {
        let cfg: SourcesConfig = toml::from_str(
            r#"
            workable = ["huggingface"]
            recruitee = ["bunq"]
            smartrecruiters = ["BoschGroup"]

            [himalayas]
            queries = ["rust"]

            [jobicy]

            [hackernews]
            months = 2
            "#,
        )
        .unwrap();
        assert_eq!(
            names(&cfg),
            [
                "himalayas",
                "jobicy",
                "hackernews",
                "workable",
                "recruitee",
                "smartrecruiters"
            ]
        );
        assert_eq!(cfg.himalayas.as_ref().unwrap().queries, ["rust"]);
        assert_eq!(cfg.himalayas.as_ref().unwrap().country, "CL");
        assert_eq!(cfg.jobicy, Some(JobicyConfig::default()));
        assert_eq!(cfg.hackernews.as_ref().unwrap().months, 2);
    }

    #[test]
    fn build_sources_from_the_feed_and_search_kinds() {
        let cfg: SourcesConfig = toml::from_str(
            r#"
            remoteyeah = true
            pythonjobs = true
            atlassian = true

            [weworkremotely]
            categories = ["remote-devops-sysadmin-jobs"]

            [workingnomads]

            [workable_search]
            queries = ["rust engineer"]
            location = "Peru"

            [fourdayweek]
            max_age_days = 7
            "#,
        )
        .unwrap();
        assert_eq!(
            names(&cfg),
            [
                "weworkremotely",
                "remoteyeah",
                "workingnomads",
                "workable_search",
                "fourdayweek",
                "pythonjobs",
                "atlassian"
            ]
        );
        assert_eq!(
            cfg.weworkremotely.as_ref().unwrap().categories,
            ["remote-devops-sysadmin-jobs"]
        );
        assert_eq!(cfg.workingnomads, Some(WorkingNomadsConfig::default()));
        let search = cfg.workable_search.as_ref().unwrap();
        assert_eq!(
            (search.queries.as_slice(), search.location.as_str()),
            (&["rust engineer".to_string()][..], "Peru")
        );
        assert_eq!(cfg.fourdayweek.as_ref().unwrap().max_age_days, 7);
    }

    #[test]
    fn empty_config_builds_no_sources() {
        let cfg: SourcesConfig = toml::from_str("").unwrap();
        assert_eq!(cfg, SourcesConfig::default());
        assert!(names(&cfg).is_empty());
    }

    #[test]
    fn empty_getonboard_table_uses_defaults() {
        let cfg: SourcesConfig = toml::from_str("[getonboard]\n").unwrap();
        assert_eq!(cfg.getonboard, Some(GetOnBoardConfig::default()));
        assert_eq!(names(&cfg), ["getonboard"]);
    }

    #[test]
    fn misspelled_key_is_rejected() {
        let error = toml::from_str::<SourcesConfig>("greenhous = [\"a\"]\n").unwrap_err();
        assert!(error.to_string().contains("greenhous"), "{error}");
    }

    #[test]
    fn load_sources_config_reads_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sources.toml");
        std::fs::write(&path, "lever = [\"palantir\"]\nremoteok = true\n").unwrap();

        let cfg = load_sources_config(&path).unwrap();

        assert_eq!(cfg.lever, ["palantir"]);
        assert!(
            cfg.remoteok && !cfg.remotive && cfg.greenhouse.is_empty() && cfg.getonboard.is_none()
        );
    }

    #[test]
    fn load_sources_config_names_the_file_when_it_is_missing_or_invalid() {
        let dir = tempfile::tempdir().unwrap();

        let missing = load_sources_config(&dir.path().join("nope.toml")).unwrap_err();
        assert!(format!("{missing:#}").contains("nope.toml"), "{missing:#}");

        let bad = dir.path().join("bad.toml");
        std::fs::write(&bad, "greenhouse = \"not a list\"\n").unwrap();
        let invalid = load_sources_config(&bad).unwrap_err();
        assert!(format!("{invalid:#}").contains("bad.toml"), "{invalid:#}");
    }

    #[test]
    fn shipped_sources_toml_parses() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("profile")
            .join("sources.toml");

        let cfg = load_sources_config(&path).unwrap();

        assert!(cfg.remoteok && cfg.remotive);
        assert!(cfg.remoteyeah && cfg.pythonjobs && cfg.atlassian);
        assert!(cfg.weworkremotely.is_some() && cfg.workingnomads.is_some());
        assert_eq!(
            cfg.workable_search
                .as_ref()
                .map(|search| search.location.as_str()),
            Some("Chile")
        );
        assert!(cfg.fourdayweek.is_some());
        assert!(
            cfg.greenhouse.len() >= 30,
            "the seed list is wide on purpose (audit finding A1)"
        );
        assert!(!names(&cfg).is_empty());
    }
}
