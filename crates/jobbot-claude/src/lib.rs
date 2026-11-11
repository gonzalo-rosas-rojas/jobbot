//! Headless Claude (`claude -p`) as a child process: its configuration, the command line, the
//! JSON reply and the runner. This crate never decides what to ask (the use cases build the
//! requests) and keeps no ledger (the application's gate records every call and its cost).
//!
//! It is an adapter: it implements the application's `ClaudeRunner` port and knows the Claude CLI
//! and nothing about the database, the job boards, the browser or the panel
//! (`tests/architecture.rs` of the application enforces that).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use jobbot_app::ports::{AccountSeen, ClaudeError, ClaudeReply, ClaudeRequest, ClaudeRunner};
use jobbot_app::profile::claude::ClaudeConfig;
use jobbot_domain::privacy::{mask_addresses, mask_email};
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// The flags and the reply shape were measured against this version on 2026-10-03.
pub const TESTED_CLAUDE_VERSION: &str = "2.1.284";

/// How long `claude --version` may take before the version is just reported as unknown.
const VERSION_DEADLINE_SECS: u64 = 10;

/// How long `claude auth logout` may take.
const LOGOUT_DEADLINE_SECS: u64 = 30;

/// Environment of every child. Without the first one the user's global CLAUDE.md is loaded into
/// each call (measured: the model quoted its first heading back, and a trivial call used 2,279
/// cache-creation tokens instead of 726). The other three keep the CLI from phoning home: nothing
/// of this program leaves the PC except the call to the model itself, so no usage telemetry, no
/// crash reports (they carry the environment) and no prompt history on disk.
pub const CHILD_ENV: [(&str, &str); 4] = [
    ("CLAUDE_CODE_DISABLE_CLAUDE_MDS", "1"),
    ("DISABLE_TELEMETRY", "1"),
    ("DISABLE_ERROR_REPORTING", "1"),
    ("CLAUDE_CODE_SKIP_PROMPT_HISTORY", "1"),
];

/// Under `%LOCALAPPDATA%`: the Claude config folder that belongs to this program alone. Its login
/// is the private subscription and nothing else (the user's own `~/.claude` may hold whatever
/// account he last used for work).
pub const DEFAULT_CONFIG_DIR: &str = "jobbot\\claude-config";

/// Variables that would silently change who is billed or which login a child uses: an API key
/// (billing the key's owner instead of the subscription), an auth token, a different API endpoint,
/// or another Claude config folder (another account). The scheduled task may run as another
/// Windows user whose environment was never meant to reach this program.
pub const CHILD_ENV_REMOVED: [&str; 4] = [
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "CLAUDE_CONFIG_DIR",
];

/// Whole families of variables that also choose the account or the bill: every `ANTHROPIC_*` (model
/// and provider settings, Foundry), the `CLAUDE_CODE_USE_*` switches that send Claude through
/// Bedrock, Vertex or Foundry (an employer's cloud account), and the `AWS_*` credentials Bedrock
/// reads. Matched in any letter case, as Windows treats names.
const CHILD_ENV_REMOVED_PREFIXES: [&str; 3] = ["ANTHROPIC_", "CLAUDE_CODE_USE_", "AWS_"];

/// Single names with the same effect: a long-lived login token and Vertex's credentials.
const CHILD_ENV_REMOVED_ALSO: [&str; 4] = [
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CONFIG_DIR",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "CLOUD_ML_REGION",
];

/// True for a variable the child must not inherit (see the three constants above).
fn picks_account_or_bill(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    CHILD_ENV_REMOVED_PREFIXES
        .iter()
        .any(|prefix| upper.starts_with(prefix))
        || CHILD_ENV_REMOVED_ALSO.contains(&upper.as_str())
}

/// The config folder a child gets: `config_dir` from `claude.toml`, or the default under
/// `%LOCALAPPDATA%` (given by the caller, so a test needs no real environment). Always absolute.
pub fn resolve_config_dir(
    config: &ClaudeConfig,
    local_appdata: Option<&Path>,
) -> Result<PathBuf, ClaudeError> {
    let raw = config.config_dir.trim();
    if !raw.is_empty() {
        let dir = PathBuf::from(raw);
        if !dir.is_absolute() {
            return Err(ClaudeError::Io(format!(
                "config_dir de profile/claude.toml debe ser una ruta absoluta: {raw}"
            )));
        }
        return Ok(dir);
    }
    local_appdata
        .map(|base| base.join(DEFAULT_CONFIG_DIR))
        .ok_or_else(|| {
            ClaudeError::Io(
                "LOCALAPPDATA no está definida: pon una ruta absoluta en config_dir de \
                 profile/claude.toml"
                    .to_string(),
            )
        })
}

/// The instruction files Claude Code reads from its working folder and from every folder above it.
const INSTRUCTION_FILES: [&str; 2] = ["CLAUDE.md", "CLAUDE.local.md"];

/// Refuses a working folder that has an instruction file in it or above it: Claude Code would read
/// it into every call (finding F4). It happens when the run does not know `%LOCALAPPDATA%` and the
/// folder falls back to `data/claude-cwd` inside the project.
pub fn check_work_dir(work_dir: &Path) -> Result<(), ClaudeError> {
    let absolute = std::path::absolute(work_dir).unwrap_or_else(|_| work_dir.to_path_buf());
    let found = absolute
        .ancestors()
        .flat_map(|dir| INSTRUCTION_FILES.iter().map(move |name| dir.join(name)))
        .find(|file| file.is_file());
    match found {
        Some(file) => Err(ClaudeError::Io(format!(
            "la carpeta de trabajo de claude ({}) tiene encima {}, y sus instrucciones llegarían a \
             cada llamada: define LOCALAPPDATA para que la carpeta quede fuera del proyecto",
            absolute.display(),
            file.display()
        ))),
        None => Ok(()),
    }
}

/// True when `a` and `b` name the same folder: the canonical paths when both exist, otherwise the
/// text with one kind of separator, no trailing one and no letter case (Windows ignores both;
/// `claude auth status` prints the folder with `/`).
pub fn same_dir(a: &Path, b: &Path) -> bool {
    if let (Ok(a), Ok(b)) = (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        return a == b;
    }
    let plain = |p: &Path| {
        p.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    plain(a) == plain(b)
}

/// Reads the fields of the `auth status` JSON (none is required: a missing one is empty).
fn account_seen(status: &Value) -> AccountSeen {
    let text = |key: &str| {
        status
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    AccountSeen {
        auth_method: text("authMethod"),
        api_provider: text("apiProvider"),
        subscription: text("subscriptionType"),
        email: mask_email(&text("email")),
        org_id: text("orgId"),
        config_dir: text("configDirectory"),
        cli_version: None,
        warnings: Vec::new(),
    }
}

fn or_dash(text: &str) -> &str {
    if text.is_empty() { "—" } else { text }
}

/// The verdict on the `auth status` JSON against `config` and the folder the child was given:
/// every way the login could be the wrong one, each with what Gonza should do. `logged_in` is
/// the exit status of the command (1 when there is no session).
pub fn judge_account(
    config: &ClaudeConfig,
    config_dir: &Path,
    status: &Value,
    exit_ok: bool,
) -> Result<AccountSeen, ClaudeError> {
    let seen = account_seen(status);
    let refuse = |message: String| ClaudeError::Account {
        message,
        seen: Some(Box::new(seen.clone())),
    };
    let logged_in = status
        .get("loggedIn")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !exit_ok || !logged_in {
        return Err(refuse(format!(
            "la cuenta privada de jobbot no tiene sesión (directorio {}): ejecuta jobbot \
             claude-login con tu cuenta personal",
            config_dir.display()
        )));
    }
    if seen.auth_method != "claude.ai" {
        return Err(refuse(format!(
            "la credencial activa es {}, no la suscripción personal: quita esa clave o token del \
             entorno y de los ajustes de Claude y repite jobbot claude-login",
            or_dash(&seen.auth_method)
        )));
    }
    if seen.api_provider != "firstParty" {
        return Err(refuse(format!(
            "Claude saldría por el proveedor {} (la nube de otra cuenta), no por Anthropic: \
             revisa las variables CLAUDE_CODE_USE_* y los ajustes administrados",
            or_dash(&seen.api_provider)
        )));
    }
    if !config
        .allowed_subscriptions
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(&seen.subscription))
    {
        return Err(refuse(format!(
            "la suscripción activa es {} (¿una cuenta de empresa?); jobbot solo acepta {}: \
             ejecuta jobbot claude-login con tu cuenta personal",
            or_dash(&seen.subscription),
            config.allowed_subscriptions.join(", ")
        )));
    }
    if !same_dir(Path::new(&seen.config_dir), config_dir) {
        return Err(refuse(format!(
            "claude usa el directorio {} en vez del de jobbot ({}): la variable CLAUDE_CONFIG_DIR \
             no llegó al proceso hijo",
            or_dash(&seen.config_dir),
            config_dir.display()
        )));
    }
    let expected_email = config.expected_email.expose().trim();
    if !expected_email.is_empty() && !expected_email.eq_ignore_ascii_case(status_email(status)) {
        return Err(refuse(format!(
            "cuenta equivocada: la sesión es de {} y se esperaba {}: ejecuta jobbot claude-login \
             con tu cuenta personal",
            or_dash(&seen.email),
            mask_email(expected_email)
        )));
    }
    let expected_org = config.expected_org_id.trim();
    if !expected_org.is_empty() && expected_org != seen.org_id {
        return Err(refuse(format!(
            "organización equivocada: la sesión pertenece a {} y se esperaba {}: elige tu \
             organización personal en jobbot claude-login",
            or_dash(&seen.org_id),
            expected_org
        )));
    }
    Ok(seen)
}

fn status_email(status: &Value) -> &str {
    status
        .get("email")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
}

/// Reads what `claude -p --output-format json` printed.
pub fn parse_reply(stdout: &str) -> Result<ClaudeReply, ClaudeError> {
    let value: Value =
        serde_json::from_str(stdout.trim()).map_err(|e| ClaudeError::Unreadable(e.to_string()))?;
    if value.get("type").and_then(Value::as_str) != Some("result") {
        return Err(ClaudeError::Unreadable(
            "la salida no es un resultado de claude".to_string(),
        ));
    }
    let cost_usd = value
        .get("total_cost_usd")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let text = value
        .get("result")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if value
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let subtype = value
            .get("subtype")
            .and_then(Value::as_str)
            .unwrap_or("error")
            .to_string();
        return Err(ClaudeError::Reported {
            subtype,
            message: text,
            cost_usd,
        });
    }
    let usage = value.get("usage");
    let tokens = |key: &str| {
        usage
            .and_then(|u| u.get(key))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    Ok(ClaudeReply {
        text,
        structured: value
            .get("structured_output")
            .filter(|v| !v.is_null())
            .cloned(),
        cost_usd,
        input_tokens: tokens("input_tokens")
            + tokens("cache_creation_input_tokens")
            + tokens("cache_read_input_tokens"),
        output_tokens: tokens("output_tokens"),
        turns: value.get("num_turns").and_then(Value::as_u64).unwrap_or(0),
    })
}

/// The arguments of one call. The prompt is not among them: it goes through stdin.
pub fn build_args(config: &ClaudeConfig, req: &ClaudeRequest) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--output-format",
        "json",
        "--setting-sources",
        "project",
        "--disable-slash-commands",
        "--strict-mcp-config",
        "--no-session-persistence",
        "--tools",
        "",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.extend([
        "--model".to_string(),
        config.model.clone(),
        "--max-budget-usd".to_string(),
        format!("{:.2}", config.per_call_budget_usd),
        "--system-prompt".to_string(),
        req.system.clone(),
    ]);
    if let Some(schema) = &req.schema {
        args.push("--json-schema".to_string());
        args.push(schema.to_string());
    }
    args
}

fn native_exe_in(npm_dir: &Path) -> PathBuf {
    npm_dir
        .join("node_modules")
        .join("@anthropic-ai")
        .join("claude-code")
        .join("bin")
        .join("claude.exe")
}

/// Where `claude.exe` is: the configured path (which must exist), a `claude.exe` on the PATH, the
/// native exe beside npm's `claude.cmd` shim on the PATH, then `%APPDATA%\npm`. The shim itself
/// cannot be started with quoted arguments from Rust, so the native exe is always what runs.
pub fn find_claude_program(
    config: &ClaudeConfig,
    path_var: &str,
    appdata: Option<&Path>,
) -> Option<PathBuf> {
    if !config.program.trim().is_empty() {
        let program = PathBuf::from(config.program.trim());
        return program.is_file().then_some(program);
    }
    for dir in std::env::split_paths(path_var) {
        let exe = dir.join("claude.exe");
        if exe.is_file() {
            return Some(exe);
        }
        let native = native_exe_in(&dir);
        if dir.join("claude.cmd").is_file() && native.is_file() {
            return Some(native);
        }
    }
    appdata
        .map(|a| native_exe_in(&a.join("npm")))
        .filter(|p| p.is_file())
}

/// What an employer's managed policy for Claude Code leaves on a Windows PC (R8c section 4): the
/// registry value `Settings` under `HKLM\SOFTWARE\Policies\ClaudeCode` (and the same under `HKCU`),
/// the folder `%ProgramFiles%\ClaudeCode` (`managed-settings.json`, `managed-settings.d\`), and
/// `remote-settings.json` in the config folder, the cache of settings that an organization's
/// console delivers to its members' logins. Such a policy can pin the account
/// (`forceLoginOrgUUID`), restrict models or turn on telemetry, so finding one is worth telling
/// Gonza about, but it does not stop anything: it is a warning, never a refusal.
///
/// The places are injected, so a test looks only inside folders it made and answers the registry
/// question itself: no test ever reads the real registry or the real Program Files.
pub struct PolicyWatch {
    program_files: Option<PathBuf>,
    registry_value_exists: Box<dyn Fn(&str) -> bool + Send + Sync>,
}

/// The registry keys whose `Settings` value is a managed policy for Claude Code.
pub const POLICY_REGISTRY_KEYS: [&str; 2] = [
    "HKLM\\SOFTWARE\\Policies\\ClaudeCode",
    "HKCU\\SOFTWARE\\Policies\\ClaudeCode",
];

const POLICY_ADVICE: &str = "Con una política así, una empresa puede fijar la cuenta o la \
     telemetría de Claude Code: revisa `claude doctor`. jobbot sigue (esto solo avisa).";

impl PolicyWatch {
    /// Looks nowhere. What a runner built for a test starts with.
    pub fn off() -> PolicyWatch {
        PolicyWatch::injected(None, |_| false)
    }

    /// The places given: `program_files` stands for `%ProgramFiles%`, and `registry_value_exists`
    /// answers whether `Settings` exists under a registry key.
    pub fn injected(
        program_files: Option<PathBuf>,
        registry_value_exists: impl Fn(&str) -> bool + Send + Sync + 'static,
    ) -> PolicyWatch {
        PolicyWatch {
            program_files,
            registry_value_exists: Box::new(registry_value_exists),
        }
    }

    /// This PC: the real `%ProgramFiles%` and the real registry (through `reg query`, read-only).
    pub fn system() -> PolicyWatch {
        let program_files = std::env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("C:\\Program Files"));
        PolicyWatch::injected(Some(program_files), reg_settings_exist)
    }

    /// One sentence per trace found; empty on a PC with no managed policy. `config_dir` is the
    /// folder the child uses, where server-managed settings are cached.
    pub fn traces(&self, config_dir: &Path) -> Vec<String> {
        let mut found = Vec::new();
        for key in POLICY_REGISTRY_KEYS {
            if (self.registry_value_exists)(key) {
                found.push(format!(
                    "hay una política administrada de Claude Code en el registro ({key}). {POLICY_ADVICE}"
                ));
            }
        }
        if let Some(program_files) = &self.program_files {
            let folder = program_files.join("ClaudeCode");
            if folder.is_dir() {
                found.push(format!(
                    "existe {} (configuración administrada de Claude Code). {POLICY_ADVICE}",
                    folder.display()
                ));
            }
        }
        let remote = config_dir.join("remote-settings.json");
        if remote.is_file() {
            found.push(format!(
                "existe {} (la organización de la cuenta entrega configuración a Claude Code). \
                 {POLICY_ADVICE}",
                remote.display()
            ));
        }
        found
    }
}

/// True when `reg query <key> /v Settings` finds the value. Read-only; any failure to ask (no
/// `reg.exe`, another OS) counts as "not there".
fn reg_settings_exist(key: &str) -> bool {
    std::process::Command::new("reg")
        .args(["query", key, "/v", "Settings"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// The real thing: one `claude.exe -p` child per request.
pub struct CliClaude {
    config: ClaudeConfig,
    program: PathBuf,
    /// Placed before the real arguments. Empty in production; the tests use it to pick a mode of
    /// the stand-in program.
    prefix_args: Vec<String>,
    /// An empty folder, so no project settings are found.
    work_dir: PathBuf,
    /// Claude's own config folder for this program (see `resolve_config_dir`).
    config_dir: PathBuf,
    /// The preflight is asked once per process: the login does not change during a run, and the
    /// first `-p` call must not go out before it passed.
    preflight_seen: std::sync::Mutex<Option<Result<AccountSeen, ClaudeError>>>,
    /// Where a managed policy would show; `PolicyWatch::off()` unless the caller says otherwise.
    policy: PolicyWatch,
}

impl CliClaude {
    pub fn locate(config: ClaudeConfig, work_dir: PathBuf) -> Result<CliClaude, ClaudeError> {
        check_work_dir(&work_dir)?;
        let path_var = std::env::var("PATH").unwrap_or_default();
        let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
        let program =
            find_claude_program(&config, &path_var, appdata.as_deref()).ok_or_else(|| {
                ClaudeError::NotFound(
                    "ni en `program` de profile/claude.toml, ni en el PATH, ni en %APPDATA%\\npm; \
                 instala Claude Code o indica la ruta de claude.exe"
                        .to_string(),
                )
            })?;
        let local_appdata = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        let config_dir = resolve_config_dir(&config, local_appdata.as_deref())?;
        Ok(
            CliClaude::with_program(config, program, Vec::new(), work_dir, config_dir)
                .with_policy_watch(PolicyWatch::system()),
        )
    }

    pub fn with_program(
        config: ClaudeConfig,
        program: PathBuf,
        prefix_args: Vec<String>,
        work_dir: PathBuf,
        config_dir: PathBuf,
    ) -> CliClaude {
        CliClaude {
            config,
            program,
            prefix_args,
            work_dir,
            config_dir,
            preflight_seen: std::sync::Mutex::new(None),
            policy: PolicyWatch::off(),
        }
    }

    /// The same runner whose preflight also looks for a managed policy (see `PolicyWatch`).
    pub fn with_policy_watch(mut self, policy: PolicyWatch) -> CliClaude {
        self.policy = policy;
        self
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// The environment every child gets, whatever it is asked to do: nothing that picks another
    /// account or bill (`picks_account_or_bill`), then this program's own config folder, then
    /// `CHILD_ENV`. The four names of `CHILD_ENV_REMOVED` are removed even when they are not set:
    /// nothing can put them there between now and the spawn.
    fn scrub_env(
        &self,
        command: &mut Command,
        env_names: impl IntoIterator<Item = std::ffi::OsString>,
    ) {
        for key in CHILD_ENV_REMOVED {
            command.env_remove(key);
        }
        for name in env_names {
            if name.to_str().is_some_and(picks_account_or_bill) {
                command.env_remove(&name);
            }
        }
        // After the removals, so the inherited folder never wins over this program's own.
        command.env("CLAUDE_CONFIG_DIR", &self.config_dir);
        for (key, value) in CHILD_ENV {
            command.env(key, value);
        }
    }

    /// `claude auth status` with exactly the environment of a `-p` child, judged by
    /// `judge_account`. Blocks on the child for at most the configured timeout.
    async fn ask_auth_status(&self) -> Result<AccountSeen, ClaudeError> {
        std::fs::create_dir_all(&self.work_dir).map_err(|e| ClaudeError::Io(e.to_string()))?;
        let mut command = self.child_command(&["auth", "status"]);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let limit = Duration::from_secs(self.config.timeout_secs);
        let output = match tokio::time::timeout(limit, command.output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => return Err(spawn_error(&self.program, e)),
            Err(_) => return Err(ClaudeError::Timeout(self.config.timeout_secs)),
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Without a session the command exits 1 and still prints its JSON (`loggedIn: false`).
        let status: Value = serde_json::from_str(stdout.trim()).map_err(|_| {
            let stderr = tail(&String::from_utf8_lossy(&output.stderr), 400);
            ClaudeError::Account {
                message: format!(
                    "claude auth status no devolvió JSON (código {}): {}",
                    output.status.code().unwrap_or(-1),
                    if stderr.is_empty() {
                        tail(&stdout, 400)
                    } else {
                        stderr
                    }
                ),
                seen: None,
            }
        })?;
        let warnings = self.policy.traces(&self.config_dir);
        let verdict = judge_account(
            &self.config,
            &self.config_dir,
            &status,
            output.status.success(),
        );
        match verdict {
            Ok(mut seen) => {
                seen.cli_version = self.version().await;
                seen.warnings = warnings;
                Ok(seen)
            }
            // A refusal keeps the warnings too: a policy may be why the login is the wrong one.
            Err(ClaudeError::Account { message, seen }) => Err(ClaudeError::Account {
                message,
                seen: seen.map(|mut seen| {
                    seen.warnings = warnings;
                    seen
                }),
            }),
            Err(other) => Err(other),
        }
    }

    /// `claude auth login` with the same scrubbed environment and this program's own config
    /// folder (created first), on the terminal so the browser opens. Nothing is parsed: the
    /// preflight that follows is the proof.
    pub async fn login(&self) -> Result<(), ClaudeError> {
        std::fs::create_dir_all(&self.config_dir).map_err(|e| ClaudeError::Io(e.to_string()))?;
        std::fs::create_dir_all(&self.work_dir).map_err(|e| ClaudeError::Io(e.to_string()))?;
        let args = self.login_args();
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let mut command = self.child_command(&args);
        command
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let status = command
            .status()
            .await
            .map_err(|e| spawn_error(&self.program, e))?;
        if !status.success() {
            return Err(ClaudeError::Failed {
                code: status.code().unwrap_or(-1),
                stderr: "claude auth login no terminó bien".to_string(),
            });
        }
        // A login changes the answer: the next preflight must ask again.
        *self.preflight_seen.lock().unwrap() = None;
        Ok(())
    }

    /// The arguments of `claude auth login`: always the subscription (`--claudeai`, never the
    /// console route that bills an API key) and, when the config fixes the private address,
    /// `--email` so the sign-in page asks for that account and not for whichever the browser
    /// remembers.
    fn login_args(&self) -> Vec<String> {
        let mut args: Vec<String> = ["auth", "login", "--claudeai"].map(String::from).into();
        let email = self.config.expected_email.expose().trim();
        if !email.is_empty() {
            args.push("--email".to_string());
            args.push(email.to_string());
        }
        args
    }

    /// The first word of `claude --version` (`2.1.284`), or `None` when it cannot be run or does
    /// not answer in `VERSION_DEADLINE_SECS`. It runs like every other child (same clean
    /// environment, same own config folder, the empty work folder): even a question about the
    /// version must not touch the account of an employer.
    pub async fn version(&self) -> Option<String> {
        std::fs::create_dir_all(&self.work_dir).ok()?;
        let mut command = self.child_command(&["--version"]);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let limit = Duration::from_secs(VERSION_DEADLINE_SECS);
        let output = tokio::time::timeout(limit, command.output())
            .await
            .ok()?
            .ok()?;
        String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .next()
            .map(str::to_string)
    }

    /// A child for `args` (after the program's own prefix) in the work folder with the scrubbed
    /// environment of the real process. The caller chooses the pipes.
    fn child_command(&self, args: &[&str]) -> Command {
        self.child_command_in(args, std::env::vars_os().map(|(name, _)| name))
    }

    /// `child_command` for an environment whose variable names are given (for the tests).
    fn child_command_in(
        &self,
        args: &[&str],
        env_names: impl IntoIterator<Item = std::ffi::OsString>,
    ) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(&self.prefix_args)
            .args(args)
            .current_dir(&self.work_dir);
        self.scrub_env(&mut command, env_names);
        command
    }

    /// The command for one request, ready to spawn: arguments, work folder, pipes and the child's
    /// environment. Kept apart from `run` so a test can read the environment without starting a
    /// process.
    fn prepare_command(&self, req: &ClaudeRequest) -> Command {
        self.prepare_command_in(req, std::env::vars_os().map(|(name, _)| name))
    }

    /// `prepare_command` for an environment whose variable names are given, so a test does not
    /// have to change the real one.
    fn prepare_command_in(
        &self,
        req: &ClaudeRequest,
        env_names: impl IntoIterator<Item = std::ffi::OsString>,
    ) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(&self.prefix_args)
            .args(build_args(&self.config, req))
            .current_dir(&self.work_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        self.scrub_env(&mut command, env_names);
        command
    }
}

/// A spawn failure as the error Gonza reads: a missing program is named, anything else is I/O.
fn spawn_error(program: &Path, e: std::io::Error) -> ClaudeError {
    if e.kind() == std::io::ErrorKind::NotFound {
        ClaudeError::NotFound(format!("{}: {e}", program.display()))
    } else {
        ClaudeError::Io(e.to_string())
    }
}

/// The last `max_chars` characters of `text`, trimmed, with any whole address masked: this is what
/// the CLI printed, and it ends up in `meta`, in `status` and in the logs.
fn tail(text: &str, max_chars: usize) -> String {
    let text = mask_addresses(text.trim());
    let text = text.as_str();
    let count = text.chars().count();
    if count <= max_chars {
        text.to_string()
    } else {
        text.chars().skip(count - max_chars).collect()
    }
}

#[async_trait]
impl ClaudeRunner for CliClaude {
    /// `claude auth logout` with the same clean environment and own folder as the login, so what
    /// it closes is exactly what the login opened. The cached verdict of the preflight is
    /// dropped whatever happens: the next question must ask again.
    async fn logout(&self) -> Result<(), ClaudeError> {
        std::fs::create_dir_all(&self.work_dir).map_err(|e| ClaudeError::Io(e.to_string()))?;
        let mut command = self.child_command(&["auth", "logout"]);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let limit = Duration::from_secs(LOGOUT_DEADLINE_SECS);
        let result = tokio::time::timeout(limit, command.output()).await;
        *self.preflight_seen.lock().unwrap() = None;
        let output = match result {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => return Err(spawn_error(&self.program, e)),
            Err(_) => return Err(ClaudeError::Timeout(LOGOUT_DEADLINE_SECS)),
        };
        if output.status.success() {
            Ok(())
        } else {
            Err(ClaudeError::Failed {
                code: output.status.code().unwrap_or(-1),
                stderr: tail(&String::from_utf8_lossy(&output.stderr), 400),
            })
        }
    }

    async fn run(&self, req: &ClaudeRequest) -> Result<ClaudeReply, ClaudeError> {
        // Fail closed: no prompt goes out until the login is known to be the private one.
        self.preflight().await?;
        std::fs::create_dir_all(&self.work_dir).map_err(|e| ClaudeError::Io(e.to_string()))?;
        let mut command = self.prepare_command(req);
        let mut child = command.spawn().map_err(|e| spawn_error(&self.program, e))?;
        // The prompt is written from its own task while the output is read, so a prompt bigger
        // than the pipe buffer cannot deadlock. A child that never reads it just breaks the pipe.
        let mut stdin = child.stdin.take().expect("stdin was piped");
        let prompt = req.prompt.clone();
        let writer = tokio::spawn(async move {
            let _ = stdin.write_all(prompt.as_bytes()).await;
            let _ = stdin.shutdown().await;
        });
        let limit = Duration::from_secs(self.config.timeout_secs);
        // Dropping the future on a timeout drops the child, and `kill_on_drop` ends the process.
        let output = match tokio::time::timeout(limit, child.wait_with_output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => return Err(ClaudeError::Io(e.to_string())),
            Err(_) => return Err(ClaudeError::Timeout(self.config.timeout_secs)),
        };
        let _ = writer.await;
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() {
            // claude prints its JSON error result even when it exits with 1: keep the cost.
            if let Err(reported @ ClaudeError::Reported { .. }) = parse_reply(&stdout) {
                return Err(reported);
            }
            return Err(ClaudeError::Failed {
                code: output.status.code().unwrap_or(-1),
                stderr: tail(&String::from_utf8_lossy(&output.stderr), 400),
            });
        }
        parse_reply(&stdout)
    }

    async fn preflight(&self) -> Result<AccountSeen, ClaudeError> {
        if let Some(cached) = self.preflight_seen.lock().unwrap().clone() {
            return cached;
        }
        let result = self.ask_auth_status().await;
        *self.preflight_seen.lock().unwrap() = Some(result.clone());
        result
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::*;

    const RESULT_JSON: &str = include_str!("../tests/fixtures/claude-result.json");
    const STRUCTURED_JSON: &str = include_str!("../tests/fixtures/claude-structured.json");

    fn request() -> ClaudeRequest {
        ClaudeRequest {
            stage: "test".into(),
            system: "You are terse.".into(),
            prompt: "SECRET PROMPT TEXT".into(),
            schema: None,
        }
    }

    #[test]
    fn a_working_folder_with_instructions_above_it_is_refused() {
        let top = tempfile::tempdir().unwrap();
        let work = top.path().join("data").join("claude-cwd");
        assert!(
            check_work_dir(&work).is_ok(),
            "a temp folder has no CLAUDE.md above it"
        );
        for name in ["CLAUDE.md", "CLAUDE.local.md"] {
            let file = top.path().join(name);
            std::fs::write(&file, "be someone else").unwrap();
            let refused = check_work_dir(&work).unwrap_err().to_string();
            assert!(refused.contains(name), "{refused}");
            std::fs::remove_file(&file).unwrap();
        }
    }

    #[test]
    #[cfg_attr(
        not(windows),
        ignore = "drive-letter paths are only absolute on Windows"
    )]
    fn the_config_dir_is_the_file_s_or_the_default_under_local_appdata() {
        let local = Path::new("C:\\Users\\x\\AppData\\Local");
        let dir = resolve_config_dir(&ClaudeConfig::default(), Some(local)).unwrap();
        assert_eq!(
            dir,
            Path::new("C:\\Users\\x\\AppData\\Local\\jobbot\\claude-config")
        );
        let fixed = ClaudeConfig {
            config_dir: "D:\\cfg".to_string(),
            ..ClaudeConfig::default()
        };
        assert_eq!(
            resolve_config_dir(&fixed, Some(local)).unwrap(),
            Path::new("D:\\cfg")
        );
        assert_eq!(
            resolve_config_dir(&fixed, None).unwrap(),
            Path::new("D:\\cfg")
        );
        let err = resolve_config_dir(&ClaudeConfig::default(), None).unwrap_err();
        assert!(err.to_string().contains("LOCALAPPDATA"), "{err}");
    }

    #[test]
    fn the_same_folder_is_recognised_with_either_separator_and_case() {
        assert!(same_dir(
            Path::new("C:/Users/x/AppData/Local/jobbot/claude-config"),
            Path::new("c:\\users\\x\\appdata\\local\\jobbot\\claude-config\\")
        ));
        assert!(!same_dir(
            Path::new("C:/Users/x/.claude"),
            Path::new("C:\\Users\\x\\jobbot\\claude-config")
        ));
        // Existing folders are compared by their canonical path.
        let dir = tempfile::tempdir().unwrap();
        let with_slashes = dir.path().display().to_string().replace('\\', "/");
        assert!(same_dir(Path::new(&with_slashes), dir.path()));
    }

    fn status_ok(dir: &str) -> Value {
        json!({
            "loggedIn": true, "authMethod": "claude.ai", "apiProvider": "firstParty",
            "subscriptionType": "max", "email": "tester@example.com", "orgId": "org-personal",
            "configDirectory": dir
        })
    }

    #[test]
    fn a_private_login_in_the_own_folder_passes_the_preflight() {
        let config = ClaudeConfig {
            expected_email: "Tester@example.com".into(),
            expected_org_id: "org-personal".to_string(),
            ..ClaudeConfig::default()
        };
        let dir = Path::new("C:\\cfg\\jobbot\\claude-config");
        let seen = judge_account(
            &config,
            dir,
            &status_ok("C:/cfg/jobbot/claude-config"),
            true,
        )
        .unwrap();
        assert_eq!(
            (
                seen.auth_method.as_str(),
                seen.subscription.as_str(),
                seen.email.as_str(),
                seen.org_id.as_str()
            ),
            ("claude.ai", "max", "t***@example.com", "org-personal")
        );
        assert!(seen.describe().contains("correo t***@example.com"));
        assert!(!seen.describe().contains("tester@"));
    }

    #[test]
    fn every_other_login_is_refused_with_what_to_do() {
        let dir = Path::new("C:\\cfg\\jobbot\\claude-config");
        let config = ClaudeConfig {
            expected_email: "tester@example.com".into(),
            expected_org_id: "org-personal".to_string(),
            ..ClaudeConfig::default()
        };
        let mut other_dir = status_ok("C:/Users/x/.claude");
        let mut api_key = status_ok("C:/cfg/jobbot/claude-config");
        api_key["authMethod"] = json!("api_key");
        let mut bedrock = status_ok("C:/cfg/jobbot/claude-config");
        bedrock["apiProvider"] = json!("bedrock");
        let mut team = status_ok("C:/cfg/jobbot/claude-config");
        team["subscriptionType"] = json!("team");
        let mut other_email = status_ok("C:/cfg/jobbot/claude-config");
        other_email["email"] = json!("someone.else@corp.example");
        let mut other_org = status_ok("C:/cfg/jobbot/claude-config");
        other_org["orgId"] = json!("org-employer");
        let logged_out = json!({"loggedIn": false, "authMethod": "none"});
        for (status, exit_ok, expect) in [
            (&logged_out, false, "no tiene sesión"),
            (&api_key, true, "es api_key"),
            (&bedrock, true, "proveedor bedrock"),
            (&team, true, "es team"),
            (&mut other_dir, true, "CLAUDE_CONFIG_DIR"),
            (&other_email, true, "cuenta equivocada"),
            (&other_org, true, "organización equivocada"),
        ] {
            let err = judge_account(&config, dir, status, exit_ok).unwrap_err();
            let ClaudeError::Account { message, seen } = &err else {
                panic!("{err:?}");
            };
            assert!(message.contains(expect), "{message}");
            assert!(
                ["jobbot claude-login", "revisa", "no llegó"]
                    .iter()
                    .any(|hint| message.contains(hint)),
                "every refusal says what happened or what to do: {message}"
            );
            assert!(seen.is_some());
            assert!(
                !message.contains("someone.else@") && !message.contains("tester@"),
                "the full address never appears: {message}"
            );
        }
        // The wrong-address message shows both sides masked.
        let err = judge_account(&config, dir, &other_email, true).unwrap_err();
        assert!(
            err.to_string().contains("s***@corp.example")
                && err.to_string().contains("t***@example.com"),
            "{err}"
        );
    }

    #[test]
    fn email_and_org_are_only_checked_when_configured() {
        let dir = Path::new("C:\\cfg\\jobbot\\claude-config");
        let mut status = status_ok("C:/cfg/jobbot/claude-config");
        status["email"] = json!("anyone@example.com");
        status["orgId"] = json!("whatever");
        assert!(judge_account(&ClaudeConfig::default(), dir, &status, true).is_ok());
        let only_pro = ClaudeConfig {
            allowed_subscriptions: vec!["pro".to_string()],
            ..ClaudeConfig::default()
        };
        assert!(judge_account(&only_pro, dir, &status, true).is_err());
    }

    #[test]
    fn a_pc_with_no_managed_policy_has_no_traces() {
        let dir = tempfile::tempdir().unwrap();
        let watch = PolicyWatch::injected(Some(dir.path().join("Program Files")), |_| false);
        assert!(watch.traces(&dir.path().join("claude-config")).is_empty());
        assert!(PolicyWatch::off().traces(dir.path()).is_empty());
    }

    #[test]
    fn a_managed_settings_folder_in_program_files_is_a_trace() {
        let dir = tempfile::tempdir().unwrap();
        let program_files = dir.path().join("Program Files");
        std::fs::create_dir_all(program_files.join("ClaudeCode")).unwrap();
        let watch = PolicyWatch::injected(Some(program_files), |_| false);

        let traces = watch.traces(&dir.path().join("claude-config"));

        assert_eq!(traces.len(), 1, "{traces:?}");
        assert!(
            traces[0].contains("ClaudeCode") && traces[0].contains("claude doctor"),
            "{traces:?}"
        );
    }

    #[test]
    fn server_managed_settings_cached_in_the_config_folder_are_a_trace() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("claude-config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("remote-settings.json"), "{}").unwrap();
        let watch = PolicyWatch::injected(None, |_| false);

        let traces = watch.traces(&config_dir);

        assert_eq!(traces.len(), 1, "{traces:?}");
        assert!(traces[0].contains("remote-settings.json"), "{traces:?}");
    }

    #[test]
    fn each_registry_policy_is_asked_by_key_and_reported_by_key() {
        let dir = tempfile::tempdir().unwrap();
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let log = asked.clone();
        let watch = PolicyWatch::injected(None, move |key| {
            log.lock().unwrap().push(key.to_string());
            key.starts_with("HKLM")
        });

        let traces = watch.traces(dir.path());

        assert_eq!(
            *asked.lock().unwrap(),
            vec![
                "HKLM\\SOFTWARE\\Policies\\ClaudeCode".to_string(),
                "HKCU\\SOFTWARE\\Policies\\ClaudeCode".to_string()
            ]
        );
        assert_eq!(traces.len(), 1, "{traces:?}");
        assert!(traces[0].contains("HKLM"), "{traces:?}");
    }

    #[test]
    fn every_trace_found_is_reported_once() {
        let dir = tempfile::tempdir().unwrap();
        let program_files = dir.path().join("pf");
        std::fs::create_dir_all(program_files.join("ClaudeCode")).unwrap();
        let config_dir = dir.path().join("cfg");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("remote-settings.json"), "{}").unwrap();
        let watch = PolicyWatch::injected(Some(program_files), |_| true);
        assert_eq!(watch.traces(&config_dir).len(), 4);
    }

    #[test]
    fn a_text_reply_is_read_from_the_real_capture() {
        let reply = parse_reply(RESULT_JSON).unwrap();
        assert_eq!(reply.text, "OK");
        assert_eq!(reply.structured, None);
        assert!((reply.cost_usd - 0.018247).abs() < 1e-9);
        // input = fresh + cache creation + cache read
        assert_eq!(
            (reply.input_tokens, reply.output_tokens, reply.turns),
            (8495, 63, 1)
        );
    }

    #[test]
    fn a_structured_reply_carries_the_parsed_object() {
        let reply = parse_reply(STRUCTURED_JSON).unwrap();
        assert_eq!(
            reply.structured,
            Some(json!({"fruit": "apple", "number": 7}))
        );
        assert_eq!(reply.turns, 2);
        assert!((reply.cost_usd - 0.003269).abs() < 1e-9);
    }

    #[test]
    fn an_error_result_becomes_an_error_that_keeps_the_cost() {
        // The error shape is assumed (only the success shape was captured): the parser must only
        // depend on `type`, `is_error`, `subtype`, `result` and `total_cost_usd`.
        let text = r#"{"type":"result","subtype":"error_max_turns","is_error":true,"result":"too many turns","total_cost_usd":0.02}"#;
        match parse_reply(text).unwrap_err() {
            ClaudeError::Reported {
                subtype,
                message,
                cost_usd,
            } => {
                assert_eq!(
                    (subtype.as_str(), message.as_str()),
                    ("error_max_turns", "too many turns")
                );
                assert!((cost_usd - 0.02).abs() < 1e-9);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn garbage_and_other_json_are_unreadable() {
        for text in [
            "",
            "not json",
            "[]",
            r#"{"type":"system"}"#,
            r#"{"result":"OK"}"#,
        ] {
            assert!(
                matches!(parse_reply(text), Err(ClaudeError::Unreadable(_))),
                "{text:?}"
            );
        }
    }

    #[test]
    fn the_command_line_has_the_flags_the_runner_depends_on() {
        let c = ClaudeConfig::default();
        let args = build_args(&c, &request());
        let has = |pair: [&str; 2]| args.windows(2).any(|w| w[0] == pair[0] && w[1] == pair[1]);
        assert_eq!(args[0], "-p");
        assert!(has(["--output-format", "json"]));
        assert!(has(["--setting-sources", "project"]));
        assert!(has(["--tools", ""]), "tools must be switched off: {args:?}");
        assert!(has(["--model", "claude-sonnet-5-5"]));
        assert!(has(["--max-budget-usd", "0.50"]));
        assert!(has(["--system-prompt", "You are terse."]));
        for flag in [
            "--disable-slash-commands",
            "--strict-mcp-config",
            "--no-session-persistence",
        ] {
            assert!(args.iter().any(|a| a == flag), "{flag}");
        }
        assert!(
            !args.iter().any(|a| a == "--bare"),
            "--bare breaks the subscription login"
        );
        assert!(!args.iter().any(|a| a == "--json-schema"));
        assert!(
            !args.iter().any(|a| a.contains("SECRET PROMPT TEXT")),
            "the prompt travels through stdin, never on the command line"
        );
    }

    #[test]
    fn a_schema_is_passed_as_one_argument() {
        let mut req = request();
        req.schema = Some(json!({"type": "object"}));
        let args = build_args(&ClaudeConfig::default(), &req);
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--json-schema" && w[1] == r#"{"type":"object"}"#)
        );
    }

    #[test]
    fn the_child_environment_turns_off_claude_md_loading_and_anything_that_phones_home() {
        for pair in [
            ("CLAUDE_CODE_DISABLE_CLAUDE_MDS", "1"),
            ("DISABLE_TELEMETRY", "1"),
            ("DISABLE_ERROR_REPORTING", "1"),
            ("CLAUDE_CODE_SKIP_PROMPT_HISTORY", "1"),
        ] {
            assert!(CHILD_ENV.contains(&pair), "{pair:?}");
        }
    }

    #[test]
    fn the_child_never_inherits_credentials_and_gets_the_own_config_folder() {
        let dir = tempfile::tempdir().unwrap();
        let own = dir.path().join("claude-config");
        let runner = CliClaude::with_program(
            ClaudeConfig::default(),
            std::path::PathBuf::from("claude"),
            Vec::new(),
            dir.path().to_path_buf(),
            own.clone(),
        );
        let command = runner.prepare_command(&request());
        let envs: std::collections::HashMap<_, _> = command
            .as_std()
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        for name in [
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_BASE_URL",
        ] {
            assert_eq!(
                envs.get(name),
                Some(&None),
                "{name} must be explicitly removed from the child's environment"
            );
        }
        // The inherited folder is dropped and the program's own one is set in its place,
        // whatever the parent had.
        assert_eq!(
            envs.get("CLAUDE_CONFIG_DIR"),
            Some(&Some(own.display().to_string()))
        );
        for name in [
            "CLAUDE_CODE_DISABLE_CLAUDE_MDS",
            "DISABLE_TELEMETRY",
            "DISABLE_ERROR_REPORTING",
            "CLAUDE_CODE_SKIP_PROMPT_HISTORY",
        ] {
            assert_eq!(envs.get(name), Some(&Some("1".to_string())), "{name}");
        }
    }

    #[test]
    fn the_child_loses_every_variable_that_picks_the_account_or_the_bill() {
        let dir = tempfile::tempdir().unwrap();
        let runner = CliClaude::with_program(
            ClaudeConfig::default(),
            std::path::PathBuf::from("claude"),
            Vec::new(),
            dir.path().to_path_buf(),
            dir.path().join("claude-config"),
        );
        // What an environment with a work cloud setup could hold, in any letter case (Windows
        // does not care), next to ordinary variables the child needs.
        let present = [
            "CLAUDE_CODE_OAUTH_TOKEN",
            "claude_code_use_bedrock",
            "CLAUDE_CODE_USE_VERTEX",
            "CLAUDE_CODE_USE_FOUNDRY",
            "ANTHROPIC_FOUNDRY_API_KEY",
            "Anthropic_Model",
            "AWS_PROFILE",
            "AWS_ACCESS_KEY_ID",
            "GOOGLE_APPLICATION_CREDENTIALS",
            "CLOUD_ML_REGION",
            "PATH",
            "USERPROFILE",
            "APPDATA",
            "HTTPS_PROXY",
            "CLAUDE_CODE_DISABLE_CLAUDE_MDS_FOR_OTHER_THINGS",
        ];
        let command =
            runner.prepare_command_in(&request(), present.iter().map(std::ffi::OsString::from));
        let envs: std::collections::HashMap<_, _> = command
            .as_std()
            .get_envs()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v.is_none()))
            .collect();
        for name in &present[..10] {
            assert_eq!(
                envs.get(*name),
                Some(&true),
                "{name} must be removed from the child's environment"
            );
        }
        for name in &present[10..] {
            assert!(
                !envs.contains_key(*name),
                "{name} is an ordinary variable and must be inherited untouched"
            );
        }
    }

    #[test]
    fn the_version_login_and_logout_children_get_the_same_clean_environment_and_work_folder() {
        let dir = tempfile::tempdir().unwrap();
        let runner = CliClaude::with_program(
            ClaudeConfig::default(),
            std::path::PathBuf::from("claude"),
            Vec::new(),
            dir.path().join("claude-cwd"),
            dir.path().join("claude-config"),
        );
        let present = [
            "CLAUDE_CODE_OAUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "CLAUDE_CONFIG_DIR",
            "AWS_PROFILE",
            "PATH",
        ];
        for args in [
            &["--version"][..],
            &["auth", "login", "--claudeai"],
            &["auth", "logout"],
        ] {
            let command =
                runner.child_command_in(args, present.iter().map(std::ffi::OsString::from));
            let std_command = command.as_std();
            let envs: std::collections::HashMap<_, _> = std_command
                .get_envs()
                .map(|(k, v)| {
                    (
                        k.to_string_lossy().into_owned(),
                        v.map(|v| v.to_string_lossy().into_owned()),
                    )
                })
                .collect();
            for name in [
                "CLAUDE_CODE_OAUTH_TOKEN",
                "ANTHROPIC_API_KEY",
                "AWS_PROFILE",
            ] {
                assert_eq!(envs.get(name), Some(&None), "{args:?}: {name} is removed");
            }
            assert_eq!(
                envs.get("CLAUDE_CONFIG_DIR"),
                Some(&Some(
                    dir.path()
                        .join("claude-config")
                        .to_string_lossy()
                        .into_owned()
                )),
                "{args:?}: the own folder, never the inherited one"
            );
            assert!(
                !envs.contains_key("PATH"),
                "{args:?}: ordinary variables pass"
            );
            assert_eq!(
                std_command.get_current_dir(),
                Some(dir.path().join("claude-cwd").as_path()),
                "{args:?}: it runs in the empty work folder"
            );
            let got: Vec<_> = std_command
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            assert_eq!(got, args, "the arguments are the ones asked for");
        }
    }

    #[test]
    fn an_account_is_anchored_only_when_both_the_address_and_the_organization_are_fixed() {
        let with = |email: &str, org: &str| ClaudeConfig {
            expected_email: email.into(),
            expected_org_id: org.to_string(),
            ..ClaudeConfig::default()
        };
        assert!(!ClaudeConfig::default().is_anchored());
        assert!(!with("", "").is_anchored());
        assert!(!with("  ", " ").is_anchored(), "blanks are no value");
        assert!(!with("me@example.com", "").is_anchored());
        assert!(!with("", "org-1").is_anchored());
        assert!(with("me@example.com", "org-1").is_anchored());
        assert!(with(" me@example.com ", " org-1 ").is_anchored());
    }

    #[test]
    fn the_login_asks_for_the_subscription_and_for_the_address_the_config_expects() {
        let dir = tempfile::tempdir().unwrap();
        let runner = |email: &str| {
            CliClaude::with_program(
                ClaudeConfig {
                    expected_email: email.into(),
                    ..ClaudeConfig::default()
                },
                std::path::PathBuf::from("claude"),
                Vec::new(),
                dir.path().join("claude-cwd"),
                dir.path().join("claude-config"),
            )
        };
        // Never the console route (an API key that bills somebody else), and no address to
        // offer when none is fixed.
        assert_eq!(
            runner("").login_args(),
            ["auth", "login", "--claudeai"].map(String::from)
        );
        assert_eq!(
            runner("  me@example.com ").login_args(),
            ["auth", "login", "--claudeai", "--email", "me@example.com"].map(String::from)
        );
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    fn native_under(dir: &Path) -> std::path::PathBuf {
        dir.join("node_modules")
            .join("@anthropic-ai")
            .join("claude-code")
            .join("bin")
            .join("claude.exe")
    }

    fn path_var(dir: &Path) -> String {
        std::env::join_paths([dir]).unwrap().into_string().unwrap()
    }

    #[test]
    fn the_configured_program_wins_and_must_exist() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("my-claude.exe");
        touch(&exe);
        let mut config = ClaudeConfig {
            program: exe.display().to_string(),
            ..ClaudeConfig::default()
        };
        assert_eq!(find_claude_program(&config, "", None), Some(exe));
        config.program = dir.path().join("missing.exe").display().to_string();
        assert_eq!(find_claude_program(&config, "", None), None);
    }

    #[test]
    fn claude_exe_is_found_on_the_path() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("claude.exe"));
        let found = find_claude_program(&ClaudeConfig::default(), &path_var(dir.path()), None);
        assert_eq!(found, Some(dir.path().join("claude.exe")));
    }

    #[test]
    fn the_native_exe_beside_the_npm_shim_is_found() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("claude.cmd"));
        touch(&native_under(dir.path()));
        let found = find_claude_program(&ClaudeConfig::default(), &path_var(dir.path()), None);
        assert_eq!(found, Some(native_under(dir.path())));
    }

    #[test]
    fn appdata_npm_is_the_last_place_looked_at() {
        let appdata = tempfile::tempdir().unwrap();
        let npm = appdata.path().join("npm");
        let config = ClaudeConfig::default();
        assert_eq!(find_claude_program(&config, "", Some(appdata.path())), None);
        touch(&native_under(&npm));
        assert_eq!(
            find_claude_program(&config, "", Some(appdata.path())),
            Some(native_under(&npm))
        );
    }
}
