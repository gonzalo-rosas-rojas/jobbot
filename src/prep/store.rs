//! Progress of the interview drill in SQLite: one card per question that Gonza has answered,
//! plus a log of every review (used to find weak topics).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use anyhow::{Result, ensure};
use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use super::sm2::sm2;
use super::{Question, load_questions};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS cards (
    qid TEXT PRIMARY KEY, ease REAL NOT NULL, interval INTEGER NOT NULL, reps INTEGER NOT NULL, due TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS reviews (
    id INTEGER PRIMARY KEY AUTOINCREMENT, qid TEXT NOT NULL, topic TEXT NOT NULL, grade INTEGER NOT NULL, at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS extra_cards (
    id TEXT PRIMARY KEY, topic TEXT NOT NULL, question TEXT NOT NULL, answer TEXT NOT NULL,
    source TEXT NOT NULL, added_at TEXT NOT NULL
);
";

/// Dates are stored as UTC text in one fixed format, so comparing the strings compares the instants.
fn iso(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Counts for `prep stats`. `due` is every question `next_question` could serve right now:
/// reviews that have come due plus questions never seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrepStats {
    pub total: usize,
    pub seen: usize,
    pub due: usize,
}

/// Progress of one topic: how many cards it has, how many Gonza has answered at least once, and
/// how many he could be served right now (never seen, or come due).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TopicProgress {
    pub topic: String,
    pub total: usize,
    pub seen: usize,
    pub due: usize,
}

/// A card a dossier added (a gap to study), with the posting it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GapRow {
    pub id: String,
    pub topic: String,
    /// The job key of the posting whose dossier produced the card.
    pub source: String,
    /// Gonza has answered it at least once.
    pub seen: bool,
}

pub struct PrepStore {
    conn: Connection,
}

impl PrepStore {
    pub fn open(path: &Path) -> Result<PrepStore> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // Same rules as the main database; a version stamp arrives with this database's first migration.
        let _ = conn
            .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0));
        conn.pragma_update(None, "synchronous", "FULL")?;
        PrepStore::with_connection(conn)
    }

    pub fn in_memory() -> Result<PrepStore> {
        PrepStore::with_connection(Connection::open_in_memory()?)
    }

    fn with_connection(conn: Connection) -> Result<PrepStore> {
        conn.execute_batch(SCHEMA)?;
        Ok(PrepStore { conn })
    }

    /// Adds cards the compiled bank does not have (the gaps of a dossier). A card whose id is
    /// already stored is left alone, so running the same dossier twice adds nothing. Returns how
    /// many cards were new.
    pub fn add_cards(&self, cards: &[Question], source: &str, now: DateTime<Utc>) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let mut added = 0;
        for card in cards {
            added += tx.execute(
                "INSERT OR IGNORE INTO extra_cards (id, topic, question, answer, source, added_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    card.id,
                    card.topic,
                    card.question,
                    card.answer,
                    source,
                    iso(now)
                ],
            )?;
        }
        tx.commit()?;
        Ok(added)
    }

    fn extra_questions(&self) -> Result<Vec<Question>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, topic, question, answer FROM extra_cards ORDER BY rowid")?;
        let rows = stmt.query_map([], |r| {
            Ok(Question {
                id: r.get(0)?,
                topic: r.get(1)?,
                question: r.get(2)?,
                answer: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The compiled bank followed by the extra cards. A bank card is never replaced by an extra
    /// card with the same id.
    pub fn all_questions(&self) -> Result<Vec<Question>> {
        let mut all = load_questions()?;
        let known: HashSet<String> = all.iter().map(|q| q.id.clone()).collect();
        all.extend(
            self.extra_questions()?
                .into_iter()
                .filter(|q| !known.contains(&q.id)),
        );
        Ok(all)
    }

    /// Due date of every card, by question id.
    fn due_dates(&self) -> Result<HashMap<String, String>> {
        let mut stmt = self.conn.prepare("SELECT qid, due FROM cards")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
    }

    /// Questions Gonza can study now, in the order they are served: reviews that have come due
    /// first (oldest due date first), then questions never seen, in bank order.
    fn candidates<'a>(
        &self,
        questions: &'a [Question],
        now: DateTime<Utc>,
        topic: Option<&str>,
    ) -> Result<Vec<&'a Question>> {
        let due_dates = self.due_dates()?;
        let cutoff = iso(now);
        // (is unseen, due date, position in the bank): `false < true` puts due reviews first.
        let mut found: Vec<(bool, String, usize)> = Vec::new();
        for (index, q) in questions.iter().enumerate() {
            if topic.is_some_and(|t| q.topic != t) {
                continue;
            }
            match due_dates.get(&q.id) {
                None => found.push((true, String::new(), index)),
                Some(due) if *due <= cutoff => found.push((false, due.clone(), index)),
                Some(_) => {}
            }
        }
        found.sort();
        Ok(found
            .into_iter()
            .map(|(_, _, index)| &questions[index])
            .collect())
    }

    /// The returned reference points into `questions`, not into the store, hence the lifetime `'a`.
    pub fn next_question<'a>(
        &self,
        questions: &'a [Question],
        now: DateTime<Utc>,
        topic: Option<&str>,
    ) -> Result<Option<&'a Question>> {
        Ok(self.candidates(questions, now, topic)?.first().copied())
    }

    /// Records a grade from 0 to 5: reschedules the card with SM-2 and logs the review.
    pub fn grade(&self, q: &Question, grade: u8, now: DateTime<Utc>) -> Result<()> {
        ensure!(grade <= 5, "grade must be between 0 and 5, got {grade}");
        let current = self
            .conn
            .query_row(
                "SELECT ease, interval, reps FROM cards WHERE qid = ?1",
                [&q.id],
                |r| {
                    Ok((
                        r.get::<_, f64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;
        let (ease, interval, reps) = current.unwrap_or((2.5, 0, 0));
        let (ease, interval, reps) = sm2(ease, interval, reps, grade);
        let due = iso(now + TimeDelta::days(interval));
        // Both writes or neither: a card must never move without its review being logged.
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO cards (qid, ease, interval, reps, due) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(qid) DO UPDATE SET ease = excluded.ease, interval = excluded.interval,
                 reps = excluded.reps, due = excluded.due",
            params![q.id, ease, interval, reps, due],
        )?;
        tx.execute(
            "INSERT INTO reviews (qid, topic, grade, at) VALUES (?1, ?2, ?3, ?4)",
            params![q.id, q.topic, grade, iso(now)],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Topics whose average grade over their last `last_n` reviews is below 4, worst first,
    /// at most `limit` of them.
    pub fn weak_topics(&self, limit: usize, last_n: usize) -> Result<Vec<(String, f64)>> {
        let mut per_topic: HashMap<String, Vec<u8>> = HashMap::new();
        let mut stmt = self
            .conn
            .prepare("SELECT topic, grade FROM reviews ORDER BY id DESC")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u8>(1)?)))?;
        for row in rows {
            let (topic, grade) = row?;
            let bucket = per_topic.entry(topic).or_default();
            if bucket.len() < last_n {
                bucket.push(grade);
            }
        }
        let mut averages: Vec<(String, f64)> = per_topic
            .into_iter()
            .filter(|(_, grades)| !grades.is_empty())
            .map(|(topic, grades)| {
                let sum: f64 = grades.iter().map(|&g| f64::from(g)).sum();
                (topic, sum / grades.len() as f64)
            })
            .filter(|(_, average)| *average < 4.0)
            .collect();
        averages.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        averages.truncate(limit);
        Ok(averages)
    }

    pub fn stats(&self, questions: &[Question], now: DateTime<Utc>) -> Result<PrepStats> {
        let due_dates = self.due_dates()?;
        Ok(PrepStats {
            total: questions.len(),
            seen: questions
                .iter()
                .filter(|q| due_dates.contains_key(&q.id))
                .count(),
            due: self.candidates(questions, now, None)?.len(),
        })
    }

    /// Per topic, sorted by topic name. "Due" counts the same cards `stats` and `next_question`
    /// would serve.
    pub fn topic_progress(
        &self,
        questions: &[Question],
        now: DateTime<Utc>,
    ) -> Result<Vec<TopicProgress>> {
        let due_dates = self.due_dates()?;
        let cutoff = iso(now);
        let mut by_topic: BTreeMap<String, TopicProgress> = BTreeMap::new();
        for q in questions {
            let entry = by_topic
                .entry(q.topic.clone())
                .or_insert_with(|| TopicProgress {
                    topic: q.topic.clone(),
                    total: 0,
                    seen: 0,
                    due: 0,
                });
            entry.total += 1;
            match due_dates.get(&q.id) {
                None => entry.due += 1,
                Some(due) => {
                    entry.seen += 1;
                    if *due <= cutoff {
                        entry.due += 1;
                    }
                }
            }
        }
        Ok(by_topic.into_values().collect())
    }

    /// The cards that dossiers added, oldest first.
    pub fn gap_rows(&self) -> Result<Vec<GapRow>> {
        let due_dates = self.due_dates()?;
        let mut stmt = self
            .conn
            .prepare("SELECT id, topic, source FROM extra_cards ORDER BY rowid")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, topic, source) = row?;
            let seen = due_dates.contains_key(&id);
            out.push(GapRow {
                id,
                topic,
                source,
                seen,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prep::load_questions;
    use crate::testing::NOW;

    fn question(id: &str, topic: &str) -> Question {
        Question {
            id: id.into(),
            topic: topic.into(),
            question: format!("Pregunta {id}"),
            answer: format!("Respuesta {id}"),
        }
    }

    fn bank() -> Vec<Question> {
        vec![
            question("q1", "javascript"),
            question("q2", "javascript"),
            question("q3", "sql"),
        ]
    }

    #[test]
    fn the_file_database_runs_in_wal_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prep.db");
        let _store = PrepStore::open(&path).unwrap();
        let conn = Connection::open(&path).unwrap();
        let mode: String = conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn next_question_unseen_in_order_then_due_first() {
        let store = PrepStore::in_memory().unwrap();
        let qs = bank();
        let now = *NOW;
        assert_eq!(
            store.next_question(&qs, now, None).unwrap().unwrap().id,
            "q1"
        );
        store.grade(&qs[0], 5, now).unwrap(); // due tomorrow
        assert_eq!(
            store.next_question(&qs, now, None).unwrap().unwrap().id,
            "q2"
        );
        store.grade(&qs[1], 5, now).unwrap();
        store.grade(&qs[2], 5, now).unwrap();
        assert!(store.next_question(&qs, now, None).unwrap().is_none());
        let later = now + TimeDelta::days(2);
        assert_eq!(
            store.next_question(&qs, later, None).unwrap().unwrap().id,
            "q1"
        );
        assert_eq!(
            store
                .next_question(&qs, later, Some("sql"))
                .unwrap()
                .unwrap()
                .id,
            "q3"
        );
    }

    #[test]
    fn due_reviews_come_before_unseen_questions() {
        let store = PrepStore::in_memory().unwrap();
        let qs = bank();
        store.grade(&qs[2], 5, *NOW).unwrap(); // q3 is due again after one day
        let later = *NOW + TimeDelta::days(2);
        assert_eq!(
            store.next_question(&qs, later, None).unwrap().unwrap().id,
            "q3"
        );
    }

    #[test]
    fn weak_topics_and_stats() {
        let store = PrepStore::in_memory().unwrap();
        let qs = bank();
        let now = *NOW;
        store.grade(&qs[0], 1, now).unwrap();
        store.grade(&qs[1], 2, now).unwrap();
        store.grade(&qs[2], 5, now).unwrap();
        assert_eq!(
            store.weak_topics(3, 10).unwrap(),
            vec![("javascript".to_string(), 1.5)]
        );
        assert_eq!(
            store.stats(&qs, now).unwrap(),
            PrepStats {
                total: 3,
                seen: 3,
                due: 0
            }
        );
        assert_eq!(store.stats(&qs, now + TimeDelta::days(2)).unwrap().due, 3);
    }

    #[test]
    fn weak_topics_only_look_at_the_last_reviews() {
        let store = PrepStore::in_memory().unwrap();
        let qs = bank();
        for _ in 0..3 {
            store.grade(&qs[0], 0, *NOW).unwrap();
        }
        for _ in 0..3 {
            store.grade(&qs[0], 5, *NOW).unwrap();
        }
        // The three old failures fall outside the window of the last 3 reviews.
        assert!(store.weak_topics(3, 3).unwrap().is_empty());
        assert_eq!(
            store.weak_topics(3, 6).unwrap(),
            vec![("javascript".to_string(), 2.5)]
        );
    }

    #[test]
    fn grade_outside_zero_to_five_is_rejected() {
        let store = PrepStore::in_memory().unwrap();
        let qs = bank();
        assert!(store.grade(&qs[0], 6, *NOW).is_err());
        assert_eq!(
            store.stats(&qs, *NOW).unwrap().seen,
            0,
            "a rejected grade changes nothing"
        );
    }

    #[test]
    fn progress_survives_reopening_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prep.db");
        let qs = bank();
        PrepStore::open(&path)
            .unwrap()
            .grade(&qs[0], 4, *NOW)
            .unwrap();
        let reopened = PrepStore::open(&path).unwrap();
        assert_eq!(reopened.stats(&qs, *NOW).unwrap().seen, 1);
    }

    fn extra(id: &str, topic: &str) -> Question {
        Question {
            id: id.to_string(),
            topic: topic.to_string(),
            question: format!("question of {id}"),
            answer: format!("answer of {id}"),
        }
    }

    #[test]
    fn extra_cards_are_added_once_and_listed_after_the_bank() {
        let store = PrepStore::in_memory().unwrap();
        let bank = load_questions().unwrap().len();
        let cards = [
            extra("gap-a-1", "kubernetes"),
            extra("gap-a-2", "terraform"),
        ];
        assert_eq!(store.add_cards(&cards, "greenhouse:a", *NOW).unwrap(), 2);
        assert_eq!(
            store.add_cards(&cards, "greenhouse:a", *NOW).unwrap(),
            0,
            "second time adds nothing"
        );
        let all = store.all_questions().unwrap();
        assert_eq!(all.len(), bank + 2);
        assert_eq!(all[bank].id, "gap-a-1");
        assert_eq!(all[bank + 1].id, "gap-a-2");
    }

    #[test]
    fn an_extra_card_cannot_replace_a_bank_card() {
        let store = PrepStore::in_memory().unwrap();
        let bank = load_questions().unwrap();
        let mut imposter = extra(&bank[0].id, "kubernetes");
        imposter.answer = "wrong".to_string();
        store.add_cards(&[imposter], "x", *NOW).unwrap();
        let all = store.all_questions().unwrap();
        assert_eq!(all.len(), bank.len());
        assert_eq!(all[0], bank[0]);
    }

    #[test]
    fn extra_cards_are_served_graded_and_counted_like_any_other() {
        let store = PrepStore::in_memory().unwrap();
        store
            .add_cards(&[extra("gap-a-1", "kubernetes")], "x", *NOW)
            .unwrap();
        let all = store.all_questions().unwrap();
        let next = store
            .next_question(&all, *NOW, Some("kubernetes"))
            .unwrap()
            .unwrap();
        assert_eq!(next.id, "gap-a-1");
        store.grade(next, 1, *NOW).unwrap();
        let weak = store.weak_topics(3, 10).unwrap();
        assert_eq!(weak[0].0, "kubernetes");
        assert!(
            store
                .next_question(&all, *NOW, Some("kubernetes"))
                .unwrap()
                .is_none(),
            "graded 1 is due tomorrow"
        );
    }

    #[test]
    fn topic_progress_counts_total_seen_and_due_per_topic() {
        let store = PrepStore::in_memory().unwrap();
        let qs = bank(); // q1, q2: javascript; q3: sql
        store.grade(&qs[0], 5, *NOW).unwrap(); // due tomorrow
        assert_eq!(
            store.topic_progress(&qs, *NOW).unwrap(),
            vec![
                TopicProgress {
                    topic: "javascript".into(),
                    total: 2,
                    seen: 1,
                    due: 1 // q2 was never seen, q1 is not due yet
                },
                TopicProgress {
                    topic: "sql".into(),
                    total: 1,
                    seen: 0,
                    due: 1
                },
            ]
        );
        let later = store
            .topic_progress(&qs, *NOW + TimeDelta::days(2))
            .unwrap();
        assert_eq!(later[0].due, 2, "q1 has come due");
    }

    #[test]
    fn gap_rows_link_each_extra_card_to_its_posting_and_say_if_it_was_answered() {
        let store = PrepStore::in_memory().unwrap();
        store
            .add_cards(
                &[
                    extra("gap-a-1", "kubernetes"),
                    extra("gap-a-2", "terraform"),
                ],
                "greenhouse:a",
                *NOW,
            )
            .unwrap();
        let all = store.all_questions().unwrap();
        let answered = all.iter().find(|q| q.id == "gap-a-1").unwrap().clone();
        store.grade(&answered, 3, *NOW).unwrap();
        assert_eq!(
            store.gap_rows().unwrap(),
            vec![
                GapRow {
                    id: "gap-a-1".into(),
                    topic: "kubernetes".into(),
                    source: "greenhouse:a".into(),
                    seen: true
                },
                GapRow {
                    id: "gap-a-2".into(),
                    topic: "terraform".into(),
                    source: "greenhouse:a".into(),
                    seen: false
                },
            ]
        );
        assert!(
            PrepStore::in_memory()
                .unwrap()
                .gap_rows()
                .unwrap()
                .is_empty()
        );
    }
}
