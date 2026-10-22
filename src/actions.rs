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

use crate::handoff::is_sent_by_hand;
use crate::models::Job;
use crate::outbound::{clean, plain_line};
use crate::paths::Paths;
use crate::prep::{PrepStore, Question};
use crate::text::{link_base, safe_key};
use crate::tracker::{NewTask, State, TaskAdded, TaskCompletion, Tracker, TrackerError};

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
        Outcome { ok: true, message }
    }

    fn refused(message: String) -> Outcome {
        Outcome { ok: false, message }
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
pub fn explain(error: &TrackerError) -> String {
    match error {
        TrackerError::UnknownKey(key) => format!("no existe la postulación {key}"),
        TrackerError::IllegalTransition { from, to } => {
            let hint = if from == State::NeedsGonza.as_str() {
                ". Está en «necesita a Gonza»: usa confirm si ya se envió, o resolve para devolverla a la cola"
            } else {
                ""
            };
            format!("no se puede pasar de {from} a {to}{hint}")
        }
        TrackerError::Db(e) => format!("error de base de datos: {e}"),
    }
}

/// Marks an application as reviewed and approved by Gonza. Repeating it changes nothing.
pub fn approve(tracker: &Tracker, key: &str) -> Result<Outcome> {
    if tracker.get_application(key)?.is_none() {
        return Ok(Outcome::refused(format!("No existe: {key}")));
    }
    tracker.approve(key)?;
    Ok(Outcome::done(format!("Aprobada: {key}")))
}

/// Takes an application out of `needs_gonza`: to `prepared` when it has its CV, otherwise to
/// `scored`, where `prepare` makes the PDF. An application that never got its PDF sent to
/// `prepared` would only be handed straight back by `submit_all`.
pub fn resolve(tracker: &Tracker, key: &str, now: DateTime<Utc>) -> Result<Outcome> {
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
        Err(TrackerError::Db(e)) => Err(e.into()),
        Err(e) => Ok(Outcome::refused(format!(
            "No se pudo resolver {key}: {}",
            explain(&e)
        ))),
    }
}

/// For `unconfirmed_submit` and assisted forms Gonza finished himself: the page did not confirm
/// to the program, but Gonza saw that it went through. It counts as a reviewed submission.
pub fn confirm(tracker: &Tracker, key: &str, now: DateTime<Utc>) -> Result<Outcome> {
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
        Err(TrackerError::Db(e)) => Err(e.into()),
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
pub fn snooze(tracker: &Tracker, key: &str, until: &str, now: DateTime<Utc>) -> Result<Outcome> {
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
pub fn unsnooze(tracker: &Tracker, key: &str) -> Result<Outcome> {
    let Some(row) = tracker.get_application(key)? else {
        return Ok(Outcome::refused(format!("No existe: {key}")));
    };
    if row.snoozed_until.is_none() {
        return Ok(Outcome::refused(format!("No estaba pospuesta: {key}")));
    }
    tracker.set_snoozed_until(key, None)?;
    Ok(Outcome::done(format!("De vuelta en tu lista: {key}")))
}

/// Records a company's answer until Gmail does it (plan 1b-3). The move still has to be legal:
/// `Tracker::transition` refuses the rest. The note is cleaned and cut; an empty one is no note.
pub fn mark(
    tracker: &Tracker,
    key: &str,
    state_text: &str,
    note: Option<&str>,
    now: DateTime<Utc>,
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
    match tracker.transition(key, state, note.as_deref(), now) {
        Ok(()) => Ok(Outcome::done(format!("{key} → {state}"))),
        Err(TrackerError::Db(e)) => Err(e.into()),
        Err(e) => Ok(Outcome::refused(format!(
            "No se pudo marcar {key}: {}",
            explain(&e)
        ))),
    }
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
pub fn record(tracker: &Tracker, spec: &RecordSpec, now: DateTime<Utc>) -> Result<Outcome> {
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
const RECORD_LINK_MAX_CHARS: usize = 2000;
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
pub fn task_add(tracker: &Tracker, spec: &TaskSpec, now: DateTime<Utc>) -> Result<Outcome> {
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
pub fn task_done(tracker: &Tracker, id: i64) -> Result<Outcome> {
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
pub fn grade_card(
    store: &PrepStore,
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
pub fn auto_approve_min(tracker: &Tracker) -> Result<Option<i64>> {
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
pub fn set_auto_approve_min(
    tracker: &Tracker,
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
pub fn stop(paths: &Paths) -> Result<()> {
    std::fs::write(&paths.stop_file, "stop")?;
    Ok(())
}

/// Removes the STOP file; one that is already gone is fine.
pub fn resume(paths: &Paths) -> Result<()> {
    match std::fs::remove_file(&paths.stop_file) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::testing::{NOW, make_job};

    /// A tracker holding one application of `make_job()` walked through `states`.
    fn tracker_with(states: &[State]) -> (Tracker, String) {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 80.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        for state in states {
            t.transition(&job.key(), *state, Some("nota"), *NOW)
                .unwrap();
        }
        (t, job.key())
    }

    fn bank() -> (PrepStore, Vec<Question>) {
        let store = PrepStore::in_memory().unwrap();
        let questions = store.all_questions().unwrap();
        (store, questions)
    }

    fn tracker_with_task(text: &str) -> (Tracker, i64) {
        let t = Tracker::in_memory().unwrap();
        t.add_task(
            &crate::tracker::NewTask {
                job_key: None,
                kind: "action_required".into(),
                text: text.into(),
                due_at: None,
                source_message_id: None,
                link: None,
            },
            *NOW,
        )
        .unwrap();
        let id = t.open_tasks().unwrap()[0].id;
        (t, id)
    }

    fn spec(text: &str) -> TaskSpec {
        TaskSpec {
            text: text.into(),
            ..TaskSpec::default()
        }
    }

    #[test]
    fn task_add_puts_a_manual_task_on_the_list_and_says_its_number() {
        let t = Tracker::in_memory().unwrap();
        let outcome = task_add(&t, &spec("Llamar a Globex"), *NOW).unwrap();
        assert_eq!(
            outcome,
            Outcome {
                ok: true,
                message: "Tarea agregada: #1 Llamar a Globex".to_string()
            }
        );
        let row = &t.open_tasks().unwrap()[0];
        assert_eq!(row.kind, "manual");
        assert_eq!(row.source_message_id, None);
        assert_eq!(
            (row.job_key.clone(), row.due_at, row.link.clone()),
            (None, None, None)
        );
        assert_eq!(row.created_at, *NOW);
    }

    #[test]
    fn task_add_cleans_the_text_and_cuts_it_at_300() {
        let t = Tracker::in_memory().unwrap();
        let hostile = format!("Hacer\u{1b}[2J\u{202e} algo\n{}", "x".repeat(400));
        let outcome = task_add(&t, &spec(&hostile), *NOW).unwrap();
        assert!(outcome.ok, "{outcome:?}");
        let text = t.open_tasks().unwrap()[0].text.clone();
        assert!(
            text.starts_with("Hacer [2J algo xxx") && text.ends_with('…'),
            "{text}"
        );
        assert_eq!(text.chars().count(), 301);
        assert!(!text.chars().any(|c| c.is_control()));
        assert!(!outcome.message.chars().any(|c| c.is_control()));

        let empty = task_add(&t, &spec(" \u{200b}\n "), *NOW).unwrap();
        assert!(!empty.ok);
        assert!(empty.message.contains("vacía"), "{empty:?}");
        assert_eq!(t.open_tasks().unwrap().len(), 1);
    }

    #[test]
    fn task_add_checks_the_deadline_with_the_bounds_of_the_importer() {
        let t = Tracker::in_memory().unwrap();
        let with_due = |due: DateTime<Utc>| TaskSpec {
            due_at: Some(due),
            ..spec("Con fecha")
        };
        let beyond = latest_due(*NOW) + chrono::TimeDelta::seconds(1);
        let before = earliest_date() - chrono::TimeDelta::seconds(1);
        for bad in [
            beyond,
            before,
            Utc.with_ymd_and_hms(9999, 12, 31, 23, 59, 59).unwrap(),
        ] {
            let outcome = task_add(&t, &with_due(bad), *NOW).unwrap();
            assert!(!outcome.ok, "{bad}");
            assert!(outcome.message.contains("fuera de rango"), "{outcome:?}");
        }
        assert!(t.open_tasks().unwrap().is_empty());
        // The limits themselves are inside.
        assert!(task_add(&t, &with_due(latest_due(*NOW)), *NOW).unwrap().ok);
        let other = TaskSpec {
            due_at: Some(earliest_date()),
            ..spec("Otra con fecha")
        };
        assert!(task_add(&t, &other, *NOW).unwrap().ok);
        assert_eq!(
            t.open_tasks().unwrap()[0].due_at,
            Some(earliest_date()),
            "soonest due first"
        );
    }

    #[test]
    fn task_add_needs_the_posting_to_exist() {
        let (t, key) = tracker_with(&[]);
        let named = |key: &str| TaskSpec {
            job_key: Some(key.into()),
            ..spec("Preparar la entrevista")
        };

        let unknown = task_add(&t, &named("nope:1"), *NOW).unwrap();
        assert!(!unknown.ok);
        assert!(unknown.message.contains("nope:1"), "{unknown:?}");
        assert!(t.open_tasks().unwrap().is_empty());

        assert!(task_add(&t, &named(&key), *NOW).unwrap().ok);
        assert_eq!(
            t.open_tasks().unwrap()[0].job_key.as_deref(),
            Some(key.as_str())
        );
    }

    #[test]
    fn task_add_keeps_only_a_web_link_that_is_short_and_clean() {
        let t = Tracker::in_memory().unwrap();
        let with_link = |link: &str| TaskSpec {
            link: Some(link.into()),
            ..spec("Con enlace")
        };
        let too_long = format!("https://example.com/{}", "a".repeat(290));
        for bad in [
            "javascript:alert(1)",
            "ftp://example.com/x",
            "file:///C:/secreto.txt",
            "example.com/sin-esquema",
            "https://",
            "https://ejemplo.com/con espacio",
            too_long.as_str(),
        ] {
            let outcome = task_add(&t, &with_link(bad), *NOW).unwrap();
            assert!(!outcome.ok, "{bad}");
            assert!(outcome.message.contains("enlace"), "{outcome:?}");
        }
        assert!(t.open_tasks().unwrap().is_empty());

        let ok = task_add(
            &t,
            &with_link("  HTTPS://Example.com/oferta?x=1\u{1b}  "),
            *NOW,
        )
        .unwrap();
        assert!(ok.ok, "{ok:?}");
        assert_eq!(
            t.open_tasks().unwrap()[0].link.as_deref(),
            Some("HTTPS://Example.com/oferta?x=1")
        );
        let plain = TaskSpec {
            link: Some("   ".into()),
            ..spec("Sin enlace de verdad")
        };
        assert!(task_add(&t, &plain, *NOW).unwrap().ok);
    }

    #[test]
    fn task_add_twice_while_the_first_is_open_is_refused_and_changes_nothing() {
        let t = Tracker::in_memory().unwrap();
        task_add(&t, &spec("Llamar a Globex"), *NOW).unwrap();

        let again = task_add(&t, &spec("  Llamar   a Globex "), *NOW).unwrap();

        assert!(!again.ok);
        assert_eq!(
            again.message,
            "Ya tienes esa tarea abierta (#1): Llamar a Globex"
        );
        assert_eq!(t.open_tasks().unwrap().len(), 1);
        // Closed, it can be added again.
        task_done(&t, 1).unwrap();
        assert!(task_add(&t, &spec("Llamar a Globex"), *NOW).unwrap().ok);
    }

    #[test]
    fn task_done_closes_an_open_task_and_says_which() {
        let (t, id) = tracker_with_task("Entregar prueba técnica de Acme");
        let outcome = task_done(&t, id).unwrap();
        assert_eq!(
            outcome,
            Outcome {
                ok: true,
                message: format!("Hecha: #{id} Entregar prueba técnica de Acme")
            }
        );
        assert!(t.open_tasks().unwrap().is_empty());
    }

    #[test]
    fn task_done_prints_the_text_without_control_characters() {
        // The importer cleans it, but the message goes to a terminal: it is cleaned again.
        let (t, id) = tracker_with_task("Hacer\u{1b}[2J algo\u{202e}\nya");
        let outcome = task_done(&t, id).unwrap();
        assert_eq!(outcome.message, format!("Hecha: #{id} Hacer [2J algo ya"));
    }

    #[test]
    fn task_done_refuses_a_task_that_is_already_done_or_unknown() {
        let (t, id) = tracker_with_task("Responder a Globex");
        task_done(&t, id).unwrap();

        let again = task_done(&t, id).unwrap();
        assert!(!again.ok);
        assert_eq!(again.message, format!("La tarea #{id} ya estaba hecha"));

        let unknown = task_done(&t, id + 50).unwrap();
        assert!(!unknown.ok);
        assert_eq!(unknown.message, format!("No existe la tarea #{}", id + 50));
    }

    #[test]
    fn approve_marks_the_application_and_can_be_repeated() {
        let (t, key) = tracker_with(&[State::Prepared]);
        let first = approve(&t, &key).unwrap();
        assert!(first.ok, "{first:?}");
        assert_eq!(first.message, format!("Aprobada: {key}"));
        assert!(t.get_application(&key).unwrap().unwrap().approved);
        assert!(approve(&t, &key).unwrap().ok, "approving twice is harmless");
    }

    #[test]
    fn approve_of_an_unknown_key_is_a_refusal_not_an_error() {
        let t = Tracker::in_memory().unwrap();
        assert_eq!(
            approve(&t, "nope:1").unwrap(),
            Outcome {
                ok: false,
                message: "No existe: nope:1".to_string()
            }
        );
    }

    #[test]
    fn resolve_returns_a_prepared_application_to_the_queue_and_forgets_the_attempt() {
        let (t, key) = tracker_with(&[State::Prepared, State::NeedsGonza]);
        t.set_cv_path(&key, std::path::Path::new("cv.pdf")).unwrap();
        t.record_attempt(&key, *NOW).unwrap();
        let outcome = resolve(&t, &key, *NOW).unwrap();
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(outcome.message, format!("De vuelta a la cola: {key}"));
        let app = t.get_application(&key).unwrap().unwrap();
        assert_eq!(app.state, State::Prepared);
        assert!(
            !t.company_in_cooldown(&app.company_norm, 90, *NOW).unwrap(),
            "the unconfirmed click no longer holds the company"
        );
    }

    #[test]
    fn resolve_without_a_cv_goes_back_to_scored_and_a_replay_is_refused_in_spanish() {
        let (t, key) = tracker_with(&[State::NeedsGonza]);
        let first = resolve(&t, &key, *NOW).unwrap();
        assert!(
            first.message.starts_with("De vuelta a puntuadas"),
            "{first:?}"
        );
        assert_eq!(
            t.get_application(&key).unwrap().unwrap().state,
            State::Scored
        );
        let again = resolve(&t, &key, *NOW).unwrap();
        assert!(!again.ok);
        assert!(
            again
                .message
                .starts_with(&format!("No se pudo resolver {key}"))
                && again
                    .message
                    .contains("no se puede pasar de scored a scored"),
            "{again:?}"
        );
    }

    #[test]
    fn confirm_marks_submitted_and_approved_and_refuses_a_replay() {
        let (t, key) = tracker_with(&[State::Prepared, State::NeedsGonza]);
        let first = confirm(&t, &key, *NOW).unwrap();
        assert!(first.ok, "{first:?}");
        assert_eq!(first.message, format!("Confirmada como enviada: {key}"));
        let app = t.get_application(&key).unwrap().unwrap();
        assert_eq!(app.state, State::Submitted);
        assert!(app.approved);
        let again = confirm(&t, &key, *NOW).unwrap();
        assert!(!again.ok);
        assert!(
            again
                .message
                .starts_with(&format!("No se pudo confirmar {key}")),
            "{again:?}"
        );
    }

    #[test]
    fn confirming_a_form_the_bot_never_filled_does_not_count_as_a_review() {
        // The review phase exists to look at forms the BOT filled. A form Gonza sent by hand (an
        // Ashby handoff, a listing-only or manual-only posting) says nothing about the filler, so
        // confirming five of them must not end the phase.
        for reason in [
            "handoff: envía tú este formulario. Hoja de respuestas: data/handoff/x.md",
            "listing_only: postular a mano desde el enlace del aviso",
            "manual_only_platform: postular a mano desde el enlace del aviso",
        ] {
            let t = Tracker::in_memory().unwrap();
            let job = make_job();
            t.add_job(&job, 80.0, &BTreeMap::new(), *NOW).unwrap();
            t.create_application(&job.key(), *NOW).unwrap();
            t.transition(&job.key(), State::Prepared, None, *NOW)
                .unwrap();
            t.transition(&job.key(), State::NeedsGonza, Some(reason), *NOW)
                .unwrap();

            let outcome = confirm(&t, &job.key(), *NOW).unwrap();

            assert!(outcome.ok, "{reason}: {outcome:?}");
            let app = t.get_application(&job.key()).unwrap().unwrap();
            assert_eq!(app.state, State::Submitted, "{reason}");
            assert!(!app.approved, "{reason}: sent by hand is not a bot review");
            assert_eq!(t.approved_submitted_total().unwrap(), 0, "{reason}");
        }
    }

    #[test]
    fn confirming_a_bot_click_gonza_checked_still_counts_as_reviewed() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        t.add_job(&job, 80.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        t.transition(&job.key(), State::Prepared, None, *NOW)
            .unwrap();
        t.transition(
            &job.key(),
            State::NeedsGonza,
            Some("unconfirmed_submit: Phone number is invalid."),
            *NOW,
        )
        .unwrap();

        confirm(&t, &job.key(), *NOW).unwrap();

        assert!(t.get_application(&job.key()).unwrap().unwrap().approved);
        assert_eq!(t.approved_submitted_total().unwrap(), 1);
    }

    #[test]
    fn mark_records_a_legal_move_with_a_clean_note() {
        let (t, key) = tracker_with(&[State::Prepared, State::Submitted]);
        let outcome = mark(&t, &key, "interview", Some("  martes 19:00\u{7}  "), *NOW).unwrap();
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(outcome.message, format!("{key} → interview"));
        let app = t.get_application(&key).unwrap().unwrap();
        assert_eq!(app.state, State::Interview);
        assert_eq!(app.note.as_deref(), Some("martes 19:00"));
    }

    #[test]
    fn an_empty_note_is_stored_as_no_note() {
        let (t, key) = tracker_with(&[State::Prepared, State::Submitted]);
        mark(&t, &key, "replied", Some("   "), *NOW).unwrap();
        assert_eq!(t.get_application(&key).unwrap().unwrap().note, None);
    }

    #[test]
    fn mark_explains_an_illegal_move_in_spanish() {
        let (t, key) = tracker_with(&[]);
        let outcome = mark(&t, &key, "interview", None, *NOW).unwrap();
        assert!(!outcome.ok);
        assert!(
            outcome
                .message
                .contains("no se puede pasar de scored a interview"),
            "{outcome:?}"
        );
        assert!(!outcome.message.contains("illegal transition"));
    }

    #[test]
    fn mark_from_needs_gonza_points_to_confirm_and_resolve() {
        let (t, key) = tracker_with(&[State::Prepared, State::NeedsGonza]);
        let outcome = mark(&t, &key, "replied", None, *NOW).unwrap();
        assert!(!outcome.ok);
        assert!(
            outcome
                .message
                .contains("no se puede pasar de needs_gonza a replied")
                && outcome.message.contains("confirm")
                && outcome.message.contains("resolve"),
            "{outcome:?}"
        );
    }

    #[test]
    fn mark_refuses_a_state_the_engine_owns_and_lists_the_valid_ones() {
        let (t, key) = tracker_with(&[]);
        let outcome = mark(&t, &key, "submitted", None, *NOW).unwrap();
        assert!(!outcome.ok);
        assert!(
            outcome.message.starts_with("Estado no válido: submitted.")
                && outcome.message.contains("replied"),
            "{outcome:?}"
        );
    }

    #[test]
    fn mark_of_an_unknown_key_says_so_in_spanish() {
        let t = Tracker::in_memory().unwrap();
        let outcome = mark(&t, "x:1", "replied", None, *NOW).unwrap();
        assert!(!outcome.ok);
        assert!(
            outcome.message.contains("no existe la postulación x:1"),
            "{outcome:?}"
        );
    }

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

    fn record_spec(company: &str, title: &str, url: &str) -> RecordSpec {
        RecordSpec {
            company: company.to_string(),
            title: title.to_string(),
            url: url.to_string(),
            ..RecordSpec::default()
        }
    }

    #[test]
    fn record_refuses_an_address_that_is_not_a_plain_http_or_https_one() {
        let t = Tracker::in_memory().unwrap();
        let too_long = format!("https://example.com/{}", "a".repeat(RECORD_LINK_MAX_CHARS));
        for bad in [
            "javascript:alert(1)",
            "file:///C:/Windows/win.ini",
            "data:text/html,x",
            "https://",
            "jobs/42",
            "https://ejemplo.com/con espacio",
            too_long.as_str(),
        ] {
            let outcome = record(&t, &record_spec("Acme", "Cargo", bad), *NOW).unwrap();
            assert!(!outcome.ok, "{bad:?} was accepted");
            assert!(
                outcome.message.starts_with("El enlace no sirve"),
                "{bad:?}: {}",
                outcome.message
            );
        }
        assert!(
            t.counts().unwrap().is_empty(),
            "a refusal stores nothing, not even the posting"
        );
        // A long tracking query is fine up to the limit.
        let long_but_fine = format!("https://example.com/job?{}", "q=1&".repeat(100));
        assert!(
            record(&t, &record_spec("Acme", "Cargo", &long_but_fine), *NOW)
                .unwrap()
                .ok
        );
    }

    #[test]
    fn record_stores_the_text_it_is_given_as_one_clean_line_and_cut() {
        let t = Tracker::in_memory().unwrap();
        let spec = RecordSpec {
            location: format!("Remoto\n{}", "x".repeat(400)),
            ..record_spec(
                "  Acme\u{7}\n Labs ",
                &format!("Backend\r\nEngineer {}", "y".repeat(400)),
                "https://jobs.example.com/acme/7",
            )
        };

        let outcome = record(&t, &spec, *NOW).unwrap();

        assert!(outcome.ok, "{outcome:?}");
        let key = outcome
            .message
            .rsplit('(')
            .next()
            .unwrap()
            .trim_end_matches(')');
        let job = t.get_job(key).unwrap().unwrap();
        assert_eq!(job.company, "Acme Labs");
        assert!(job.title.starts_with("Backend Engineer y") && job.title.chars().count() <= 301);
        assert!(job.location.starts_with("Remoto x") && job.location.chars().count() <= 201);
        assert!(
            ![&job.company, &job.title, &job.location]
                .iter()
                .any(|text| text.contains(['\n', '\r', '\u{7}']))
        );
    }

    #[test]
    fn record_of_a_new_posting_creates_it_and_stamps_the_given_time() {
        let t = Tracker::in_memory().unwrap();
        let spec = RecordSpec {
            location: "Santiago, Chile".to_string(),
            cv_path: Some(std::path::PathBuf::from("cv.pdf")),
            note: Some("  enviada con Playwright\u{7} ".to_string()),
            salary_min_monthly_usd: Some(5000.0),
            ..record_spec(
                "Globex",
                "Senior Software Engineer",
                "https://jobs.ashbyhq.com/globex/abc/application?utm_source=x",
            )
        };

        let outcome = record(&t, &spec, *NOW).unwrap();

        assert!(outcome.ok, "{outcome:?}");
        let key = "manual:jobs.ashbyhq.com_globex_abc";
        assert_eq!(
            outcome.message,
            format!("Registrada como enviada: Globex — Senior Software Engineer ({key})")
        );
        let app = t.get_application(key).unwrap().unwrap();
        assert_eq!(app.state, State::Submitted);
        assert_eq!(app.submitted_at, Some(*NOW));
        assert!(
            !app.approved,
            "a hand-sent form is not a review of the filler"
        );
        assert_eq!(app.cv_path.as_deref(), Some("cv.pdf"));
        assert_eq!(app.note.as_deref(), Some("enviada con Playwright"));
        let job = t.get_job(key).unwrap().unwrap();
        assert_eq!(job.source, "manual");
        assert_eq!(job.company, "Globex");
        assert_eq!(job.location, "Santiago, Chile");
        assert_eq!(job.salary_min_monthly_usd, Some(5000.0));
        let trail: Vec<State> = t
            .events_for(key)
            .unwrap()
            .iter()
            .map(|e| e.to_state)
            .collect();
        assert_eq!(
            trail,
            vec![State::Scored, State::NeedsGonza, State::Submitted]
        );
        assert_eq!(t.approved_submitted_total().unwrap(), 0);
        assert!(
            t.company_in_cooldown("globex", 90, *NOW).unwrap(),
            "the per-company wait must know about it"
        );
    }

    #[test]
    fn record_reuses_a_stored_posting_found_by_its_link() {
        let (t, key) = tracker_with(&[]);
        let job = t.get_job(&key).unwrap().unwrap();
        let spec = record_spec(
            "Otro nombre",
            "Otro cargo",
            &format!("{}?ref=linkedin#form", job.apply_url),
        );

        let outcome = record(&t, &spec, *NOW).unwrap();

        assert!(outcome.ok, "{outcome:?}");
        assert!(outcome.message.contains(&key), "{outcome:?}");
        assert_eq!(
            t.get_application(&key).unwrap().unwrap().state,
            State::Submitted
        );
        assert_eq!(t.counts().unwrap().get("submitted"), Some(&1));
        assert_eq!(
            t.counts().unwrap().len(),
            1,
            "no second posting was created"
        );
    }

    #[test]
    fn record_reuses_a_stored_posting_with_the_same_company_and_title() {
        let (t, key) = tracker_with(&[]);
        let spec = record_spec(
            "ACME AI, Inc.",
            " senior ai engineer ",
            "https://other.example/x",
        );

        let outcome = record(&t, &spec, *NOW).unwrap();

        assert!(outcome.ok && outcome.message.contains(&key), "{outcome:?}");
        assert_eq!(t.counts().unwrap().len(), 1);
    }

    #[test]
    fn record_with_a_key_uses_that_posting_and_an_unknown_key_is_refused() {
        let (t, key) = tracker_with(&[State::Prepared]);
        let spec = RecordSpec {
            key: Some(key.clone()),
            ..RecordSpec::default()
        };
        let outcome = record(&t, &spec, *NOW).unwrap();
        assert!(outcome.ok, "{outcome:?}");
        let trail: Vec<State> = t
            .events_for(&key)
            .unwrap()
            .iter()
            .map(|e| e.to_state)
            .collect();
        assert_eq!(
            trail,
            vec![State::Scored, State::Prepared, State::Submitted],
            "a prepared form goes straight to submitted"
        );

        let unknown = RecordSpec {
            key: Some("nope:1".to_string()),
            ..RecordSpec::default()
        };
        assert_eq!(
            record(&t, &unknown, *NOW).unwrap(),
            Outcome {
                ok: false,
                message: "No existe: nope:1".to_string()
            }
        );
    }

    #[test]
    fn record_from_needs_gonza_marks_it_submitted_without_approving_it() {
        let (t, key) = tracker_with(&[State::Prepared, State::NeedsGonza]);
        let spec = RecordSpec {
            key: Some(key.clone()),
            ..RecordSpec::default()
        };

        assert!(record(&t, &spec, *NOW).unwrap().ok);

        let app = t.get_application(&key).unwrap().unwrap();
        assert_eq!(app.state, State::Submitted);
        assert!(!app.approved);
        assert_eq!(app.needs_reason, None);
    }

    #[test]
    fn recording_the_same_application_twice_is_refused_and_changes_nothing() {
        let (t, key) = tracker_with(&[]);
        let spec = RecordSpec {
            key: Some(key.clone()),
            ..RecordSpec::default()
        };
        record(&t, &spec, *NOW).unwrap();
        let later = *NOW + chrono::TimeDelta::hours(3);

        let again = record(&t, &spec, later).unwrap();

        assert!(!again.ok);
        assert!(
            again.message.contains("ya está registrada") && again.message.contains("submitted"),
            "{again:?}"
        );
        let app = t.get_application(&key).unwrap().unwrap();
        assert_eq!(app.submitted_at, Some(*NOW), "the first time stands");
        assert_eq!(t.events_for(&key).unwrap().len(), 3);
    }

    #[test]
    fn record_refuses_a_withdrawn_application() {
        let (t, key) = tracker_with(&[State::Withdrawn]);
        let spec = RecordSpec {
            key: Some(key.clone()),
            ..RecordSpec::default()
        };

        let outcome = record(&t, &spec, *NOW).unwrap();

        assert!(!outcome.ok);
        assert!(outcome.message.contains("withdrawn"), "{outcome:?}");
        assert_eq!(
            t.get_application(&key).unwrap().unwrap().state,
            State::Withdrawn
        );
    }

    #[test]
    fn record_without_a_key_needs_company_title_and_link() {
        let t = Tracker::in_memory().unwrap();
        for spec in [
            record_spec("", "Cargo", "https://x.example/1"),
            record_spec("Empresa", "  ", "https://x.example/1"),
            record_spec("Empresa", "Cargo", ""),
        ] {
            let outcome = record(&t, &spec, *NOW).unwrap();
            assert!(!outcome.ok, "{spec:?}");
            assert!(
                outcome.message.contains("--company")
                    && outcome.message.contains("--title")
                    && outcome.message.contains("--url"),
                "{outcome:?}"
            );
        }
        assert!(t.counts().unwrap().is_empty(), "nothing was stored");
    }

    #[test]
    fn grade_card_records_the_review() {
        let (store, questions) = bank();
        let id = questions[0].id.clone();
        let outcome = grade_card(&store, &questions, &id, 4, *NOW).unwrap();
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(outcome.message, format!("{id}: nota 4 registrada."));
        assert_eq!(store.stats(&questions, *NOW).unwrap().seen, 1);
    }

    #[test]
    fn grade_card_refuses_an_unknown_card_and_a_grade_above_five() {
        let (store, questions) = bank();
        let id = questions[0].id.clone();
        assert_eq!(
            grade_card(&store, &questions, "nope", 3, *NOW).unwrap(),
            Outcome {
                ok: false,
                message: "Pregunta desconocida: nope".to_string()
            }
        );
        let high = grade_card(&store, &questions, &id, 6, *NOW).unwrap();
        assert!(!high.ok && high.message.contains("0 a 5"), "{high:?}");
        assert_eq!(
            store.stats(&questions, *NOW).unwrap().seen,
            0,
            "a refused grade changes nothing"
        );
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
    fn snooze_puts_an_application_off_and_unsnooze_brings_it_back() {
        let (t, key) = tracker_with(&[State::Prepared, State::NeedsGonza]);
        let done = snooze(&t, &key, "3d", *NOW).unwrap();
        assert_eq!(
            done,
            Outcome {
                ok: true,
                message: format!("Pospuesta hasta 2026-10-04 09:00: {key}")
            }
        );
        let row = t.get_application(&key).unwrap().unwrap();
        assert_eq!(row.snoozed_until, Some(*NOW + TimeDelta::days(3)));
        assert_eq!(row.state, State::NeedsGonza, "the state does not move");

        let back = unsnooze(&t, &key).unwrap();
        assert!(back.ok && back.message.starts_with("De vuelta"), "{back:?}");
        assert_eq!(
            t.get_application(&key).unwrap().unwrap().snoozed_until,
            None
        );
        let twice = unsnooze(&t, &key).unwrap();
        assert!(!twice.ok && twice.message.starts_with("No estaba pospuesta"));
        assert!(!unsnooze(&t, "nope:1").unwrap().ok);
    }

    #[test]
    fn snooze_refuses_the_past_a_year_away_nonsense_and_an_unknown_key() {
        let (t, key) = tracker_with(&[]);
        let refused = |until: &str| {
            let outcome = snooze(&t, &key, until, *NOW).unwrap();
            assert!(!outcome.ok, "{until:?} was accepted");
            outcome.message
        };
        assert!(refused("2026-10-01T12:00:00Z").starts_with("La fecha ya pasó"));
        assert!(refused("2020-01-01T00:00:00Z").starts_with("La fecha ya pasó"));
        assert!(refused("2027-10-02T12:00:00Z").starts_with("Hasta un año"));
        let nonsense = refused("pasado mañana\u{0}");
        assert!(
            nonsense.starts_with("No entiendo hasta cuándo: pasado mañana."),
            "{nonsense}"
        );
        assert!(
            snooze(&t, "nope:1", "1d", *NOW)
                .unwrap()
                .message
                .starts_with("No existe")
        );
        assert_eq!(
            t.get_application(&key).unwrap().unwrap().snoozed_until,
            None,
            "a refusal changes nothing"
        );
    }

    #[test]
    fn the_auto_approval_threshold_is_kept_within_what_the_queue_itself_accepts() {
        let t = Tracker::in_memory().unwrap();
        assert_eq!(auto_approve_min(&t).unwrap(), None, "off until set");

        // The apply threshold of the profile is 60: nothing below it could even be queued.
        let set = |value, threshold| set_auto_approve_min(&t, value, threshold).unwrap();
        for (value, ok) in [
            (59, false),
            (60, true),
            (85, true),
            (100, true),
            (101, false),
            (0, false),
            (-5, false),
            (i64::MAX, false),
        ] {
            let outcome = set(Some(value), Some(60.0));
            assert_eq!(outcome.ok, ok, "{value}: {}", outcome.message);
            if !ok {
                assert!(
                    outcome.message.contains("60") && outcome.message.contains("100"),
                    "the refusal says the range: {}",
                    outcome.message
                );
            }
        }
        assert_eq!(
            auto_approve_min(&t).unwrap(),
            Some(100),
            "a refusal changes nothing"
        );

        // A threshold that is not a whole number rounds up: 62.5 queues from 63.
        assert!(!set(Some(62), Some(62.5)).ok);
        assert!(set(Some(63), Some(62.5)).ok);
        // Without the profile there is nothing to check a number against: it is not kept.
        let unreadable = set(Some(90), None);
        assert!(!unreadable.ok && unreadable.message.contains("criteria.toml"));
        assert_eq!(auto_approve_min(&t).unwrap(), Some(63));

        // Turning it off needs no profile and is fine twice.
        assert!(set(None, None).ok);
        assert_eq!(auto_approve_min(&t).unwrap(), None);
        assert!(set(None, None).ok);
    }

    #[test]
    fn a_stored_threshold_that_makes_no_sense_reads_as_off() {
        let t = Tracker::in_memory().unwrap();
        for stored in ["", "abc", "90.5", "-3", "101", "9999999999999999999999"] {
            t.set_meta(AUTO_APPROVE_KEY, stored).unwrap();
            assert_eq!(auto_approve_min(&t).unwrap(), None, "{stored:?}");
        }
        t.set_meta(AUTO_APPROVE_KEY, " 88 ").unwrap();
        assert_eq!(auto_approve_min(&t).unwrap(), Some(88));
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
