//! Interview drill: a question bank, SM-2 scheduling and a progress
//! store. The bank is compiled into the binary from two TOML files: `questions.toml` (the
//! interview seed questions) and `rust_concepts.toml` (the Rust concept cards).

pub mod sm2;
pub mod store;

use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

pub use sm2::sm2;
pub use store::{GapRow, PrepStats, PrepStore, TopicProgress};

const QUESTIONS: &str = include_str!("questions.toml");
const RUST_CONCEPTS: &str = include_str!("rust_concepts.toml");

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Question {
    pub id: String,
    pub topic: String,
    pub question: String,
    pub answer: String,
}

/// Shape of both TOML files: a list of `[[question]]` tables. A file with no cards yet is valid.
#[derive(Deserialize)]
struct QuestionFile {
    #[serde(default)]
    question: Vec<Question>,
}

fn parse_questions(name: &str, text: &str) -> Result<Vec<Question>> {
    let file: QuestionFile = toml::from_str(text).with_context(|| format!("parsing {name}"))?;
    Ok(file
        .question
        .into_iter()
        .map(|q| Question {
            question: q.question.trim().to_string(),
            answer: q.answer.trim().to_string(),
            ..q
        })
        .collect())
}

/// Parses several `(file name, text)` sources into one bank. Ids must be unique across all of them.
fn combine(sources: &[(&str, &str)]) -> Result<Vec<Question>> {
    let mut all = Vec::new();
    for (name, text) in sources {
        all.extend(parse_questions(name, text)?);
    }
    let mut ids = HashSet::new();
    for q in &all {
        if !ids.insert(q.id.as_str()) {
            bail!("duplicate question id {:?}", q.id);
        }
    }
    Ok(all)
}

/// The whole bank: the 18 interview seed questions followed by the Rust concept cards.
pub fn load_questions() -> Result<Vec<Question>> {
    combine(&[
        ("questions.toml", QUESTIONS),
        ("rust_concepts.toml", RUST_CONCEPTS),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_bank_is_complete_and_unique() {
        let seed = parse_questions("questions.toml", QUESTIONS).unwrap();
        assert_eq!(seed.len(), 18);
        let topics: HashSet<&str> = seed.iter().map(|q| q.topic.as_str()).collect();
        let expected = [
            "javascript",
            "typescript",
            "react",
            "sql",
            "http-auth",
            "system-design",
            "llm-rag",
            "agents-mcp",
            "devops",
        ];
        assert_eq!(topics, HashSet::from(expected));
        assert!(
            seed.iter()
                .all(|q| !q.question.is_empty() && !q.answer.is_empty())
        );

        let all = load_questions().unwrap();
        let ids: HashSet<&str> = all.iter().map(|q| q.id.as_str()).collect();
        assert_eq!(ids.len(), all.len(), "ids are unique across both files");
        assert!(all.len() >= 18);
    }

    #[test]
    fn rust_concept_cards_are_part_of_the_bank() {
        let all = load_questions().unwrap();
        let rust: Vec<&Question> = all.iter().filter(|q| q.topic == "rust").collect();
        assert!(
            rust.len() >= 3,
            "expected at least 3 Rust cards, found {}",
            rust.len()
        );
        assert!(
            rust.iter().all(|q| q.id.starts_with("rust-t")
                && !q.question.is_empty()
                && !q.answer.is_empty())
        );
    }

    #[test]
    fn every_task_of_plan_1b2_left_three_rust_cards() {
        let all = load_questions().unwrap();
        for task in 24..=32 {
            for n in 1..=3 {
                let id = format!("rust-t{task}-{n}");
                let card = all.iter().find(|q| q.id == id);
                assert!(
                    card.is_some_and(|q| q.topic == "rust"
                        && !q.question.is_empty()
                        && !q.answer.is_empty()),
                    "missing or empty card {id}"
                );
            }
        }
    }

    #[test]
    fn duplicate_ids_across_files_are_rejected() {
        let a = "[[question]]\nid = \"x\"\ntopic = \"t\"\nquestion = \"q\"\nanswer = \"a\"\n";
        let err = combine(&[("one.toml", a), ("two.toml", a)])
            .unwrap_err()
            .to_string();
        assert!(err.contains("duplicate question id"), "{err}");
    }

    #[test]
    fn empty_and_broken_files() {
        assert!(combine(&[("empty.toml", "")]).unwrap().is_empty());
        let err = format!(
            "{:#}",
            combine(&[("bad.toml", "[[question]]\nid = 3\n")]).unwrap_err()
        );
        assert!(err.contains("parsing bad.toml"), "{err}");
    }

    #[test]
    fn text_is_trimmed() {
        let text = "[[question]]\nid = \"x\"\ntopic = \"t\"\nquestion = \"  q  \"\nanswer = \"\"\"\n  a\n\"\"\"\n";
        let qs = combine(&[("t.toml", text)]).unwrap();
        assert_eq!((qs[0].question.as_str(), qs[0].answer.as_str()), ("q", "a"));
    }
}
