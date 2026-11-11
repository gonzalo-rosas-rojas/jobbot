//! What Gonza can do to an application, a task, a drill card or the STOP switch, in one place. The command
//! line and the panel both call these functions, so a button and a command can never disagree.
//! Messages are Spanish and end up on a terminal or in the panel. None of these actions starts a
//! discovery, a browser or a Claude call: they only write the tracker, the prep store or the STOP
//! file, which is why, like `approve` and `prep grade` before them, they ignore STOP and the
//! execution window.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, Datelike, Months, NaiveTime, TimeDelta, TimeZone, Utc};
use chrono_tz::America::Santiago;
use jobbot_domain::handoff::is_sent_by_hand;
use jobbot_domain::models::Job;
use jobbot_domain::outbound::{clean, plain_line};
use jobbot_domain::prep::Question;
use jobbot_domain::state::State;
use jobbot_domain::text::{link_base, safe_key};

use crate::paths::Paths;
use crate::ports::store::LearnRepo;
use crate::ports::store::{NewTask, Store, StoreError, TaskAdded, TaskCompletion, UndoRefusal};
use tracing::field::Empty;

const NOTE_MAX_CHARS: usize = 500;

/// The reason `record` leaves for the instant between `scored` and `submitted`. It starts with
/// `handoff::MANUAL_RECORD_PREFIX`, so `is_sent_by_hand` knows it.
const RECORD_REASON: &str = "manual_record: enviada a mano, registrándola";

/// The result of one action. `ok == false` is "not possible" (unknown key, illegal move, bad
/// input), never a failure of the program: those come back as `Err`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub message: String,
}

impl Outcome {
    fn done(message: String) -> Outcome {
        Outcome::traced(true, message)
    }

    fn refused(message: String) -> Outcome {
        Outcome::traced(false, message)
    }

    /// The outcome, also recorded as the `ok` field of the action's span.
    fn traced(ok: bool, message: String) -> Outcome {
        tracing::Span::current().record("ok", ok);
        Outcome { ok, message }
    }
}

/// The states Gonza may record by hand: everything that happens after a submission, plus
/// withdrawing. The engine owns the others.
pub const MARKABLE: [State; 8] = [
    State::Replied,
    State::TechnicalTest,
    State::Interview,
    State::Offer,
    State::Accepted,
    State::Rejected,
    State::Withdrawn,
    State::Ghosted,
];

/// The hand-recordable states an application in `state` may move to, in the state machine's order.
pub fn marks_from(state: State) -> Vec<State> {
    state
        .allowed_next()
        .iter()
        .copied()
        .filter(|next| MARKABLE.contains(next))
        .collect()
}

/// Spanish text for a refusal of the tracker. An application in `needs_gonza` gets a hint: it
/// cannot be marked directly, the way out is `confirm` (it was sent) or `resolve` (back to the queue).
pub fn explain(error: &StoreError) -> String {
    match error {
        StoreError::UnknownKey(key) => format!("no existe la postulación {key}"),
        StoreError::IllegalTransition { from, to } => {
            let hint = if from == State::NeedsGonza.as_str() {
                ". Está en «necesita a Gonza»: usa confirm si ya se envió, o resolve para devolverla a la cola"
            } else {
                ""
            };
            format!("no se puede pasar de {from} a {to}{hint}")
        }
        // The database, the disk or a stored value failed: the whole chain, so the cause is not
        // lost.
        other => {
            let mut text = other.to_string();
            let mut source = std::error::Error::source(other);
            while let Some(cause) = source {
                text.push_str(": ");
                text.push_str(&cause.to_string());
                source = cause.source();
            }
            format!("error de base de datos: {text}")
        }
    }
}

/// Marks an application as reviewed and approved by Gonza. Repeating it changes nothing. An
/// application waiting in `prepared` with no saved capture is refused: the approval is his review
/// of the filled form, and without a capture there is nothing he could have reviewed.
#[tracing::instrument(skip_all, fields(ok = Empty))]
pub fn approve(tracker: &dyn Store, key: &str) -> Result<Outcome> {
    let Some(row) = tracker.get_application(key)? else {
        return Ok(Outcome::refused(format!("No existe: {key}")));
    };
    if row.state == State::Prepared && row.screenshot_path.is_none() {
        return Ok(Outcome::refused(format!(
            "No hay captura que revisar: {key} aún no tiene el formulario llenado (corre el ensayo)"
        )));
    }
    tracker.approve(key)?;
    Ok(Outcome::done(format!("Aprobada: {key}")))
}

/// Takes an application out of `needs_gonza`: to `prepared` when it has its CV, otherwise to
/// `scored`, where `prepare` makes the PDF. An application that never got its PDF sent to
/// `prepared` would only be handed straight back by `submit_all`.
#[tracing::instrument(skip_all, fields(ok = Empty))]
pub fn resolve(tracker: &dyn Store, key: &str, now: DateTime<Utc>) -> Result<Outcome> {
    let Some(row) = tracker.get_application(key)? else {
        return Ok(Outcome::refused(format!("No existe: {key}")));
    };
    let (target, message) = if row.cv_path.is_some() {
        (State::Prepared, "De vuelta a la cola")
    } else {
        (
            State::Scored,
            "De vuelta a puntuadas (sin CV, prepare le hará el PDF)",
        )
    };
    match tracker.transition(key, target, None, now) {
        Ok(()) => {
            // Gonza fixed what stopped it: the next run fills the form again and takes a fresh
            // screenshot, and an earlier unconfirmed click no longer holds the company.
            tracker.clear_for_retry(key)?;
            Ok(Outcome::done(format!("{message}: {key}")))
        }
        // A database failure is a real error; the other variants are just "cannot resolve this one".
        Err(e @ StoreError::Backend(_)) => Err(e.into()),
        Err(e) => Ok(Outcome::refused(format!(
            "No se pudo resolver {key}: {}",
            explain(&e)
        ))),
    }
}

/// For `unconfirmed_submit` and assisted forms Gonza finished himself: the page did not confirm
/// to the program, but Gonza saw that it went through. It counts as a reviewed submission.
#[tracing::instrument(skip_all, fields(ok = Empty))]
pub fn confirm(tracker: &dyn Store, key: &str, now: DateTime<Utc>) -> Result<Outcome> {
    let Some(row) = tracker.get_application(key)? else {
        return Ok(Outcome::refused(format!("No existe: {key}")));
    };
    let sent_by_hand = row.needs_reason.as_deref().is_some_and(is_sent_by_hand);
    match tracker.transition(key, State::Submitted, Some("confirmada por Gonza"), now) {
        Ok(()) => {
            // A bot click that Gonza checked is a reviewed submission. A form he sent himself
            // (handoff, listing-only, manual-only) was never filled by the bot, so it proves
            // nothing about the filler and must not end the "first N reviewed" phase.
            if !sent_by_hand {
                tracker.approve(key)?;
            }
            Ok(Outcome::done(format!("Confirmada como enviada: {key}")))
        }
        Err(e @ StoreError::Backend(_)) => Err(e.into()),
        Err(e) => Ok(Outcome::refused(format!(
            "No se pudo confirmar {key}: {}",
            explain(&e)
        ))),
    }
}

/// The longest an application may be put off: past that, "later" means "never".
const SNOOZE_MAX_DAYS: i64 = 365;

/// When a snooze ends, from what Gonza typed or clicked: a number of days (`1d`, `3d`, `7d`), the
/// next Monday (`monday`, at 00:00 in Santiago so the card is back when he opens the panel that
/// morning), or an RFC 3339 moment. `None` for anything else.
pub fn parse_snooze_until(text: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let text = text.trim();
    if let Some(days) = text
        .strip_suffix('d')
        .and_then(|n| n.parse::<i64>().ok())
        .filter(|n| (1..=SNOOZE_MAX_DAYS).contains(n))
    {
        return Some(now + TimeDelta::days(days));
    }
    if text.eq_ignore_ascii_case("monday") {
        let today = now.with_timezone(&Santiago).date_naive();
        let ahead = (7 - today.weekday().num_days_from_monday() as i64) % 7;
        let monday = today + TimeDelta::days(if ahead == 0 { 7 } else { ahead });
        return Santiago
            .from_local_datetime(&monday.and_time(NaiveTime::MIN))
            .earliest()
            .map(|at| at.with_timezone(&Utc));
    }
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

/// Puts an application off until `until` (see `parse_snooze_until`): the panel keeps it out of
/// Gonza's queues until then and nothing else changes. The moment must be ahead of `now` and at
/// most a year away. Any state goes, since the card may sit in any list he reads.
#[tracing::instrument(skip_all, fields(ok = Empty))]
pub fn snooze(tracker: &dyn Store, key: &str, until: &str, now: DateTime<Utc>) -> Result<Outcome> {
    if tracker.get_application(key)?.is_none() {
        return Ok(Outcome::refused(format!("No existe: {key}")));
    }
    let Some(at) = parse_snooze_until(until, now) else {
        return Ok(Outcome::refused(format!(
            "No entiendo hasta cuándo: {}. Usa 1d, 3d, 7d, monday o una fecha RFC 3339",
            clean(until, 40)
        )));
    };
    if at <= now {
        return Ok(Outcome::refused(
            "La fecha ya pasó: posponer es hacia adelante".to_string(),
        ));
    }
    if at > now + TimeDelta::days(SNOOZE_MAX_DAYS) {
        return Ok(Outcome::refused(format!(
            "Hasta un año como máximo ({SNOOZE_MAX_DAYS} días)"
        )));
    }
    tracker.set_snoozed_until(key, Some(at))?;
    Ok(Outcome::done(format!(
        "Pospuesta hasta {}: {key}",
        at.with_timezone(&Santiago).format("%Y-%m-%d %H:%M")
    )))
}

/// Brings a put-off application back to the queues. One that was not put off is a refusal.
#[tracing::instrument(skip_all, fields(ok = Empty))]
pub fn unsnooze(tracker: &dyn Store, key: &str) -> Result<Outcome> {
    let Some(row) = tracker.get_application(key)? else {
        return Ok(Outcome::refused(format!("No existe: {key}")));
    };
    if row.snoozed_until.is_none() {
        return Ok(Outcome::refused(format!("No estaba pospuesta: {key}")));
    }
    tracker.set_snoozed_until(key, None)?;
    Ok(Outcome::done(format!("De vuelta en tu lista: {key}")))
}

/// How long after a mark Gonza can still take it back (`unmark`).
pub const UNDO_WINDOW: TimeDelta = TimeDelta::minutes(10);

/// Records a company's answer by hand. The move still has to be legal: `Tracker::transition`
/// refuses the rest. The note is cleaned and cut; an empty one is no note. For `UNDO_WINDOW`
/// afterwards `unmark` can take it back.
#[tracing::instrument(skip_all, fields(ok = Empty))]
pub fn mark(
    tracker: &dyn Store,
    key: &str,
    state_text: &str,
    note: Option<&str>,
    now: DateTime<Utc>,
) -> Result<Outcome> {
    record_mark(tracker, key, state_text, note, now, true)
}

/// `mark` for an answer read from an email (`inbox`): Gonza did not make it, so there is nothing
/// for him to undo.
pub(crate) fn mark_from_email(
    tracker: &dyn Store,
    key: &str,
    state_text: &str,
    note: Option<&str>,
    at: DateTime<Utc>,
) -> Result<Outcome> {
    record_mark(tracker, key, state_text, note, at, false)
}

fn record_mark(
    tracker: &dyn Store,
    key: &str,
    state_text: &str,
    note: Option<&str>,
    now: DateTime<Utc>,
    by_hand: bool,
) -> Result<Outcome> {
    let Some(state) = State::parse(state_text).filter(|s| MARKABLE.contains(s)) else {
        let names: Vec<&str> = MARKABLE.iter().map(State::as_str).collect();
        return Ok(Outcome::refused(format!(
            "Estado no válido: {state_text}. Usa uno de: {}",
            names.join(", ")
        )));
    };
    let note = note
        .map(|text| clean(text, NOTE_MAX_CHARS))
        .filter(|text| !text.is_empty());
    let moved = if by_hand {
        tracker.mark_transition(key, state, note.as_deref(), now)
    } else {
        tracker.transition(key, state, note.as_deref(), now)
    };
    match moved {
        Ok(()) => Ok(Outcome::done(format!("{key} → {state}"))),
        Err(e @ StoreError::Backend(_)) => Err(e.into()),
        Err(e) => Ok(Outcome::refused(format!(
            "No se pudo marcar {key}: {}",
            explain(&e)
        ))),
    }
}

/// Takes back Gonza's last mark of an application (`mark`), within `UNDO_WINDOW` and only while
/// nothing else has changed it since: the application is left exactly as it was before.
#[tracing::instrument(skip_all, fields(ok = Empty))]
pub fn unmark(tracker: &dyn Store, key: &str, now: DateTime<Utc>) -> Result<Outcome> {
    let message = match tracker.undo_mark(key, now - UNDO_WINDOW)? {
        Ok(undone) => {
            return Ok(Outcome::done(format!(
                "Deshecho: {key} vuelve a {}",
                undone.from
            )));
        }
        Err(UndoRefusal::UnknownKey) => format!("No existe la postulación {key}"),
        Err(UndoRefusal::NoRecentMark) => "No hay una marca reciente que deshacer".to_string(),
        Err(UndoRefusal::ChangedSince) => {
            "Hubo otro cambio después: ya no se puede deshacer".to_string()
        }
        Err(UndoRefusal::TooOld) => format!(
            "Ya pasaron más de {} minutos: ya no se puede deshacer",
            UNDO_WINDOW.num_minutes()
        ),
    };
    Ok(Outcome::refused(message))
}

/// An application that went out by hand (Gonza, or Claude with Playwright) and now has to enter the
/// tracker. With `key`, the posting is one already stored (see `top`); without it, `company`,
/// `title` and `url` identify it and a posting that is not stored yet is created.
#[derive(Debug, Clone, Default)]
pub struct RecordSpec {
    pub key: Option<String>,
    pub company: String,
    pub title: String,
    pub url: String,
    pub location: String,
    pub cv_path: Option<PathBuf>,
    pub salary_min_monthly_usd: Option<f64>,
    pub salary_max_monthly_usd: Option<f64>,
    pub note: Option<String>,
}

/// Puts an application sent outside jobbot into the tracker as `submitted` at `now` (the real
/// moment it went out). It walks the state machine (`scored` to `needs_gonza` to `submitted`, or
/// straight from `prepared`/`needs_gonza`), so the history, the per-company cap and the 90-day wait
/// all know about it. It stays unapproved: a form nobody filled with the bot proves nothing about
/// the filler. An application already submitted (or later) or withdrawn is refused, and nothing
/// changes.
#[tracing::instrument(skip_all, fields(ok = Empty))]
pub fn record(tracker: &dyn Store, spec: &RecordSpec, now: DateTime<Utc>) -> Result<Outcome> {
    let key = match &spec.key {
        Some(key) => {
            if tracker.get_job(key)?.is_none() {
                return Ok(Outcome::refused(format!("No existe: {key}")));
            }
            key.clone()
        }
        None => {
            // The page and the command line hand over free text: it is stored as one clean line,
            // cut, and the address must be one that is safe to open (the posting is shown with it).
            let company = clean(&plain_line(&spec.company), RECORD_COMPANY_MAX_CHARS);
            let title = clean(&plain_line(&spec.title), RECORD_TITLE_MAX_CHARS);
            let raw_url = plain_line(&spec.url);
            if company.is_empty() || title.is_empty() || raw_url.is_empty() {
                return Ok(Outcome::refused(
                    "Faltan datos: sin --key hacen falta --company, --title y --url".to_string(),
                ));
            }
            let Some(url) = clean_link(&raw_url, RECORD_LINK_MAX_CHARS) else {
                return Ok(Outcome::refused(format!(
                    "El enlace no sirve: debe ser una dirección http:// o https://, sin espacios, de hasta {RECORD_LINK_MAX_CHARS} caracteres"
                )));
            };
            let base = link_base(&url);
            let id_source = if base.is_empty() { url.clone() } else { base };
            let job = Job {
                source: "manual".to_string(),
                external_id: safe_key(
                    id_source
                        .trim_start_matches("https://")
                        .trim_start_matches("http://"),
                ),
                company,
                title,
                apply_url: url.clone(),
                url,
                location: clean(&plain_line(&spec.location), RECORD_LOCATION_MAX_CHARS),
                remote: true,
                description: "Enviada a mano; el aviso original no se guardó en la base."
                    .to_string(),
                salary_min_monthly_usd: spec.salary_min_monthly_usd,
                salary_max_monthly_usd: spec.salary_max_monthly_usd,
                posted_at: None,
            };
            match tracker.find_job_key(&job)? {
                Some(existing) => existing,
                None => {
                    tracker.add_job(&job, 0.0, &BTreeMap::new(), now)?;
                    job.key()
                }
            }
        }
    };
    tracker.create_application(&key, now)?;
    let Some(row) = tracker.get_application(&key)? else {
        return Ok(Outcome::refused(format!("No existe: {key}")));
    };
    match row.state {
        State::Scored | State::Prepared | State::NeedsGonza => {}
        State::Withdrawn => {
            return Ok(Outcome::refused(format!(
                "No se registró: la postulación está en withdrawn ({key})"
            )));
        }
        other => {
            return Ok(Outcome::refused(format!(
                "No se registró: ya está registrada como {other} ({key})"
            )));
        }
    }
    let note = spec
        .note
        .as_deref()
        .map(|text| clean(text, NOTE_MAX_CHARS))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "enviada a mano".to_string());
    if row.state == State::Scored {
        tracker.transition(&key, State::NeedsGonza, Some(RECORD_REASON), now)?;
    }
    tracker.transition(&key, State::Submitted, Some(&note), now)?;
    if let Some(cv) = &spec.cv_path {
        tracker.set_cv_path(&key, cv)?;
    }
    let job = tracker.get_job(&key)?;
    let (company, title) = job.map(|job| (job.company, job.title)).unwrap_or_default();
    Ok(Outcome::done(format!(
        "Registrada como enviada: {company} — {title} ({key})"
    )))
}

/// The earliest date that makes sense for an email or a deadline. A date before it is a mistake
/// (a year 0000 in a mail header), and the importer and `task add` both refuse it.
pub fn earliest_date() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0)
        .single()
        .expect("2000-01-01 exists")
}

/// The latest deadline that makes sense: five years from `now`.
pub fn latest_due(now: DateTime<Utc>) -> DateTime<Utc> {
    now.checked_add_months(Months::new(60)).unwrap_or(now)
}

const TASK_TEXT_MAX_CHARS: usize = 300;
const TASK_LINK_MAX_CHARS: usize = 300;
/// A posting's address can carry a long tracking query; this is generous but not unbounded.
pub const RECORD_LINK_MAX_CHARS: usize = 2000;
const RECORD_COMPANY_MAX_CHARS: usize = 200;
const RECORD_TITLE_MAX_CHARS: usize = 300;
const RECORD_LOCATION_MAX_CHARS: usize = 200;

/// A to-do that Gonza (or Claude, in a chat) puts on his list by hand, with no email behind it.
#[derive(Debug, Clone, Default)]
pub struct TaskSpec {
    pub text: String,
    pub due_at: Option<DateTime<Utc>>,
    /// The posting it is about; it must be in the tracker.
    pub job_key: Option<String>,
    /// An address to open for it: http or https only.
    pub link: Option<String>,
}

/// An http or https address of at most `max` characters, one line, with a host. `None` for anything
/// else (a `javascript:` or `file:` address, a relative one, one with spaces or too long).
fn clean_link(text: &str, max: usize) -> Option<String> {
    let link = plain_line(text);
    if link.chars().count() > max || link.chars().any(char::is_whitespace) {
        return None;
    }
    let url = url::Url::parse(&link).ok()?;
    (matches!(url.scheme(), "http" | "https") && url.host_str().is_some()).then_some(link)
}

/// Puts a task on Gonza's list (`jobbot task add`): the text is cleaned and cut at 300, the
/// deadline must be a date the importer would accept, the posting must exist, and the same text
/// for the same posting is not added while one like it is still open. Refusals change nothing.
#[tracing::instrument(skip_all, fields(ok = Empty))]
pub fn task_add(tracker: &dyn Store, spec: &TaskSpec, now: DateTime<Utc>) -> Result<Outcome> {
    let text = clean(&plain_line(&spec.text), TASK_TEXT_MAX_CHARS);
    if text.is_empty() {
        return Ok(Outcome::refused("La tarea está vacía".to_string()));
    }
    if let Some(due) = spec.due_at
        && (due < earliest_date() || due > latest_due(now))
    {
        return Ok(Outcome::refused(
            "La fecha límite está fuera de rango: debe ser desde el 2000-01-01 hasta cinco años desde hoy"
                .to_string(),
        ));
    }
    let job_key = spec
        .job_key
        .as_deref()
        .map(plain_line)
        .filter(|key| !key.is_empty());
    if let Some(key) = &job_key
        && tracker.get_job(key)?.is_none()
    {
        return Ok(Outcome::refused(format!("No existe el aviso: {key}")));
    }
    let link = match spec
        .link
        .as_deref()
        .map(plain_line)
        .filter(|l| !l.is_empty())
    {
        Some(raw) => match clean_link(&raw, TASK_LINK_MAX_CHARS) {
            Some(link) => Some(link),
            None => {
                return Ok(Outcome::refused(format!(
                    "El enlace no sirve: debe ser una dirección http:// o https://, sin espacios, de hasta {TASK_LINK_MAX_CHARS} caracteres"
                )));
            }
        },
        None => None,
    };
    let task = NewTask {
        job_key,
        kind: "manual".to_string(),
        text: text.clone(),
        due_at: spec.due_at,
        source_message_id: None,
        link,
    };
    Ok(match tracker.add_manual_task(&task, now)? {
        TaskAdded::Created(id) => Outcome::done(format!("Tarea agregada: #{id} {text}")),
        TaskAdded::AlreadyOpen(id) => {
            Outcome::refused(format!("Ya tienes esa tarea abierta (#{id}): {text}"))
        }
    })
}

/// Closes one of Gonza's tasks (`jobbot task done ID`). A task that is already done or does not
/// exist is a refusal; nothing changes.
#[tracing::instrument(skip_all, fields(ok = Empty))]
pub fn task_done(tracker: &dyn Store, id: i64) -> Result<Outcome> {
    let Some(task) = tracker.get_task(id)? else {
        return Ok(Outcome::refused(format!("No existe la tarea #{id}")));
    };
    Ok(match tracker.complete_task(id)? {
        TaskCompletion::Done => Outcome::done(format!("Hecha: #{id} {}", plain_line(&task.text))),
        TaskCompletion::AlreadyDone => Outcome::refused(format!("La tarea #{id} ya estaba hecha")),
        TaskCompletion::Unknown => Outcome::refused(format!("No existe la tarea #{id}")),
    })
}

/// Records Gonza's grade (0 to 5) for a card of the drill. Anything else changes nothing.
#[tracing::instrument(skip_all, fields(ok = Empty))]
pub fn grade_card(
    store: &dyn LearnRepo,
    questions: &[Question],
    id: &str,
    grade: u8,
    now: DateTime<Utc>,
) -> Result<Outcome> {
    if grade > 5 {
        return Ok(Outcome::refused(format!("La nota va de 0 a 5, no {grade}")));
    }
    let Some(question) = questions.iter().find(|q| q.id == id) else {
        return Ok(Outcome::refused(format!("Pregunta desconocida: {id}")));
    };
    store.grade(question, grade, now)?;
    Ok(Outcome::done(format!(
        "{}: nota {grade} registrada.",
        question.id
    )))
}

/// Where the score from which the engine may approve without Gonza is kept: `meta`, written only
/// by this action (from the panel or the command line) and read by the snapshot and, when it
/// exists, the engine.
pub const AUTO_APPROVE_KEY: &str = "auto_approve_min";
/// The highest score there is.
const SCORE_CEILING: i64 = 100;

/// The auto-approval threshold, or `None` when it is off. A stored value that is not a whole number
/// from 1 to 100 counts as off: a damaged setting must never approve anything.
pub fn auto_approve_min(tracker: &dyn Store) -> Result<Option<i64>> {
    Ok(tracker
        .get_meta(AUTO_APPROVE_KEY)?
        .and_then(|text| text.trim().parse::<i64>().ok())
        .filter(|value| (1..=SCORE_CEILING).contains(value)))
}

/// Sets the score from which postings may be approved without Gonza (`Some`), or turns that off
/// (`None`). It has to be a whole number from `apply_threshold` (rounded up: below it a posting is
/// not even queued) to 100. `apply_threshold` is `criteria.toml`'s; when it could not be read
/// (`None`) no number is accepted, since there is nothing to check it against, but turning it off
/// always works. A refusal changes nothing.
#[tracing::instrument(skip_all, fields(ok = Empty))]
pub fn set_auto_approve_min(
    tracker: &dyn Store,
    value: Option<i64>,
    apply_threshold: Option<f64>,
) -> Result<Outcome> {
    let Some(value) = value else {
        tracker.delete_meta(AUTO_APPROVE_KEY)?;
        return Ok(Outcome::done("Auto-aprobación apagada".to_string()));
    };
    let Some(threshold) = apply_threshold.filter(|t| t.is_finite()) else {
        return Ok(Outcome::refused(
            "No pude leer criteria.toml, así que no puedo comprobar el umbral: no lo cambié"
                .to_string(),
        ));
    };
    let floor = (threshold.ceil() as i64).clamp(1, SCORE_CEILING);
    if !(floor..=SCORE_CEILING).contains(&value) {
        return Ok(Outcome::refused(format!(
            "El umbral de auto-aprobación debe ser un entero de {floor} (el puntaje mínimo para \
             postular, criteria.toml) a {SCORE_CEILING}"
        )));
    }
    tracker.set_meta(AUTO_APPROVE_KEY, &value.to_string())?;
    Ok(Outcome::done(format!(
        "Auto-aprobación desde {value} puntos"
    )))
}

/// Creates the STOP file.
#[tracing::instrument(skip_all)]
pub fn stop(paths: &Paths) -> Result<()> {
    std::fs::write(&paths.stop_file, "stop")?;
    Ok(())
}

/// Removes the STOP file; one that is already gone is fine.
#[tracing::instrument(skip_all)]
pub fn resume(paths: &Paths) -> Result<()> {
    match std::fs::remove_file(&paths.stop_file) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use jobbot_domain::fixtures::NOW;

    use super::*;

    #[test]
    fn marks_from_lists_only_the_moves_gonza_may_record() {
        assert_eq!(
            marks_from(State::Submitted),
            vec![
                State::Replied,
                State::TechnicalTest,
                State::Interview,
                State::Rejected,
                State::Ghosted,
                State::Withdrawn
            ]
        );
        assert_eq!(marks_from(State::Scored), vec![State::Withdrawn]);
        assert_eq!(marks_from(State::Accepted), Vec::<State>::new());
        assert!(!marks_from(State::Interview).contains(&State::Submitted));
    }

    #[test]
    fn the_reason_record_leaves_while_it_works_counts_as_sent_by_hand() {
        // If the program dies between the two moves, the row waits in needs_gonza with this
        // reason; prune must never withdraw it and `confirm` must not count it as a review.
        assert!(is_sent_by_hand(RECORD_REASON));
    }

    #[test]
    fn a_snooze_is_days_the_next_monday_or_a_moment() {
        // NOW is Thursday 2026-10-01 12:00Z, 09:00 in Santiago (UTC-3 in October).
        let until = |text: &str| parse_snooze_until(text, *NOW);
        assert_eq!(until("1d"), Some(*NOW + TimeDelta::days(1)));
        assert_eq!(until(" 7d "), Some(*NOW + TimeDelta::days(7)));
        assert_eq!(
            until("monday").map(|at| at.to_rfc3339()),
            Some("2026-10-05T03:00:00+00:00".to_string()),
            "Monday 00:00 Santiago"
        );
        let monday = Utc.with_ymd_and_hms(2026, 10, 5, 15, 0, 0).unwrap();
        assert_eq!(
            parse_snooze_until("Monday", monday).map(|at| at.to_rfc3339()),
            Some("2026-10-12T03:00:00+00:00".to_string()),
            "on a Monday, the next one"
        );
        assert_eq!(
            until("2026-10-09T15:00:00Z"),
            Some(Utc.with_ymd_and_hms(2026, 10, 9, 15, 0, 0).unwrap())
        );
        for bad in ["", "0d", "-1d", "d", "400d", "mañana", "2026-10-09", "3 d"] {
            assert_eq!(until(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn stop_creates_the_file_and_resume_removes_it_even_twice() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path());
        stop(&paths).unwrap();
        assert!(paths.stop_file.exists());
        resume(&paths).unwrap();
        assert!(!paths.stop_file.exists());
        resume(&paths).unwrap(); // already gone: not an error
    }
}
