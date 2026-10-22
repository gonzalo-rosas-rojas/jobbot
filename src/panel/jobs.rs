//! The explorer: every application the tracker has, filtered and paged, for the places where the
//! board is cut (a closed column shows 25 of `total`) and for a search over all of them. Read only,
//! the same `Card` as the snapshot's, so a card looks the same wherever it comes from.

use std::collections::BTreeMap;

use anyhow::Result;
use serde::Serialize;

use super::Panel;
use super::snapshot::{Card, card, card_context};
use crate::outbound::clean;
use crate::rules::load_criteria;
use crate::tracker::{ApplicationRow, State};

/// Cards per page.
pub const PAGE_SIZE: usize = 50;
/// The longest search or source name taken, in characters.
const TEXT_MAX_CHARS: usize = 100;
/// The last page number taken (a typo should not ask for the millionth).
const PAGE_MAX: usize = 100_000;
const PARAMS: [&str; 5] = ["state", "min_score", "source", "q", "page"];

/// What the explorer was asked for: `state=&min_score=&source=&q=&page=`, all optional. An empty
/// value is no filter. Anything else that is wrong is refused with a message, because a filter
/// that is silently ignored would show everything and look like an answer.
#[derive(Debug, PartialEq)]
pub struct JobsQuery {
    pub state: Option<State>,
    /// Inclusive, 0 to 100.
    pub min_score: Option<f64>,
    /// Lower case.
    pub source: Option<String>,
    /// Lower case, words separated by one space: every word must appear in company or title.
    pub q: Option<String>,
    /// From 1.
    pub page: usize,
}

impl JobsQuery {
    /// The query from its decoded `name=value` pairs, or the Spanish reason it is not valid.
    pub fn from_pairs(pairs: &BTreeMap<String, String>) -> std::result::Result<JobsQuery, String> {
        if let Some(unknown) = pairs.keys().find(|name| !PARAMS.contains(&name.as_str())) {
            return Err(format!("Parámetro desconocido: {}", clean(unknown, 40)));
        }
        let given = |name: &str| {
            pairs
                .get(name)
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
        };
        let state = match given("state") {
            None => None,
            Some(text) => Some(
                State::parse(text)
                    .ok_or_else(|| format!("Estado desconocido: {}", clean(text, 40)))?,
            ),
        };
        let min_score = match given("min_score") {
            None => None,
            Some(text) => match text.parse::<f64>() {
                Ok(score) if score.is_finite() && (0.0..=100.0).contains(&score) => Some(score),
                _ => return Err("min_score debe ser un número de 0 a 100".to_string()),
            },
        };
        let source = match given("source") {
            None => None,
            Some(text) if text.chars().count() > TEXT_MAX_CHARS => {
                return Err(format!("source admite hasta {TEXT_MAX_CHARS} caracteres"));
            }
            Some(text) => Some(text.to_lowercase()),
        };
        let q = match given("q") {
            None => None,
            Some(text) => {
                let words = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if words.chars().count() > TEXT_MAX_CHARS {
                    return Err(format!("q admite hasta {TEXT_MAX_CHARS} caracteres"));
                }
                Some(words.to_lowercase())
            }
        };
        let page = match given("page") {
            None => 1,
            Some(text) => match text.parse::<usize>() {
                Ok(page) if (1..=PAGE_MAX).contains(&page) => page,
                _ => return Err(format!("page debe ser un entero de 1 a {PAGE_MAX}")),
            },
        };
        Ok(JobsQuery {
            state,
            min_score,
            source,
            q,
            page,
        })
    }

    fn accepts(&self, row: &ApplicationRow) -> bool {
        if self.min_score.is_some_and(|min| row.score < min) {
            return false;
        }
        if self
            .source
            .as_deref()
            .is_some_and(|source| row.source.to_lowercase() != source)
        {
            return false;
        }
        match &self.q {
            None => true,
            Some(q) => {
                let text = format!("{} {}", row.company, row.title).to_lowercase();
                q.split(' ').all(|word| text.contains(word))
            }
        }
    }
}

/// One page of the explorer. `pages` is 0 when nothing matches; a page past the last one is empty
/// and still says how many there are.
#[derive(Debug, Serialize)]
pub struct JobsPage {
    pub page: usize,
    pub per_page: usize,
    pub total: usize,
    pub pages: usize,
    pub cards: Vec<Card>,
}

/// The applications that match, best score first (then key, so a page never repeats or skips a
/// card between requests), `PAGE_SIZE` at a time. Reads only.
pub fn build_jobs_page(panel: &Panel, query: &JobsQuery) -> Result<JobsPage> {
    let mut rows: Vec<ApplicationRow> = match query.state {
        Some(state) => panel.tracker.list_by_state(state)?,
        None => {
            let mut all = Vec::new();
            for state in State::ALL {
                all.extend(panel.tracker.list_by_state(state)?);
            }
            all.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.job_key.cmp(&b.job_key))
            });
            all
        }
    };
    rows.retain(|row| query.accepts(row));
    let total = rows.len();
    let dossiers = panel.paths.data_dir.join("dossiers");
    let handoffs = panel.paths.data_dir.join("handoff");
    let criteria = load_criteria(&panel.paths.profile_dir.join("criteria.toml")).ok();
    let ctx = card_context(panel.now(), &dossiers, &handoffs, criteria.as_ref());
    Ok(JobsPage {
        page: query.page,
        per_page: PAGE_SIZE,
        total,
        pages: total.div_ceil(PAGE_SIZE),
        cards: rows
            .iter()
            .skip((query.page - 1) * PAGE_SIZE)
            .take(PAGE_SIZE)
            .map(|row| card(row, &ctx))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::panel::fixtures::panel;
    use crate::panel::snapshot::build_snapshot;
    use crate::testing::{NOW, make_job_with};
    use crate::tracker::State;

    fn pairs(items: &[(&str, &str)]) -> BTreeMap<String, String> {
        items
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// Adds a job and walks its application to `state`; returns the key.
    fn put(
        p: &Panel,
        id: &str,
        company: &str,
        title: &str,
        source: &str,
        score: f64,
        state: State,
    ) -> String {
        let job = make_job_with(|j| {
            j.source = source.into();
            j.external_id = id.into();
            j.company = company.into();
            j.title = title.into();
        });
        p.tracker
            .add_job(&job, score, &BTreeMap::new(), *NOW)
            .unwrap();
        p.tracker.create_application(&job.key(), *NOW).unwrap();
        let path: &[State] = match state {
            State::Scored => &[],
            State::Prepared => &[State::Prepared],
            State::NeedsGonza => &[State::Prepared, State::NeedsGonza],
            State::Submitted => &[State::Prepared, State::Submitted],
            State::Rejected => &[State::Prepared, State::Submitted, State::Rejected],
            other => panic!("no path to {other:?} in this helper"),
        };
        for next in path {
            p.tracker.transition(&job.key(), *next, None, *NOW).unwrap();
        }
        job.key()
    }

    fn page_of(p: &Panel, items: &[(&str, &str)]) -> JobsPage {
        let query = JobsQuery::from_pairs(&pairs(items)).unwrap();
        build_jobs_page(p, &query).unwrap()
    }

    fn keys(page: &JobsPage) -> Vec<String> {
        page.cards.iter().map(|c| c.key.clone()).collect()
    }

    #[test]
    fn the_query_is_read_strictly_and_an_empty_value_is_no_filter() {
        let ok = JobsQuery::from_pairs(&pairs(&[
            ("state", "rejected"),
            ("min_score", "72.5"),
            ("source", " Ashby "),
            ("q", " Acme  Senior "),
            ("page", "3"),
        ]))
        .unwrap();
        assert_eq!(ok.state, Some(State::Rejected));
        assert_eq!(ok.min_score, Some(72.5));
        assert_eq!(ok.source.as_deref(), Some("ashby"));
        assert_eq!(ok.q.as_deref(), Some("acme senior"));
        assert_eq!(ok.page, 3);

        let none = JobsQuery::from_pairs(&pairs(&[
            ("state", ""),
            ("min_score", " "),
            ("source", ""),
            ("q", ""),
            ("page", ""),
        ]))
        .unwrap();
        assert_eq!(
            none,
            JobsQuery {
                state: None,
                min_score: None,
                source: None,
                q: None,
                page: 1
            }
        );
        assert_eq!(JobsQuery::from_pairs(&pairs(&[])).unwrap().page, 1);

        let long = "x".repeat(101);
        for (bad, why) in [
            (("state", "nope"), "an unknown state"),
            (("min_score", "high"), "a score that is not a number"),
            (("min_score", "NaN"), "a score that is not finite"),
            (("min_score", "-1"), "a score under 0"),
            (("min_score", "101"), "a score over 100"),
            (("page", "0"), "page zero"),
            (("page", "-2"), "a negative page"),
            (("page", "two"), "a page that is not a number"),
            (("page", "100001"), "a page past the cap"),
            (("q", long.as_str()), "a search that is too long"),
            (("source", long.as_str()), "a source that is too long"),
            (("min_scor", "80"), "a misspelled parameter"),
        ] {
            let err = JobsQuery::from_pairs(&pairs(&[bad])).unwrap_err();
            assert!(!err.is_empty(), "{why}");
        }
    }

    #[test]
    fn it_pages_by_fifty_best_score_first_and_never_repeats_or_loses_one() {
        let (p, _d) = panel();
        for n in 0..120 {
            // 20 distinct scores, so the key breaks the ties.
            put(
                &p,
                &format!("{n:03}"),
                &format!("Co {n}"),
                "Role",
                "test",
                50.0 + f64::from(n % 20),
                State::Scored,
            );
        }
        let first = page_of(&p, &[]);
        assert_eq!(
            (first.page, first.per_page, first.total, first.pages),
            (1, 50, 120, 3)
        );
        assert_eq!(first.cards.len(), 50);
        let second = page_of(&p, &[("page", "2")]);
        let third = page_of(&p, &[("page", "3")]);
        assert_eq!((second.cards.len(), third.cards.len()), (50, 20));
        let all: Vec<String> = [&first, &second, &third]
            .iter()
            .flat_map(|page| keys(page))
            .collect();
        assert_eq!(
            all.iter().collect::<std::collections::BTreeSet<_>>().len(),
            120,
            "no card twice"
        );
        let scores: Vec<i64> = [&first, &second, &third]
            .iter()
            .flat_map(|page| page.cards.iter().map(|c| c.score))
            .collect();
        assert!(scores.windows(2).all(|w| w[0] >= w[1]), "best first");
        // Past the last page there is nothing, and it says how many there are.
        let past = page_of(&p, &[("page", "9")]);
        assert!(past.cards.is_empty());
        assert_eq!((past.total, past.pages), (120, 3));
        // Nothing at all: no pages.
        let empty = page_of(&p, &[("q", "no such company")]);
        assert_eq!((empty.total, empty.pages, empty.cards.len()), (0, 0, 0));
    }

    #[test]
    fn each_filter_narrows_and_they_add_up() {
        let (p, _d) = panel();
        let a = put(
            &p,
            "1",
            "Acme Robotics",
            "Senior Engineer",
            "ashby",
            90.0,
            State::Submitted,
        );
        let b = put(
            &p,
            "2",
            "Acme Robotics",
            "Junior Engineer",
            "lever",
            65.0,
            State::Rejected,
        );
        let c = put(
            &p,
            "3",
            "Über Cloud",
            "Staff Engineer",
            "ashby",
            80.0,
            State::Rejected,
        );
        let d = put(
            &p,
            "4",
            "Zeta",
            "Data Scientist",
            "greenhouse",
            70.0,
            State::Scored,
        );

        assert_eq!(page_of(&p, &[]).total, 4);
        assert_eq!(
            keys(&page_of(&p, &[("state", "rejected")])),
            [c.as_str(), b.as_str()]
        );
        assert_eq!(keys(&page_of(&p, &[("state", "scored")])), [d.as_str()]);
        assert_eq!(
            keys(&page_of(&p, &[("min_score", "75")])),
            [a.as_str(), c.as_str()]
        );
        assert_eq!(
            keys(&page_of(&p, &[("min_score", "80")])),
            [a.as_str(), c.as_str()],
            "the minimum is inclusive"
        );
        assert_eq!(
            keys(&page_of(&p, &[("source", "ASHBY")])),
            [a.as_str(), c.as_str()],
            "a source is matched whatever its case"
        );
        // Company and title, every word, in any order, whatever the case, accents included.
        assert_eq!(
            keys(&page_of(&p, &[("q", "acme")])),
            [a.as_str(), b.as_str()]
        );
        assert_eq!(
            keys(&page_of(&p, &[("q", "engineer acme")])),
            [a.as_str(), b.as_str()]
        );
        assert_eq!(keys(&page_of(&p, &[("q", "JUNIOR")])), [b.as_str()]);
        assert_eq!(keys(&page_of(&p, &[("q", "über")])), [c.as_str()]);
        assert_eq!(keys(&page_of(&p, &[("q", "ÜBER cloud")])), [c.as_str()]);
        // The wildcards of a database are plain characters here.
        assert_eq!(page_of(&p, &[("q", "%")]).total, 0);
        assert_eq!(page_of(&p, &[("q", "_")]).total, 0);
        // All together.
        assert_eq!(
            keys(&page_of(
                &p,
                &[
                    ("state", "rejected"),
                    ("source", "ashby"),
                    ("min_score", "70"),
                    ("q", "staff"),
                ]
            )),
            [c.as_str()]
        );
        assert_eq!(
            page_of(&p, &[("state", "rejected"), ("source", "greenhouse")]).total,
            0
        );
    }

    #[test]
    fn a_card_is_the_same_one_the_snapshot_serves() {
        let (p, _d) = panel();
        let key = put(&p, "1", "Acme", "Role", "ashby", 88.0, State::NeedsGonza);
        p.tracker
            .set_snoozed_until(&key, Some(*NOW + chrono::TimeDelta::days(2)))
            .unwrap();
        let from_explorer = serde_json::to_value(&page_of(&p, &[]).cards[0]).unwrap();
        let snapshot = serde_json::to_value(build_snapshot(&p).unwrap()).unwrap();
        let from_board = snapshot["board"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|column| column["cards"].as_array().unwrap())
            .find(|card| card["key"] == key.as_str())
            .unwrap();
        assert_eq!(&from_explorer, from_board);
        assert!(
            from_explorer["snoozed_until"].is_string(),
            "what is snoozed is still listed, marked as such"
        );
    }
}
