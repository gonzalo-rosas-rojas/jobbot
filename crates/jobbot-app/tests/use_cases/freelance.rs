//! The `freelance` use cases against the real SQLite stores, the fake Claude and the shipped
//! profile (moved from the root crate's `src/freelance.rs`).

use serde_json::{Value, json};

use jobbot_app::claude::ClaudeGate;
use jobbot_app::freelance::*;
use jobbot_app::profile::claude::ClaudeConfig;
use jobbot_app::profile::load_exclusions;
use jobbot_domain::rules::Exclusions;
use jobbot_testkit::Tracker;
use jobbot_testkit::{FakeClaude, NOW, SAMPLE_CV, real_paths};

fn exclusions() -> Exclusions {
    load_exclusions(&real_paths().profile_dir.join("exclusions.toml")).unwrap()
}
fn ready() -> FreelanceConfig {
    FreelanceConfig {
        platforms: vec!["upwork".into()],
        min_hourly_usd: 35.0,
        never_name: vec![],
    }
}
fn profile_json() -> Value {
    json!({
        "headline": "AI engineer who ships LLM agents",
        "bio": "I build LLM agents and RAG systems with Python and TypeScript.",
        "skills": ["Python", "python", "Kubernetes", "TypeScript"],
        "hourly_rate_usd": 20.0,
        "portfolio_ideas": [{"title": "Local RAG demo", "description": "A small public RAG demo built from scratch."}]
    })
}
fn raw_profile(value: Value) -> RawProfile {
    serde_json::from_value(value).unwrap()
}
#[test]
fn the_shipped_file_parses_and_is_deliberately_not_ready() {
    let shipped = load_freelance_config(&real_paths().profile_dir.join("freelance.toml")).unwrap();
    assert!(shipped.platforms.is_empty() && shipped.never_name.is_empty());
    assert!(shipped.check_ready("upwork").is_err());
}
#[test]
fn never_name_joins_the_exclusion_list_as_names() {
    let config = FreelanceConfig {
        never_name: vec!["DeskMate".into()],
        ..ready()
    };
    let rules = config.guard_rules(&exclusions());
    assert!(
        rules.names.contains(&"DeskMate".to_string())
            && rules.names.contains(&"Harbor Light".to_string())
    );
    assert_eq!(rules.categories, exclusions().categories);
    assert_eq!(
        forbidden_in("I built DeskMate", &rules).as_deref(),
        Some("name:DeskMate")
    );
    assert_eq!(forbidden_in("I built a SaaS", &rules), None);
}
#[test]
fn skills_missing_from_the_cv_are_dropped_and_duplicates_collapse() {
    let cv = jobbot_domain::outbound::strip_excluded_entries(
        &jobbot_domain::outbound::redact_contact(SAMPLE_CV),
        &exclusions(),
    );
    let draft =
        assemble_profile(raw_profile(profile_json()), &cv, &ready(), &exclusions()).unwrap();
    assert_eq!(draft.skills, ["Python", "TypeScript"]);
    assert_eq!(draft.dropped_skills, 1, "Kubernetes is not in the CV");
}
#[test]
fn a_skill_must_be_a_whole_word_of_the_cv_not_a_piece_of_another_word() {
    let cv = "Stack: MongoDB, PostgreSQL, Google Cloud Run, Node.js, C#";
    let mut json = profile_json();
    json["skills"] = json!(["Go", "R", "Rust", "Node.js", "C#", "Cloud Run"]);
    let draft = assemble_profile(raw_profile(json), cv, &ready(), &exclusions()).unwrap();
    assert_eq!(draft.skills, ["Node.js", "C#", "Cloud Run"]);
    assert_eq!(
        draft.dropped_skills, 3,
        "Go sits inside MongoDB, R inside Run, Rust nowhere"
    );
}
#[test]
fn a_rate_below_the_minimum_is_raised_and_flagged_and_a_wild_one_is_capped() {
    let cv = SAMPLE_CV;
    let low = assemble_profile(raw_profile(profile_json()), cv, &ready(), &exclusions()).unwrap();
    assert_eq!((low.rate_usd, low.rate_raised), (35.0, true));
    let mut ok = profile_json();
    ok["hourly_rate_usd"] = json!(60.0);
    let fine = assemble_profile(raw_profile(ok), cv, &ready(), &exclusions()).unwrap();
    assert_eq!((fine.rate_usd, fine.rate_raised), (60.0, false));
    let mut wild = profile_json();
    wild["hourly_rate_usd"] = json!(100000.0);
    let capped = assemble_profile(raw_profile(wild), cv, &ready(), &exclusions()).unwrap();
    assert_eq!(capped.rate_usd, 300.0);
    let missing = assemble_profile(
        raw_profile(json!({"headline": "x", "bio": "y"})),
        cv,
        &ready(),
        &exclusions(),
    )
    .unwrap();
    assert_eq!(
        (missing.rate_usd, missing.rate_raised),
        (35.0, true),
        "no rate means the minimum"
    );
}
#[test]
fn a_draft_that_touches_the_exclusion_list_is_rejected() {
    for (field, text) in [
        ("headline", "I automate Harbor Light dashboards"),
        ("bio", "I specialise in remote fleet tracking platforms."),
        ("bio", "Freight forwarding desk workflows are my thing."),
    ] {
        let mut json = profile_json();
        json[field] = json!(text);
        let err =
            assemble_profile(raw_profile(json), SAMPLE_CV, &ready(), &exclusions()).unwrap_err();
        assert!(
            err.to_string().contains("lista de exclusión"),
            "{text}: {err}"
        );
    }
    let mut idea = profile_json();
    idea["portfolio_ideas"][0]["description"] =
        json!("A dashboard like the ones at Tidewater Systems");
    assert!(assemble_profile(raw_profile(idea), SAMPLE_CV, &ready(), &exclusions()).is_err());
}
#[test]
fn hostile_text_is_cleaned_and_capped_in_a_profile() {
    let mut json = profile_json();
    json["bio"] = json!(format!("\u{7}{}", "b".repeat(5000)));
    json["portfolio_ideas"] = json!(
        (0..20)
            .map(|i| json!({"title": format!("t{i}"), "description": "d"}))
            .collect::<Vec<_>>()
    );
    let draft = assemble_profile(raw_profile(json), SAMPLE_CV, &ready(), &exclusions()).unwrap();
    assert!(!draft.bio.contains('\u{7}') && draft.bio.chars().count() <= 1501);
    assert_eq!(draft.portfolio.len(), 5);
}
#[test]
fn a_proposal_is_cleaned_capped_and_checked() {
    let ok = assemble_proposal(
            serde_json::from_value(json!({"proposal": "I would start with a spike.", "questions_for_client": ["Scope?", "", "Budget?", "a", "b", "c"]})).unwrap(),
            &exclusions(),
        )
        .unwrap();
    assert_eq!(
        ok.questions.len(),
        4,
        "empty questions go, at most four stay"
    );
    let empty = assemble_proposal(
        serde_json::from_value(json!({"proposal": "  "})).unwrap(),
        &exclusions(),
    );
    assert!(empty.is_err(), "an empty proposal is not a draft");
    let bad = assemble_proposal(
        serde_json::from_value(json!({"proposal": "I did this at Harbor Light."})).unwrap(),
        &exclusions(),
    );
    assert!(bad.unwrap_err().to_string().contains("lista de exclusión"));
}
struct Env {
    tracker: Tracker,
    config: ClaudeConfig,
    dir: tempfile::TempDir,
}
fn env() -> Env {
    Env {
        tracker: Tracker::in_memory().unwrap(),
        config: ClaudeConfig::default(),
        dir: tempfile::tempdir().unwrap(),
    }
}
#[tokio::test]
async fn the_profile_prompt_never_has_the_employer_the_contact_line_or_a_never_name_entry() {
    let e = env();
    let fake = FakeClaude::new(vec![FakeClaude::structured(profile_json(), 0.04)]);
    let gate = ClaudeGate::new(&fake, &e.tracker, e.config.daily_budget_usd);
    let config = FreelanceConfig {
        never_name: vec!["stackwave.io".into()],
        ..ready()
    };
    let path = draft_profile(
        &gate,
        &config,
        &exclusions(),
        SAMPLE_CV,
        "Upwork",
        e.dir.path(),
        *NOW,
    )
    .await
    .unwrap();
    let asked = fake.requests();
    assert_eq!(asked[0].stage, "freelance-profile");
    for secret in [
        "Harbor Light",
        "Fractional Operations",
        "jane.doe@example.com",
        "+56",
        "stackwave.io",
    ] {
        assert!(
            !asked[0].prompt.contains(secret),
            "{secret} leaked into the prompt:\n{}",
            asked[0].prompt
        );
    }
    assert!(
        asked[0].prompt.contains("Local LLM Lab"),
        "the rest of the CV is sent"
    );
    assert!(asked[0].system.contains("Never name any employer"));
    assert_eq!(path.file_name().unwrap(), "upwork-profile-2026-10-01.md");
    let md = std::fs::read_to_string(&path).unwrap();
    for part in [
        "# Perfil freelance — upwork",
        "## Titular",
        "## Bio",
        "## Habilidades",
        "1 habilidad(es) descartada(s)",
        "## Tarifa",
        "US$35",
        "(subida a tu mínimo",
        "## Ideas de portafolio",
    ] {
        assert!(md.contains(part), "missing {part:?}:\n{md}");
    }
    assert!(!md.contains("Kubernetes"));
}
#[tokio::test]
async fn a_rejected_draft_leaves_no_file_but_stays_in_the_ledger() {
    let e = env();
    let mut bad = profile_json();
    bad["bio"] = json!("I build remote fleet tracking dashboards.");
    let fake = FakeClaude::new(vec![FakeClaude::structured(bad, 0.04)]);
    let gate = ClaudeGate::new(&fake, &e.tracker, e.config.daily_budget_usd);
    let err = draft_profile(
        &gate,
        &ready(),
        &exclusions(),
        SAMPLE_CV,
        "upwork",
        e.dir.path(),
        *NOW,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("lista de exclusión"), "{err:#}");
    assert_eq!(std::fs::read_dir(e.dir.path()).unwrap().count(), 0);
    assert!(
        (e.tracker
            .claude_spend_since(*NOW - chrono::TimeDelta::hours(1))
            .unwrap()
            - 0.04)
            .abs()
            < 1e-9
    );
}
#[tokio::test]
async fn a_platform_that_is_not_ready_never_reaches_claude() {
    let e = env();
    let fake = FakeClaude::new(vec![]);
    let gate = ClaudeGate::new(&fake, &e.tracker, e.config.daily_budget_usd);
    let err = draft_profile(
        &gate,
        &ready(),
        &exclusions(),
        SAMPLE_CV,
        "fiverr",
        e.dir.path(),
        *NOW,
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("no está en `platforms`"),
        "{err:#}"
    );
    assert!(fake.requests().is_empty());
}
#[tokio::test]
async fn a_posting_in_an_excluded_category_is_refused_before_claude_is_called() {
    let e = env();
    let fake = FakeClaude::new(vec![]);
    let gate = ClaudeGate::new(&fake, &e.tracker, e.config.daily_budget_usd);
    let posting = "We need a developer for our freight forwarding desk scheduling dashboard.";
    let err = draft_proposal(
        &gate,
        &ready(),
        &exclusions(),
        SAMPLE_CV,
        "upwork",
        posting,
        "job",
        e.dir.path(),
        *NOW,
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("El aviso cae en la lista de exclusión"),
        "{err:#}"
    );
    assert!(fake.requests().is_empty());
    let none = draft_proposal(
        &gate,
        &ready(),
        &exclusions(),
        SAMPLE_CV,
        "upwork",
        "   ",
        "job",
        e.dir.path(),
        *NOW,
    )
    .await;
    assert!(
        none.is_err() && fake.requests().is_empty(),
        "an empty posting is refused too"
    );
}
#[tokio::test]
async fn a_proposal_is_written_from_a_clean_cv_and_a_cut_posting() {
    let e = env();
    let reply = json!({"proposal": "I would start with a one-day spike on your RAG pipeline.", "questions_for_client": ["What is the corpus size?"]});
    let fake = FakeClaude::new(vec![FakeClaude::structured(reply, 0.03)]);
    let gate = ClaudeGate::new(&fake, &e.tracker, e.config.daily_budget_usd);
    let posting = format!("Need a RAG engineer. {}", "x".repeat(100_000));
    let path = draft_proposal(
        &gate,
        &ready(),
        &exclusions(),
        SAMPLE_CV,
        "upwork",
        &posting,
        "Mi Aviso: RAG/1",
        e.dir.path(),
        *NOW,
    )
    .await
    .unwrap();
    let asked = fake.requests();
    assert!(
        asked[0].prompt.len() < 20_000,
        "the posting is cut: {}",
        asked[0].prompt.len()
    );
    assert!(
        !asked[0].prompt.contains("Harbor Light")
            && !asked[0].prompt.contains("jane.doe@example.com")
    );
    assert!(asked[0].system.contains("untrusted"));
    assert_eq!(
        path.file_name().unwrap(),
        "proposal-upwork-2026-10-01-mi-aviso-rag-1.md"
    );
    let md = std::fs::read_to_string(&path).unwrap();
    assert!(
        md.contains("# Propuesta (borrador) — upwork")
            && md.contains("## Preguntas para el cliente"),
        "{md}"
    );
}
