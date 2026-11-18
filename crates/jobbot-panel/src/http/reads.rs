//! The GET routes that are neither the snapshot nor a document: the tail of the results log and the
//! study cards.

use anyhow::Result;
use serde_json::json;

use super::Reply;
use crate::Panel;
use crate::snapshot::offer_label;
use jobbot_app::apply::results::{read_last, results_path};

/// Lines `/api/results` serves at most.
const MAX_RESULTS: usize = 500;
const DEFAULT_RESULTS: usize = 20;

/// The tail of `data/results.jsonl`, newest last, as `jobbot results --last N` prints it.
pub(super) fn results(panel: &Panel, last: Option<&str>) -> Result<Reply> {
    let last = match last {
        None => DEFAULT_RESULTS,
        Some(text) => match text.trim().parse::<usize>() {
            Ok(n) => n.clamp(1, MAX_RESULTS),
            Err(_) => return Ok(Reply::message(400, "last debe ser un número")),
        },
    };
    let found = read_last(&results_path(&panel.paths.data_dir), last)?;
    Ok(Reply::json(
        200,
        json!({ "total": found.total, "lines": found.lines }),
    ))
}

pub(super) fn learn_next(panel: &Panel, topic: Option<&str>) -> Result<Reply> {
    let questions = panel.store.all_questions()?;
    Ok(
        match panel.store.next_question(&questions, panel.now(), topic)? {
            Some(q) => {
                let offer = panel
                    .store
                    .gap_rows()?
                    .into_iter()
                    .find(|gap| gap.id == q.id)
                    .map(|gap| offer_label(panel, &gap.source));
                Reply::json(
                    200,
                    json!({ "card": { "id": q.id, "topic": q.topic, "question": q.question, "offer": offer } }),
                )
            }
            None => Reply::json(200, json!({ "card": null })),
        },
    )
}

pub(super) fn learn_answer(panel: &Panel, id: &str) -> Result<Reply> {
    let questions = panel.store.all_questions()?;
    Ok(match questions.iter().find(|q| q.id == id) {
        Some(q) => Reply::json(200, json!({ "answer": q.answer })),
        None => Reply::message(404, "Pregunta desconocida"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::support_tests::{encode, guard, json_of, request, text, without};
    use crate::http::{TOKEN_HEADER, handle};
    use chrono::TimeDelta;

    use crate::fixtures::panel;
    use jobbot_domain::state::State;

    #[test]
    fn results_serves_the_tail_of_the_log() {
        let (p, _d) = panel();
        let get = |query: &str| {
            handle(
                &p,
                &guard(),
                &request("GET", &format!("/api/results{query}"), ""),
            )
        };

        let empty = json_of(&get(""));
        assert_eq!(
            (
                empty["total"].as_u64(),
                empty["lines"].as_array().map(Vec::len)
            ),
            (Some(0), Some(0))
        );

        let path = results_path(&p.paths.data_dir);
        // More lines than the default tail, so the default is told from "everything there is".
        for n in 1..=25 {
            let job = jobbot_testkit::make_job_with(|j| j.external_id = n.to_string());
            let line = jobbot_app::apply::results::ResultLine::new(
                &job,
                jobbot_app::apply::results::AttemptMode::DryRun,
                State::Prepared,
                jobbot_domain::handoff::FormTally {
                    filled: Some(4),
                    left: vec!["Pronombres".into()],
                },
                None,
                None,
                *jobbot_testkit::NOW + TimeDelta::minutes(n),
            );
            jobbot_app::apply::results::append_result(&path, &line).unwrap();
        }
        let two = json_of(&get("?last=2"));
        assert_eq!(two["total"], 25);
        let keys: Vec<&str> = two["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["key"].as_str().unwrap())
            .collect();
        assert_eq!(keys, ["test:24", "test:25"], "the newest two, oldest first");
        assert_eq!(two["lines"][0]["mode"], "dry_run");
        assert_eq!(two["lines"][0]["fields_left"][0], "Pronombres");
        assert_eq!(
            json_of(&get(""))["lines"].as_array().unwrap().len(),
            20,
            "the tail is 20 lines by default"
        );
        assert_eq!(
            json_of(&get("?last=0"))["lines"].as_array().unwrap().len(),
            1,
            "held to 1..=500"
        );
        assert_eq!(get("?last=muchos").status, 400);
        assert_eq!(
            handle(&p, &guard(), &request("POST", "/api/results", "{}")).status,
            405
        );
        let no_token = without(request("GET", "/api/results", ""), TOKEN_HEADER);
        assert_eq!(handle(&p, &guard(), &no_token).status, 401);
    }

    #[test]
    fn the_drill_serves_a_card_without_its_answer_then_grades_it() {
        let (p, _d) = panel();
        let next = handle(&p, &guard(), &request("GET", "/api/learn/next", ""));
        assert_eq!(next.status, 200);
        assert!(
            !text(&next).contains("\"answer\""),
            "the answer waits for the reveal"
        );
        let card = json_of(&next)["card"].clone();
        let id = card["id"].as_str().unwrap().to_string();
        assert!(card["question"].as_str().is_some_and(|q| !q.is_empty()));
        let reveal = handle(
            &p,
            &guard(),
            &request("GET", &format!("/api/learn/answer?id={}", encode(&id)), ""),
        );
        assert!(
            json_of(&reveal)["answer"]
                .as_str()
                .is_some_and(|a| !a.is_empty())
        );
        let grade = |id: &str, points: u32| {
            let body = serde_json::json!({ "id": id, "grade": points }).to_string();
            handle(&p, &guard(), &request("POST", "/api/learn/grade", &body))
        };
        assert_eq!(grade(&id, 5).status, 200);
        let after = json_of(&handle(
            &p,
            &guard(),
            &request("GET", "/api/learn/next", ""),
        ));
        assert_ne!(
            after["card"]["id"],
            id.as_str(),
            "a graded card is not served again right away"
        );
        assert_eq!(grade("nope", 3).status, 409);
        assert_eq!(grade(&id, 6).status, 409, "above five is a refusal");
        assert_eq!(
            grade(&id, 300).status,
            400,
            "not even a number of the right size"
        );
        let none = json_of(&handle(
            &p,
            &guard(),
            &request("GET", "/api/learn/next?topic=nope", ""),
        ));
        assert!(none["card"].is_null());
        let snapshot = json_of(&handle(&p, &guard(), &request("GET", "/api/snapshot", "")));
        assert_eq!(snapshot["learn"]["seen"], 1);
    }
}
