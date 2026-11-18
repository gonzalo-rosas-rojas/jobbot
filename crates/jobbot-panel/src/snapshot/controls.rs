//! The state of the engine and of the Claude account: whether it is stopped, when it last ran and
//! how that run ended, the run window, the scheduled task, the login the engine uses and the
//! score from which it may approve on its own.

use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};
use chrono_tz::America::Santiago;
use serde::Serialize;

use super::iso;
use crate::Panel;
use crate::scheduler::TaskAnswer;
use jobbot_app::actions::auto_approve_min;
use jobbot_app::privacy::UserPaths;
use jobbot_app::profile::claude::load_claude_config;
use jobbot_app::profile::load_schedule;
use jobbot_domain::privacy::mask_email;
use jobbot_domain::window::{in_run_window, next_window_start};

/// An engine whose last run is older than this has missed at least one night: "stale".
const ENGINE_STALE_HOURS: i64 = 36;

/// What the preflight of the last engine run found out about the Claude login
/// (`meta.claude_account_last`, written by `check_account`).
#[derive(Debug, Serialize, PartialEq)]
pub struct ClaudeAccount {
    /// The login was the private one and Claude was used.
    pub ok: bool,
    /// Why it was not, in Spanish.
    pub message: Option<String>,
    pub subscription: Option<String>,
    /// Masked (`g***@example.com`): a whole address never reaches the page.
    pub email: Option<String>,
    pub checked_at: Option<String>,
    /// How the login was made (`claude.ai` is the subscription; `api_key` and the like are not).
    pub auth_method: Option<String>,
    /// Who serves the model (`firstParty` is Anthropic).
    pub api_provider: Option<String>,
    pub org_id: Option<String>,
    /// The folder of the private login, with the folder of the Windows user replaced by
    /// `%LOCALAPPDATA%`, `%APPDATA%` or `%USERPROFILE%`.
    pub config_dir: Option<String>,
    /// The first word of `claude --version`, when the preflight could read it.
    pub cli_version: Option<String>,
    /// The profile fixes both `expected_email` and `expected_org_id`, so a login is compared with
    /// something. `false` is "cuenta sin anclar": any `pro` or `max` login in the own folder
    /// passes the check, whoever it belongs to. Read from the profile as it is now, not from the
    /// record of the check: it is what the next run will use. An unreadable profile counts as
    /// not anchored.
    pub anchored: bool,
}

/// The Windows scheduled task that runs the engine, as Windows last said (`panel::scheduler`).
/// `Controls.scheduled_task` is `null` when there is no answer: Windows has not been asked yet, it
/// failed, or it took too long. Only `exists: false` means that Windows answered and the task is
/// not registered.
#[derive(Debug, Serialize, PartialEq)]
pub struct ScheduledTask {
    pub exists: bool,
    pub next_run_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Controls {
    pub stopped: bool,
    /// `None` when `profile/schedule.toml` cannot be read.
    pub window_open: Option<bool>,
    /// Santiago time, only while the window is closed.
    pub next_window: Option<String>,
    /// When the engine last ran: `meta.last_run_at`, or else `meta.last_discover_at`.
    pub last_run_at: Option<String>,
    /// "never", "stale" (the last run is more than `ENGINE_STALE_HOURS` old) or "recent". The
    /// window being open says nothing about this: the page must not confuse the two.
    pub engine: &'static str,
    /// How the run that wrote `last_run_at` ended, as `run-daily` stored it in
    /// `meta.last_run_result`: `ok`, `stopped`, `window_closed` or `error: <one line>`. `None` when
    /// `last_run_at` is not a run's own stamp (no run yet, or only discovery's).
    pub last_run_result: Option<String>,
    /// What each step of that run did, as `run-daily` stored it in `meta.last_run_steps`: an
    /// object with `discover {new}`, `prepare {prepared}`, `submit {submitted, needs_gonza}` and
    /// `dossiers {written}`, each with `ran: true`, or `{ran: false, skipped: <why>}`.
    pub last_run_steps: Option<serde_json::Value>,
    pub claude_account: Option<ClaudeAccount>,
    pub scheduled_task: Option<ScheduledTask>,
    /// The score from which the engine may approve without Gonza (`meta.auto_approve_min`, set with
    /// `POST /api/settings` or `jobbot auto-approve`): a whole number from 1 to 100, or `null` when
    /// it is off, which is how it starts. A stored value that is not valid reads as off.
    pub auto_approve_min: Option<i64>,
}

/// The last run the engine recorded: `last_run_at` when a run wrote it, else the discovery stamp.
/// A value that is not a moment counts as none.
fn last_run_at(panel: &Panel) -> Result<Option<DateTime<Utc>>> {
    for key in ["last_run_at", "last_discover_at"] {
        let at = panel
            .tracker
            .get_meta(key)?
            .and_then(|text| DateTime::parse_from_rfc3339(&text).ok())
            .map(|at| at.with_timezone(&Utc));
        if at.is_some() {
            return Ok(at);
        }
    }
    Ok(None)
}

/// `meta.claude_account_last` as the page shows it; `None` when there is none or it is not the
/// JSON `check_account` writes. Empty strings are no value.
fn claude_account(panel: &Panel, user: &UserPaths) -> Result<Option<ClaudeAccount>> {
    let Some(text) = panel.tracker.get_meta("claude_account_last")? else {
        return Ok(None);
    };
    let Ok(serde_json::Value::Object(record)) = serde_json::from_str::<serde_json::Value>(&text)
    else {
        return Ok(None);
    };
    let field = |name: &str| {
        record
            .get(name)
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    Ok(Some(ClaudeAccount {
        ok: record.get("ok").and_then(serde_json::Value::as_bool) == Some(true),
        message: field("message").map(|text| user.hide_in_text(&text)),
        subscription: field("subscription"),
        // Stored masked already; masking again costs nothing and keeps a whole address out even
        // if an older record or a hand edit left one in `meta`.
        email: field("email").map(|text| mask_email(&text)),
        checked_at: field("checked_at"),
        auth_method: field("auth_method"),
        api_provider: field("api_provider"),
        org_id: field("org_id"),
        config_dir: field("config_dir").map(|path| user.hide_path(&path)),
        cli_version: field("cli_version"),
        anchored: load_claude_config(&panel.paths.profile_dir.join("claude.toml"))
            .is_ok_and(|config| config.is_anchored()),
    }))
}

/// The result and the steps of the last `run-daily`, with the folder of the user taken out of any
/// text. `run-daily` writes `meta.last_run_at` last, so they belong to a finished run only when
/// that stamp is there; with only discovery's stamp (or none) they are not given.
fn last_run_record(
    panel: &Panel,
    user: &UserPaths,
) -> Result<(Option<String>, Option<serde_json::Value>)> {
    let own_stamp = panel
        .tracker
        .get_meta("last_run_at")?
        .is_some_and(|text| DateTime::parse_from_rfc3339(&text).is_ok());
    if !own_stamp {
        return Ok((None, None));
    }
    let result = panel
        .tracker
        .get_meta("last_run_result")?
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .map(|text| user.hide_in_text(&text));
    let steps = panel
        .tracker
        .get_meta("last_run_steps")?
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .filter(serde_json::Value::is_object)
        .map(|steps| hide_in_strings(steps, user));
    Ok((result, steps))
}

/// `value` with `UserPaths::hide_in_text` applied to every string in it.
fn hide_in_strings(value: serde_json::Value, user: &UserPaths) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::String(text) => Value::String(user.hide_in_text(&text)),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| hide_in_strings(item, user))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, item)| (key, hide_in_strings(item, user)))
                .collect(),
        ),
        other => other,
    }
}

pub(super) fn controls(panel: &Panel, now: DateTime<Utc>) -> Result<Controls> {
    let user = UserPaths::from_env();
    let last_run = last_run_at(panel)?;
    let (last_run_result, last_run_steps) = last_run_record(panel, &user)?;
    let engine = match last_run {
        None => "never",
        Some(at) if now - at > TimeDelta::hours(ENGINE_STALE_HOURS) => "stale",
        Some(_) => "recent",
    };
    let schedule = load_schedule(&panel.paths.profile_dir.join("schedule.toml")).ok();
    let window_open = schedule.as_ref().map(|s| in_run_window(s, now));
    let next_window = match (&schedule, window_open) {
        (Some(s), Some(false)) => Some(
            next_window_start(s, now)
                .with_timezone(&Santiago)
                .format("%Y-%m-%d %H:%M")
                .to_string(),
        ),
        _ => None,
    };
    Ok(Controls {
        stopped: panel.paths.stop_file.exists(),
        window_open,
        next_window,
        last_run_at: last_run.map(iso),
        engine,
        last_run_result,
        last_run_steps,
        claude_account: claude_account(panel, &user)?,
        auto_approve_min: auto_approve_min(&*panel.tracker)?,
        scheduled_task: panel.scheduled_task().map(|task| match task {
            TaskAnswer::Missing => ScheduledTask {
                exists: false,
                next_run_at: None,
            },
            TaskAnswer::Registered { next_run_at } => ScheduledTask {
                exists: true,
                next_run_at: next_run_at.map(iso),
            },
        }),
    })
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;
    use crate::fixtures::panel;
    use crate::snapshot::build_snapshot;
    use crate::snapshot::support_tests::{SAMPLE_SCHEDULE, profile_with};
    use jobbot_testkit::NOW;

    #[test]
    fn controls_show_stop_and_tolerate_a_missing_schedule_and_claude_config() {
        let (mut panel, dir) = panel();
        panel.paths.profile_dir = profile_with(dir.path(), None, Some(SAMPLE_SCHEDULE));
        assert!(!build_snapshot(&panel).unwrap().controls.stopped);
        std::fs::write(&panel.paths.stop_file, "stop").unwrap();
        let controls = build_snapshot(&panel).unwrap().controls;
        assert!(controls.stopped);
        assert!(controls.window_open.is_some());
        // The budget is whatever the profile's own claude.toml says: this one is written here.
        panel.paths.profile_dir = profile_with(dir.path(), None, None);
        std::fs::write(
            panel.paths.profile_dir.join("claude.toml"),
            "daily_budget_usd = 2.25\n",
        )
        .unwrap();
        assert_eq!(
            build_snapshot(&panel).unwrap().claude.unwrap().daily_budget,
            Some(2.25)
        );
        panel.paths.profile_dir = dir.path().join("no-profile");
        let snap = build_snapshot(&panel).unwrap();
        assert_eq!(
            (snap.controls.window_open, snap.controls.next_window),
            (None, None)
        );
        assert_eq!(snap.claude.as_ref().unwrap().daily_budget, None);
    }

    /// The `controls` of a fresh snapshot, as the page receives them.
    fn controls_json(panel: &Panel) -> serde_json::Value {
        serde_json::to_value(build_snapshot(panel).unwrap().controls).unwrap()
    }

    #[test]
    fn controls_say_when_the_engine_last_ran() {
        let (panel, _dir) = panel();

        let never = controls_json(&panel);
        assert_eq!(never["engine"], "never");
        assert!(never["last_run_at"].is_null());
        assert!(never["claude_account"].is_null());
        assert!(never["scheduled_task"].is_null());

        // Only discovery ran, a day and a half ago: that counts as the last run, and it is stale.
        panel
            .tracker
            .set_meta(
                "last_discover_at",
                &(*NOW - TimeDelta::hours(37)).to_rfc3339(),
            )
            .unwrap();
        let stale = controls_json(&panel);
        assert_eq!(stale["engine"], "stale");
        assert_eq!(stale["last_run_at"], "2026-09-29T23:00:00Z");

        // A whole run wrote its own stamp: it wins over discovery's.
        panel
            .tracker
            .set_meta("last_run_at", &(*NOW - TimeDelta::hours(36)).to_rfc3339())
            .unwrap();
        let recent = controls_json(&panel);
        assert_eq!(
            recent["engine"], "recent",
            "36 hours is the edge, still recent"
        );
        assert_eq!(recent["last_run_at"], "2026-09-30T00:00:00Z");

        // A stamp that is not a moment is no stamp; the other key still counts.
        panel.tracker.set_meta("last_run_at", "ayer").unwrap();
        assert_eq!(controls_json(&panel)["last_run_at"], "2026-09-29T23:00:00Z");
    }

    #[test]
    fn controls_parse_the_claude_account_record() {
        let (panel, _dir) = panel();
        assert!(controls_json(&panel)["claude_account"].is_null());

        panel
            .tracker
            .set_meta(
                "claude_account_last",
                r#"{"auth_method":"api_key","subscription":"","email":"g***@example.com","org_id":"org-x","ok":false,"message":"Claude no se usó: la credencial activa es api_key","checked_at":"2026-10-01T09:00:00+00:00"}"#,
            )
            .unwrap();
        let account = controls_json(&panel)["claude_account"].clone();
        assert_eq!(account["ok"], false);
        assert_eq!(
            account["message"],
            "Claude no se usó: la credencial activa es api_key"
        );
        assert!(
            account["subscription"].is_null(),
            "an empty string is no value"
        );
        assert_eq!(account["email"], "g***@example.com");
        assert_eq!(account["checked_at"], "2026-10-01T09:00:00+00:00");
        assert_eq!(account["org_id"], "org-x");
        assert_eq!(account["auth_method"], "api_key");
        assert!(account["api_provider"].is_null());
        assert!(account["config_dir"].is_null());
        assert!(account["cli_version"].is_null());

        panel
            .tracker
            .set_meta(
                "claude_account_last",
                r#"{"subscription":"max","email":"g***@example.com","ok":true,"message":null,"checked_at":"2026-10-01T11:00:00+00:00"}"#,
            )
            .unwrap();
        let account = controls_json(&panel)["claude_account"].clone();
        assert_eq!(
            (account["ok"].as_bool(), account["subscription"].as_str()),
            (Some(true), Some("max"))
        );
        assert!(account["message"].is_null());

        panel
            .tracker
            .set_meta("claude_account_last", "not json")
            .unwrap();
        assert!(controls_json(&panel)["claude_account"].is_null());
    }

    #[test]
    fn the_account_says_whether_the_profile_pins_it_to_an_address_and_an_organization() {
        let (mut panel, dir) = panel();
        panel
            .tracker
            .set_meta(
                "claude_account_last",
                r#"{"subscription":"max","email":"g***@example.com","ok":true,"message":null,"checked_at":"2026-10-01T11:00:00+00:00"}"#,
            )
            .unwrap();
        // Its own profile folder: the developer's local file must not decide a test.
        let folder = profile_with(dir.path(), None, None);
        panel.paths.profile_dir = folder.clone();
        let anchored = |panel: &Panel| {
            controls_json(panel)["claude_account"]["anchored"]
                .as_bool()
                .expect("`anchored` is a boolean")
        };
        let write = |name: &str, text: &str| std::fs::write(folder.join(name), text).unwrap();

        // Nothing to compare the login with, or no way to read what there is: not anchored. A
        // verified account is not the same as an account that was checked against anything.
        assert!(!anchored(&panel), "no claude.toml at all");
        write("claude.toml", "model = \"claude-sonnet-5-5\"\n");
        assert!(!anchored(&panel), "the shipped file leaves both empty");
        write("claude.local.toml", "expected_email = \"me@example.com\"\n");
        assert!(!anchored(&panel), "the address without the organization");
        write(
            "claude.local.toml",
            "expected_email = \"me@example.com\"\nexpected_org_id = \"org-1\"\n",
        );
        assert!(anchored(&panel));
        write("claude.local.toml", "expected_email = [\n");
        assert!(!anchored(&panel), "a broken file anchors nothing");
    }

    #[test]
    fn controls_carry_the_auto_approval_threshold_or_null() {
        let (panel, _dir) = panel();
        let off = controls_json(&panel);
        assert!(
            off.as_object().unwrap().contains_key("auto_approve_min")
                && off["auto_approve_min"].is_null(),
            "off is a null that is there"
        );
        panel.tracker.set_meta("auto_approve_min", "90").unwrap();
        assert_eq!(controls_json(&panel)["auto_approve_min"], 90);
        // A damaged setting is off, never a number the page would trust.
        panel.tracker.set_meta("auto_approve_min", "999").unwrap();
        assert!(controls_json(&panel)["auto_approve_min"].is_null());
    }

    #[test]
    fn controls_carry_how_the_last_run_ended_and_what_each_step_did() {
        let (panel, _dir) = panel();
        let controls = controls_json;
        let meta = |key: &str, value: &str| panel.tracker.set_meta(key, value).unwrap();

        // No run yet: both keys are there, empty.
        let none = controls(&panel);
        assert!(none["last_run_result"].is_null() && none["last_run_steps"].is_null());
        assert!(none.as_object().unwrap().contains_key("last_run_result"));
        assert!(none.as_object().unwrap().contains_key("last_run_steps"));

        // What `run-daily` writes (`record_run` in `jobbot_app::reporting::status`): the result,
        // the steps, then the stamp.
        let steps = serde_json::json!({
            "discover": {"ran": true, "new": 3},
            "prepare": {"ran": true, "prepared": 2},
            "submit": {"ran": false, "skipped": "window_closed"},
            "dossiers": {"ran": false, "skipped": "window_closed"},
        });
        meta("last_run_result", "window_closed");
        meta("last_run_steps", &steps.to_string());
        meta("last_run_at", &(*NOW - TimeDelta::hours(2)).to_rfc3339());
        let ended = controls(&panel);
        assert_eq!(ended["last_run_result"], "window_closed");
        assert_eq!(
            ended["last_run_steps"], steps,
            "the steps come as they were written"
        );
        assert_eq!(ended["last_run_at"], "2026-10-01T10:00:00Z");

        // A failure carries its text, one line, without the folder of the user.
        meta(
            "last_run_result",
            r"error: no pude abrir C:\Users\ana\AppData\Local\jobbot\data.db",
        );
        assert_eq!(
            controls(&panel)["last_run_result"],
            r"error: no pude abrir %LOCALAPPDATA%\jobbot\data.db"
        );
        meta(
            "last_run_steps",
            r#"{"discover":{"ran":false,"skipped":"error","where":"C:\\Users\\ana\\x"}}"#,
        );
        assert_eq!(
            controls(&panel)["last_run_steps"]["discover"]["where"],
            r"%USERPROFILE%\x"
        );

        // What is not a record is no record: an empty result, steps that are not an object.
        meta("last_run_result", "  ");
        meta("last_run_steps", "[1, 2]");
        let odd = controls(&panel);
        assert!(odd["last_run_result"].is_null() && odd["last_run_steps"].is_null());
        meta("last_run_steps", "not json");
        assert!(controls(&panel)["last_run_steps"].is_null());

        // A result with no stamp of its own (only discovery's) does not belong to any finished run.
        meta("last_run_result", "ok");
        meta("last_run_steps", &steps.to_string());
        meta("last_run_at", "ayer");
        meta(
            "last_discover_at",
            &(*NOW - TimeDelta::hours(1)).to_rfc3339(),
        );
        let orphan = controls(&panel);
        assert_eq!(orphan["last_run_at"], "2026-10-01T11:00:00Z");
        assert!(orphan["last_run_result"].is_null() && orphan["last_run_steps"].is_null());
    }

    #[test]
    fn the_scheduled_task_is_null_when_nobody_knows_and_exists_false_only_when_windows_said_so() {
        use crate::scheduler::{FixedTask, TaskAnswer};

        // An empty `meta` and no question asked (A15): the engine never ran, and nobody knows
        // about the task. "No data" is `null`, never `exists: false`.
        let (bare, _dir) = panel();
        let controls = controls_json(&bare);
        assert_eq!(controls["engine"], "never");
        assert!(controls["last_run_at"].is_null());
        assert!(controls["scheduled_task"].is_null());

        let task_of = |answer: Option<TaskAnswer>| {
            let (panel, _dir) = panel();
            let panel = panel.with_task_query(FixedTask(answer));
            panel.tasks.settle();
            controls_json(&panel)["scheduled_task"].clone()
        };
        let next = *NOW + TimeDelta::minutes(65);
        assert_eq!(
            task_of(Some(TaskAnswer::Registered {
                next_run_at: Some(next)
            })),
            serde_json::json!({"exists": true, "next_run_at": "2026-10-01T13:05:00Z"})
        );
        assert_eq!(
            task_of(Some(TaskAnswer::Registered { next_run_at: None })),
            serde_json::json!({"exists": true, "next_run_at": null}),
            "a task that is registered but has nothing left to fire"
        );
        assert_eq!(
            task_of(Some(TaskAnswer::Missing)),
            serde_json::json!({"exists": false, "next_run_at": null})
        );
        assert!(
            task_of(None).is_null(),
            "Windows failed or took too long: no data"
        );
    }

    #[test]
    fn a_scheduler_that_never_answers_does_not_hold_up_the_snapshot() {
        use crate::scheduler::{TaskAnswer, TaskQuery};
        use std::sync::{Mutex, mpsc};

        struct Stuck(Mutex<mpsc::Receiver<()>>);
        impl TaskQuery for Stuck {
            fn ask(&self) -> Option<TaskAnswer> {
                let _ = self.0.lock().unwrap().recv();
                None
            }
        }
        let (release, wait) = mpsc::channel();
        let (panel, _dir) = panel();
        let panel = panel.with_task_query(Stuck(Mutex::new(wait)));

        let started = std::time::Instant::now();
        let snapshot = build_snapshot(&panel).unwrap();

        assert!(snapshot.controls.scheduled_task.is_none());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "the snapshot does not wait for Windows"
        );
        release.send(()).unwrap();
        panel.tasks.settle();
    }

    #[test]
    fn the_account_says_how_the_login_was_made_and_never_shows_a_whole_address_or_a_user_folder() {
        let (panel, _dir) = panel();
        let profile = std::env::var("USERPROFILE").unwrap_or_else(|_| r"C:\Users\ana".into());
        let forward = profile.replace('\\', "/");
        // What `check_account` stores, and worse: the address unmasked, the folder in the user's own
        // words with forward slashes (as `claude auth status` prints it), and a message that quotes
        // both.
        let record = serde_json::json!({
            "auth_method": "claude.ai",
            "api_provider": "firstParty",
            "subscription": "max",
            "email": "gina.roe@example.com",
            "org_id": "org-0000",
            "config_dir": format!("{forward}/AppData/Local/jobbot/claude-config"),
            "cli_version": "2.1.284",
            "warnings": [],
            "ok": false,
            "message": format!(
                "claude usa el directorio {profile}\\.claude, no {profile}\\AppData\\Local\\jobbot\\claude-config; sesión de gina.roe@example.com"
            ),
            "checked_at": "2026-10-01T09:00:00+00:00",
        });
        panel
            .tracker
            .set_meta("claude_account_last", &record.to_string())
            .unwrap();

        let snapshot = serde_json::to_value(build_snapshot(&panel).unwrap()).unwrap();
        let account = &snapshot["controls"]["claude_account"];

        assert_eq!(account["auth_method"], "claude.ai");
        assert_eq!(account["api_provider"], "firstParty");
        assert_eq!(account["subscription"], "max");
        assert_eq!(account["org_id"], "org-0000");
        assert_eq!(account["cli_version"], "2.1.284");
        assert_eq!(account["email"], "g***@example.com");
        let config_dir = account["config_dir"].as_str().unwrap();
        assert!(
            config_dir.starts_with('%') && config_dir.ends_with(r"\jobbot\claude-config"),
            "the folder of the user is a variable, got {config_dir}"
        );
        assert!(account.get("warnings").is_none());

        let text = snapshot.to_string().to_lowercase();
        let doubled = profile.replace('\\', r"\\"); // how a backslash looks inside JSON text
        for secret in ["gina.roe@", &profile, &forward, &doubled] {
            assert!(
                !text.contains(&secret.to_lowercase()),
                "the snapshot must not carry {secret:?}"
            );
        }
    }
}
