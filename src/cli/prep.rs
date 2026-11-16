//! Interview preparation: the spaced-repetition drill (`prep next`, `reveal`, `grade`, `stats`).

use std::io::Write;

use anyhow::Result;
use chrono::{DateTime, Utc};

use super::{EXIT_ERROR, EXIT_OK, PrepCommand};
use crate::actions;
use crate::paths::Paths;
use crate::prep::PrepStore;

pub(super) fn open_prep_store(paths: &Paths) -> Result<PrepStore> {
    std::fs::create_dir_all(&paths.data_dir)?;
    Ok(PrepStore::open(&paths.data_dir.join("prep.db"))?)
}

/// The interview drill. Claude runs the mock interview in the chat: `next` to pick a question,
/// `reveal` to compare Gonza's answer with the reference, `grade` to record the score.
/// Study commands ignore the STOP file and the execution window: they send nothing anywhere.
pub(super) fn cmd_prep(
    paths: &Paths,
    command: PrepCommand,
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let store = open_prep_store(paths)?;
    let questions = store.all_questions()?;
    match command {
        PrepCommand::Next { topic } => {
            match store.next_question(&questions, now, topic.as_deref())? {
                Some(q) => writeln!(out, "{} [{}]\n{}", q.id, q.topic, q.question)?,
                None => writeln!(out, "Estás al día: no hay preguntas pendientes.")?,
            }
        }
        PrepCommand::Reveal { id } => match questions.iter().find(|q| q.id == id) {
            Some(q) => writeln!(out, "{}", q.answer)?,
            None => {
                writeln!(out, "Pregunta desconocida: {id}")?;
                return Ok(EXIT_ERROR);
            }
        },
        PrepCommand::Grade { id, grade } => {
            let outcome = actions::grade_card(&store, &questions, &id, grade, now)?;
            writeln!(out, "{}", outcome.message)?;
            if !outcome.ok {
                return Ok(EXIT_ERROR);
            }
        }
        PrepCommand::Stats => {
            let stats = store.stats(&questions, now)?;
            writeln!(
                out,
                "Vistas: {}/{} · Pendientes hoy: {}",
                stats.seen, stats.total, stats.due
            )?;
            for (topic, average) in store.weak_topics(3, 10)? {
                writeln!(out, "Tema débil: {topic} (promedio {average:.1})")?;
            }
        }
    }
    Ok(EXIT_OK)
}

#[cfg(test)]
mod tests {
    use crate::cli::test_support::{busy_now, open_now, plain, temp_root};

    #[tokio::test]
    async fn prep_flow_next_reveal_grade_stats() {
        let root = temp_root();
        let (code, out) = plain(root.path(), open_now(), &["prep", "next"]).await;
        assert_eq!(code, 0, "{out}");
        // The first line is "<id> [<topic>]".
        let qid = out
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .to_string();

        let (code, out) = plain(root.path(), open_now(), &["prep", "reveal", qid.as_str()]).await;
        assert_eq!(code, 0);
        assert!(!out.trim().is_empty());

        let (code, out) = plain(
            root.path(),
            open_now(),
            &["prep", "grade", qid.as_str(), "4"],
        )
        .await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("nota 4 registrada"), "{out}");

        // Grades outside 0-5 are a usage error: clap rejects them before anything is stored.
        let (code, _) = plain(
            root.path(),
            open_now(),
            &["prep", "grade", qid.as_str(), "9"],
        )
        .await;
        assert_eq!(code, 1);

        let (code, out) = plain(root.path(), open_now(), &["prep", "stats"]).await;
        assert_eq!(code, 0);
        assert!(out.contains("Vistas: 1/"), "{out}");
    }

    #[tokio::test]
    async fn prep_next_can_filter_by_topic() {
        let root = temp_root();
        let (code, out) = plain(root.path(), open_now(), &["prep", "next", "--topic", "sql"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.lines().next().unwrap().ends_with("[sql]"), "{out}");
        let (_, out) = plain(
            root.path(),
            open_now(),
            &["prep", "next", "--topic", "no-such-topic"],
        )
        .await;
        assert!(out.contains("Estás al día"), "{out}");
    }

    #[tokio::test]
    async fn prep_unknown_question_is_reported() {
        let root = temp_root();
        let (code, out) = plain(root.path(), open_now(), &["prep", "reveal", "nope"]).await;
        assert_eq!(code, 1);
        assert!(out.contains("Pregunta desconocida: nope"), "{out}");
        let (code, _) = plain(root.path(), open_now(), &["prep", "grade", "nope", "3"]).await;
        assert_eq!(code, 1);
    }

    #[tokio::test]
    async fn prep_stats_lists_weak_topics_and_ignores_stop_and_window() {
        let root = temp_root();
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        // A low grade makes the topic weak. The study commands run even with STOP and in a busy hour.
        let (_, first) = plain(
            root.path(),
            busy_now(),
            &["prep", "next", "--topic", "javascript"],
        )
        .await;
        let qid = first
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .to_string();
        let (code, _) = plain(
            root.path(),
            busy_now(),
            &["prep", "grade", qid.as_str(), "1"],
        )
        .await;
        assert_eq!(code, 0);
        let (code, out) = plain(root.path(), busy_now(), &["prep", "stats"]).await;
        assert_eq!(code, 0);
        assert!(
            out.contains("Tema débil: javascript (promedio 1.0)"),
            "{out}"
        );
    }
}
