//! Stand-in for `claude.exe`, used only by `tests/claude_cli.rs`. The first argument is the mode.
//! `auth status` and `auth login` are answered too: the modes that start with `auth-` describe a
//! login that is not the private one; every other mode reports a private `max` login in the
//! folder the child was given. With no mode at all (the real runner passes none) `auth` is the
//! first argument and the login is the private one.

use std::io::{Read, Write};

use serde_json::json;

fn reply(result: &str) -> String {
    json!({
        "type": "result", "subtype": "success", "is_error": false, "result": result,
        "total_cost_usd": 0.01, "num_turns": 1,
        "usage": {"input_tokens": 3, "output_tokens": 4}
    })
    .to_string()
}

/// What the child was told its config folder is, with the slashes the real CLI prints.
fn config_dir_seen() -> String {
    std::env::var("CLAUDE_CONFIG_DIR")
        .unwrap_or_default()
        .replace('\\', "/")
}

/// `claude auth status` for `mode`: the JSON and the exit code.
fn auth_status(mode: &str) -> (serde_json::Value, i32) {
    let mut status = json!({
        "loggedIn": true, "authMethod": "claude.ai", "apiProvider": "firstParty",
        "subscriptionType": "max", "email": "tester@example.com", "orgId": "org-test",
        "orgName": "Tester", "configDirectory": config_dir_seen()
    });
    match mode {
        "auth-none" => {
            return (
                json!({"loggedIn": false, "authMethod": "none", "configDirectory": config_dir_seen()}),
                1,
            );
        }
        "auth-api-key" => {
            status["authMethod"] = json!("api_key");
            status["apiKeySource"] = json!("ANTHROPIC_API_KEY");
        }
        "auth-team" => status["subscriptionType"] = json!("team"),
        "auth-other-dir" => status["configDirectory"] = json!("C:/Users/someone/.claude"),
        "auth-other-email" => status["email"] = json!("someone.else@corp.example"),
        _ => {}
    }
    (status, 0)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mode, rest) = match args.first().map(String::as_str) {
        Some("auth") => ("ok".to_string(), &args[..]),
        Some(mode) => (mode.to_string(), &args[1..]),
        None => (String::new(), &args[..]),
    };
    if rest.first().map(String::as_str) == Some("--version") {
        println!("2.1.284 (Claude Code)");
        return;
    }
    if rest.first().map(String::as_str) == Some("auth") {
        match rest.get(1).map(String::as_str) {
            Some("status") => {
                // One line per call in the work folder, so a test can count the preflights.
                if let Ok(mut log) = std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open("auth-status-calls")
                {
                    let _ = writeln!(log, "{mode}");
                }
                if mode == "auth-noisy" {
                    // No JSON, and a whole address in what it does say.
                    eprintln!("no se pudo leer la sesión de persona@corp.example");
                    std::process::exit(1);
                }
                let (status, code) = auth_status(&mode);
                print!("{status}");
                let _ = std::io::stdout().flush();
                std::process::exit(code);
            }
            Some("login") => {
                // The real login writes its credentials in the config folder: leave a trace
                // there so a test can see which folder the login was pointed at.
                let dir = std::env::var("CLAUDE_CONFIG_DIR").unwrap_or_default();
                let _ = std::fs::write(std::path::Path::new(&dir).join("fake-login.txt"), b"ok");
                // What it was asked, so a test can see the flags.
                let _ = std::fs::write(
                    std::path::Path::new(&dir).join("fake-login-args.txt"),
                    rest.join("|"),
                );
                println!("fake login ok");
                return;
            }
            Some("logout") => {
                if mode == "logout-fails" {
                    eprintln!("no se pudo cerrar la sesión de persona@corp.example");
                    std::process::exit(1);
                }
                // The credentials go; a note of where it ran says with which folder.
                let dir = std::env::var("CLAUDE_CONFIG_DIR").unwrap_or_default();
                let _ = std::fs::remove_file(std::path::Path::new(&dir).join("fake-login.txt"));
                let cwd = std::env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                let _ = std::fs::write(
                    std::path::Path::new(&dir).join("fake-logout.txt"),
                    format!("{cwd}|{dir}"),
                );
                println!("fake logout ok");
                return;
            }
            _ => {
                eprintln!("fake_claude: unknown auth command");
                std::process::exit(2);
            }
        }
    }
    if mode.starts_with("auth-") {
        // A `-p` call after a refused preflight is the bug the tests look for.
        let _ = std::fs::write("p-was-called", b"x");
    }
    let mut prompt = String::new();
    if mode != "version" {
        let _ = std::io::stdin().read_to_string(&mut prompt);
    }
    match mode.as_str() {
        "ok" => print!("{}", reply(&format!("echo:{prompt}"))),
        "len" => print!("{}", reply(&format!("len:{}", prompt.len()))),
        "env" => {
            let flag = std::env::var("CLAUDE_CODE_DISABLE_CLAUDE_MDS").unwrap_or_default();
            let cwd = std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            let config_dir = std::env::var("CLAUDE_CONFIG_DIR").unwrap_or_default();
            let telemetry = std::env::var("DISABLE_TELEMETRY").unwrap_or_default();
            print!(
                "{}",
                reply(&format!(
                    "env={flag}|cwd={cwd}|config_dir={config_dir}|telemetry={telemetry}"
                ))
            );
        }
        "args" => print!("{}", reply(&rest.join("|"))),
        "version" => println!("2.1.284 (Claude Code)"),
        "reported" => {
            print!(
                "{}",
                json!({"type": "result", "subtype": "error_max_turns", "is_error": true,
                       "result": "too many turns", "total_cost_usd": 0.02})
            );
            std::process::exit(1);
        }
        "fail" => {
            eprintln!("not logged in");
            std::process::exit(1);
        }
        "garbage" => print!("this is not json"),
        "slow" => std::thread::sleep(std::time::Duration::from_secs(30)),
        other => {
            eprintln!("fake_claude: unknown mode {other}");
            std::process::exit(2);
        }
    }
    let _ = std::io::stdout().flush();
}
