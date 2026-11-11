//! Claude as a port: one question in, one reply (with its cost) out, and the check of which login
//! the calls would use. The runner (`claude -p` as a child process) is the `jobbot-claude` adapter;
//! the tests use fakes.

use async_trait::async_trait;
use serde_json::Value;

/// One question for Claude.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeRequest {
    /// Which part of the program asks (`dossier`, `freelance-profile`, ...); the ledger groups by it.
    pub stage: String,
    pub system: String,
    pub prompt: String,
    /// JSON Schema of the wanted answer, when it is a structure.
    pub schema: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeReply {
    pub text: String,
    /// `structured_output`, already parsed, when a schema was given.
    pub structured: Option<Value>,
    pub cost_usd: f64,
    /// Fresh input plus cache creation plus cache read.
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub turns: u64,
}

impl ClaudeReply {
    /// The structured answer as a `T`: read from `structured_output` when there is one, otherwise
    /// from the text. The errors are in Spanish because Gonza sees them.
    pub fn into_json<T: serde::de::DeserializeOwned>(self) -> Result<T, ShapeError> {
        let value = match self.structured {
            Some(value) => value,
            None => serde_json::from_str(&self.text).map_err(ShapeError::NotJson)?,
        };
        serde_json::from_value(value).map_err(ShapeError::WrongShape)
    }
}

/// Why a reply could not be read as the structure asked for. In Spanish: Gonza sees it.
#[derive(Debug, thiserror::Error)]
pub enum ShapeError {
    #[error("la respuesta de Claude no es JSON")]
    NotJson(#[source] serde_json::Error),
    #[error("la respuesta de Claude no tiene la forma esperada")]
    WrongShape(#[source] serde_json::Error),
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ClaudeError {
    /// The login a child would use is not the private subscription (or there is none). The
    /// message says what was found and what to do; `seen` is what `auth status` reported
    /// (boxed: it is seven strings, and every `Result` of this module carries the enum).
    #[error("{message}")]
    Account {
        message: String,
        seen: Option<Box<AccountSeen>>,
    },
    #[error("no se encontró claude: {0}")]
    NotFound(String),
    #[error("error del sistema al ejecutar claude: {0}")]
    Io(String),
    #[error("claude no respondió en {0} s")]
    Timeout(u64),
    #[error("claude terminó con código {code}: {stderr}")]
    Failed { code: i32, stderr: String },
    #[error("respuesta de claude ilegible: {0}")]
    Unreadable(String),
    #[error("claude devolvió un error ({subtype}): {message}")]
    Reported {
        subtype: String,
        message: String,
        cost_usd: f64,
    },
    #[error("presupuesto diario de Claude agotado (US${spent:.2} de US${limit:.2} en 24 h)")]
    BudgetExhausted { spent: f64, limit: f64 },
}

impl ClaudeError {
    /// What the failed call cost, when Claude said so.
    pub fn cost_usd(&self) -> f64 {
        match self {
            ClaudeError::Reported { cost_usd, .. } => *cost_usd,
            _ => 0.0,
        }
    }
}

/// What `claude auth status` said about the login a child would use.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
pub struct AccountSeen {
    pub auth_method: String,
    pub api_provider: String,
    pub subscription: String,
    /// Masked (`g***@example.com`): the full address never reaches a log or the database.
    pub email: String,
    pub org_id: String,
    pub config_dir: String,
    /// The first word of `claude --version`, when it could be read.
    pub cli_version: Option<String>,
    /// What the preflight noticed without it being a reason to refuse (a managed policy on this PC,
    /// see `PolicyWatch`). Shown to Gonza and kept with the verdict.
    pub warnings: Vec<String>,
}

impl AccountSeen {
    /// One line for Gonza, in Spanish.
    pub fn describe(&self) -> String {
        format!(
            "método {} · proveedor {} · suscripción {} · correo {} · org {} · directorio {}",
            or_dash(&self.auth_method),
            or_dash(&self.api_provider),
            or_dash(&self.subscription),
            or_dash(&self.email),
            or_dash(&self.org_id),
            or_dash(&self.config_dir)
        )
    }
}

#[async_trait]
pub trait ClaudeRunner: Send + Sync {
    async fn run(&self, req: &ClaudeRequest) -> Result<ClaudeReply, ClaudeError>;
    /// Which login the calls would use, refused when it is not the private subscription. The
    /// real runner asks `claude auth status` once per process; the fakes answer from memory.
    async fn preflight(&self) -> Result<AccountSeen, ClaudeError>;
    /// Ends the login that this program's own config folder holds (`claude auth logout`), so a
    /// login the preflight refused leaves no credentials behind. A runner that has no login of
    /// its own (the fakes) has nothing to close.
    async fn logout(&self) -> Result<(), ClaudeError> {
        Ok(())
    }
}

fn or_dash(text: &str) -> &str {
    if text.is_empty() { "—" } else { text }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    #[test]
    fn into_json_reads_structured_output_then_text_and_names_the_failure() {
        #[derive(Debug, PartialEq, Deserialize)]
        struct Pair {
            a: i32,
        }
        let reply = |text: &str, structured: Option<Value>| ClaudeReply {
            text: text.to_string(),
            structured,
            cost_usd: 0.0,
            input_tokens: 0,
            output_tokens: 0,
            turns: 1,
        };
        let from_structured = reply("ignored", Some(json!({"a": 1}))).into_json::<Pair>();
        assert_eq!(from_structured.unwrap(), Pair { a: 1 });
        let from_text = reply(r#"{"a": 2}"#, None).into_json::<Pair>();
        assert_eq!(from_text.unwrap(), Pair { a: 2 });
        let not_json = reply("sorry, no JSON", None)
            .into_json::<Pair>()
            .unwrap_err();
        assert!(
            format!("{not_json:#}").contains("no es JSON"),
            "{not_json:#}"
        );
        let wrong_shape = reply("", Some(json!({"a": "x"})))
            .into_json::<Pair>()
            .unwrap_err();
        assert!(
            format!("{wrong_shape:#}").contains("forma esperada"),
            "{wrong_shape:#}"
        );
    }
}
