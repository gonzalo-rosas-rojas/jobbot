//! Claude's account: the gate every Claude command goes through, the runner, and the
//! `claude-check` and `claude-login` commands that verify the login.

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};

use super::{Deps, EXIT_ERROR, EXIT_OK, open_tracker};
use crate::claude::{
    ClaudeConfig, ClaudeGate, ClaudeRequest, ClaudeRunner, CliClaude, TESTED_CLAUDE_VERSION,
    is_account_error, load_claude_config,
};
use crate::paths::Paths;
use crate::tracker::Tracker;

/// The gate every Claude command goes through: the day's budget and the cap on one call come
/// from `claude.toml`.
pub(super) fn claude_gate<'a>(
    runner: &'a dyn ClaudeRunner,
    tracker: &'a Tracker,
    config: &ClaudeConfig,
) -> ClaudeGate<'a> {
    ClaudeGate::new(runner, tracker, config.daily_budget_usd)
        .with_per_call_cap(config.per_call_budget_usd)
}

/// The injected runner (tests) or the real `claude.exe`, started in the empty `data/claude-cwd`.
pub(super) fn claude_runner(
    paths: &Paths,
    deps: &Deps,
    config: &ClaudeConfig,
) -> Result<Arc<dyn ClaudeRunner>> {
    if let Some(injected) = &deps.claude {
        return Ok(injected.clone());
    }
    let cli = CliClaude::locate(config.clone(), paths.claude_work.clone())?;
    Ok(Arc::new(cli))
}

pub(super) async fn cmd_claude_check(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let config = load_claude_config(&paths.profile_dir.join("claude.toml"))?;
    let tracker = open_tracker(paths)?;
    let runner = claude_runner(paths, deps, &config)?;
    if deps.claude.is_none() {
        // Only the real program has a path and a version to report.
        let real = CliClaude::locate(config.clone(), paths.claude_work.clone())?;
        writeln!(out, "Programa: {}", real.program().display())?;
        match real.version().await {
            Some(v) if v == TESTED_CLAUDE_VERSION => writeln!(out, "Versión: {v} (la probada)")?,
            Some(v) => writeln!(
                out,
                "Aviso: versión {v}, la probada es {TESTED_CLAUDE_VERSION}. Si algo falla, \
                 revisa que las banderas de claude -p sigan siendo las mismas."
            )?,
            None => writeln!(out, "Aviso: no pude leer la versión de claude.")?,
        }
    }
    let gate = claude_gate(runner.as_ref(), &tracker, &config);
    let seen = gate.check_account(now).await?;
    writeln!(out, "Cuenta: {}", seen.describe())?;
    print_account_warnings(&seen, out)?;
    if let Some(note) = unanchored_note(&config) {
        writeln!(out, "{note}")?;
    }
    let request = ClaudeRequest {
        stage: "check".to_string(),
        system: "You are a connectivity check. Answer in one word.".to_string(),
        prompt: "Reply with exactly the word OK and nothing else.".to_string(),
        schema: None,
    };
    let reply = gate.ask(&request, now).await?;
    writeln!(
        out,
        "Respuesta: {} · costo US${:.4}",
        reply.text.trim(),
        reply.cost_usd
    )?;
    let spent = tracker.claude_spend_since(now - TimeDelta::hours(24))?;
    writeln!(
        out,
        "Gastado en 24 h: US${spent:.2} de US${:.2}",
        config.daily_budget_usd
    )?;
    Ok(EXIT_OK)
}

/// The line that says, in red-flag words, that nothing fixes which account is the right one:
/// `None` when `expected_email` and `expected_org_id` are both set.
pub(super) fn unanchored_note(config: &ClaudeConfig) -> Option<String> {
    (!config.is_anchored()).then(|| {
        "Aviso: cuenta sin anclar. profile/claude.local.toml no fija expected_email y \
         expected_org_id, así que cualquier sesión Pro o Max en la carpeta de jobbot pasa la \
         verificación, sea de quien sea. Fíjalos (jobbot claude-login te dice cómo)."
            .to_string()
    })
}

/// One `Aviso:` line per warning the preflight attached to the account (a managed policy found on
/// this PC). They never stop a command: Gonza reads them, the verdict stays what it was.
pub(super) fn print_account_warnings(
    seen: &crate::claude::AccountSeen,
    out: &mut dyn Write,
) -> Result<()> {
    for warning in &seen.warnings {
        writeln!(out, "Aviso: {warning}")?;
    }
    Ok(())
}

/// One-time setup: `claude auth login` into this program's own config folder, then the same
/// preflight every run makes, so Gonza sees at once whether the login is the private one. With
/// an injected runner (tests) only the preflight runs: nothing else can open a browser.
pub(super) async fn cmd_claude_login(
    paths: &Paths,
    deps: &Deps,
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let config = load_claude_config(&paths.profile_dir.join("claude.toml"))?;
    let tracker = open_tracker(paths)?;
    let runner = claude_runner(paths, deps, &config)?;
    if deps.claude.is_none() {
        let real = CliClaude::locate(config.clone(), paths.claude_work.clone())?;
        writeln!(out, "{}", login_notice(&config, real.config_dir()))?;
        real.login().await?;
    }
    let gate = claude_gate(runner.as_ref(), &tracker, &config);
    match gate.check_account(now).await {
        Ok(seen) => {
            writeln!(out, "Sesión verificada. Cuenta: {}", seen.describe())?;
            print_account_warnings(&seen, out)?;
            if !config.is_anchored() {
                // Not in claude.toml: that file is committed and the repository may be published.
                writeln!(
                    out,
                    "Aviso: cuenta sin anclar. Sugerencia: para que cada corrida exija exactamente esta cuenta, crea \
                     profile/claude.local.toml (git lo ignora; no pongas el correo en \
                     claude.toml, que se publica con el repositorio) con:\n  \
                     expected_email = \"tu correo personal\"\n  expected_org_id = \"{}\"",
                    if seen.org_id.is_empty() {
                        "el orgId de `claude auth status`"
                    } else {
                        &seen.org_id
                    }
                )?;
            }
            Ok(EXIT_OK)
        }
        Err(err) => {
            writeln!(out, "La sesión no sirve: {err:#}")?;
            if is_account_error(&err) {
                // A login that cannot be used must not stay on this PC: close it, with the same
                // clean environment and folder that opened it.
                match runner.logout().await {
                    Ok(()) => writeln!(
                        out,
                        "Cerré esa sesión (claude auth logout) para no dejar guardadas las \
                         credenciales de una cuenta que jobbot no puede usar. Repite \
                         jobbot claude-login en una ventana privada del navegador."
                    )?,
                    Err(e) => writeln!(
                        out,
                        "No pude cerrar esa sesión ({e}). Ciérrala a mano antes de seguir: borra \
                         la carpeta de la sesión de jobbot (la que dice más arriba; por defecto \
                         %LOCALAPPDATA%\\jobbot\\claude-config)."
                    )?,
                }
            }
            Ok(EXIT_ERROR)
        }
    }
}

/// What `claude-login` says before the browser opens: whose account to choose, and that the
/// browser must not be one where the session of an employer is open (the sign-in page would offer
/// it, and one careless click would authorize Claude Code for that organization from this PC).
/// The address is shown masked: it is only there so Gonza knows which account to pick.
fn login_notice(config: &ClaudeConfig, config_dir: &Path) -> String {
    let which = match config.expected_email.expose().trim() {
        "" => "Elige tu cuenta personal (y tu organización personal, si te pregunta).".to_string(),
        email => format!(
            "Elige la cuenta {} (y tu organización personal, si te pregunta).",
            crate::privacy::mask_email(email)
        ),
    };
    format!(
        "Iniciando sesión en el directorio propio de jobbot: {}\n\
         ATENCIÓN: abre el enlace en una VENTANA PRIVADA del navegador (o en un navegador donde \
         NO esté abierta la sesión de claude.ai de una empresa ni de un cliente). Si se abre \
         sola con esa sesión, ciérrala y copia el enlace a la ventana privada.\n{which}",
        config_dir.display()
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::cli::test_support::{
        busy_now, cli, dossier_file, fake_deps, open_now, plain, seed_replied, temp_root,
        tracker_of, with_claude, write_claude_toml, write_cv_master,
    };
    use crate::testing::{FakeClaude, SAMPLE_CV, make_job, sample_dossier_json};

    #[tokio::test]
    async fn claude_check_asks_once_and_reports_the_spend() {
        let root = temp_root();
        let fake = Arc::new(FakeClaude::new(vec![FakeClaude::text("OK", 0.0123)]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-check"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Respuesta: OK") && out.contains("US$0.0123"),
            "{out}"
        );
        assert!(out.contains("de US$1.50"), "{out}");
        assert_eq!(fake.requests().len(), 1);
        let ledger = tracker_of(root.path())
            .claude_calls_by_stage(open_now() - TimeDelta::hours(1))
            .unwrap();
        assert_eq!(ledger["check"].0, 1);
    }

    #[tokio::test]
    async fn claude_check_respects_stop_and_the_window() {
        let root = temp_root();
        let fake = Arc::new(FakeClaude::new(vec![]));
        let (code, _) = cli(
            root.path(),
            busy_now(),
            with_claude(fake_deps(vec![]).0, &fake),
            &["claude-check"],
        )
        .await;
        assert_eq!(code, 4);
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        let (code, _) = cli(
            root.path(),
            open_now(),
            with_claude(fake_deps(vec![]).0, &fake),
            &["claude-check"],
        )
        .await;
        assert_eq!(code, 3);
        assert!(
            fake.requests().is_empty(),
            "no call may be made while blocked"
        );
    }

    #[tokio::test]
    async fn claude_check_says_when_claude_is_not_there() {
        let root = temp_root();
        let nowhere = root.path().join("nowhere").join("claude.exe");
        write_claude_toml(
            root.path(),
            &format!("program = {:?}\n", nowhere.display().to_string()),
        );
        let (code, out) = cli(
            root.path(),
            open_now(),
            fake_deps(vec![]).0,
            &["claude-check"],
        )
        .await;
        assert_eq!(code, 1);
        assert!(out.contains("no se encontró claude"), "{out}");
    }

    #[tokio::test]
    async fn claude_check_stops_when_the_budget_is_spent() {
        let root = temp_root();
        write_claude_toml(root.path(), "daily_budget_usd = 0.01\n");
        tracker_of(root.path())
            .record_claude_call("dossier", open_now(), 0.02, 1, 1, true)
            .unwrap();
        let fake = Arc::new(FakeClaude::new(vec![FakeClaude::text("OK", 0.0)]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-check"]).await;
        assert_eq!(code, 1);
        assert!(out.contains("presupuesto diario"), "{out}");
        assert!(fake.requests().is_empty());
    }

    /// The JSON `claude_account_last` of the tracker, as the panel and the digest will read it.
    fn account_record(root: &Path) -> serde_json::Value {
        let text = tracker_of(root)
            .get_meta("claude_account_last")
            .unwrap()
            .expect("the preflight was recorded");
        serde_json::from_str(&text).unwrap()
    }

    #[tokio::test]
    async fn claude_check_prints_the_account_the_calls_use_and_records_it() {
        let root = temp_root();
        let fake = Arc::new(FakeClaude::new(vec![FakeClaude::text("OK", 0.01)]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-check"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Cuenta: método claude.ai · proveedor firstParty · suscripción max")
                && out.contains("correo t***@example.com · org org-test · directorio"),
            "{out}"
        );
        let record = account_record(root.path());
        assert_eq!(record["ok"], serde_json::json!(true));
        assert_eq!(record["cli_version"], serde_json::json!("2.1.284"));
        assert_eq!(
            tracker_of(root.path())
                .get_meta("claude_account_last_check")
                .unwrap()
                .as_deref(),
            Some(open_now().to_rfc3339().as_str())
        );
    }

    #[tokio::test]
    async fn claude_check_stops_before_the_call_when_the_login_is_not_private() {
        let root = temp_root();
        let fake = Arc::new(
            FakeClaude::new(vec![FakeClaude::text("OK", 0.01)])
                .refusing_account("la credencial activa es api_key, no la suscripción personal"),
        );
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-check"]).await;
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("es api_key"), "{out}");
        assert!(fake.requests().is_empty(), "no prompt may go out");
        assert_eq!(account_record(root.path())["ok"], serde_json::json!(false));
    }

    #[tokio::test]
    async fn run_daily_fails_before_any_prompt_when_the_login_is_not_private() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed_replied(root.path(), &make_job());
        let fake = Arc::new(
            FakeClaude::new(vec![FakeClaude::structured(sample_dossier_json(), 0.05)])
                .refusing_account("la cuenta privada de jobbot no tiene sesión"),
        );
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;
        assert_eq!(code, 1, "{out}");
        assert!(
            out.contains("Claude no se usó") && out.contains("no tiene sesión"),
            "{out}"
        );
        assert!(fake.requests().is_empty(), "run-daily never called -p");
        assert!(!dossier_file(root.path(), "test:1").exists());
        // The digest is still written: it is what Gonza reads to learn why the run stopped.
        assert!(
            root.path()
                .join("data")
                .join("digest-2026-10-14.md")
                .exists()
        );
        let record = account_record(root.path());
        assert_eq!(record["ok"], serde_json::json!(false));
        assert!(
            record["message"]
                .as_str()
                .unwrap()
                .contains("no tiene sesión")
        );
    }

    #[tokio::test]
    async fn claude_login_with_an_injected_runner_only_verifies_the_login() {
        let root = temp_root();
        let fake = Arc::new(FakeClaude::new(vec![]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-login"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("Sesión verificada") && out.contains("suscripción max"),
            "{out}"
        );
        // The shipped file leaves the address and the org empty: say how to pin them, in the
        // git-ignored local file with the orgId already filled in, never in the committed one.
        assert!(
            out.contains("profile/claude.local.toml")
                && out.contains("expected_email = ")
                && out.contains("expected_org_id = \"org-test\""),
            "{out}"
        );
        assert!(
            !out.contains("de profile/claude.toml"),
            "claude.toml is committed: {out}"
        );
        assert_eq!(account_record(root.path())["ok"], serde_json::json!(true));

        let refused = Arc::new(FakeClaude::new(vec![]).refusing_account("cuenta equivocada"));
        let deps = with_claude(fake_deps(vec![]).0, &refused);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-login"]).await;
        assert_eq!(code, 1, "{out}");
        assert!(
            out.contains("La sesión no sirve") && out.contains("cuenta equivocada"),
            "{out}"
        );
    }

    #[tokio::test]
    async fn status_claude_check_and_login_say_when_the_account_is_not_anchored() {
        let root = temp_root();
        let fake = Arc::new(FakeClaude::new(vec![FakeClaude::text("OK", 0.0)]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, check) = cli(root.path(), open_now(), deps, &["claude-check"]).await;
        assert_eq!(code, 0, "{check}");
        let (_, status) = plain(root.path(), open_now(), &["status"]).await;
        let fake = Arc::new(FakeClaude::new(vec![]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (_, login) = cli(root.path(), open_now(), deps, &["claude-login"]).await;
        for (what, out) in [
            ("claude-check", &check),
            ("status", &status),
            ("claude-login", &login),
        ] {
            assert!(
                out.contains("cuenta sin anclar") && out.contains("claude.local.toml"),
                "{what}: {out}"
            );
        }

        // Both keys fixed in the local file: nothing to warn about.
        std::fs::write(
            root.path().join("profile").join("claude.local.toml"),
            "expected_email = \"me@example.com\"\nexpected_org_id = \"org-test\"\n",
        )
        .unwrap();
        let fake = Arc::new(FakeClaude::new(vec![FakeClaude::text("OK", 0.0)]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, check) = cli(root.path(), open_now(), deps, &["claude-check"]).await;
        assert_eq!(code, 0, "{check}");
        let (_, status) = plain(root.path(), open_now(), &["status"]).await;
        for (what, out) in [("claude-check", &check), ("status", &status)] {
            assert!(!out.contains("sin anclar"), "{what}: {out}");
        }
    }

    #[tokio::test]
    async fn a_login_the_preflight_refuses_is_closed_so_no_credentials_stay_behind() {
        let root = temp_root();
        let refused = Arc::new(FakeClaude::new(vec![]).refusing_account("cuenta equivocada"));
        let deps = with_claude(fake_deps(vec![]).0, &refused);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-login"]).await;
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("cuenta equivocada"), "{out}");
        assert_eq!(refused.logouts(), 1, "{out}");
        assert!(
            out.contains("Cerré esa sesión") && out.contains("ventana privada"),
            "{out}"
        );

        // A login that passes is left alone.
        let accepted = Arc::new(FakeClaude::new(vec![]));
        let deps = with_claude(fake_deps(vec![]).0, &accepted);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-login"]).await;
        assert_eq!(code, 0, "{out}");
        assert_eq!(accepted.logouts(), 0, "{out}");
        assert!(!out.contains("Cerré esa sesión"), "{out}");
    }

    #[test]
    fn the_notice_before_the_browser_opens_asks_for_a_private_window_and_never_prints_the_address()
    {
        let plain = login_notice(
            &ClaudeConfig::default(),
            Path::new("C:/jobbot/claude-config"),
        );
        assert!(plain.contains("ventana privada"), "{plain}");
        assert!(plain.contains("C:/jobbot/claude-config"), "{plain}");
        assert!(plain.contains("empresa"), "the employer is named: {plain}");

        let fixed = login_notice(
            &ClaudeConfig {
                expected_email: "me@example.com".into(),
                ..ClaudeConfig::default()
            },
            Path::new("C:/jobbot/claude-config"),
        );
        assert!(fixed.contains("ventana privada"), "{fixed}");
        assert!(fixed.contains("m***@example.com"), "{fixed}");
        assert!(!fixed.contains("me@example.com"), "{fixed}");
    }

    #[tokio::test]
    async fn a_managed_policy_is_a_warning_in_claude_check_login_and_status_never_a_refusal() {
        let root = temp_root();
        let note = "hay una política administrada de prueba";
        let fake = Arc::new(FakeClaude::new(vec![FakeClaude::text("OK", 0.0)]).warning_about(note));

        // claude-check: the verdict is the same, the warning is one more line.
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-check"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains(&format!("Aviso: {note}")), "{out}");
        assert!(
            out.contains("Respuesta: OK"),
            "the call still went out: {out}"
        );

        // It is kept with the verdict, so `status` can show it later without asking again.
        assert_eq!(
            account_record(root.path())["warnings"],
            serde_json::json!([note])
        );
        let (_, out) = plain(root.path(), open_now(), &["status"]).await;
        assert!(
            out.contains("Cuenta de Claude: verificada") && out.contains(&format!("Aviso: {note}")),
            "{out}"
        );

        // claude-login prints it too, and still succeeds.
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), open_now(), deps, &["claude-login"]).await;
        assert_eq!(code, 0, "{out}");
        assert!(out.contains(&format!("Aviso: {note}")), "{out}");
    }

    #[tokio::test]
    async fn run_daily_prints_the_policy_warning_when_it_asks_claude_for_dossiers() {
        let root = temp_root();
        write_cv_master(root.path(), SAMPLE_CV);
        seed_replied(root.path(), &make_job());
        let note = "hay una política administrada de prueba";
        let fake = Arc::new(
            FakeClaude::new(vec![FakeClaude::structured(sample_dossier_json(), 0.05)])
                .warning_about(note),
        );
        let deps = with_claude(fake_deps(vec![]).0, &fake);

        let (code, out) = cli(root.path(), open_now(), deps, &["run-daily"]).await;

        assert_eq!(code, 0, "{out}");
        assert!(out.contains(&format!("Aviso: {note}")), "{out}");
        assert!(dossier_file(root.path(), "test:1").exists());
    }

    #[tokio::test]
    async fn claude_login_stops_suggesting_once_the_local_file_pins_the_account() {
        let root = temp_root();
        std::fs::write(
            root.path().join("profile").join("claude.local.toml"),
            "expected_email = \"me@example.com\"\nexpected_org_id = \"org-test\"\n",
        )
        .unwrap();
        let fake = Arc::new(FakeClaude::new(vec![]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);

        let (code, out) = cli(root.path(), open_now(), deps, &["claude-login"]).await;

        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Sesión verificada"), "{out}");
        assert!(!out.contains("Sugerencia"), "the keys are set: {out}");
        assert!(
            !out.contains("me@example.com"),
            "the full address is never printed: {out}"
        );
    }

    #[tokio::test]
    async fn claude_login_ignores_stop_and_the_window() {
        let root = temp_root();
        std::fs::write(root.path().join("STOP"), "x").unwrap();
        let fake = Arc::new(FakeClaude::new(vec![]));
        let deps = with_claude(fake_deps(vec![]).0, &fake);
        let (code, out) = cli(root.path(), busy_now(), deps, &["claude-login"]).await;
        assert_eq!(code, 0, "{out}");
    }
}
