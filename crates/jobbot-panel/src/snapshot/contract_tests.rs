//! The contract with the page: `panel-src/test/fixtures/snapshot-keys.json` lists every key the
//! front reads, and these tests check that the snapshot has each of them, with and without data.

use chrono::TimeDelta;

use super::support_tests::{email, seen_at, task, use_sample_criteria};
use super::*;
use crate::fixtures::{add, panel};
use jobbot_app::ports::store::EmailKind;
use jobbot_testkit::NOW;

/// The names of the fields of a JSON object, sorted.
fn keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    keys
}

/// True when `path` ("a.b[].c") exists in `node`; `[]` means "in at least one element" and a key
/// that holds `null` exists. The same walk as `has()` in `panel-src/test/helpers/contract.mjs`.
fn has_path(node: &serde_json::Value, parts: &[&str]) -> bool {
    let Some((head, rest)) = parts.split_first() else {
        return true;
    };
    if let Some(name) = head.strip_suffix("[]") {
        return node
            .get(name)
            .and_then(serde_json::Value::as_array)
            .is_some_and(|list| list.iter().any(|item| has_path(item, rest)));
    }
    node.as_object().is_some_and(|map| map.contains_key(*head)) && has_path(&node[*head], rest)
}

/// The paths of the front's contract that `snapshot` lacks: `always`, plus `withCards` when
/// asked, plus `whenAccount` when `controls.claude_account` is there and `whenTrust` when
/// `trust` is. A section listed in `optional` must be there and may be null (it could not be
/// built): then the paths inside it are not asked for.
pub(super) fn missing_contract_paths(
    snapshot: &serde_json::Value,
    with_cards: bool,
) -> Vec<String> {
    let contract: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../panel-src/test/fixtures/snapshot-keys.json"
    ))
    .expect("snapshot-keys.json is JSON");
    let list = |group: &str| -> Vec<String> {
        contract[group]
            .as_array()
            .unwrap_or_else(|| panic!("`{group}` is a list"))
            .iter()
            .map(|path| path.as_str().expect("a string").to_string())
            .collect()
    };
    let mut groups = vec!["always"];
    if with_cards {
        groups.push("withCards");
    }
    if snapshot["controls"]["claude_account"].is_object() {
        groups.push("whenAccount");
    }
    if snapshot["trust"].is_object() {
        groups.push("whenTrust");
    }
    let optional = list("optional");
    let absent = optional
        .iter()
        .filter(|section| !has_path(snapshot, &[section.as_str()]))
        .cloned();
    let inside_an_empty_section = |path: &str| {
        let head = path.split('.').next().unwrap_or("").trim_end_matches("[]");
        optional.iter().any(|section| section == head) && snapshot[head].is_null()
    };
    let lacking = groups
        .iter()
        .flat_map(|group| list(group))
        .filter(|path| !inside_an_empty_section(path))
        .filter(|path| !has_path(snapshot, &path.split('.').collect::<Vec<_>>()));
    absent.chain(lacking).collect()
}

#[test]
fn an_empty_snapshot_has_every_key_the_front_relies_on() {
    let (panel, _dir) = panel();
    let json = serde_json::to_value(build_snapshot(&panel).unwrap()).unwrap();
    assert_eq!(missing_contract_paths(&json, false), Vec::<String>::new());
    // The walk can fail: a key the snapshot does not have is reported. (Through a list it is
    // checked in the test with cards: every column here is empty, so any path through one fails.)
    assert!(!has_path(&json, &["controls", "no_such_key"]));
    assert!(!has_path(&json, &["board", "no_such_key"]));
    // The two keys that say nothing until the engine and Windows have spoken are there, empty.
    for path in [
        "controls.claude_account",
        "controls.scheduled_task",
        "controls.last_run_result",
        "controls.last_run_steps",
        "controls.auto_approve_min",
    ] {
        assert!(
            has_path(&json, &path.split('.').collect::<Vec<_>>())
                && json["controls"][path.rsplit('.').next().unwrap()].is_null(),
            "{path} is null, and present"
        );
    }
    assert_eq!(json["activity"]["moves_by_state"], serde_json::json!({}));
    assert_eq!(json["funnel"]["by_source"], serde_json::json!({}));
}

#[test]
fn a_snapshot_with_data_has_every_key_the_front_relies_on_down_to_the_cards() {
    use crate::scheduler::{FixedTask, TaskAnswer};
    let (panel, _dir) = panel();
    let panel = panel.with_task_query(FixedTask(Some(TaskAnswer::Registered {
        next_run_at: Some(*NOW + TimeDelta::hours(1)),
    })));
    panel.tasks.settle();
    add(&panel, "1", 80.0, &[State::Prepared]);
    add(&panel, "2", 80.0, &[State::Prepared, State::NeedsGonza]);
    seen_at(&panel, "3", 60.0, *NOW - TimeDelta::hours(1), Some(&[]));
    task(&panel, "Llamar", Some(*NOW + TimeDelta::hours(1)), None);
    email(
        &panel,
        "m1",
        EmailKind::Recruiter,
        *NOW - TimeDelta::hours(1),
        "Hola",
    );
    let set = |key: &str, value: &str| panel.tracker.set_meta(key, value).unwrap();
    set(
        "claude_account_last",
        r#"{"auth_method":"claude.ai","api_provider":"firstParty","subscription":"max","email":"d***@example.com","org_id":"org-0000","config_dir":"C:/Users/ana/AppData/Local/jobbot/claude-config","cli_version":"2.1.0","ok":true,"message":null,"checked_at":"2026-10-01T09:00:00+00:00"}"#,
    );
    set("last_run_at", &(*NOW - TimeDelta::hours(7)).to_rfc3339());
    set("last_run_result", "ok");
    set("last_run_steps", r#"{"discover":{"ran":true,"new":1}}"#);

    let json = serde_json::to_value(build_snapshot(&panel).unwrap()).unwrap();

    assert_eq!(missing_contract_paths(&json, true), Vec::<String>::new());
    // The walk through a list finds a key the cards have and reports one they do not.
    assert!(has_path(&json, &["board[]", "cards[]", "key"]));
    assert!(!has_path(&json, &["board[]", "cards[]", "no_such_key"]));
    // The sub-keys the front checks on the account and on the task (`ACCOUNT_KEYS`, `TASK_KEYS`
    // of `panel-src/test/helpers/contract.mjs`); null in an empty snapshot, so only here.
    assert_eq!(
        keys(&json["controls"]["claude_account"]),
        [
            "anchored",
            "api_provider",
            "auth_method",
            "checked_at",
            "cli_version",
            "config_dir",
            "email",
            "message",
            "ok",
            "org_id",
            "subscription"
        ]
    );
    assert_eq!(
        keys(&json["controls"]["scheduled_task"]),
        ["exists", "next_run_at"]
    );
    assert_eq!(json["controls"]["last_run_result"], "ok");
    assert_eq!(json["activity"]["moves_by_state"]["scored"], 3);
}

#[test]
fn the_new_fields_are_named_the_way_the_page_reads_them() {
    let (mut panel, dir) = panel();
    use_sample_criteria(&mut panel, dir.path());
    seen_at(&panel, "1", 80.0, *NOW, Some(&[]));

    let json = serde_json::to_value(build_snapshot(&panel).unwrap()).unwrap();

    assert_eq!(
        keys(&json["pipeline"]),
        [
            "above",
            "complete",
            "days",
            "score_counts",
            "seen",
            "sent",
            "threshold",
            "with_cv"
        ]
    );
    assert_eq!(
        keys(&json["trust"]),
        [
            "cap",
            "cap_after",
            "first_n",
            "review_phase",
            "reviewed",
            "sent_24h"
        ]
    );
    assert_eq!(
        keys(&json["activity"]),
        [
            "from",
            "moves",
            "moves_by_state",
            "moves_total",
            "slot_minutes",
            "slots",
            "to"
        ]
    );
    assert_eq!(keys(&json["activity"]["slots"][0]), ["at", "open", "seen"]);
    assert_eq!(
        keys(&json["controls"]),
        [
            "auto_approve_min",
            "claude_account",
            "engine",
            "last_run_at",
            "last_run_result",
            "last_run_steps",
            "next_window",
            "scheduled_task",
            "stopped",
            "window_open"
        ]
    );
    assert_eq!(
        keys(&json["funnel"]),
        [
            "answered",
            "by_origin",
            "by_region",
            "by_source",
            "interviews",
            "offers",
            "sent",
            "tests"
        ]
    );
    assert_eq!(keys(&json["snoozed"]), ["cards", "total"]);
    assert_eq!(
        keys(&json["board"][0]["cards"][0]),
        [
            "actions",
            "age_days",
            "approved",
            "company",
            "days_in_state",
            "first_seen",
            "has_cv",
            "has_dossier",
            "has_handoff",
            "has_screenshot",
            "key",
            "marks",
            "note",
            "posted_at",
            "prune_in_days",
            "reason",
            "reason_kind",
            "reason_short",
            "salary_max",
            "salary_min",
            "score",
            "snoozed_until",
            "source",
            "state",
            "title",
            "undo",
            "updated_at",
            "url",
            "waiting_since"
        ]
    );
    assert_eq!(
        keys(&json["activity"]["moves"][0]),
        ["at", "company", "key", "title", "to"]
    );
    assert_eq!(
        json["pipeline"]["score_counts"].as_array().unwrap().len(),
        101
    );
    assert_eq!(json["activity"]["slots"].as_array().unwrap().len(), 48);
}
