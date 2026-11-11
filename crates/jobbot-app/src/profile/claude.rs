//! `profile/claude.toml` (with the git-ignored `claude.local.toml` laid over it): how the program
//! may use Claude. The budget, the model and the account the login must be. The runner adapter
//! reads it to build the command line and judge the login; the gate and the panel read the budget
//! and whether the account is anchored.

use std::path::{Path, PathBuf};

use jobbot_domain::privacy::Pii;
use serde::Deserialize;

/// Subscriptions a login may have unless `claude.toml` says otherwise: the personal plans. `team`
/// and `enterprise` are an employer's account, whose admins see every session.
pub const DEFAULT_ALLOWED_SUBSCRIPTIONS: [&str; 2] = ["pro", "max"];

/// What `profile/claude.toml` may set. Every key is optional; the defaults are the shipped values.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaudeConfig {
    /// Full path of `claude.exe`. Empty means: look for it (PATH, then npm's global folder).
    #[serde(default)]
    pub program: String,
    #[serde(default = "default_model")]
    pub model: String,
    /// Most the program may spend in any rolling 24 hours (API-equivalent dollars).
    #[serde(default = "default_daily_budget")]
    pub daily_budget_usd: f64,
    /// Passed to `--max-budget-usd`: one call never spends more than this.
    #[serde(default = "default_call_budget")]
    pub per_call_budget_usd: f64,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_max_dossiers")]
    pub max_dossiers_per_run: usize,
    /// Claude's config folder for this program (credentials, `.claude.json`). Empty means
    /// `%LOCALAPPDATA%\jobbot\claude-config`; otherwise it must be absolute, because the scheduled
    /// task does not start where a terminal would.
    #[serde(default)]
    pub config_dir: String,
    /// The email `claude auth status` must report. Empty means: not checked. His own address, so
    /// it never prints (`Pii`).
    #[serde(default)]
    pub expected_email: Pii<String>,
    /// The organization id it must report. Empty means: not checked.
    #[serde(default)]
    pub expected_org_id: String,
    /// The `subscriptionType` values accepted (`pro`, `max` by default).
    #[serde(default = "default_allowed_subscriptions")]
    pub allowed_subscriptions: Vec<String>,
}

fn default_model() -> String {
    "claude-sonnet-5-5".to_string()
}
fn default_allowed_subscriptions() -> Vec<String> {
    DEFAULT_ALLOWED_SUBSCRIPTIONS
        .iter()
        .map(|s| s.to_string())
        .collect()
}
fn default_daily_budget() -> f64 {
    1.5
}
fn default_call_budget() -> f64 {
    0.5
}
fn default_timeout() -> u64 {
    300
}
fn default_max_dossiers() -> usize {
    2
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        ClaudeConfig {
            program: String::new(),
            model: default_model(),
            daily_budget_usd: default_daily_budget(),
            per_call_budget_usd: default_call_budget(),
            timeout_secs: default_timeout(),
            max_dossiers_per_run: default_max_dossiers(),
            config_dir: String::new(),
            expected_email: Pii::default(),
            expected_org_id: String::new(),
            allowed_subscriptions: default_allowed_subscriptions(),
        }
    }
}

impl ClaudeConfig {
    /// True when the config fixes both the address and the organization the login must be. Only
    /// then does the preflight compare the account with anything: without them any `pro` or
    /// `max` login in the own folder passes, whoever it belongs to ("cuenta sin anclar").
    pub fn is_anchored(&self) -> bool {
        !self.expected_email.expose().trim().is_empty() && !self.expected_org_id.trim().is_empty()
    }
}

/// Why `claude.toml`, or the local file laid over it, cannot be used. The message of each layer
/// says where; the cause is its `source`, so a report of the whole chain reads like
/// `in profile/claude.local.toml: TOML parse error ... unknown field`.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("reading {}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// A mistake reported against the file, or the pair of files, that holds it.
    #[error("in {place}")]
    In {
        place: String,
        #[source]
        source: Box<ConfigError>,
    },
    #[error(transparent)]
    Toml(#[from] toml::de::Error),
    #[error("laying claude.local.toml over claude.toml")]
    Merge(#[source] toml::ser::Error),
    /// A value of the right type that breaks one of the rules of `parse_claude_config`.
    #[error("{0}")]
    Rule(&'static str),
}

/// `Err(ConfigError::Rule(message))` unless `holds`.
fn rule(holds: bool, message: &'static str) -> Result<(), ConfigError> {
    if holds {
        Ok(())
    } else {
        Err(ConfigError::Rule(message))
    }
}

/// `error` reported against `place`.
fn found_in(place: impl std::fmt::Display) -> impl FnOnce(ConfigError) -> ConfigError {
    move |error| ConfigError::In {
        place: place.to_string(),
        source: Box::new(error),
    }
}

pub fn parse_claude_config(text: &str) -> Result<ClaudeConfig, ConfigError> {
    let config: ClaudeConfig = toml::from_str(text)?;
    rule(
        config.daily_budget_usd > 0.0,
        "daily_budget_usd must be above 0",
    )?;
    rule(
        config.per_call_budget_usd > 0.0,
        "per_call_budget_usd must be above 0",
    )?;
    rule(
        config.timeout_secs >= 10,
        "timeout_secs must be at least 10",
    )?;
    rule(!config.model.trim().is_empty(), "model must not be empty")?;
    rule(
        config.config_dir.trim().is_empty() || Path::new(config.config_dir.trim()).is_absolute(),
        "config_dir must be empty or an absolute path",
    )?;
    rule(
        !config.allowed_subscriptions.is_empty(),
        "allowed_subscriptions must name at least one plan",
    )?;
    Ok(config)
}

/// `claude.toml` with, when it exists, `claude.local.toml` laid over it key by key (the local value
/// wins). The local file is for what must not be committed, above all `expected_email`: the
/// repository may be published. It is git-ignored (`profile/*.local.toml`). A mistake in it (an
/// unknown key, a value of the wrong type) is reported against that file, not against the
/// versioned one, and the merged result passes the same checks as a single file.
pub fn load_claude_config(path: &Path) -> Result<ClaudeConfig, ConfigError> {
    let read = |path: &Path| {
        std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })
    };
    let text = read(path)?;
    let local_path = path.with_file_name("claude.local.toml");
    if !local_path.is_file() {
        return parse_claude_config(&text).map_err(found_in(path.display()));
    }
    let local = read(&local_path)?;
    parse_claude_config(&local).map_err(found_in(local_path.display()))?;
    let mut merged: toml::Table = toml::from_str(&text)
        .map_err(ConfigError::from)
        .map_err(found_in(path.display()))?;
    let overrides: toml::Table = toml::from_str(&local)
        .map_err(ConfigError::from)
        .map_err(found_in(local_path.display()))?;
    merged.extend(overrides);
    let merged = toml::to_string(&merged).map_err(ConfigError::Merge)?;
    parse_claude_config(&merged).map_err(found_in(format!(
        "{} with {} laid over it",
        path.display(),
        local_path.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `error` and every cause under it, joined the way a report shows them.
    fn chain(error: &dyn std::error::Error) -> String {
        let mut text = error.to_string();
        let mut cause = error.source();
        while let Some(next) = cause {
            text.push_str(": ");
            text.push_str(&next.to_string());
            cause = next.source();
        }
        text
    }

    #[test]
    fn the_config_never_prints_its_address() {
        let c = parse_claude_config("expected_email = \"x@example.com\"\n").unwrap();
        assert!(!format!("{c:?}").contains("x@example.com"), "{c:?}");
        assert_eq!(c.expected_email.expose(), "x@example.com");
    }

    #[test]
    fn an_empty_file_gives_the_documented_defaults() {
        let c = parse_claude_config("").unwrap();
        assert_eq!(c, ClaudeConfig::default());
        assert_eq!(
            (
                c.model.as_str(),
                c.daily_budget_usd,
                c.per_call_budget_usd,
                c.timeout_secs,
                c.max_dossiers_per_run
            ),
            ("claude-sonnet-5-5", 1.5, 0.5, 300, 2)
        );
        // The account keys: nothing fixed, nothing checked, the personal plans only.
        assert_eq!(
            (
                c.config_dir.as_str(),
                c.expected_email.expose().as_str(),
                c.expected_org_id.as_str(),
                c.allowed_subscriptions.clone()
            ),
            ("", "", "", vec!["pro".to_string(), "max".to_string()])
        );
    }

    #[test]
    #[cfg_attr(
        not(windows),
        ignore = "drive-letter paths are only absolute on Windows"
    )]
    fn the_account_keys_are_read_from_the_file() {
        let c = parse_claude_config(
            "config_dir = \"C:\\\\cfg\\\\claude\"\nexpected_email = \"x@example.com\"\n\
             expected_org_id = \"org-1\"\nallowed_subscriptions = [\"max\"]\n",
        )
        .unwrap();
        assert_eq!(c.config_dir, "C:\\cfg\\claude");
        assert_eq!(c.expected_email.expose(), "x@example.com");
        assert_eq!(c.expected_org_id, "org-1");
        assert_eq!(c.allowed_subscriptions, vec!["max".to_string()]);
    }

    #[test]
    fn bad_settings_are_rejected() {
        for (text, mentions) in [
            ("daily_budget_usd = 0", "daily_budget_usd"),
            ("per_call_budget_usd = -1", "per_call_budget_usd"),
            ("timeout_secs = 5", "timeout_secs"),
            ("model = \"  \"", "model"),
            ("modle = \"x\"", "modle"),
            ("config_dir = \"claude-config\"", "config_dir"),
            ("allowed_subscriptions = []", "allowed_subscriptions"),
        ] {
            let err = parse_claude_config(text).unwrap_err().to_string();
            assert!(err.contains(mentions), "{text}: {err}");
        }
    }

    /// `claude.toml` with `shipped`, and `claude.local.toml` with `local` when given, in a new folder.
    fn profile_with(shipped: &str, local: Option<&str>) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("claude.toml");
        std::fs::write(&path, shipped).unwrap();
        if let Some(local) = local {
            std::fs::write(dir.path().join("claude.local.toml"), local).unwrap();
        }
        (dir, path)
    }

    #[test]
    fn a_local_file_wins_key_by_key_over_the_versioned_one() {
        let (_dir, path) = profile_with(
            "model = \"versioned-model\"\ndaily_budget_usd = 2.0\nexpected_email = \"\"\n",
            Some(
                "expected_email = \"someone@example.com\"\nexpected_org_id = \"org-1\"\n\
                 daily_budget_usd = 3.0\n",
            ),
        );
        let c = load_claude_config(&path).unwrap();
        assert_eq!(c.expected_email.expose(), "someone@example.com");
        assert_eq!(c.expected_org_id, "org-1");
        assert_eq!(c.daily_budget_usd, 3.0, "the local value has priority");
        assert_eq!(c.model, "versioned-model", "what it does not name is kept");
        assert_eq!(c.timeout_secs, 300, "and so are the defaults");
    }

    #[test]
    fn without_a_local_file_the_versioned_one_is_all_there_is() {
        let (_dir, path) = profile_with("expected_org_id = \"org-9\"\n", None);
        let c = load_claude_config(&path).unwrap();
        assert_eq!(
            (
                c.expected_org_id.as_str(),
                c.expected_email.expose().as_str()
            ),
            ("org-9", "")
        );
    }

    #[test]
    fn a_local_file_that_is_empty_changes_nothing() {
        let (_dir, path) = profile_with("model = \"m\"\n", Some("# nothing yet\n"));
        assert_eq!(load_claude_config(&path).unwrap().model, "m");
    }

    #[test]
    fn a_mistake_in_the_local_file_names_the_local_file() {
        // A misspelt key must not silently leave the account unchecked.
        let (_dir, path) = profile_with("", Some("expected_mail = \"someone@example.com\"\n"));
        let err = chain(&load_claude_config(&path).unwrap_err());
        assert!(
            err.contains("claude.local.toml") && err.contains("expected_mail"),
            "{err}"
        );

        let (_dir, path) = profile_with("", Some("this is = not [valid"));
        let err = chain(&load_claude_config(&path).unwrap_err());
        assert!(err.contains("claude.local.toml"), "{err}");
    }

    #[test]
    fn a_local_value_that_breaks_a_rule_is_refused() {
        let (_dir, path) = profile_with("", Some("config_dir = \"relative\\\\folder\"\n"));
        let err = chain(&load_claude_config(&path).unwrap_err());
        assert!(
            err.contains("config_dir must be empty or an absolute path"),
            "{err}"
        );
    }
}
