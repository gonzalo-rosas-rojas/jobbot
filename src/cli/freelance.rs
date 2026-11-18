//! Freelance drafts: the profile and the proposal Claude writes for a platform. Nothing is sent.

use std::io::Write;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use super::claude::{ClaudeSetup, print_account_warnings, read_cv_master};
use super::{Deps, EXIT_ERROR, EXIT_OK, FreelanceCommand, open_tracker};
use crate::claude::load_claude_config;
use crate::freelance::{draft_profile, draft_proposal, load_freelance_config};
use crate::paths::Paths;
use crate::rules::load_exclusions;

pub(super) async fn cmd_freelance(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    command: FreelanceCommand,
    out: &mut dyn Write,
) -> Result<i32> {
    let config = load_claude_config(&paths.profile_dir.join("claude.toml"))?;
    let freelance = load_freelance_config(&paths.profile_dir.join("freelance.toml"))?;
    let exclusions = load_exclusions(&paths.profile_dir.join("exclusions.toml"))?;
    let platform = match &command {
        FreelanceCommand::Profile(a) => a.platform.clone(),
        FreelanceCommand::Proposal(a) => a.platform.clone(),
    };
    // Say what is missing before reading anything else or touching Claude.
    if let Err(reason) = freelance.check_ready(&platform) {
        writeln!(out, "{reason}")?;
        return Ok(EXIT_ERROR);
    }
    let setup = ClaudeSetup::with(paths, deps, config, open_tracker(paths)?)?;
    let gate = setup.gate();
    // A managed policy on the account is shown, as in `run-daily` and `dossier`.
    let seen = gate.check_account(now).await?;
    print_account_warnings(&seen, out)?;
    let cv_md = read_cv_master(paths)?;
    let dir = paths.data_dir.join("freelance");
    let path = match command {
        FreelanceCommand::Profile(_) => {
            draft_profile(&gate, &freelance, &exclusions, &cv_md, &platform, &dir, now).await?
        }
        FreelanceCommand::Proposal(a) => {
            let posting = std::fs::read_to_string(&a.job_file)
                .with_context(|| format!("leyendo {}", a.job_file.display()))?;
            let name = a
                .job_file
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            draft_proposal(
                &gate,
                &freelance,
                &exclusions,
                &cv_md,
                &platform,
                &posting,
                &name,
                &dir,
                now,
            )
            .await?
        }
    };
    writeln!(out, "Borrador listo: {}", path.display())?;
    writeln!(
        out,
        "Es un borrador: revísalo y publícalo tú; el programa no envía nada a ninguna plataforma."
    )?;
    Ok(EXIT_OK)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use crate::cli::test_support::{
        busy_now, cli, fake_deps, open_now, temp_root, with_claude, write_cv_master,
    };
    use crate::testing::{FakeClaude, SAMPLE_CV};

    fn write_freelance_toml(root: &Path, text: &str) {
        std::fs::write(root.join("profile").join("freelance.toml"), text).unwrap();
    }

    fn profile_reply() -> Arc<FakeClaude> {
        Arc::new(FakeClaude::new(vec![FakeClaude::structured(
            serde_json::json!({
                "headline": "AI engineer", "bio": "I build LLM agents with Python.",
                "skills": ["Python", "Kubernetes"], "hourly_rate_usd": 20.0,
                "portfolio_ideas": [{"title": "RAG demo", "description": "Built from scratch."}]
            }),
            0.04,
        )]))
    }

    fn freelance_file(root: &Path, name: &str) -> PathBuf {
        root.join("data").join("freelance").join(name)
    }

    #[tokio::test]
    async fn freelance_refuses_until_platforms_and_a_rate_are_defined() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        let fake = profile_reply();
        let go = || with_claude(fake_deps(vec![]).0, &fake);
        // The shipped file has no platforms.
        let (code, out) = cli(
            root.path(),
            open_now(),
            go(),
            &["freelance", "profile", "upwork"],
        )
        .await;
        assert_eq!(code, 1);
        assert!(out.contains("Falta definir `platforms`"), "{out}");
        write_freelance_toml(root.path(), "platforms = [\"upwork\"]\n");
        let (code, out) = cli(
            root.path(),
            open_now(),
            go(),
            &["freelance", "profile", "upwork"],
        )
        .await;
        assert_eq!(code, 1);
        assert!(out.contains("Falta definir `min_hourly_usd`"), "{out}");
        assert!(fake.requests().is_empty(), "nothing reached Claude");
    }

    #[tokio::test]
    async fn freelance_profile_writes_a_draft_and_says_nothing_was_sent() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        write_freelance_toml(
            root.path(),
            "platforms = [\"upwork\"]\nmin_hourly_usd = 35\n",
        );
        let fake = profile_reply();
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(
            root.path(),
            open_now(),
            deps,
            &["freelance", "profile", "upwork"],
        )
        .await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Borrador listo") && out.contains("no envía nada"),
            "{out}"
        );
        let md =
            std::fs::read_to_string(freelance_file(root.path(), "upwork-profile-2026-10-14.md"))
                .unwrap();
        assert!(
            md.contains("US$35") && md.contains("descartada") && !md.contains("Kubernetes"),
            "{md}"
        );
        for secret in ["Harbor Light", "jane.doe@example.com", "+56"] {
            assert!(!fake.requests()[0].prompt.contains(secret), "{secret}");
        }
    }

    #[tokio::test]
    async fn freelance_shows_the_managed_policy_warning_of_the_account() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        write_freelance_toml(
            root.path(),
            "platforms = [\"upwork\"]\nmin_hourly_usd = 35\n",
        );
        let note = "hay una política administrada de prueba";
        let fake = Arc::new(
            FakeClaude::new(vec![FakeClaude::structured(
                serde_json::json!({
                    "headline": "AI engineer", "bio": "I build LLM agents with Python.",
                    "skills": ["Python"], "hourly_rate_usd": 20.0,
                    "portfolio_ideas": [{"title": "RAG demo", "description": "Built."}]
                }),
                0.04,
            )])
            .warning_about(note),
        );
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(
            root.path(),
            open_now(),
            deps,
            &["freelance", "profile", "upwork"],
        )
        .await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains(&format!("Aviso: {note}")), "{out}");
    }

    #[tokio::test]
    async fn freelance_proposal_reads_the_posting_file_and_refuses_excluded_ones() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        write_freelance_toml(
            root.path(),
            "platforms = [\"upwork\"]\nmin_hourly_usd = 35\n",
        );
        let good = root.path().join("rag-job.txt");
        std::fs::write(&good, "Need a RAG engineer for a small internal tool.").unwrap();
        let bad = root.path().join("freight-job.txt");
        std::fs::write(&bad, "Dashboard for a freight forwarding desk.").unwrap();
        let fake = Arc::new(FakeClaude::new(vec![FakeClaude::structured(
            serde_json::json!({"proposal": "I would start with a spike.", "questions_for_client": ["Scope?"]}),
            0.03,
        )]));
        let go = || with_claude(fake_deps(vec![]).0, &fake);
        let bad_path = bad.display().to_string();
        let good_path = good.display().to_string();
        let (code, out) = cli(
            root.path(),
            open_now(),
            go(),
            &["freelance", "proposal", "upwork", "--job-file", &bad_path],
        )
        .await;
        assert_eq!(code, 1);
        assert!(
            out.contains("El aviso cae en la lista de exclusión"),
            "{out}"
        );
        assert!(
            fake.requests().is_empty(),
            "an excluded posting never reaches Claude"
        );
        let (code, out) = cli(
            root.path(),
            open_now(),
            go(),
            &["freelance", "proposal", "upwork", "--job-file", &good_path],
        )
        .await;
        assert_eq!(code, 0, "{out}");
        assert!(freelance_file(root.path(), "proposal-upwork-2026-10-14-rag-job.md").exists());
    }

    #[tokio::test]
    async fn two_freelance_drafts_the_same_day_do_not_overwrite_each_other() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        write_freelance_toml(
            root.path(),
            "platforms = [\"upwork\"]\nmin_hourly_usd = 35\n",
        );
        for _ in 0..2 {
            let fake = profile_reply();
            let deps = with_claude(fake_deps(vec![]).0, &fake);
            assert_eq!(
                cli(
                    root.path(),
                    open_now(),
                    deps,
                    &["freelance", "profile", "upwork"]
                )
                .await
                .0,
                0
            );
        }
        assert!(freelance_file(root.path(), "upwork-profile-2026-10-14.md").exists());
        assert!(freelance_file(root.path(), "upwork-profile-2026-10-14-2.md").exists());
    }

    #[tokio::test]
    async fn freelance_commands_respect_stop_and_the_window() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        write_freelance_toml(
            root.path(),
            "platforms = [\"upwork\"]\nmin_hourly_usd = 35\n",
        );
        let fake = profile_reply();
        let go = || with_claude(fake_deps(vec![]).0, &fake);
        assert_eq!(
            cli(
                root.path(),
                busy_now(),
                go(),
                &["freelance", "profile", "upwork"]
            )
            .await
            .0,
            4
        );
        assert_eq!(
            cli(
                root.path(),
                busy_now(),
                go(),
                &["freelance", "profile", "upwork", "--ignore-window"]
            )
            .await
            .0,
            0
        );
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        assert_eq!(
            cli(
                root.path(),
                open_now(),
                go(),
                &["freelance", "profile", "upwork", "--ignore-window"]
            )
            .await
            .0,
            3
        );
    }
}
