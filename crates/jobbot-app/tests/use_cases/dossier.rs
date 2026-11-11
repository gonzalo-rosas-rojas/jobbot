//! The `dossier` use cases against the real SQLite stores, the fake Claude and the shipped
//! profile (moved from the root crate's `src/dossier.rs`).

use serde_json::json;

use jobbot_app::claude::ClaudeGate;
use jobbot_app::claude::is_budget_error;
use jobbot_app::learn::dossier::*;
use jobbot_app::profile::claude::ClaudeConfig;
use jobbot_app::profile::load_exclusions;
use jobbot_domain::models::Job;
use jobbot_domain::outbound::{key_slug, redact_contact};
use jobbot_testkit::PrepStore;
use jobbot_testkit::Tracker;
use jobbot_testkit::{
    FakeClaude, NOW, SAMPLE_CV, make_job, make_job_with, real_paths, sample_dossier_json,
};

fn raw(value: serde_json::Value) -> RawDossier {
    serde_json::from_value(value).unwrap()
}
struct Env {
    tracker: Tracker,
    store: PrepStore,
    config: ClaudeConfig,
    dir: tempfile::TempDir,
}
fn env() -> Env {
    Env {
        tracker: Tracker::in_memory().unwrap(),
        store: PrepStore::in_memory().unwrap(),
        config: ClaudeConfig::default(),
        dir: tempfile::tempdir().unwrap(),
    }
}
async fn build(e: &Env, fake: &FakeClaude, job: &Job) -> anyhow::Result<DossierOutcome> {
    let gate = ClaudeGate::new(fake, &e.tracker, e.config.daily_budget_usd);
    let exclusions = load_exclusions(&real_paths().profile_dir.join("exclusions.toml")).unwrap();
    build_dossier(
        &gate,
        &e.store,
        job,
        SAMPLE_CV,
        &exclusions,
        e.dir.path(),
        *NOW,
    )
    .await
}
#[test]
fn only_stories_with_evidence_in_the_cv_survive() {
    let d = assemble(raw(sample_dossier_json()), &redact_contact(SAMPLE_CV));
    let titles: Vec<&str> = d.stories.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(titles, ["MCP server", "Job system"]);
    assert_eq!(d.dropped_stories, 1);
    assert_eq!(d.questions.len(), 1, "the empty question is dropped");
    let topics: Vec<&str> = d.gaps.iter().map(|g| g.topic.as_str()).collect();
    assert_eq!(topics, ["kubernetes-operators", "gap"]);
}
#[test]
fn the_markdown_is_in_spanish_and_complete() {
    let d = assemble(raw(sample_dossier_json()), SAMPLE_CV);
    let md = render_markdown(&make_job(), &d, *NOW);
    for part in [
        "# Dossier — Acme AI · Senior AI Engineer",
        "## Qué dice el aviso de la empresa",
        "## Preguntas probables",
        "## Historias (solo hechos de tu CV maestro)",
        "Evidencia en el CV: «",
        "1 historia(s) descartada(s)",
        "## Huecos a estudiar",
        "jobbot prep next --topic kubernetes-operators",
        "2026-10-01",
    ] {
        assert!(md.contains(part), "missing {part:?} in:\n{md}");
    }
}
#[test]
fn card_ids_come_from_the_slugged_key() {
    let d = assemble(raw(sample_dossier_json()), SAMPLE_CV);
    let cards = gap_cards("greenhouse:acme/42", &d);
    let base = key_slug("greenhouse:acme/42");
    assert!(base.starts_with("greenhouse-acme-42-"), "{base}");
    assert_eq!(cards[0].id, format!("gap-{base}-1"));
    assert_eq!(cards[1].id, format!("gap-{base}-2"));
    assert_eq!(cards[0].topic, "kubernetes-operators");
}
#[tokio::test]
async fn a_dossier_is_written_with_its_cards_and_the_call_is_recorded() {
    let e = env();
    let fake = FakeClaude::new(vec![FakeClaude::structured(sample_dossier_json(), 0.05)]);
    let outcome = build(&e, &fake, &make_job()).await.unwrap();
    let path = dossier_path(e.dir.path(), "test:1");
    assert_eq!(
        outcome,
        DossierOutcome::Written {
            path: path.clone(),
            stories: 2,
            dropped: 1,
            cards: 2
        }
    );
    let md = std::fs::read_to_string(&path).unwrap();
    assert!(
        md.contains("MCP server") && !md.contains("Kubernetes operators at Google"),
        "{md}"
    );
    let asked = fake.requests();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].stage, "dossier");
    assert!(asked[0].schema.is_some());
    assert!(!asked[0].prompt.contains('@') && !asked[0].prompt.contains("+56"));
    assert!(
        !asked[0].prompt.contains("Harbor Light")
            && !asked[0].prompt.contains("Fractional Operations"),
        "the current employer's entry never reaches Claude"
    );
    let spent = e
        .tracker
        .claude_spend_since(*NOW - chrono::TimeDelta::hours(1))
        .unwrap();
    assert!((spent - 0.05).abs() < 1e-9);
    let ids: Vec<String> = e
        .store
        .all_questions()
        .unwrap()
        .into_iter()
        .map(|q| q.id)
        .collect();
    let card_id = |n: u32| format!("gap-{}-{n}", key_slug("test:1"));
    assert!(ids.contains(&card_id(1)) && ids.contains(&card_id(2)));
}
#[tokio::test]
async fn a_story_built_on_the_current_employer_is_dropped_because_it_was_never_sent() {
    let e = env();
    let mut reply = sample_dossier_json();
    reply["stories"][0]["evidence"] = json!(
        "Design and build internal workflow-automation and reporting software for a multi-office healthcare practice"
    );
    let fake = FakeClaude::new(vec![FakeClaude::structured(reply, 0.05)]);
    let outcome = build(&e, &fake, &make_job()).await.unwrap();
    // Of the three stories in the reply only "Job system" has evidence in the text Claude saw.
    assert!(
        matches!(
            outcome,
            DossierOutcome::Written {
                stories: 1,
                dropped: 2,
                ..
            }
        ),
        "{outcome:?}"
    );
}
#[tokio::test]
async fn an_existing_dossier_is_not_asked_again() {
    let e = env();
    let fake = FakeClaude::new(vec![FakeClaude::structured(sample_dossier_json(), 0.05)]);
    build(&e, &fake, &make_job()).await.unwrap();
    let second = build(&e, &fake, &make_job()).await.unwrap();
    assert!(matches!(second, DossierOutcome::AlreadyThere(_)));
    assert_eq!(fake.requests().len(), 1);
}
#[tokio::test]
async fn rebuilding_a_dossier_does_not_duplicate_its_cards() {
    let e = env();
    let fake = FakeClaude::new(vec![
        FakeClaude::structured(sample_dossier_json(), 0.05),
        FakeClaude::structured(sample_dossier_json(), 0.05),
    ]);
    build(&e, &fake, &make_job()).await.unwrap();
    let total = e.store.all_questions().unwrap().len();
    std::fs::remove_file(dossier_path(e.dir.path(), "test:1")).unwrap();
    let again = build(&e, &fake, &make_job()).await.unwrap();
    assert!(
        matches!(again, DossierOutcome::Written { cards: 0, .. }),
        "{again:?}"
    );
    assert_eq!(e.store.all_questions().unwrap().len(), total);
}
#[tokio::test]
async fn an_empty_reply_writes_nothing() {
    let e = env();
    let empty = json!({"company_summary": "", "likely_questions": [], "stories": [], "gaps": []});
    let fake = FakeClaude::new(vec![FakeClaude::structured(empty, 0.01)]);
    let err = build(&e, &fake, &make_job()).await.unwrap_err();
    assert!(err.to_string().contains("vacío"), "{err:#}");
    assert!(!dossier_path(e.dir.path(), "test:1").exists());
    assert_eq!(
        e.store.all_questions().unwrap().len(),
        jobbot_domain::prep::load_questions().unwrap().len()
    );
}
#[tokio::test]
async fn a_reply_with_the_wrong_shape_is_an_error_not_a_panic() {
    let e = env();
    let fake = FakeClaude::new(vec![FakeClaude::structured(
        json!({"stories": "not a list"}),
        0.01,
    )]);
    let err = build(&e, &fake, &make_job()).await.unwrap_err();
    assert!(format!("{err:#}").contains("forma esperada"), "{err:#}");
    assert!(!dossier_path(e.dir.path(), "test:1").exists());
}
#[tokio::test]
async fn a_reply_with_no_structured_output_is_read_from_its_text() {
    let e = env();
    let fake = FakeClaude::new(vec![FakeClaude::text(
        &sample_dossier_json().to_string(),
        0.05,
    )]);
    assert!(matches!(
        build(&e, &fake, &make_job()).await.unwrap(),
        DossierOutcome::Written { stories: 2, .. }
    ));
    let fake = FakeClaude::new(vec![FakeClaude::text("sorry, no JSON here", 0.05)]);
    let job = make_job_with(|j| j.external_id = "2".into());
    let err = build(&e, &fake, &job).await.unwrap_err();
    assert!(format!("{err:#}").contains("no es JSON"), "{err:#}");
}
#[tokio::test]
async fn hostile_text_is_cleaned_and_capped() {
    let e = env();
    let mut reply = sample_dossier_json();
    reply["stories"][0]["title"] = json!(format!("\u{7}bell{}", "T".repeat(500)));
    reply["gaps"][0]["answer"] = json!("A".repeat(10_000));
    let fake = FakeClaude::new(vec![FakeClaude::structured(reply, 0.05)]);
    build(&e, &fake, &make_job()).await.unwrap();
    let md = std::fs::read_to_string(dossier_path(e.dir.path(), "test:1")).unwrap();
    assert!(!md.contains('\u{7}') && md.len() < 20_000, "{}", md.len());
    let card = e
        .store
        .all_questions()
        .unwrap()
        .into_iter()
        .find(|q| q.id == format!("gap-{}-1", key_slug("test:1")))
        .unwrap();
    assert!(
        card.answer.chars().count() <= 1501,
        "{}",
        card.answer.chars().count()
    );
}
#[tokio::test]
async fn the_budget_error_passes_through_and_nothing_is_written() {
    let mut e = env();
    e.config.daily_budget_usd = 0.01;
    e.tracker
        .record_claude_call("dossier", *NOW, 0.02, 1, 1, true)
        .unwrap();
    let fake = FakeClaude::new(vec![FakeClaude::structured(sample_dossier_json(), 0.05)]);
    let err = build(&e, &fake, &make_job()).await.unwrap_err();
    assert!(is_budget_error(&err));
    assert!(fake.requests().is_empty());
    assert!(!dossier_path(e.dir.path(), "test:1").exists());
}
