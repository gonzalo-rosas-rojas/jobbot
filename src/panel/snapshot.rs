//! The data the page draws, as plain serialisable structs. Built fresh from the tracker, the prep
//! store and a few small files on every request, so there is nothing to synchronise: building it
//! twice gives the same data, and whatever Gonza marks is in SQLite when the request returns.
//! Text from job boards (titles, descriptions, notes) is kept exactly as stored; the page draws it
//! with `textContent`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, DurationRound, SecondsFormat, TimeDelta, TimeZone, Utc};
use chrono_tz::America::Santiago;
use serde::Serialize;

use super::Panel;
use super::privacy::UserPaths;
use super::scheduler::TaskAnswer;
use crate::actions::{auto_approve_min, marks_from};
use crate::claude::{load_claude_config, mask_email};
use crate::discovery::MAY_HAVE_BEEN_SENT;
use crate::dossier::dossier_path;
use crate::handoff::sheet_path;
use crate::prep::{GapRow, TopicProgress};
use crate::rules::{Criteria, load_criteria};
use crate::today::RECENT_DAYS;
use crate::tracker::{ApplicationRow, State, TaskRow};
use crate::window::{in_run_window, load_schedule, next_window_start};

/// Cards shown per board column, and in the queue of things that need Gonza.
pub const COLUMN_CAP: usize = 25;
pub const QUEUE_CAP: usize = 50;
/// Applications the company still has to answer or decide on: the page lists every one of them.
const LIVE_STATES: [State; 6] = [
    State::Submitted,
    State::Replied,
    State::TechnicalTest,
    State::Interview,
    State::Offer,
    State::Ghosted,
];
const DESCRIPTION_CAP: usize = 8_000;
const DOSSIER_CAP: usize = 30_000;
/// The weekly pipeline looks at the postings first seen in the last 7 days.
const PIPELINE_DAYS: i64 = 7;
/// The night strip: 48 half-hour slots, the last one holding "now"; its list of state changes
/// shows the newest `MOVES_CAP`.
const SLOT_MINUTES: i64 = 30;
const SLOT_COUNT: usize = 48;
const MOVES_CAP: usize = 60;
/// An engine whose last run is older than this has missed at least one night: "stale".
const ENGINE_STALE_HOURS: i64 = 36;

#[derive(Debug, Serialize)]
pub struct Card {
    pub key: String,
    pub company: String,
    pub title: String,
    pub score: i64,
    pub state: &'static str,
    pub source: String,
    pub url: String,
    /// Why it needs Gonza (only while it is in `needs_gonza`).
    pub reason: Option<String>,
    /// The kind of help behind `reason`, so the page can group the queue: "manual_apply",
    /// "missing_answers", "handoff", "blocked", "may_have_sent", "error" or "other".
    pub reason_kind: Option<&'static str>,
    /// One plain sentence for `reason_kind`; the long `reason` stays behind a fold.
    pub reason_short: Option<&'static str>,
    /// The last note: an interview date, "confirmada por Gonza", the reason for a stop.
    pub note: Option<String>,
    pub approved: bool,
    pub has_screenshot: bool,
    /// The tailored CV is still on disk.
    pub has_cv: bool,
    pub has_dossier: bool,
    /// The answer sheet written for a hand-over (`data/handoff/<key>.md`) is on disk. It stays
    /// after the application was sent: this says the file exists, not that the hand-over is open.
    pub has_handoff: bool,
    pub updated_at: String,
    /// Buttons for the engine's own decisions: "approve", "resolve", "confirm".
    pub actions: Vec<&'static str>,
    /// States Gonza may record by hand from here.
    pub marks: Vec<&'static str>,
    /// The date the posting says it was published, and when it was first stored.
    pub posted_at: Option<String>,
    pub first_seen: Option<String>,
    /// Monthly USD as the posting says it; most say nothing.
    pub salary_min: Option<f64>,
    pub salary_max: Option<f64>,
    /// Whole days since it last entered its current state.
    pub days_in_state: i64,
    /// Gonza put it off until then; only while that moment is still ahead.
    pub snoozed_until: Option<String>,
    /// When it entered `needs_gonza` (only there): how long the queue has waited for him.
    pub waiting_since: Option<String>,
    /// Whole days since `posted_at`; `None` when the posting has no date.
    pub age_days: Option<i64>,
    /// Days left before discovery's prune withdraws it (`max_posting_age_days` minus its age by
    /// the gate's rule: the posting's date, or the day it was first seen). Only while it was
    /// never sent and `criteria.toml` can be read; negative when the prune is already due.
    pub prune_in_days: Option<i64>,
}

/// A list that may have been cut: `total` is how many exist, `cards` how many are shown.
#[derive(Debug, Serialize)]
pub struct Listing {
    pub total: usize,
    pub cards: Vec<Card>,
}

#[derive(Debug, Serialize)]
pub struct Column {
    pub state: &'static str,
    pub label: &'static str,
    pub total: usize,
    pub cards: Vec<Card>,
}

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

#[derive(Debug, Serialize, PartialEq)]
pub struct Metrics {
    pub sent_week: i64,
    pub responses_week: i64,
    pub response_rate: Option<f64>,
    pub tests_week: i64,
    pub interviews_week: i64,
    pub interviews_total: i64,
    pub offers_total: i64,
    pub interview_to_offer: Option<f64>,
}

/// How far the applications have got, all time. Each application counts once per stage it ever
/// reached: it is the line of stations of the "Procesos" tab, not today's board. The technical
/// test is optional, so `interviews` can exceed `tests`; only `answered` bounds the three stages
/// after it.
#[derive(Debug, Serialize, PartialEq)]
pub struct Funnel {
    pub sent: i64,
    /// Any answer at all: a reply, a test, an interview, an offer or a rejection.
    pub answered: i64,
    pub tests: i64,
    pub interviews: i64,
    pub offers: i64,
    /// The same stages per source the posting came from, with the base `n` of each. Counts only:
    /// with a handful of sends per source a rate would be noise, so the page shows "2 of 11".
    pub by_source: BTreeMap<String, SourceFunnel>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct SourceFunnel {
    /// Applications of the source, sent or not.
    pub n: i64,
    pub sent: i64,
    pub answered: i64,
    pub tests: i64,
    pub interviews: i64,
    pub offers: i64,
}

/// How far the postings first seen in the last `days` days got. Each count is "reached at least
/// this layer", so `sent <= complete <= with_cv <= above <= seen`.
#[derive(Debug, Serialize, PartialEq)]
pub struct Pipeline {
    pub days: i64,
    /// `apply_threshold` of `profile/criteria.toml`; `None` when it cannot be read.
    pub threshold: Option<f64>,
    pub seen: i64,
    /// With an application, whatever its state (withdrawn included).
    pub above: i64,
    /// With a CV on disk, or further along.
    pub with_cv: i64,
    /// Sent, or prepared with its dry-run screenshot on disk.
    pub complete: i64,
    pub sent: i64,
    /// 101 counts: how many of the `seen` have each score, rounded and held to 0..=100.
    pub score_counts: Vec<u32>,
}

/// How much room the engine has to send, as the gate counts it. `None` in the snapshot when
/// `profile/criteria.toml` cannot be read.
#[derive(Debug, Serialize, PartialEq)]
pub struct Trust {
    /// Submissions Gonza approved before they went out.
    pub reviewed: i64,
    pub first_n: u32,
    /// Fewer than `first_n` reviewed: the lower cap applies and each form needs his approval.
    pub review_phase: bool,
    /// Clicks of the last 24 hours, confirmed or not.
    pub sent_24h: i64,
    /// The daily cap in force now.
    pub cap: u32,
    /// The daily cap once the review phase is over.
    pub cap_after: u32,
}

/// One half hour of the night strip.
#[derive(Debug, Serialize)]
pub struct Slot {
    /// Start of the slot.
    pub at: String,
    /// The run window is open at the start of the slot; `None` when `schedule.toml` cannot be read.
    pub open: Option<bool>,
    /// Postings first seen inside the slot.
    pub seen: i64,
}

/// A state change in the strip's span, with the posting it happened to.
#[derive(Debug, Serialize)]
pub struct Move {
    pub at: String,
    pub key: String,
    pub company: String,
    pub title: String,
    pub to: &'static str,
}

/// The last 24 hours in half hours: what was found and what changed state. `from` is the start of
/// the first slot and `to` the end of the last, the one that holds "now".
#[derive(Debug, Serialize)]
pub struct Activity {
    pub from: String,
    pub to: String,
    pub slot_minutes: i64,
    pub slots: Vec<Slot>,
    /// Newest first, cut at `MOVES_CAP`; `moves_total` says how many there are.
    pub moves: Vec<Move>,
    pub moves_total: i64,
    /// Every state change of the span by the state it went into, not cut at `MOVES_CAP`: the
    /// counts add up to `moves_total`. Only the states that had a change appear.
    pub moves_by_state: BTreeMap<&'static str, i64>,
}

#[derive(Debug, Serialize)]
pub struct StageSpend {
    pub stage: String,
    pub calls: i64,
    pub dollars: f64,
}

#[derive(Debug, Serialize)]
pub struct ClaudeSpend {
    pub spent_24h: f64,
    /// `None` when `profile/claude.toml` cannot be read.
    pub daily_budget: Option<f64>,
    pub by_stage: Vec<StageSpend>,
}

#[derive(Debug, Serialize)]
pub struct WeakTopic {
    pub topic: String,
    pub average: f64,
}

/// A card that a dossier added, with the offer it came from.
#[derive(Debug, Serialize)]
pub struct GapView {
    pub id: String,
    pub topic: String,
    pub offer_key: String,
    pub offer: String,
    pub seen: bool,
}

#[derive(Debug, Serialize)]
pub struct Learn {
    pub total: usize,
    pub seen: usize,
    pub due: usize,
    pub weak_topics: Vec<WeakTopic>,
    pub topics: Vec<TopicProgress>,
    pub gaps: Vec<GapView>,
}

/// One of Gonza's open tasks, as "Tu lista de hoy" shows it.
#[derive(Debug, Serialize)]
pub struct TodayTask {
    pub id: i64,
    pub text: String,
    pub job_key: Option<String>,
    pub due_at: Option<String>,
    pub overdue: bool,
    /// An http(s) address that came with a task added by hand; the page shows it as a link.
    pub link: Option<String>,
    /// "manual" for one added by hand, else the kind of the email it came from.
    pub kind: String,
    pub created_at: String,
}

/// A recent email that is not an acknowledgement. The text came from someone else's email: the
/// page draws it with `textContent`.
#[derive(Debug, Serialize)]
pub struct TodayEmail {
    pub received_at: String,
    pub from: String,
    /// The company of the posting the email is linked to, when it is linked.
    pub company: Option<String>,
    pub job_key: Option<String>,
    pub kind: &'static str,
    pub subject: String,
    pub summary: String,
}

/// "Tu lista de hoy": the open tasks (soonest due first) and the answers of the last days. The
/// lists are cut at `QUEUE_CAP`; the totals say how many there are.
#[derive(Debug, Serialize)]
pub struct Today {
    pub tasks_total: usize,
    pub tasks: Vec<TodayTask>,
    pub emails_total: usize,
    pub emails: Vec<TodayEmail>,
}

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub generated_at: String,
    pub controls: Controls,
    pub metrics: Metrics,
    pub funnel: Funnel,
    pub pipeline: Pipeline,
    pub trust: Option<Trust>,
    pub activity: Activity,
    pub claude: ClaudeSpend,
    pub today: Today,
    /// Minus what is snoozed (see `snoozed`).
    pub needs_gonza: Listing,
    pub awaiting_approval: Listing,
    /// Every application Gonza put off and whose time has not come, soonest back first. They stay
    /// in `board`; they leave `needs_gonza`, `awaiting_approval` and the tasks of `today`.
    pub snoozed: Listing,
    pub board: Vec<Column>,
    pub learn: Learn,
}

#[derive(Debug, Serialize)]
pub struct EventView {
    pub at: String,
    pub from: Option<&'static str>,
    pub to: &'static str,
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JobDetail {
    pub card: Card,
    pub location: String,
    pub remote: bool,
    pub apply_url: String,
    pub salary_min: Option<f64>,
    pub salary_max: Option<f64>,
    pub description: String,
    pub score_detail: BTreeMap<String, f64>,
    pub events: Vec<EventView>,
    pub dossier: Option<String>,
    pub gaps: Vec<GapView>,
    /// When the posting was first stored.
    pub first_seen: Option<String>,
    /// The date the posting says it was published, when it says one.
    pub posted_at: Option<String>,
}

fn label(state: State) -> &'static str {
    match state {
        State::Scored => "Puntuadas",
        State::Prepared => "Preparadas",
        State::NeedsGonza => "Necesitan a Gonza",
        State::Submitted => "Enviadas",
        State::Replied => "Respondieron",
        State::TechnicalTest => "Prueba técnica",
        State::Interview => "Entrevista",
        State::Offer => "Oferta",
        State::Accepted => "Aceptadas",
        State::Rejected => "Rechazadas",
        State::Withdrawn => "Retiradas",
        State::Ghosted => "Sin respuesta",
    }
}

fn iso(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max_chars).collect::<String>())
    }
}

fn ratio(part: i64, whole: i64) -> Option<f64> {
    (whole > 0).then(|| part as f64 / whole as f64)
}

/// The reason says the form may have gone out although the application never reached `Submitted`
/// (including a run that died between the click and its confirmation, `INTERRUPTED_PREFIX`).
/// The handoff reasons are not in this set: they are a form Gonza still has to send.
fn may_have_sent(reason: &str) -> bool {
    MAY_HAVE_BEEN_SENT
        .iter()
        .any(|prefix| reason.starts_with(prefix))
}

/// Sorts a `needs_gonza` reason by the prefix the engine writes, with the sentence the page shows.
fn reason_kind(reason: &str) -> (&'static str, &'static str) {
    let reason = reason.trim();
    if reason.starts_with("listing_only") {
        ("manual_apply", "Postula tú desde el enlace del aviso")
    } else if reason.starts_with("faltan respuestas") {
        (
            "missing_answers",
            "El formulario pide respuestas que no tengo",
        )
    } else if reason.starts_with("handoff") {
        (
            "handoff",
            "Envía tú el formulario: la hoja de respuestas está lista",
        )
    } else if matches!(reason, "captcha" | "login_required" | "blocked_page") {
        (
            "blocked",
            "Una página me frenó (captcha, ingreso o bloqueo)",
        )
    } else if may_have_sent(reason) {
        (
            "may_have_sent",
            "Puede que ya se haya enviado: revisa antes de volver a postular",
        )
    } else if reason.starts_with("page_error") {
        ("error", "La página falló al llenarla: hay que mirarla")
    } else {
        ("other", "Necesita una mirada tuya")
    }
}

/// What a card needs besides its row: the clock, the dossier and hand-over folders and the age limit.
pub(super) struct CardContext<'a> {
    now: DateTime<Utc>,
    dossiers: &'a Path,
    handoffs: &'a Path,
    /// `max_posting_age_days` of `profile/criteria.toml`; `None` when it cannot be read.
    max_age_days: Option<i64>,
}

/// Whole days from `since` to `now`, never negative (a posting dated in the future is 0 days old).
fn days_since(now: DateTime<Utc>, since: DateTime<Utc>) -> i64 {
    (now - since).num_days().max(0)
}

/// A snooze that still holds at `now`.
fn snoozed_at(row: &ApplicationRow, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    row.snoozed_until.filter(|until| *until > now)
}

pub(super) fn card(row: &ApplicationRow, ctx: &CardContext<'_>) -> Card {
    let mut actions = Vec::new();
    match row.state {
        State::NeedsGonza => {
            actions.push("resolve");
            actions.push("confirm");
        }
        State::Prepared if row.screenshot_path.is_some() && !row.approved => {
            actions.push("approve");
        }
        _ => {}
    }
    let reason = (row.state == State::NeedsGonza)
        .then(|| row.waiting_reason().map(str::to_string))
        .flatten();
    let kind = reason.as_deref().map(reason_kind);
    // When it may already have gone out, the first answer is whether it did: sending it back to the
    // queue (which clears the unconfirmed click) comes second, so the page never leads with a retry.
    if kind.is_some_and(|(kind, _)| kind == "may_have_sent") {
        actions.sort_by_key(|action| *action != "confirm");
    }
    // A row always has the event that created it; `updated_at` only covers a history that was lost.
    let state_since = row.state_since.unwrap_or(row.updated_at);
    let unsent = row.submitted_at.is_none()
        && matches!(
            row.state,
            State::Scored | State::Prepared | State::NeedsGonza
        );
    let prune_in_days = match ctx.max_age_days {
        Some(max) if unsent => {
            Some(max - days_since(ctx.now, row.posted_at.unwrap_or(row.first_seen)))
        }
        _ => None,
    };
    Card {
        key: row.job_key.clone(),
        company: row.company.clone(),
        title: row.title.clone(),
        score: row.score.round() as i64,
        state: row.state.as_str(),
        source: row.source.clone(),
        url: row.url.clone(),
        reason,
        reason_kind: kind.map(|(kind, _)| kind),
        reason_short: kind.map(|(_, short)| short),
        note: row.note.clone(),
        approved: row.approved,
        has_screenshot: row
            .screenshot_path
            .as_deref()
            .is_some_and(|p| Path::new(p).is_file()),
        has_cv: row
            .cv_path
            .as_deref()
            .is_some_and(|p| Path::new(p).is_file()),
        has_dossier: dossier_path(ctx.dossiers, &row.job_key).is_file(),
        has_handoff: sheet_path(ctx.handoffs, &row.job_key).is_file(),
        updated_at: iso(row.updated_at),
        actions,
        marks: marks_from(row.state)
            .into_iter()
            .map(|s| s.as_str())
            .collect(),
        posted_at: row.posted_at.map(iso),
        first_seen: Some(iso(row.first_seen)),
        salary_min: row.salary_min,
        salary_max: row.salary_max,
        days_in_state: days_since(ctx.now, state_since),
        snoozed_until: snoozed_at(row, ctx.now).map(iso),
        waiting_since: (row.state == State::NeedsGonza).then(|| iso(state_since)),
        age_days: row.posted_at.map(|at| days_since(ctx.now, at)),
        prune_in_days,
    }
}

fn listing(rows: &[ApplicationRow], cap: usize, ctx: &CardContext<'_>) -> Listing {
    Listing {
        total: rows.len(),
        cards: rows.iter().take(cap).map(|r| card(r, ctx)).collect(),
    }
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

fn controls(panel: &Panel, now: DateTime<Utc>) -> Result<Controls> {
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
        auto_approve_min: auto_approve_min(&panel.tracker)?,
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

fn metrics(panel: &Panel, now: DateTime<Utc>) -> Result<Metrics> {
    let t = &panel.tracker;
    let week = now - TimeDelta::days(7);
    let ever = Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
    let sent_week = t.submitted_since(week)?;
    let responses_week = t.jobs_reaching(
        &[
            State::Replied,
            State::TechnicalTest,
            State::Interview,
            State::Offer,
            State::Rejected,
        ],
        week,
    )?;
    let interviews_total = t.jobs_reaching(&[State::Interview], ever)?;
    let offers_total = t.jobs_reaching(&[State::Offer], ever)?;
    Ok(Metrics {
        sent_week,
        responses_week,
        response_rate: ratio(responses_week, sent_week),
        tests_week: t.jobs_reaching(&[State::TechnicalTest], week)?,
        interviews_week: t.jobs_reaching(&[State::Interview], week)?,
        interviews_total,
        offers_total,
        interview_to_offer: ratio(offers_total, interviews_total),
    })
}

fn funnel(panel: &Panel) -> Result<Funnel> {
    let t = &panel.tracker;
    let ever = Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
    Ok(Funnel {
        sent: t.submitted_since(ever)?,
        answered: t.jobs_reaching(
            &[
                State::Replied,
                State::TechnicalTest,
                State::Interview,
                State::Offer,
                State::Rejected,
            ],
            ever,
        )?,
        tests: t.jobs_reaching(&[State::TechnicalTest], ever)?,
        interviews: t.jobs_reaching(&[State::Interview], ever)?,
        offers: t.jobs_reaching(&[State::Offer], ever)?,
        by_source: t
            .funnel_by_source()?
            .into_iter()
            .map(|row| {
                (
                    row.source,
                    SourceFunnel {
                        n: row.n,
                        sent: row.sent,
                        answered: row.answered,
                        tests: row.tests,
                        interviews: row.interviews,
                        offers: row.offers,
                    },
                )
            })
            .collect(),
    })
}

/// The weekly pipeline. The layers nest by construction: only an application can have a CV, a
/// screenshot or a submission, so the counts only ever shrink from `seen` to `sent`.
fn pipeline(panel: &Panel, now: DateTime<Utc>, criteria: Option<&Criteria>) -> Result<Pipeline> {
    let on_disk = |path: &Option<String>| path.as_deref().is_some_and(|p| Path::new(p).is_file());
    let mut layers = [0i64; 5];
    let mut score_counts = vec![0u32; 101];
    for row in panel
        .tracker
        .cohort_since(now - TimeDelta::days(PIPELINE_DAYS))?
    {
        let sent = row.submitted_at.is_some();
        let complete =
            sent || (row.state == Some(State::Prepared) && on_disk(&row.screenshot_path));
        let with_cv = complete || on_disk(&row.cv_path);
        let above = row.state.is_some();
        for (count, reached) in layers
            .iter_mut()
            .zip([true, above, with_cv, complete, sent])
        {
            *count += i64::from(reached);
        }
        // A score is held to 0..=100 on the page's axis; NaN lands on 0.
        score_counts[row.score.round().clamp(0.0, 100.0) as usize] += 1;
    }
    let [seen, above, with_cv, complete, sent] = layers;
    Ok(Pipeline {
        days: PIPELINE_DAYS,
        threshold: criteria.map(|c| c.apply_threshold),
        seen,
        above,
        with_cv,
        complete,
        sent,
        score_counts,
    })
}

/// The gate's own numbers: reviewed sends, clicks of the last 24 hours and the cap in force.
fn trust(panel: &Panel, now: DateTime<Utc>, criteria: Option<&Criteria>) -> Result<Option<Trust>> {
    let Some(criteria) = criteria else {
        return Ok(None);
    };
    let reviewed = panel.tracker.approved_submitted_total()?;
    let review_phase = reviewed < i64::from(criteria.first_n_reviewed);
    Ok(Some(Trust {
        reviewed,
        first_n: criteria.first_n_reviewed,
        review_phase,
        sent_24h: panel.tracker.clicked_since(now - TimeDelta::hours(24))?,
        cap: if review_phase {
            criteria.daily_cap_until_reviewed
        } else {
            criteria.daily_cap
        },
        cap_after: criteria.daily_cap,
    }))
}

/// The last 24 hours in half-hour slots aligned to the clock (UTC, whose half hours are also
/// Santiago's): the last slot is the one that holds `now`.
fn activity(panel: &Panel, now: DateTime<Utc>) -> Result<Activity> {
    let last = now.duration_trunc(TimeDelta::minutes(SLOT_MINUTES))?;
    let first = last - TimeDelta::minutes(SLOT_MINUTES * (SLOT_COUNT as i64 - 1));
    let end = last + TimeDelta::minutes(SLOT_MINUTES);
    let mut seen = vec![0i64; SLOT_COUNT];
    for at in panel.tracker.first_seen_since(first)? {
        let slot = usize::try_from((at - first).num_seconds() / (SLOT_MINUTES * 60));
        if let Some(count) = slot.ok().and_then(|n| seen.get_mut(n)) {
            *count += 1;
        }
    }
    let schedule = load_schedule(&panel.paths.profile_dir.join("schedule.toml")).ok();
    let slots = seen
        .into_iter()
        .enumerate()
        .map(|(n, seen)| {
            let start = first + TimeDelta::minutes(SLOT_MINUTES * n as i64);
            Slot {
                at: iso(start),
                open: schedule.as_ref().map(|s| in_run_window(s, start)),
                seen,
            }
        })
        .collect();
    let (moves_total, moves) = panel.tracker.moves_between(first, now, MOVES_CAP)?;
    let moves_by_state = panel
        .tracker
        .move_counts_between(first, now)?
        .into_iter()
        .map(|(state, n)| (state.as_str(), n))
        .collect();
    Ok(Activity {
        from: iso(first),
        to: iso(end),
        slot_minutes: SLOT_MINUTES,
        slots,
        moves: moves
            .into_iter()
            .map(|m| Move {
                at: iso(m.at),
                key: m.job_key,
                company: m.company,
                title: m.title,
                to: m.to_state.as_str(),
            })
            .collect(),
        moves_total,
        moves_by_state,
    })
}

fn claude_spend(panel: &Panel, now: DateTime<Utc>) -> Result<ClaudeSpend> {
    let since = now - TimeDelta::hours(24);
    let daily_budget = load_claude_config(&panel.paths.profile_dir.join("claude.toml"))
        .ok()
        .map(|config| config.daily_budget_usd);
    let by_stage = panel
        .tracker
        .claude_calls_by_stage(since)?
        .into_iter()
        .map(|(stage, (calls, dollars))| StageSpend {
            stage,
            calls,
            dollars,
        })
        .collect();
    Ok(ClaudeSpend {
        spent_24h: panel.tracker.claude_spend_since(since)?,
        daily_budget,
        by_stage,
    })
}

/// `snoozed` are the keys of the applications put off: a task about one of them waits with it.
fn today(panel: &Panel, now: DateTime<Utc>, snoozed: &BTreeSet<String>) -> Result<Today> {
    let tasks: Vec<TaskRow> = panel
        .tracker
        .open_tasks()?
        .into_iter()
        .filter(|task| {
            !task
                .job_key
                .as_ref()
                .is_some_and(|key| snoozed.contains(key))
        })
        .collect();
    let emails = panel
        .tracker
        .recent_emails(now - TimeDelta::days(RECENT_DAYS))?;
    Ok(Today {
        tasks_total: tasks.len(),
        tasks: tasks
            .iter()
            .take(QUEUE_CAP)
            .map(|task| TodayTask {
                id: task.id,
                text: task.text.clone(),
                job_key: task.job_key.clone(),
                due_at: task.due_at.map(iso),
                overdue: task.is_overdue(now),
                link: task.link.clone(),
                kind: task.kind.clone(),
                created_at: iso(task.created_at),
            })
            .collect(),
        emails_total: emails.len(),
        emails: emails
            .into_iter()
            .take(QUEUE_CAP)
            .map(|email| TodayEmail {
                received_at: iso(email.received_at),
                from: email.from_addr,
                company: email.company,
                job_key: email.job_key,
                kind: email.kind.as_str(),
                subject: email.subject,
                summary: email.summary,
            })
            .collect(),
    })
}

/// "Company — Title" of a posting, or the key itself when the posting is unknown.
pub fn offer_label(panel: &Panel, key: &str) -> String {
    match panel.tracker.get_job(key) {
        Ok(Some(job)) => format!("{} — {}", job.company, job.title),
        _ => key.to_string(),
    }
}

fn gap_view(panel: &Panel, gap: GapRow) -> GapView {
    GapView {
        offer: offer_label(panel, &gap.source),
        offer_key: gap.source,
        id: gap.id,
        topic: gap.topic,
        seen: gap.seen,
    }
}

fn learn(panel: &Panel, now: DateTime<Utc>) -> Result<Learn> {
    let questions = panel.store.all_questions()?;
    let stats = panel.store.stats(&questions, now)?;
    Ok(Learn {
        total: stats.total,
        seen: stats.seen,
        due: stats.due,
        weak_topics: panel
            .store
            .weak_topics(5, 10)?
            .into_iter()
            .map(|(topic, average)| WeakTopic { topic, average })
            .collect(),
        topics: panel.store.topic_progress(&questions, now)?,
        gaps: panel
            .store
            .gap_rows()?
            .into_iter()
            .map(|gap| gap_view(panel, gap))
            .collect(),
    })
}

pub(super) fn card_context<'a>(
    now: DateTime<Utc>,
    dossiers: &'a Path,
    handoffs: &'a Path,
    criteria: Option<&Criteria>,
) -> CardContext<'a> {
    CardContext {
        now,
        dossiers,
        handoffs,
        max_age_days: criteria.map(|c| c.max_posting_age_days),
    }
}

pub fn build_snapshot(panel: &Panel) -> Result<Snapshot> {
    let now = panel.now();
    let dossiers = panel.paths.data_dir.join("dossiers");
    let handoffs = panel.paths.data_dir.join("handoff");
    let criteria = load_criteria(&panel.paths.profile_dir.join("criteria.toml")).ok();
    let ctx = card_context(now, &dossiers, &handoffs, criteria.as_ref());
    let mut board = Vec::new();
    // What Gonza put off, from every column: the board keeps it, the queues and today's tasks skip it.
    let mut snoozed: Vec<ApplicationRow> = Vec::new();
    for state in State::ALL {
        let rows = panel.tracker.list_by_state(state)?;
        snoozed.extend(
            rows.iter()
                .filter(|r| snoozed_at(r, now).is_some())
                .cloned(),
        );
        // The Procesos tab draws one row per application in progress, so those columns are never cut.
        let cap = if LIVE_STATES.contains(&state) {
            usize::MAX
        } else {
            COLUMN_CAP
        };
        let shown = listing(&rows, cap, &ctx);
        board.push(Column {
            state: state.as_str(),
            label: label(state),
            total: shown.total,
            cards: shown.cards,
        });
    }
    snoozed.sort_by_key(|r| (r.snoozed_until, r.job_key.clone()));
    let snoozed_keys: BTreeSet<String> = snoozed.iter().map(|r| r.job_key.clone()).collect();
    let needs: Vec<ApplicationRow> = panel
        .tracker
        .list_by_state(State::NeedsGonza)?
        .into_iter()
        .filter(|r| !snoozed_keys.contains(&r.job_key))
        .collect();
    let awaiting: Vec<ApplicationRow> = panel
        .tracker
        .list_by_state(State::Prepared)?
        .into_iter()
        .filter(|r| r.screenshot_path.is_some() && !r.approved)
        .filter(|r| !snoozed_keys.contains(&r.job_key))
        .collect();
    Ok(Snapshot {
        generated_at: iso(now),
        controls: controls(panel, now)?,
        metrics: metrics(panel, now)?,
        funnel: funnel(panel)?,
        pipeline: pipeline(panel, now, criteria.as_ref())?,
        trust: trust(panel, now, criteria.as_ref())?,
        activity: activity(panel, now)?,
        claude: claude_spend(panel, now)?,
        today: today(panel, now, &snoozed_keys)?,
        needs_gonza: listing(&needs, QUEUE_CAP, &ctx),
        awaiting_approval: listing(&awaiting, QUEUE_CAP, &ctx),
        snoozed: listing(&snoozed, QUEUE_CAP, &ctx),
        board,
        learn: learn(panel, now)?,
    })
}

/// Everything about one application for the detail drawer; `None` when there is none.
pub fn build_job_detail(panel: &Panel, key: &str) -> Result<Option<JobDetail>> {
    let Some(row) = panel.tracker.get_application(key)? else {
        return Ok(None);
    };
    let Some(job) = panel.tracker.get_job(key)? else {
        return Ok(None);
    };
    let dossiers = panel.paths.data_dir.join("dossiers");
    let handoffs = panel.paths.data_dir.join("handoff");
    let criteria = load_criteria(&panel.paths.profile_dir.join("criteria.toml")).ok();
    let ctx = card_context(panel.now(), &dossiers, &handoffs, criteria.as_ref());
    let events = panel
        .tracker
        .events_for(key)?
        .into_iter()
        .map(|e| EventView {
            at: iso(e.at),
            from: e.from_state.map(|s| s.as_str()),
            to: e.to_state.as_str(),
            note: e.note,
        })
        .collect();
    let dossier = std::fs::read_to_string(dossier_path(&dossiers, key))
        .ok()
        .map(|text| truncate(&text, DOSSIER_CAP));
    let gaps = panel
        .store
        .gap_rows()?
        .into_iter()
        .filter(|gap| gap.source == key)
        .map(|gap| gap_view(panel, gap))
        .collect();
    Ok(Some(JobDetail {
        card: card(&row, &ctx),
        location: job.location,
        remote: job.remote,
        apply_url: job.apply_url,
        salary_min: job.salary_min_monthly_usd,
        salary_max: job.salary_max_monthly_usd,
        description: truncate(&job.description, DESCRIPTION_CAP),
        score_detail: panel.tracker.score_detail(key)?,
        events,
        dossier,
        gaps,
        first_seen: panel.tracker.job_first_seen(key)?.map(iso),
        posted_at: job.posted_at.map(iso),
    }))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use chrono::{TimeDelta, Timelike};

    use super::*;
    use crate::panel::fixtures::{add, panel};
    use crate::paths::Paths;
    use crate::prep::{PrepStore, Question, load_questions};
    use crate::rules::load_criteria;
    use crate::testing::{NOW, make_job_with, real_paths};
    use crate::tracker::{EmailKind, NewEmail, NewTask, Tracker};

    fn column<'a>(snap: &'a Snapshot, state: &str) -> &'a Column {
        snap.board.iter().find(|c| c.state == state).unwrap()
    }

    #[test]
    fn an_empty_database_gives_an_empty_but_complete_snapshot() {
        let (panel, _dir) = panel();
        let snap = build_snapshot(&panel).unwrap();
        assert_eq!(snap.board.len(), 12);
        assert!(
            snap.board
                .iter()
                .all(|c| c.total == 0 && c.cards.is_empty())
        );
        assert_eq!(snap.needs_gonza.total, 0);
        assert_eq!(snap.awaiting_approval.total, 0);
        assert_eq!(
            snap.metrics,
            Metrics {
                sent_week: 0,
                responses_week: 0,
                response_rate: None,
                tests_week: 0,
                interviews_week: 0,
                interviews_total: 0,
                offers_total: 0,
                interview_to_offer: None
            }
        );
        assert_eq!(
            snap.funnel,
            Funnel {
                sent: 0,
                answered: 0,
                tests: 0,
                interviews: 0,
                offers: 0,
                by_source: BTreeMap::new(),
            }
        );
        assert_eq!(snap.snoozed.total, 0);
        assert_eq!(snap.controls.engine, "never");
        assert!(snap.controls.last_run_at.is_none() && snap.controls.claude_account.is_none());
        let bank = load_questions().unwrap().len();
        assert_eq!(
            (snap.learn.total, snap.learn.seen, snap.learn.due),
            (bank, 0, bank)
        );
        assert!(snap.learn.gaps.is_empty() && snap.learn.weak_topics.is_empty());
        assert!(!snap.controls.stopped);
        assert_eq!(snap.claude.spent_24h, 0.0);
        assert!(snap.claude.by_stage.is_empty());
        assert_eq!((snap.today.tasks_total, snap.today.emails_total), (0, 0));
        assert!(snap.today.tasks.is_empty() && snap.today.emails.is_empty());
        let shipped = shipped_criteria();
        let pipeline = &snap.pipeline;
        assert_eq!(
            (pipeline.days, pipeline.threshold),
            (7, Some(shipped.apply_threshold))
        );
        assert_eq!(
            (
                pipeline.seen,
                pipeline.above,
                pipeline.with_cv,
                pipeline.complete,
                pipeline.sent
            ),
            (0, 0, 0, 0, 0)
        );
        assert_eq!(pipeline.score_counts, vec![0; 101]);
        assert_eq!(
            snap.trust,
            Some(Trust {
                reviewed: 0,
                first_n: shipped.first_n_reviewed,
                review_phase: true,
                sent_24h: 0,
                cap: shipped.daily_cap_until_reviewed,
                cap_after: shipped.daily_cap,
            })
        );
        assert_eq!(snap.activity.slots.len(), 48);
        assert!(snap.activity.slots.iter().all(|s| s.seen == 0));
        assert!(snap.activity.moves.is_empty());
        assert_eq!(snap.activity.moves_total, 0);
        assert!(snap.activity.moves_by_state.is_empty());
        let json = serde_json::to_value(&snap.activity).unwrap();
        assert_eq!(
            json["moves_by_state"],
            serde_json::json!({}),
            "an object even when empty, so the page can always index it"
        );
    }

    fn task(panel: &Panel, text: &str, due: Option<DateTime<Utc>>, job_key: Option<&str>) {
        panel
            .tracker
            .add_task(
                &NewTask {
                    job_key: job_key.map(str::to_string),
                    kind: "action_required".into(),
                    text: text.into(),
                    due_at: due,
                    source_message_id: None,
                    link: None,
                },
                *NOW,
            )
            .unwrap();
    }

    fn email(panel: &Panel, id: &str, kind: EmailKind, at: DateTime<Utc>, subject: &str) {
        panel
            .tracker
            .add_email(
                &NewEmail {
                    message_id: id.into(),
                    thread_id: format!("th-{id}"),
                    from_addr: "Ana <ana@acme.example>".into(),
                    subject: subject.into(),
                    received_at: at,
                    job_key: None,
                    kind,
                    summary: format!("Resumen {id}"),
                },
                None,
                *NOW,
            )
            .unwrap();
    }

    #[test]
    fn today_lists_open_tasks_soonest_first_with_an_overdue_flag() {
        let (panel, _dir) = panel();
        task(&panel, "sin fecha", None, None);
        task(
            &panel,
            "pronto",
            Some(*NOW + TimeDelta::hours(3)),
            Some("test:1"),
        );
        task(&panel, "vencida", Some(*NOW - TimeDelta::hours(3)), None);
        task(&panel, "hecha", Some(*NOW), None);
        let done = panel
            .tracker
            .open_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.text == "hecha")
            .unwrap()
            .id;
        panel.tracker.complete_task(done).unwrap();

        let today = serde_json::to_value(build_snapshot(&panel).unwrap().today).unwrap();

        assert_eq!(today["tasks_total"], 3);
        let tasks = today["tasks"].as_array().unwrap();
        let texts: Vec<&str> = tasks.iter().map(|t| t["text"].as_str().unwrap()).collect();
        assert_eq!(texts, ["vencida", "pronto", "sin fecha"]);
        assert_eq!(tasks[0]["overdue"], true);
        assert_eq!(tasks[1]["overdue"], false);
        assert_eq!(tasks[2]["overdue"], false, "no date, never overdue");
        assert_eq!(tasks[1]["job_key"], "test:1");
        assert_eq!(tasks[1]["due_at"], "2026-10-01T15:00:00Z");
        assert!(tasks[2]["due_at"].is_null() && tasks[2]["job_key"].is_null());
        assert!(tasks[1]["id"].as_i64().unwrap() > 0);
        assert_eq!(tasks[0]["kind"], "action_required");
        assert_eq!(tasks[0]["created_at"], "2026-10-01T12:00:00Z");
    }

    #[test]
    fn today_shows_a_manual_task_with_its_link() {
        let (panel, _dir) = panel();
        panel
            .tracker
            .add_manual_task(
                &NewTask {
                    job_key: None,
                    kind: "manual".into(),
                    text: "Llamar a Acme".into(),
                    due_at: Some(*NOW + TimeDelta::hours(2)),
                    source_message_id: None,
                    link: Some("https://example.com/oferta".into()),
                },
                *NOW,
            )
            .unwrap();
        task(&panel, "sin enlace", None, None);

        let today = serde_json::to_value(build_snapshot(&panel).unwrap().today).unwrap();

        assert_eq!(today["tasks_total"], 2);
        let tasks = today["tasks"].as_array().unwrap();
        assert_eq!(tasks[0]["text"], "Llamar a Acme");
        assert_eq!(tasks[0]["link"], "https://example.com/oferta");
        assert!(tasks[1]["link"].is_null(), "a task without a link has none");
    }

    #[test]
    fn today_lists_the_recent_emails_that_are_not_acks_newest_first() {
        let (panel, _dir) = panel();
        let day = TimeDelta::days(1);
        email(
            &panel,
            "old",
            EmailKind::Rejection,
            *NOW - day * 2,
            "Hace dos días",
        );
        email(
            &panel,
            "new",
            EmailKind::InterviewInvite,
            *NOW - TimeDelta::hours(1),
            "Entrevista",
        );
        email(
            &panel,
            "ack",
            EmailKind::Ack,
            *NOW - TimeDelta::hours(2),
            "Recibimos tu postulación",
        );
        email(
            &panel,
            "ancient",
            EmailKind::Recruiter,
            *NOW - day * 9,
            "Muy viejo",
        );

        let today = serde_json::to_value(build_snapshot(&panel).unwrap().today).unwrap();

        assert_eq!(today["emails_total"], 2);
        let emails = today["emails"].as_array().unwrap();
        let subjects: Vec<&str> = emails
            .iter()
            .map(|e| e["subject"].as_str().unwrap())
            .collect();
        assert_eq!(subjects, ["Entrevista", "Hace dos días"]);
        assert_eq!(emails[0]["kind"], "interview_invite");
        assert_eq!(emails[0]["from"], "Ana <ana@acme.example>");
        assert_eq!(emails[0]["summary"], "Resumen new");
        assert_eq!(emails[0]["received_at"], "2026-10-01T11:00:00Z");
        assert!(emails[0]["company"].is_null());
    }

    #[test]
    fn today_names_the_company_of_the_linked_posting() {
        let (panel, _dir) = panel();
        let key = add(&panel, "1", 80.0, &[]);
        panel
            .tracker
            .add_email(
                &NewEmail {
                    message_id: "m1".into(),
                    thread_id: "t1".into(),
                    from_addr: "rrhh@co1.example".into(),
                    subject: "Hola".into(),
                    received_at: *NOW,
                    job_key: Some(key.clone()),
                    kind: EmailKind::Recruiter,
                    summary: "x".into(),
                },
                None,
                *NOW,
            )
            .unwrap();

        let today = build_snapshot(&panel).unwrap().today;

        assert_eq!(today.emails[0].company.as_deref(), Some("Co 1"));
        assert_eq!(today.emails[0].job_key.as_deref(), Some(key.as_str()));
    }

    #[test]
    fn a_long_today_is_capped_but_counts_everything() {
        let (panel, _dir) = panel();
        for n in 0..(QUEUE_CAP + 5) {
            task(&panel, &format!("tarea {n}"), None, None);
            email(
                &panel,
                &format!("m{n}"),
                EmailKind::Recruiter,
                *NOW - TimeDelta::minutes(i64::try_from(n).unwrap()),
                "x",
            );
        }

        let today = build_snapshot(&panel).unwrap().today;

        assert_eq!(
            (today.tasks_total, today.emails_total),
            (QUEUE_CAP + 5, QUEUE_CAP + 5)
        );
        assert_eq!(
            (today.tasks.len(), today.emails.len()),
            (QUEUE_CAP, QUEUE_CAP)
        );
    }

    #[test]
    fn hostile_text_in_an_email_or_a_task_stays_plain_data() {
        let (panel, _dir) = panel();
        let hostile = "</script><img src=x onerror=alert(1)>";
        task(&panel, hostile, None, None);
        email(&panel, "m1", EmailKind::Other, *NOW, hostile);

        let today = build_snapshot(&panel).unwrap().today;

        assert_eq!(today.tasks[0].text, hostile);
        assert_eq!(today.emails[0].subject, hostile);
    }

    #[test]
    fn the_queue_lists_what_needs_gonza_and_what_awaits_his_approval() {
        let (panel, dir) = panel();
        let needy = add(&panel, "1", 80.0, &[State::NeedsGonza]);
        let review = add(&panel, "2", 70.0, &[State::Prepared]);
        let shot = dir.path().join("shot.png");
        std::fs::write(&shot, b"png").unwrap();
        panel.tracker.set_screenshot(&review, &shot).unwrap();

        let snap = build_snapshot(&panel).unwrap();
        let card = &snap.needs_gonza.cards[0];
        assert_eq!(card.key, needy);
        assert_eq!(card.reason.as_deref(), Some("falta email"));
        assert_eq!(card.actions, vec!["resolve", "confirm"]);
        let waiting = &snap.awaiting_approval.cards[0];
        assert_eq!(waiting.key, review);
        assert_eq!(waiting.actions, vec!["approve"]);
        assert!(waiting.has_screenshot && !waiting.approved);

        panel.tracker.approve(&review).unwrap();
        let snap = build_snapshot(&panel).unwrap();
        assert_eq!(
            snap.awaiting_approval.total, 0,
            "approved: no longer waiting"
        );
        assert!(column(&snap, "prepared").cards[0].actions.is_empty());
    }

    #[test]
    fn a_long_queue_is_capped_and_sorted_by_score() {
        let (panel, _dir) = panel();
        for n in 1..=60 {
            add(&panel, &n.to_string(), f64::from(n), &[]);
            add(&panel, &format!("n{n}"), f64::from(n), &[State::NeedsGonza]);
        }
        let snap = build_snapshot(&panel).unwrap();
        let scored = column(&snap, "scored");
        assert_eq!(scored.total, 60);
        assert_eq!(scored.cards.len(), COLUMN_CAP);
        assert_eq!(scored.cards[0].score, 60);
        assert!(scored.cards.windows(2).all(|w| w[0].score >= w[1].score));
        assert_eq!(snap.needs_gonza.total, 60);
        assert_eq!(snap.needs_gonza.cards.len(), QUEUE_CAP);
    }

    #[test]
    fn every_application_in_progress_is_listed_however_many_there_are() {
        let (panel, _dir) = panel();
        for n in 1..=COLUMN_CAP + 15 {
            add(
                &panel,
                &n.to_string(),
                f64::from(n as u32),
                &[State::Prepared, State::Submitted],
            );
        }
        let snap = build_snapshot(&panel).unwrap();
        let sent = column(&snap, "submitted");
        assert_eq!(sent.total, COLUMN_CAP + 15);
        assert_eq!(
            sent.cards.len(),
            COLUMN_CAP + 15,
            "the Procesos tab draws one row per live application: none may be cut"
        );
    }

    #[test]
    fn a_card_lists_the_answers_gonza_may_record_from_its_state() {
        let (panel, _dir) = panel();
        add(&panel, "1", 80.0, &[State::Prepared, State::Submitted]);
        let snap = build_snapshot(&panel).unwrap();
        let card = &column(&snap, "submitted").cards[0];
        assert_eq!(
            card.marks,
            vec![
                "replied",
                "technical_test",
                "interview",
                "rejected",
                "ghosted",
                "withdrawn"
            ]
        );
        assert!(card.actions.is_empty());
        assert_eq!(card.note.as_deref(), Some("falta email"));
    }

    #[test]
    fn a_waiting_card_says_what_kind_of_help_it_needs_in_one_sentence() {
        let (panel, _dir) = panel();
        let cases = [
            (
                "listing_only: postular a mano desde el enlace del aviso",
                "manual_apply",
            ),
            (
                "faltan respuestas: Cover Letter*; Have you used it?",
                "missing_answers",
            ),
            (
                "handoff: envía tú este formulario. Hoja de respuestas: data/handoff/x.md",
                "handoff",
            ),
            ("captcha", "blocked"),
            ("login_required", "blocked"),
            ("blocked_page", "blocked"),
            (
                "unconfirmed_submit: sin confirmación visible",
                "may_have_sent",
            ),
            (
                "assist_closed: cerraste la pestaña sin que la página confirmara; si sí la enviaste, `jobbot confirm KEY` la marca",
                "may_have_sent",
            ),
            (
                "assist_timeout: sin confirmación en 10 minutos",
                "may_have_sent",
            ),
            (
                "interrupted_submit: la corrida se cortó entre el clic y su confirmación",
                "may_have_sent",
            ),
            ("page_error: timeout", "error"),
            ("falta email", "other"),
        ];
        for (index, (reason, _)) in cases.iter().enumerate() {
            let key = add(&panel, &index.to_string(), 80.0, &[]);
            panel
                .tracker
                .transition(&key, State::NeedsGonza, Some(reason), *NOW)
                .unwrap();
        }
        // A card that is not waiting for anyone has no reason and no kind.
        add(&panel, "sent", 70.0, &[State::Prepared]);

        let snap = build_snapshot(&panel).unwrap();

        let mut seen: Vec<(String, &str)> = snap
            .needs_gonza
            .cards
            .iter()
            .map(|c| {
                assert!(
                    c.reason_short.is_some_and(|s| !s.is_empty()),
                    "every kind comes with a sentence: {c:?}"
                );
                (c.reason.clone().unwrap(), c.reason_kind.unwrap())
            })
            .collect();
        seen.sort();
        let mut expected: Vec<(String, &str)> = cases
            .iter()
            .map(|(reason, kind)| (reason.to_string(), *kind))
            .collect();
        expected.sort();
        assert_eq!(seen, expected);
        let prepared = column(&snap, "prepared");
        assert!(
            prepared.cards[0].reason_kind.is_none() && prepared.cards[0].reason_short.is_none()
        );
    }

    #[test]
    fn a_card_that_may_already_have_gone_out_says_so_and_keeps_its_buttons() {
        let (panel, _dir) = panel();
        let reasons = [
            "unconfirmed_submit: Phone number is invalid.",
            "assist_closed: cerraste la pestaña",
            "assist_timeout: sin confirmación en 10 minutos",
            "interrupted_submit: la corrida se cortó",
        ];
        for (index, reason) in reasons.iter().enumerate() {
            let key = add(&panel, &index.to_string(), 80.0, &[]);
            panel
                .tracker
                .transition(&key, State::NeedsGonza, Some(reason), *NOW)
                .unwrap();
        }
        // A form Gonza still has to send is not one that may have gone out.
        let key = add(&panel, "handoff", 80.0, &[]);
        panel
            .tracker
            .transition(
                &key,
                State::NeedsGonza,
                Some("handoff: envía tú este formulario"),
                *NOW,
            )
            .unwrap();

        let snap = build_snapshot(&panel).unwrap();

        let (may, other): (Vec<&Card>, Vec<&Card>) = snap
            .needs_gonza
            .cards
            .iter()
            .partition(|c| c.reason_kind == Some("may_have_sent"));
        assert_eq!(may.len(), reasons.len());
        for card in &may {
            assert_eq!(
                card.reason_short,
                Some("Puede que ya se haya enviado: revisa antes de volver a postular")
            );
            // The same two answers as any waiting card: the form went out (confirm) or it did not (resolve),
            // but the first one offered is "it went out", never the retry.
            assert_eq!(card.actions, vec!["confirm", "resolve"]);
        }
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].reason_kind, Some("handoff"));
    }

    #[test]
    fn the_funnel_counts_each_application_once_per_stage_it_ever_reached() {
        let (panel, _dir) = panel();
        let sent = [State::Prepared, State::Submitted];
        add(&panel, "1", 80.0, &sent);
        add(
            &panel,
            "2",
            80.0,
            &[sent.as_slice(), &[State::Replied]].concat(),
        );
        // Went on to a test and an interview: it also counts as answered and nowhere twice.
        add(
            &panel,
            "3",
            80.0,
            &[
                sent.as_slice(),
                &[State::Replied, State::TechnicalTest, State::Interview],
            ]
            .concat(),
        );
        add(
            &panel,
            "4",
            80.0,
            &[
                sent.as_slice(),
                &[State::Replied, State::Interview, State::Offer],
            ]
            .concat(),
        );
        // A rejection is an answer too.
        add(
            &panel,
            "5",
            80.0,
            &[sent.as_slice(), &[State::Rejected]].concat(),
        );
        // Silence is not an answer, and an application never sent is not in the funnel.
        add(
            &panel,
            "6",
            80.0,
            &[sent.as_slice(), &[State::Ghosted]].concat(),
        );
        add(&panel, "7", 80.0, &[State::NeedsGonza]);

        let funnel = build_snapshot(&panel).unwrap().funnel;

        assert_eq!(
            (
                funnel.sent,
                funnel.answered,
                funnel.tests,
                funnel.interviews,
                funnel.offers
            ),
            (6, 4, 1, 2, 1)
        );
        // Every fixture posting comes from the one source: its line is the whole funnel plus the
        // unsent one in its base.
        assert_eq!(
            funnel.by_source.get("test"),
            Some(&SourceFunnel {
                n: 7,
                sent: 6,
                answered: 4,
                tests: 1,
                interviews: 2,
                offers: 1,
            })
        );
    }

    #[test]
    fn the_funnel_is_split_by_source_with_a_base_for_each() {
        let (panel, _dir) = panel();
        let walk = |source: &str, id: &str, states: &[State]| {
            let job = make_job_with(|j| {
                j.source = source.into();
                j.external_id = id.into();
            });
            panel
                .tracker
                .add_job(&job, 80.0, &BTreeMap::new(), *NOW)
                .unwrap();
            panel.tracker.create_application(&job.key(), *NOW).unwrap();
            for state in states {
                panel
                    .tracker
                    .transition(&job.key(), *state, None, *NOW)
                    .unwrap();
            }
        };
        walk(
            "ashby",
            "1",
            &[State::Prepared, State::Submitted, State::Rejected],
        );
        walk("ashby", "2", &[State::NeedsGonza]);
        walk(
            "lever",
            "3",
            &[
                State::Prepared,
                State::Submitted,
                State::Replied,
                State::Interview,
            ],
        );

        let json = serde_json::to_value(build_snapshot(&panel).unwrap().funnel).unwrap();

        assert_eq!(json["sent"], 2);
        assert_eq!(
            json["by_source"]["ashby"],
            serde_json::json!({ "n": 2, "sent": 1, "answered": 1, "tests": 0, "interviews": 0, "offers": 0 })
        );
        assert_eq!(
            json["by_source"]["lever"],
            serde_json::json!({ "n": 1, "sent": 1, "answered": 1, "tests": 0, "interviews": 1, "offers": 0 })
        );
        assert_eq!(json["by_source"].as_object().unwrap().len(), 2);
    }

    #[test]
    fn metrics_count_each_application_once_and_split_week_from_all_time() {
        let (panel, _dir) = panel();
        let t = &panel.tracker;
        add(
            &panel,
            "a",
            80.0,
            &[
                State::Prepared,
                State::Submitted,
                State::Interview,
                State::Interview,
                State::Offer,
            ],
        );
        add(
            &panel,
            "b",
            80.0,
            &[State::Prepared, State::Submitted, State::Replied],
        );
        add(
            &panel,
            "c",
            80.0,
            &[State::Prepared, State::Submitted, State::TechnicalTest],
        );
        // An application that went to interview ten days ago: it counts for all time, not for the week.
        let old = *NOW - TimeDelta::days(10);
        let d = add(&panel, "d", 80.0, &[]);
        for state in [State::Prepared, State::Submitted, State::Interview] {
            t.transition(&d, state, None, old).unwrap();
        }
        let m = build_snapshot(&panel).unwrap().metrics;
        assert_eq!(m.sent_week, 3);
        assert_eq!(m.responses_week, 3);
        assert_eq!(m.response_rate, Some(1.0));
        assert_eq!(m.tests_week, 1);
        assert_eq!(
            m.interviews_week, 1,
            "a's second round does not count twice"
        );
        assert_eq!(m.interviews_total, 2);
        assert_eq!(m.offers_total, 1);
        assert_eq!(m.interview_to_offer, Some(0.5));
    }

    #[test]
    fn building_the_snapshot_twice_gives_the_same_data() {
        let (panel, _dir) = panel();
        add(&panel, "1", 80.0, &[State::NeedsGonza]);
        add(
            &panel,
            "2",
            70.0,
            &[State::Prepared, State::Submitted, State::Replied],
        );
        panel
            .store
            .add_cards(
                &[Question {
                    id: "gap-x-1".into(),
                    topic: "kubernetes".into(),
                    question: "q".into(),
                    answer: "a".into(),
                }],
                "test:2",
                *NOW,
            )
            .unwrap();
        task(&panel, "Responder", Some(*NOW + TimeDelta::hours(2)), None);
        email(&panel, "m1", EmailKind::Recruiter, *NOW, "Hola");
        let first = serde_json::to_value(build_snapshot(&panel).unwrap()).unwrap();
        let second = serde_json::to_value(build_snapshot(&panel).unwrap()).unwrap();
        assert_eq!(
            first, second,
            "reading the panel changes nothing and duplicates nothing"
        );
    }

    #[test]
    fn controls_show_stop_and_tolerate_a_missing_schedule_and_claude_config() {
        let (mut panel, dir) = panel();
        assert!(!build_snapshot(&panel).unwrap().controls.stopped);
        std::fs::write(&panel.paths.stop_file, "stop").unwrap();
        let controls = build_snapshot(&panel).unwrap().controls;
        assert!(controls.stopped);
        assert!(controls.window_open.is_some());
        assert_eq!(
            build_snapshot(&panel).unwrap().claude.daily_budget,
            Some(1.5)
        );
        panel.paths.profile_dir = dir.path().join("no-profile");
        let snap = build_snapshot(&panel).unwrap();
        assert_eq!(
            (snap.controls.window_open, snap.controls.next_window),
            (None, None)
        );
        assert_eq!(snap.claude.daily_budget, None);
    }

    #[test]
    fn claude_spend_covers_the_last_24_hours_only() {
        let (panel, _dir) = panel();
        let t = &panel.tracker;
        t.record_claude_call("dossier", *NOW - TimeDelta::hours(1), 0.25, 10, 5, true)
            .unwrap();
        t.record_claude_call("dossier", *NOW - TimeDelta::hours(30), 9.0, 1, 1, true)
            .unwrap();
        let claude = build_snapshot(&panel).unwrap().claude;
        assert!((claude.spent_24h - 0.25).abs() < 1e-9);
        assert_eq!(claude.by_stage.len(), 1);
        assert_eq!(
            (claude.by_stage[0].stage.as_str(), claude.by_stage[0].calls),
            ("dossier", 1)
        );
    }

    #[test]
    fn the_learn_summary_links_gaps_to_their_offer_and_shows_weak_topics() {
        let (panel, _dir) = panel();
        let key = add(&panel, "1", 80.0, &[]);
        panel
            .store
            .add_cards(
                &[Question {
                    id: "gap-x-1".into(),
                    topic: "kubernetes".into(),
                    question: "q".into(),
                    answer: "a".into(),
                }],
                &key,
                *NOW,
            )
            .unwrap();
        let questions = panel.store.all_questions().unwrap();
        panel.store.grade(&questions[0], 1, *NOW).unwrap();
        let learn = build_snapshot(&panel).unwrap().learn;
        assert_eq!(learn.seen, 1);
        assert_eq!(learn.total, questions.len());
        assert_eq!(learn.weak_topics.len(), 1);
        assert_eq!(learn.weak_topics[0].topic, questions[0].topic);
        assert_eq!(learn.gaps.len(), 1);
        assert_eq!(learn.gaps[0].topic, "kubernetes");
        assert_eq!(learn.gaps[0].offer, "Co 1 — Role 1");
        assert!(!learn.gaps[0].seen);
        assert!(
            learn
                .topics
                .iter()
                .any(|t| t.topic == questions[0].topic && t.seen == 1)
        );
    }

    #[test]
    fn job_detail_has_the_history_the_dossier_and_the_gaps_of_that_job() {
        let (panel, dir) = panel();
        let key = add(
            &panel,
            "1",
            80.0,
            &[State::Prepared, State::Submitted, State::Replied],
        );
        let dossiers = dir.path().join("data").join("dossiers");
        std::fs::create_dir_all(&dossiers).unwrap();
        std::fs::write(
            crate::dossier::dossier_path(&dossiers, &key),
            "# Dossier\nhola",
        )
        .unwrap();
        panel
            .store
            .add_cards(
                &[Question {
                    id: "gap-x-1".into(),
                    topic: "kubernetes".into(),
                    question: "q".into(),
                    answer: "a".into(),
                }],
                &key,
                *NOW,
            )
            .unwrap();
        let detail = build_job_detail(&panel, &key).unwrap().unwrap();
        assert_eq!(detail.card.state, "replied");
        assert!(detail.card.has_dossier);
        let path: Vec<&str> = detail.events.iter().map(|e| e.to).collect();
        assert_eq!(path, vec!["scored", "prepared", "submitted", "replied"]);
        assert_eq!(detail.events[0].from, None);
        assert_eq!(detail.dossier.as_deref(), Some("# Dossier\nhola"));
        assert_eq!(detail.gaps.len(), 1);
        assert_eq!(detail.gaps[0].offer, "Co 1 — Role 1");
        assert!(build_job_detail(&panel, "nope:1").unwrap().is_none());
    }

    #[test]
    fn a_huge_description_is_cut_and_hostile_text_is_kept_as_plain_data() {
        let (panel, _dir) = panel();
        let hostile = "</script><img src=x onerror=alert(1)>";
        let job = make_job_with(|j| {
            j.title = hostile.into();
            j.description = "x".repeat(50_000);
        });
        panel
            .tracker
            .add_job(&job, 80.0, &BTreeMap::new(), *NOW)
            .unwrap();
        panel.tracker.create_application(&job.key(), *NOW).unwrap();
        let detail = build_job_detail(&panel, &job.key()).unwrap().unwrap();
        assert_eq!(
            detail.description.chars().count(),
            8_000 + 1,
            "cut, plus the ellipsis"
        );
        assert_eq!(
            detail.card.title, hostile,
            "the data layer keeps text as it came; escaping is the renderer's job"
        );
    }

    /// Stores job `id` as first seen at `seen`. With `walk`, an application is made at that
    /// moment and walked through those states; `None` leaves the posting without one.
    fn seen_at(
        panel: &Panel,
        id: &str,
        score: f64,
        seen: DateTime<Utc>,
        walk: Option<&[State]>,
    ) -> String {
        let job = make_job_with(|j| {
            j.external_id = id.into();
            j.company = format!("Co {id}");
            j.title = format!("Role {id}");
        });
        panel
            .tracker
            .add_job(&job, score, &BTreeMap::new(), seen)
            .unwrap();
        if let Some(walk) = walk {
            panel.tracker.create_application(&job.key(), seen).unwrap();
            for state in walk {
                panel
                    .tracker
                    .transition(&job.key(), *state, None, seen)
                    .unwrap();
            }
        }
        job.key()
    }

    /// A panel over fresh databases whose clock stands at `at`.
    fn panel_at(at: DateTime<Utc>) -> (Panel, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = Paths::new(dir.path());
        paths.profile_dir = real_paths().profile_dir;
        let panel = Panel::new(
            paths,
            Tracker::in_memory().unwrap(),
            PrepStore::in_memory().unwrap(),
            Box::new(move || at),
        );
        (panel, dir)
    }

    /// A profile folder inside `dir` holding only the files given.
    fn profile_with(dir: &Path, criteria: Option<&str>, schedule: Option<&str>) -> PathBuf {
        let folder = dir.join("profile-under-test");
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        if let Some(text) = criteria {
            std::fs::write(folder.join("criteria.toml"), text).unwrap();
        }
        if let Some(text) = schedule {
            std::fs::write(folder.join("schedule.toml"), text).unwrap();
        }
        folder
    }

    /// The shipped `criteria.toml` with `first_n_reviewed` changed.
    fn criteria_with_first_n(first_n: u32) -> String {
        let shipped =
            std::fs::read_to_string(real_paths().profile_dir.join("criteria.toml")).unwrap();
        let mut found = false;
        let text: Vec<String> = shipped
            .lines()
            .map(|line| {
                if line.starts_with("first_n_reviewed") {
                    found = true;
                    format!("first_n_reviewed = {first_n}")
                } else {
                    line.to_string()
                }
            })
            .collect();
        assert!(found, "the shipped criteria.toml sets first_n_reviewed");
        text.join("\n")
    }

    fn shipped_criteria() -> crate::rules::Criteria {
        load_criteria(&real_paths().profile_dir.join("criteria.toml")).unwrap()
    }

    #[test]
    fn the_pipeline_counts_the_week_by_the_furthest_layer_each_posting_reached() {
        let (panel, dir) = panel();
        let t = &panel.tracker;
        let file = |name: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, b"x").unwrap();
            path
        };
        let gone = dir.path().join("gone.png");
        let fresh = *NOW - TimeDelta::days(1);

        // No application: only seen.
        seen_at(&panel, "below", 30.0, fresh, None);
        // An application and nothing else yet, and one that was withdrawn: both are above.
        seen_at(&panel, "plain", 65.0, fresh, Some(&[]));
        seen_at(&panel, "withdrawn", 65.0, fresh, Some(&[State::Withdrawn]));
        // Paths that point at files that are gone prove nothing.
        let lost = seen_at(&panel, "lost", 65.0, fresh, Some(&[State::Prepared]));
        t.set_cv_path(&lost, &gone).unwrap();
        t.set_screenshot(&lost, &gone).unwrap();
        // A CV on disk, and nothing more.
        let cv = seen_at(&panel, "cv", 65.0, fresh, Some(&[State::Prepared]));
        t.set_cv_path(&cv, &file("cv.pdf")).unwrap();
        // Prepared with its screenshot: complete, and with a CV by the same token.
        let done = seen_at(&panel, "done", 65.0, fresh, Some(&[State::Prepared]));
        t.set_screenshot(&done, &file("shot.png")).unwrap();
        // The screenshot of a form that then needed Gonza is not a finished form; its CV counts.
        let needy = seen_at(&panel, "needy", 65.0, fresh, Some(&[State::NeedsGonza]));
        t.set_screenshot(&needy, &file("needy.png")).unwrap();
        t.set_cv_path(&needy, &file("needy.pdf")).unwrap();
        // Sent, with nothing left on disk: it counts in every layer.
        seen_at(
            &panel,
            "sent",
            65.0,
            fresh,
            Some(&[State::Prepared, State::Submitted]),
        );
        // First seen exactly seven days ago counts; a second earlier does not, however far it got.
        seen_at(&panel, "edge", 65.0, *NOW - TimeDelta::days(7), None);
        seen_at(
            &panel,
            "old",
            90.0,
            *NOW - TimeDelta::days(7) - TimeDelta::seconds(1),
            Some(&[State::Prepared, State::Submitted]),
        );

        let p = build_snapshot(&panel).unwrap().pipeline;

        assert_eq!(p.days, 7);
        assert_eq!(
            (p.seen, p.above, p.with_cv, p.complete, p.sent),
            (9, 7, 4, 2, 1)
        );
        assert!(p.sent <= p.complete && p.complete <= p.with_cv);
        assert!(p.with_cv <= p.above && p.above <= p.seen);
        assert_eq!(
            p.score_counts.iter().map(|&n| i64::from(n)).sum::<i64>(),
            p.seen
        );
    }

    #[test]
    fn score_counts_round_each_score_and_clamp_it_to_0_to_100() {
        let (panel, _dir) = panel();
        let fresh = *NOW - TimeDelta::hours(1);
        for (n, score) in [0.0, 0.4, -5.0, 0.5, 59.5, 60.2, 100.0, 100.4, 250.0]
            .into_iter()
            .enumerate()
        {
            seen_at(&panel, &n.to_string(), score, fresh, None);
        }
        // A posting from before the week is not counted.
        seen_at(&panel, "old", 77.0, *NOW - TimeDelta::days(8), None);

        let counts = build_snapshot(&panel).unwrap().pipeline.score_counts;

        assert_eq!(counts.len(), 101);
        let mut expected = vec![0u32; 101];
        expected[0] = 3;
        expected[1] = 1;
        expected[60] = 2;
        expected[100] = 3;
        assert_eq!(counts, expected);
    }

    #[test]
    fn the_threshold_comes_from_criteria_toml_and_is_none_when_it_cannot_be_read() {
        let (mut panel, dir) = panel();
        seen_at(&panel, "1", 70.0, *NOW, Some(&[]));
        let snap = build_snapshot(&panel).unwrap();
        assert_eq!(
            snap.pipeline.threshold,
            Some(shipped_criteria().apply_threshold)
        );

        // No criteria.toml at all, and one that does not parse: same answer, the counts stay.
        for criteria in [None, Some("floor_monthly_usd = ")] {
            panel.paths.profile_dir = profile_with(dir.path(), criteria, None);
            let snap = build_snapshot(&panel).unwrap();
            assert_eq!(snap.pipeline.threshold, None);
            assert!(snap.trust.is_none());
            assert_eq!((snap.pipeline.seen, snap.pipeline.above), (1, 1));
        }
    }

    #[test]
    fn trust_counts_the_reviewed_sends_and_the_cap_in_force() {
        let (mut panel, dir) = panel();
        let shipped = shipped_criteria();
        assert_ne!(
            shipped.daily_cap, shipped.daily_cap_until_reviewed,
            "the test needs two different caps to tell them apart"
        );
        panel.paths.profile_dir = profile_with(dir.path(), Some(&criteria_with_first_n(2)), None);
        let send = |id: &str, approve: bool| {
            let key = add(&panel, id, 80.0, &[State::Prepared]);
            if approve {
                panel.tracker.approve(&key).unwrap();
            }
            panel
                .tracker
                .transition(&key, State::Submitted, None, *NOW)
                .unwrap();
        };

        let trust = build_snapshot(&panel).unwrap().trust.unwrap();
        assert_eq!(
            trust,
            Trust {
                reviewed: 0,
                first_n: 2,
                review_phase: true,
                sent_24h: 0,
                cap: shipped.daily_cap_until_reviewed,
                cap_after: shipped.daily_cap,
            }
        );

        // One approved send, and one nobody reviewed: only the first counts as reviewed.
        send("a", true);
        send("b", false);
        let trust = build_snapshot(&panel).unwrap().trust.unwrap();
        assert_eq!((trust.reviewed, trust.review_phase), (1, true));
        assert_eq!(trust.sent_24h, 2);
        assert_eq!(trust.cap, shipped.daily_cap_until_reviewed);

        // The second approved send ends the review phase: the larger cap is in force now.
        send("c", true);
        let trust = build_snapshot(&panel).unwrap().trust.unwrap();
        assert_eq!(
            (trust.reviewed, trust.first_n, trust.review_phase),
            (2, 2, false)
        );
        assert_eq!(trust.sent_24h, 3);
        assert_eq!(trust.cap, shipped.daily_cap);
        assert_eq!(trust.cap_after, shipped.daily_cap);
    }

    #[test]
    fn sent_24h_counts_what_the_gate_counts_as_a_click() {
        let (panel, _dir) = panel();
        let hour = TimeDelta::hours(1);
        let send = |id: &str, at: DateTime<Utc>| {
            let key = add(&panel, id, 80.0, &[State::Prepared]);
            panel
                .tracker
                .transition(&key, State::Submitted, None, at)
                .unwrap();
        };
        send("recent", *NOW - hour);
        send("edge", *NOW - hour * 24);
        send("old", *NOW - hour * 25);
        // A click the page did not confirm may have gone out: it counts. A prepared form does not.
        let clicked = add(&panel, "clicked", 80.0, &[State::Prepared]);
        panel
            .tracker
            .record_attempt(&clicked, *NOW - hour * 2)
            .unwrap();
        add(&panel, "idle", 80.0, &[State::Prepared]);

        let trust = build_snapshot(&panel).unwrap().trust.unwrap();

        assert_eq!(trust.sent_24h, 3);
    }

    #[test]
    fn the_activity_is_48_half_hour_slots_ending_with_the_one_that_holds_now() {
        let at = |h, m, s| Utc.with_ymd_and_hms(2026, 10, 1, h, m, s).unwrap();
        // (clock, start of the last slot)
        let cases = [
            (at(12, 0, 0), "2026-10-01T12:00:00Z"),
            (at(12, 17, 45), "2026-10-01T12:00:00Z"),
            (at(12, 29, 59), "2026-10-01T12:00:00Z"),
            (at(12, 30, 0), "2026-10-01T12:30:00Z"),
            (at(0, 5, 0), "2026-10-01T00:00:00Z"),
        ];
        for (now, last) in cases {
            let (panel, _dir) = panel_at(now);
            let activity = build_snapshot(&panel).unwrap().activity;

            assert_eq!(activity.slot_minutes, 30);
            assert_eq!(activity.slots.len(), 48);
            assert_eq!(activity.slots[47].at, last, "clock at {now}");
            let starts: Vec<DateTime<Utc>> = activity
                .slots
                .iter()
                .map(|s| {
                    DateTime::parse_from_rfc3339(&s.at)
                        .unwrap()
                        .with_timezone(&Utc)
                })
                .collect();
            assert!(
                starts
                    .windows(2)
                    .all(|w| w[1] - w[0] == TimeDelta::minutes(30)),
                "a slot every half hour, oldest first"
            );
            assert_eq!(activity.from, activity.slots[0].at);
            let end = starts[47] + TimeDelta::minutes(30);
            assert_eq!(activity.to, iso(end));
            assert!(starts[47] <= now && now < end, "the last slot holds now");
            assert!(
                starts
                    .iter()
                    .all(|s| s.minute() % 30 == 0 && s.second() == 0),
                "slots are aligned to the clock"
            );
        }
    }

    #[test]
    fn each_slot_counts_the_postings_first_seen_inside_it() {
        let (panel, _dir) = panel();
        let slot = TimeDelta::minutes(30);
        // The span is 2026-09-30T12:30Z to 2026-10-01T12:30Z; slot 10 starts at 17:30Z.
        let from = *NOW - slot * 47;
        let tenth = from + slot * 10;
        seen_at(&panel, "first", 50.0, from, None);
        seen_at(&panel, "before", 50.0, from - TimeDelta::seconds(1), None);
        seen_at(&panel, "ten-a", 50.0, tenth, None);
        seen_at(
            &panel,
            "ten-b",
            50.0,
            tenth + slot - TimeDelta::seconds(1),
            None,
        );
        seen_at(&panel, "now", 50.0, *NOW, None);
        seen_at(
            &panel,
            "end",
            50.0,
            *NOW + slot - TimeDelta::seconds(1),
            None,
        );
        seen_at(&panel, "past-end", 50.0, *NOW + slot, None);

        let activity = build_snapshot(&panel).unwrap().activity;

        assert_eq!(activity.from, "2026-09-30T12:30:00Z");
        assert_eq!(activity.to, "2026-10-01T12:30:00Z");
        let mut expected = vec![0i64; 48];
        expected[0] = 1;
        expected[10] = 2;
        expected[47] = 2;
        let seen: Vec<i64> = activity.slots.iter().map(|s| s.seen).collect();
        assert_eq!(seen, expected);
        assert_eq!(activity.slots[10].at, "2026-09-30T17:30:00Z");
    }

    #[test]
    fn a_slot_is_open_when_the_schedule_has_no_busy_block_on_it() {
        let (mut panel, dir) = panel();
        // Busy 13:00 to 14:00 UTC every day: of the 48 slots only 13:00 and 13:30 of the 30th.
        let schedule = "[[busy]]\nzone = \"UTC\"\nstart = \"13:00\"\nend = \"14:00\"\n\
                        days = [\"mon\", \"tue\", \"wed\", \"thu\", \"fri\", \"sat\", \"sun\"]\n";
        panel.paths.profile_dir = profile_with(dir.path(), None, Some(schedule));

        let activity = build_snapshot(&panel).unwrap().activity;

        let closed: Vec<&str> = activity
            .slots
            .iter()
            .filter(|s| s.open == Some(false))
            .map(|s| s.at.as_str())
            .collect();
        assert_eq!(closed, ["2026-09-30T13:00:00Z", "2026-09-30T13:30:00Z"]);
        assert_eq!(
            activity
                .slots
                .iter()
                .filter(|s| s.open == Some(true))
                .count(),
            46
        );
    }

    #[test]
    fn a_schedule_that_cannot_be_read_leaves_every_slot_without_an_answer() {
        let (mut panel, dir) = panel();
        seen_at(&panel, "1", 50.0, *NOW, None);
        for schedule in [None, Some("this is not a schedule")] {
            panel.paths.profile_dir = profile_with(dir.path(), None, schedule);
            let activity = build_snapshot(&panel).unwrap().activity;
            assert_eq!(activity.slots.len(), 48);
            assert!(activity.slots.iter().all(|s| s.open.is_none()));
            assert_eq!(activity.slots[47].seen, 1, "the counts do not depend on it");
        }
    }

    #[test]
    fn the_moves_are_the_state_changes_of_the_span_newest_first() {
        let (panel, _dir) = panel();
        let from = *NOW - TimeDelta::minutes(47 * 30);
        let second = TimeDelta::seconds(1);
        seen_at(&panel, "before", 50.0, from - second, Some(&[]));
        let first = seen_at(&panel, "first", 50.0, from, Some(&[]));
        let mid = seen_at(&panel, "mid", 50.0, *NOW - TimeDelta::hours(2), Some(&[]));
        panel
            .tracker
            .transition(&mid, State::Prepared, None, *NOW - TimeDelta::hours(1))
            .unwrap();
        let last = seen_at(&panel, "last", 50.0, *NOW, Some(&[]));
        seen_at(&panel, "after", 50.0, *NOW + second, Some(&[]));

        let activity = build_snapshot(&panel).unwrap().activity;

        assert_eq!(activity.moves_total, 4);
        assert_eq!(
            serde_json::to_value(&activity.moves_by_state).unwrap(),
            serde_json::json!({"prepared": 1, "scored": 3})
        );
        let seen: Vec<(&str, &str, &str, &str, &str)> = activity
            .moves
            .iter()
            .map(|m| {
                (
                    m.at.as_str(),
                    m.key.as_str(),
                    m.company.as_str(),
                    m.title.as_str(),
                    m.to,
                )
            })
            .collect();
        assert_eq!(
            seen,
            [
                (
                    "2026-10-01T12:00:00Z",
                    last.as_str(),
                    "Co last",
                    "Role last",
                    "scored"
                ),
                (
                    "2026-10-01T11:00:00Z",
                    mid.as_str(),
                    "Co mid",
                    "Role mid",
                    "prepared"
                ),
                (
                    "2026-10-01T10:00:00Z",
                    mid.as_str(),
                    "Co mid",
                    "Role mid",
                    "scored"
                ),
                (
                    "2026-09-30T12:30:00Z",
                    first.as_str(),
                    "Co first",
                    "Role first",
                    "scored"
                ),
            ]
        );
    }

    #[test]
    fn a_busy_day_shows_the_newest_60_moves_and_counts_all_of_them() {
        let (panel, _dir) = panel();
        let keys: Vec<String> = (0..65)
            .map(|n| {
                seen_at(
                    &panel,
                    &n.to_string(),
                    50.0,
                    *NOW - TimeDelta::minutes(n),
                    Some(&[]),
                )
            })
            .collect();

        let activity = build_snapshot(&panel).unwrap().activity;

        assert_eq!(activity.moves_total, 65);
        assert_eq!(activity.moves.len(), 60);
        assert_eq!(activity.moves[0].key, keys[0]);
        assert_eq!(activity.moves[59].key, keys[59]);
        assert!(activity.moves.windows(2).all(|w| w[0].at > w[1].at));
    }

    #[test]
    fn the_moves_by_state_count_what_the_cut_list_leaves_out() {
        let (panel, _dir) = panel();
        // 65 postings scored in the last hour, and 10 of them prepared right after: 75 moves, of
        // which the list keeps the newest 60 but the counts keep all.
        for n in 0..65 {
            let key = seen_at(
                &panel,
                &n.to_string(),
                50.0,
                *NOW - TimeDelta::minutes(n + 1),
                Some(&[]),
            );
            if n < 10 {
                panel
                    .tracker
                    .transition(&key, State::Prepared, None, *NOW - TimeDelta::seconds(n))
                    .unwrap();
            }
        }

        let activity = build_snapshot(&panel).unwrap().activity;

        assert_eq!(activity.moves_total, 75);
        assert_eq!(activity.moves.len(), 60);
        let by_state = serde_json::to_value(&activity.moves_by_state).unwrap();
        assert_eq!(by_state, serde_json::json!({"prepared": 10, "scored": 65}));
        let sum: i64 = activity.moves_by_state.values().sum();
        assert_eq!(sum, activity.moves_total, "the counts add up to the total");
    }

    #[test]
    fn the_new_fields_are_named_the_way_the_page_reads_them() {
        let (panel, _dir) = panel();
        seen_at(&panel, "1", 80.0, *NOW, Some(&[]));

        let json = serde_json::to_value(build_snapshot(&panel).unwrap()).unwrap();

        let keys = |value: &serde_json::Value| -> Vec<String> {
            let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            keys
        };
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

    #[test]
    fn a_card_says_whether_its_answer_sheet_for_a_hand_over_is_on_disk() {
        let (panel, dir) = panel();
        let with_sheet = add(&panel, "1", 80.0, &[State::Prepared, State::NeedsGonza]);
        add(&panel, "2", 80.0, &[State::Prepared, State::NeedsGonza]);
        let sent = add(&panel, "3", 80.0, &[State::Prepared, State::Submitted]);
        // Where `handoff::write_sheet` puts it: `<data>/handoff/<key as a file name>.md`.
        let handoffs = dir.path().join("data").join("handoff");
        std::fs::create_dir_all(&handoffs).unwrap();
        for key in [&with_sheet, &sent] {
            std::fs::write(crate::handoff::sheet_path(&handoffs, key), "# hoja").unwrap();
        }
        // A folder with the sheet's name is not a sheet.
        let folder = add(&panel, "4", 80.0, &[State::Prepared, State::NeedsGonza]);
        std::fs::create_dir(crate::handoff::sheet_path(&handoffs, &folder)).unwrap();

        let snap = build_snapshot(&panel).unwrap();

        let flags = |state: &str| -> Vec<(String, bool)> {
            column(&snap, state)
                .cards
                .iter()
                .map(|c| (c.company.clone(), c.has_handoff))
                .collect()
        };
        assert_eq!(
            flags("needs_gonza"),
            [
                ("Co 1".to_string(), true),
                ("Co 2".to_string(), false),
                ("Co 4".to_string(), false)
            ]
        );
        // The sheet outlives the hand-over: it is a file, and the card says so.
        assert_eq!(flags("submitted"), [("Co 3".to_string(), true)]);
        // The same card in the queue and in the detail drawer agrees.
        assert!(snap.needs_gonza.cards.iter().any(|c| c.has_handoff));
        let detail = build_job_detail(&panel, &with_sheet).unwrap().unwrap();
        assert!(detail.card.has_handoff);
    }

    #[test]
    fn a_card_says_whether_its_cv_is_on_disk() {
        let (panel, dir) = panel();
        let with_cv = add(&panel, "1", 80.0, &[State::Prepared]);
        let lost_cv = add(&panel, "2", 80.0, &[State::Prepared]);
        add(&panel, "3", 80.0, &[State::Prepared]);
        let cv = dir.path().join("cv.pdf");
        std::fs::write(&cv, b"pdf").unwrap();
        panel.tracker.set_cv_path(&with_cv, &cv).unwrap();
        panel
            .tracker
            .set_cv_path(&lost_cv, &dir.path().join("gone.pdf"))
            .unwrap();

        let snap = build_snapshot(&panel).unwrap();

        let has_cv: Vec<bool> = column(&snap, "prepared")
            .cards
            .iter()
            .map(|c| c.has_cv)
            .collect();
        // Best score first, then by key: Co 1, Co 2, Co 3.
        assert_eq!(has_cv, [true, false, false]);
    }

    #[test]
    fn job_detail_says_when_the_posting_was_first_seen_and_when_it_was_posted() {
        let (panel, _dir) = panel();
        let dated = make_job_with(|j| {
            j.external_id = "dated".into();
            j.posted_at = Some(*NOW - TimeDelta::days(3));
        });
        let undated = make_job_with(|j| j.external_id = "undated".into());
        for (job, seen) in [
            (&dated, *NOW - TimeDelta::days(1)),
            (&undated, *NOW - TimeDelta::hours(5)),
        ] {
            panel
                .tracker
                .add_job(job, 80.0, &BTreeMap::new(), seen)
                .unwrap();
            panel.tracker.create_application(&job.key(), seen).unwrap();
        }

        let dated = build_job_detail(&panel, &dated.key()).unwrap().unwrap();
        let undated = build_job_detail(&panel, &undated.key()).unwrap().unwrap();

        assert_eq!(dated.first_seen.as_deref(), Some("2026-09-30T12:00:00Z"));
        assert_eq!(dated.posted_at.as_deref(), Some("2026-09-28T12:00:00Z"));
        assert_eq!(undated.first_seen.as_deref(), Some("2026-10-01T07:00:00Z"));
        assert_eq!(undated.posted_at, None);
    }

    /// The card of `key` wherever it sits on the board.
    fn card_of(snap: &Snapshot, key: &str) -> serde_json::Value {
        let card = snap
            .board
            .iter()
            .flat_map(|c| c.cards.iter())
            .find(|c| c.key == key)
            .unwrap_or_else(|| panic!("{key} is on the board"));
        serde_json::to_value(card).unwrap()
    }

    #[test]
    fn a_card_carries_the_posting_dates_the_pay_and_its_ages() {
        let (panel, dir) = panel();
        let t = &panel.tracker;
        // Dated, paid, waiting for Gonza since a day and a half ago.
        let dated = make_job_with(|j| {
            j.external_id = "dated".into();
            j.posted_at = Some(*NOW - TimeDelta::days(31));
            j.salary_min_monthly_usd = Some(4000.0);
            j.salary_max_monthly_usd = Some(6000.0);
        });
        let seen = *NOW - TimeDelta::days(2);
        t.add_job(&dated, 80.0, &BTreeMap::new(), seen).unwrap();
        t.create_application(&dated.key(), seen).unwrap();
        t.transition(
            &dated.key(),
            State::NeedsGonza,
            Some("listing_only"),
            *NOW - TimeDelta::hours(36),
        )
        .unwrap();
        // Undated and sent yesterday: no age, and the prune no longer applies.
        let sent = make_job_with(|j| j.external_id = "sent".into());
        t.add_job(&sent, 80.0, &BTreeMap::new(), *NOW - TimeDelta::days(50))
            .unwrap();
        t.create_application(&sent.key(), *NOW - TimeDelta::days(50))
            .unwrap();
        for state in [State::Prepared, State::Submitted] {
            t.transition(&sent.key(), state, None, *NOW - TimeDelta::days(1))
                .unwrap();
        }
        // Undated and never sent: the prune counts from the day it was first seen.
        let old = make_job_with(|j| j.external_id = "old".into());
        t.add_job(&old, 80.0, &BTreeMap::new(), *NOW - TimeDelta::days(50))
            .unwrap();
        t.create_application(&old.key(), *NOW - TimeDelta::days(50))
            .unwrap();

        let snap = build_snapshot(&panel).unwrap();
        let max_age = shipped_criteria().max_posting_age_days;

        let card = card_of(&snap, &dated.key());
        assert_eq!(card["posted_at"], "2026-08-31T12:00:00Z");
        assert_eq!(card["first_seen"], "2026-09-29T12:00:00Z");
        assert_eq!(
            (card["salary_min"].as_f64(), card["salary_max"].as_f64()),
            (Some(4000.0), Some(6000.0))
        );
        assert_eq!(card["age_days"], 31);
        assert_eq!(card["prune_in_days"], max_age - 31);
        assert_eq!(card["days_in_state"], 1, "36 hours are one whole day");
        assert_eq!(card["waiting_since"], "2026-09-30T00:00:00Z");
        assert!(card["snoozed_until"].is_null());

        let card = card_of(&snap, &sent.key());
        assert!(card["posted_at"].is_null() && card["age_days"].is_null());
        assert!(card["salary_min"].is_null() && card["salary_max"].is_null());
        assert!(
            card["prune_in_days"].is_null(),
            "sent: the prune never touches it"
        );
        assert_eq!(card["days_in_state"], 1);
        assert!(card["waiting_since"].is_null(), "only in needs_gonza");

        let card = card_of(&snap, &old.key());
        assert_eq!(card["prune_in_days"], max_age - 50, "already due: negative");
        assert!(card["age_days"].is_null());

        // The detail drawer shows the same card.
        let detail = build_job_detail(&panel, &dated.key()).unwrap().unwrap();
        assert_eq!(detail.card.age_days, Some(31));

        // Without a readable criteria.toml nobody knows the age limit.
        let mut panel = panel;
        panel.paths.profile_dir = profile_with(dir.path(), None, None);
        let snap = build_snapshot(&panel).unwrap();
        assert!(card_of(&snap, &dated.key())["prune_in_days"].is_null());
        assert_eq!(card_of(&snap, &dated.key())["age_days"], 31);
    }

    #[test]
    fn a_snoozed_application_leaves_the_queues_and_today_until_its_time() {
        let (panel, _dir) = panel();
        let t = &panel.tracker;
        let waiting = add(&panel, "1", 80.0, &[State::NeedsGonza]);
        let other = add(&panel, "2", 70.0, &[State::NeedsGonza]);
        let proposed = add(&panel, "3", 90.0, &[State::Prepared]);
        t.set_screenshot(&proposed, Path::new("shot.png")).unwrap();
        task(&panel, "Postular", None, Some(&waiting));
        task(&panel, "Otra", None, Some(&other));
        task(&panel, "Suelta", None, None);
        t.set_snoozed_until(&waiting, Some(*NOW + TimeDelta::days(1)))
            .unwrap();
        t.set_snoozed_until(&proposed, Some(*NOW + TimeDelta::days(3)))
            .unwrap();
        // A snooze whose time has passed is over, even if nobody cleared it.
        t.set_snoozed_until(&other, Some(*NOW - TimeDelta::hours(1)))
            .unwrap();

        let snap = build_snapshot(&panel).unwrap();

        assert_eq!(snap.needs_gonza.total, 1);
        assert_eq!(snap.needs_gonza.cards[0].key, other);
        assert!(snap.needs_gonza.cards[0].snoozed_until.is_none());
        assert_eq!(snap.awaiting_approval.total, 0);
        let snoozed: Vec<(&str, Option<&str>)> = snap
            .snoozed
            .cards
            .iter()
            .map(|c| (c.key.as_str(), c.snoozed_until.as_deref()))
            .collect();
        assert_eq!(
            snoozed,
            [
                (waiting.as_str(), Some("2026-10-02T12:00:00Z")),
                (proposed.as_str(), Some("2026-10-04T12:00:00Z")),
            ],
            "soonest back first"
        );
        assert_eq!(snap.snoozed.total, 2);
        assert_eq!(
            column(&snap, "needs_gonza").total,
            2,
            "the board keeps everything"
        );
        let texts: Vec<&str> = snap.today.tasks.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(
            texts,
            ["Otra", "Suelta"],
            "a task about a put-off application waits with it"
        );
        assert_eq!(snap.today.tasks_total, 2);

        t.set_snoozed_until(&waiting, None).unwrap();
        let snap = build_snapshot(&panel).unwrap();
        assert_eq!(snap.needs_gonza.total, 2);
        assert_eq!(snap.snoozed.total, 1);
        assert_eq!(snap.today.tasks_total, 3);
    }

    #[test]
    fn controls_say_when_the_engine_last_ran_and_which_login_it_used() {
        let (panel, _dir) = panel();
        let controls =
            |panel: &Panel| serde_json::to_value(build_snapshot(panel).unwrap().controls).unwrap();

        let never = controls(&panel);
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
        let stale = controls(&panel);
        assert_eq!(stale["engine"], "stale");
        assert_eq!(stale["last_run_at"], "2026-09-29T23:00:00Z");

        // A whole run wrote its own stamp: it wins over discovery's.
        panel
            .tracker
            .set_meta("last_run_at", &(*NOW - TimeDelta::hours(36)).to_rfc3339())
            .unwrap();
        let recent = controls(&panel);
        assert_eq!(
            recent["engine"], "recent",
            "36 hours is the edge, still recent"
        );
        assert_eq!(recent["last_run_at"], "2026-09-30T00:00:00Z");

        // A stamp that is not a moment is no stamp; the other key still counts.
        panel.tracker.set_meta("last_run_at", "ayer").unwrap();
        assert_eq!(controls(&panel)["last_run_at"], "2026-09-29T23:00:00Z");

        panel
            .tracker
            .set_meta(
                "claude_account_last",
                r#"{"auth_method":"api_key","subscription":"","email":"g***@example.com","org_id":"org-x","ok":false,"message":"Claude no se usó: la credencial activa es api_key","checked_at":"2026-10-01T09:00:00+00:00"}"#,
            )
            .unwrap();
        let account = controls(&panel)["claude_account"].clone();
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
        let account = controls(&panel)["claude_account"].clone();
        assert_eq!(
            (account["ok"].as_bool(), account["subscription"].as_str()),
            (Some(true), Some("max"))
        );
        assert!(account["message"].is_null());

        panel
            .tracker
            .set_meta("claude_account_last", "not json")
            .unwrap();
        assert!(controls(&panel)["claude_account"].is_null());
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
            serde_json::to_value(build_snapshot(panel).unwrap().controls).unwrap()["claude_account"]
                ["anchored"]
                .clone()
        };
        let write = |name: &str, text: &str| std::fs::write(folder.join(name), text).unwrap();

        // Nothing to compare the login with, or no way to read what there is: not anchored. A
        // verified account is not the same as an account that was checked against anything.
        assert_eq!(anchored(&panel), false, "no claude.toml at all");
        write("claude.toml", "model = \"claude-sonnet-5-5\"\n");
        assert_eq!(
            anchored(&panel),
            false,
            "the shipped file leaves both empty"
        );
        write("claude.local.toml", "expected_email = \"me@example.com\"\n");
        assert_eq!(
            anchored(&panel),
            false,
            "the address without the organization"
        );
        write(
            "claude.local.toml",
            "expected_email = \"me@example.com\"\nexpected_org_id = \"org-1\"\n",
        );
        assert_eq!(anchored(&panel), true);
        write("claude.local.toml", "expected_email = [\n");
        assert_eq!(anchored(&panel), false, "a broken file anchors nothing");
    }

    #[test]
    fn controls_carry_the_auto_approval_threshold_or_null() {
        let (panel, _dir) = panel();
        let controls =
            |panel: &Panel| serde_json::to_value(build_snapshot(panel).unwrap().controls).unwrap();
        let off = controls(&panel);
        assert!(
            off.as_object().unwrap().contains_key("auto_approve_min")
                && off["auto_approve_min"].is_null(),
            "off is a null that is there"
        );
        panel.tracker.set_meta("auto_approve_min", "90").unwrap();
        assert_eq!(controls(&panel)["auto_approve_min"], 90);
        // A damaged setting is off, never a number the page would trust.
        panel.tracker.set_meta("auto_approve_min", "999").unwrap();
        assert!(controls(&panel)["auto_approve_min"].is_null());
    }

    #[test]
    fn controls_carry_how_the_last_run_ended_and_what_each_step_did() {
        let (panel, _dir) = panel();
        let controls =
            |panel: &Panel| serde_json::to_value(build_snapshot(panel).unwrap().controls).unwrap();
        let meta = |key: &str, value: &str| panel.tracker.set_meta(key, value).unwrap();

        // No run yet: both keys are there, empty.
        let none = controls(&panel);
        assert!(none["last_run_result"].is_null() && none["last_run_steps"].is_null());
        assert!(none.as_object().unwrap().contains_key("last_run_result"));
        assert!(none.as_object().unwrap().contains_key("last_run_steps"));

        // What `run-daily` writes (src/cli.rs, `record_run`): the result, the steps, then the stamp.
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
        use crate::panel::scheduler::{FixedTask, TaskAnswer};

        // An empty `meta` and no question asked: the engine never ran, and nobody knows
        // about the task. "No data" is `null`, never `exists: false`.
        let (bare, _dir) = panel();
        let controls = serde_json::to_value(build_snapshot(&bare).unwrap().controls).unwrap();
        assert_eq!(controls["engine"], "never");
        assert!(controls["last_run_at"].is_null());
        assert!(controls["scheduled_task"].is_null());

        let task_of = |answer: Option<TaskAnswer>| {
            let (panel, _dir) = panel();
            let panel = panel.with_task_query(FixedTask(answer));
            panel.tasks.settle();
            let controls = serde_json::to_value(build_snapshot(&panel).unwrap().controls).unwrap();
            controls["scheduled_task"].clone()
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
        use crate::panel::scheduler::{TaskAnswer, TaskQuery};
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
            "email": "jane.doe@example.com",
            "org_id": "org-0000",
            "config_dir": format!("{forward}/AppData/Local/jobbot/claude-config"),
            "cli_version": "2.1.284",
            "warnings": [],
            "ok": false,
            "message": format!(
                "claude usa el directorio {profile}\\.claude, no {profile}\\AppData\\Local\\jobbot\\claude-config; sesión de jane.doe@example.com"
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
        assert_eq!(account["email"], "j***@example.com");
        let config_dir = account["config_dir"].as_str().unwrap();
        assert!(
            config_dir.starts_with('%') && config_dir.ends_with(r"\jobbot\claude-config"),
            "the folder of the user is a variable, got {config_dir}"
        );
        assert!(account.get("warnings").is_none());

        let text = snapshot.to_string().to_lowercase();
        let doubled = profile.replace('\\', r"\\"); // how a backslash looks inside JSON text
        for secret in ["jane.doe@", &profile, &forward, &doubled] {
            assert!(
                !text.contains(&secret.to_lowercase()),
                "the snapshot must not carry {secret:?}"
            );
        }
    }

    // ---- the contract the front fixed (tests/fixtures/snapshot-keys.json)

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
    /// asked, plus `whenAccount` when `controls.claude_account` is there.
    fn missing_contract_paths(snapshot: &serde_json::Value, with_cards: bool) -> Vec<String> {
        let contract: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/snapshot-keys.json"))
                .expect("snapshot-keys.json is JSON");
        let mut groups = vec!["always"];
        if with_cards {
            groups.push("withCards");
        }
        if snapshot["controls"]["claude_account"].is_object() {
            groups.push("whenAccount");
        }
        groups
            .iter()
            .flat_map(|group| contract[*group].as_array().expect("a list").iter())
            .map(|path| path.as_str().expect("a string").to_string())
            .filter(|path| !has_path(snapshot, &path.split('.').collect::<Vec<_>>()))
            .collect()
    }

    #[test]
    fn an_empty_snapshot_has_every_key_the_front_relies_on() {
        let (panel, _dir) = panel();
        let json = serde_json::to_value(build_snapshot(&panel).unwrap()).unwrap();
        assert_eq!(missing_contract_paths(&json, false), Vec::<String>::new());
        // The walk can fail: a key the snapshot does not have is reported, with and without a list.
        assert!(!has_path(&json, &["controls", "no_such_key"]));
        assert!(!has_path(&json, &["board", "no_such_key"]));
        assert!(!has_path(&json, &["board[]", "cards[]", "key"]));
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
        use crate::panel::scheduler::{FixedTask, TaskAnswer};
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
        // The sub-keys the front checks on the account and on the task (`ACCOUNT_KEYS`, `TASK_KEYS`
        // of `panel-src/test/helpers/contract.mjs`); null in an empty snapshot, so only here.
        let keys = |value: &serde_json::Value| -> Vec<String> {
            let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            keys
        };
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
}
