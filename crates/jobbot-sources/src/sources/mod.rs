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
use serde::Deserialize;

use crate::http::Http;

pub use jobbot_app::ports::Source;

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
    /// remote postings are read, plus those in `smartrecruiters_onsite_countries`.
    pub smartrecruiters: Vec<String>,
    /// ISO alpha-2 country codes (`["au"]`) whose on-site and hybrid SmartRecruiters postings are
    /// read too, for every company in `smartrecruiters`.
    pub smartrecruiters_onsite_countries: Vec<String>,
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
    /// Not a key of `sources.toml`: the caller fills it from `criteria.extra_open_title_words`, so
    /// sources that cannot read every posting read the relevant titles first.
    #[serde(skip)]
    pub relevant_title_words: Vec<String>,
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

/// One kind of source: its name (also its key in `profile/sources.toml` and the start of its job
/// keys) and how it is built when the configuration turns it on.
pub struct SourceRow {
    pub name: &'static str,
    /// The source `cfg` asks for, sharing `http`; `None` when its key is off or missing.
    pub build: fn(&SourcesConfig, &Http) -> Option<Box<dyn Source>>,
}

/// A source from a list of boards: on when the list is not empty.
fn listed<S: Source + 'static>(
    boards: &[String],
    http: &Http,
    new: fn(Vec<String>, Http) -> S,
) -> Option<Box<dyn Source>> {
    (!boards.is_empty()).then(|| Box::new(new(boards.to_vec(), http.clone())) as Box<dyn Source>)
}

fn boxed(source: impl Source + 'static) -> Box<dyn Source> {
    Box::new(source)
}

/// Every source jobbot can read, in the order discovery reads them. Adding a source is a module, a
/// key in `SourcesConfig` and one row here.
pub const SOURCE_REGISTRY: [SourceRow; 19] = [
    SourceRow {
        name: "greenhouse",
        build: |cfg, http| {
            listed(&cfg.greenhouse, http, |boards, http| {
                GreenhouseSource::new(boards, http)
            })
        },
    },
    SourceRow {
        name: "lever",
        build: |cfg, http| {
            listed(&cfg.lever, http, |boards, http| {
                LeverSource::new(boards, http)
            })
        },
    },
    SourceRow {
        name: "ashby",
        build: |cfg, http| {
            listed(&cfg.ashby, http, |boards, http| {
                AshbySource::new(boards, http)
            })
        },
    },
    SourceRow {
        name: "remoteok",
        build: |cfg, http| {
            cfg.remoteok
                .then(|| boxed(RemoteOkSource::new(http.clone())))
        },
    },
    SourceRow {
        name: "remotive",
        build: |cfg, http| {
            cfg.remotive
                .then(|| boxed(RemotiveSource::new(http.clone())))
        },
    },
    SourceRow {
        name: "getonboard",
        build: |cfg, http| {
            cfg.getonboard.as_ref().map(|getonboard| {
                boxed(
                    GetOnBoardSource::new(http.clone())
                        .with_categories(getonboard.categories.clone())
                        .with_max_pages(getonboard.max_pages)
                        .with_per_page(getonboard.per_page),
                )
            })
        },
    },
    SourceRow {
        name: "himalayas",
        build: |cfg, http| {
            cfg.himalayas.as_ref().map(|himalayas| {
                boxed(HimalayasSource::new(http.clone()).with_config(himalayas.clone()))
            })
        },
    },
    SourceRow {
        name: "jobicy",
        build: |cfg, http| {
            cfg.jobicy
                .as_ref()
                .map(|jobicy| boxed(JobicySource::new(http.clone()).with_config(jobicy.clone())))
        },
    },
    SourceRow {
        name: "hackernews",
        build: |cfg, http| {
            cfg.hackernews.as_ref().map(|hackernews| {
                boxed(HackerNewsSource::new(http.clone()).with_config(hackernews.clone()))
            })
        },
    },
    SourceRow {
        name: "workable",
        build: |cfg, http| {
            listed(&cfg.workable, http, |boards, http| {
                WorkableSource::new(boards, http)
            })
        },
    },
    SourceRow {
        name: "recruitee",
        build: |cfg, http| {
            listed(&cfg.recruitee, http, |boards, http| {
                RecruiteeSource::new(boards, http)
            })
        },
    },
    SourceRow {
        name: "smartrecruiters",
        build: |cfg, http| {
            (!cfg.smartrecruiters.is_empty()).then(|| {
                boxed(
                    SmartRecruitersSource::new(cfg.smartrecruiters.clone(), http.clone())
                        .with_onsite_countries(&cfg.smartrecruiters_onsite_countries)
                        .with_title_words(&cfg.relevant_title_words),
                )
            })
        },
    },
    SourceRow {
        name: "weworkremotely",
        build: |cfg, http| {
            cfg.weworkremotely.as_ref().map(|weworkremotely| {
                boxed(WeWorkRemotelySource::new(http.clone()).with_config(weworkremotely.clone()))
            })
        },
    },
    SourceRow {
        name: "remoteyeah",
        build: |cfg, http| {
            cfg.remoteyeah
                .then(|| boxed(RemoteYeahSource::new(http.clone())))
        },
    },
    SourceRow {
        name: "workingnomads",
        build: |cfg, http| {
            cfg.workingnomads.as_ref().map(|workingnomads| {
                boxed(WorkingNomadsSource::new(http.clone()).with_config(workingnomads.clone()))
            })
        },
    },
    SourceRow {
        name: "workable_search",
        build: |cfg, http| {
            cfg.workable_search.as_ref().map(|workable_search| {
                boxed(WorkableSearchSource::new(http.clone()).with_config(workable_search.clone()))
            })
        },
    },
    SourceRow {
        name: "fourdayweek",
        build: |cfg, http| {
            cfg.fourdayweek.as_ref().map(|fourdayweek| {
                boxed(FourDayWeekSource::new(http.clone()).with_config(fourdayweek.clone()))
            })
        },
    },
    SourceRow {
        name: "pythonjobs",
        build: |cfg, http| {
            cfg.pythonjobs
                .then(|| boxed(PythonJobsSource::new(http.clone())))
        },
    },
    SourceRow {
        name: "atlassian",
        build: |cfg, http| {
            cfg.atlassian
                .then(|| boxed(AtlassianSource::new(http.clone())))
        },
    },
];

/// The sources `cfg` turns on, in the registry's order. Every source gets a clone of `http`: one
/// client, one board of resting hosts and one deadline serve the whole run.
pub fn build_sources(cfg: &SourcesConfig, http: &Http) -> Vec<Box<dyn Source>> {
    SOURCE_REGISTRY
        .iter()
        .filter_map(|row| (row.build)(cfg, http))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(cfg: &SourcesConfig) -> Vec<String> {
        build_sources(cfg, &Http::new(reqwest::Client::new()))
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

    /// Every key `profile/sources.toml` knows, each turned on.
    const EVERY_SOURCE: &str = r#"
        greenhouse = ["a"]
        lever = ["b"]
        ashby = ["c"]
        remoteok = true
        remotive = true
        workable = ["d"]
        recruitee = ["e"]
        smartrecruiters = ["f"]
        smartrecruiters_onsite_countries = ["au"]
        remoteyeah = true
        pythonjobs = true
        atlassian = true
        [getonboard]
        [himalayas]
        [jobicy]
        [hackernews]
        [weworkremotely]
        [workingnomads]
        [workable_search]
        [fourdayweek]
    "#;

    #[test]
    fn every_registry_row_builds_a_source_under_its_own_name_in_the_documented_order() {
        let cfg: SourcesConfig = toml::from_str(EVERY_SOURCE).unwrap();
        let registered: Vec<&str> = SOURCE_REGISTRY.iter().map(|row| row.name).collect();
        assert_eq!(names(&cfg), registered);
        assert_eq!(
            registered,
            [
                "greenhouse",
                "lever",
                "ashby",
                "remoteok",
                "remotive",
                "getonboard",
                "himalayas",
                "jobicy",
                "hackernews",
                "workable",
                "recruitee",
                "smartrecruiters",
                "weworkremotely",
                "remoteyeah",
                "workingnomads",
                "workable_search",
                "fourdayweek",
                "pythonjobs",
                "atlassian",
            ]
        );
    }

    #[test]
    fn a_row_builds_nothing_when_its_key_is_off() {
        let off = SourcesConfig::default();
        let http = Http::new(reqwest::Client::new());
        for row in &SOURCE_REGISTRY {
            assert!((row.build)(&off, &http).is_none(), "{}", row.name);
        }
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
}
