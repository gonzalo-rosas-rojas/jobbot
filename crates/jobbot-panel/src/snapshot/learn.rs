//! The study section: how many questions were seen and are due, the weakest topics and the gaps
//! that dossiers added.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;

use super::offer_label;
use crate::Panel;
use jobbot_app::ports::store::{GapRow, TopicProgress};

/// The learn section lists this many weakest topics, judged on each topic's last reviews.
const WEAK_TOPICS_SHOWN: usize = 5;
const WEAK_TOPICS_LAST_REVIEWS: usize = 10;

#[derive(Debug, Serialize)]
pub struct WeakTopic {
    pub topic: String,
    pub average: f64,
}

/// A card that a dossier added, with the offer it came from.
#[derive(Debug, Serialize)]
pub struct GapView {
    pub id: String,
    pub topic: String,
    pub offer_key: String,
    pub offer: String,
    pub seen: bool,
}

#[derive(Debug, Serialize)]
pub struct Learn {
    pub total: usize,
    pub seen: usize,
    pub due: usize,
    pub weak_topics: Vec<WeakTopic>,
    pub topics: Vec<TopicProgress>,
    pub gaps: Vec<GapView>,
}

pub(super) fn gap_view(panel: &Panel, gap: GapRow) -> GapView {
    GapView {
        offer: offer_label(panel, &gap.source),
        offer_key: gap.source,
        id: gap.id,
        topic: gap.topic,
        seen: gap.seen,
    }
}

pub(super) fn learn(panel: &Panel, now: DateTime<Utc>) -> Result<Learn> {
    let questions = panel.store.all_questions()?;
    let stats = panel.store.stats(&questions, now)?;
    Ok(Learn {
        total: stats.total,
        seen: stats.seen,
        due: stats.due,
        weak_topics: panel
            .store
            .weak_topics(WEAK_TOPICS_SHOWN, WEAK_TOPICS_LAST_REVIEWS)?
            .into_iter()
            .map(|(topic, average)| WeakTopic { topic, average })
            .collect(),
        topics: panel.store.topic_progress(&questions, now)?,
        gaps: panel
            .store
            .gap_rows()?
            .into_iter()
            .map(|gap| gap_view(panel, gap))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use crate::fixtures::{add, panel};
    use crate::snapshot::build_snapshot;
    use jobbot_domain::prep::Question;
    use jobbot_testkit::NOW;

    #[test]
    fn the_learn_summary_links_gaps_to_their_offer_and_shows_weak_topics() {
        let (panel, _dir) = panel();
        let key = add(&panel, "1", 80.0, &[]);
        panel
            .store
            .add_cards(
                &[Question {
                    id: "gap-x-1".into(),
                    topic: "kubernetes".into(),
                    question: "q".into(),
                    answer: "a".into(),
                }],
                &key,
                *NOW,
            )
            .unwrap();
        let questions = panel.store.all_questions().unwrap();
        panel.store.grade(&questions[0], 1, *NOW).unwrap();
        let learn = build_snapshot(&panel).unwrap().learn.unwrap();
        assert_eq!(learn.seen, 1);
        assert_eq!(learn.total, questions.len());
        assert_eq!(learn.weak_topics.len(), 1);
        assert_eq!(learn.weak_topics[0].topic, questions[0].topic);
        assert_eq!(learn.gaps.len(), 1);
        assert_eq!(learn.gaps[0].topic, "kubernetes");
        assert_eq!(learn.gaps[0].offer, "Co 1 — Role 1");
        assert!(!learn.gaps[0].seen);
        assert!(
            learn
                .topics
                .iter()
                .any(|t| t.topic == questions[0].topic && t.seen == 1)
        );
    }
}
