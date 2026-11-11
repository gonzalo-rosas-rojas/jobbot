//! Gonza's job emails, imported from a JSON file. jobbot never reads Gmail: a separate step (Claude
//! reading the mailbox in a chat) writes the file, and this module only validates it and writes the
//! tracker. Everything in the file comes from other people's emails, so it is untrusted data: each
//! text is stripped of control and invisible characters and cut to a fixed length, and nothing in it
//! is ever interpreted.

use anyhow::{Result, anyhow, bail};
use chrono::{DateTime, TimeDelta, Utc};
use jobbot_domain::outbound::{clean, plain_line};
use jobbot_domain::state::State;
use jobbot_domain::text::normalize;
use serde_json::{Map, Value};

use crate::actions;
use crate::ports::store::{EmailAdded, EmailKind, EmailRow, NewEmail, NewTask, Store};
use tracing::field::Empty;

/// Longest accepted id. An id is never cut (a cut id could collide with another), so a longer one
/// rejects the message.
const ID_MAX: usize = 200;
const FROM_MAX: usize = 200;
const SUBJECT_MAX: usize = 300;
const SUMMARY_MAX: usize = 600;
const COMPANY_MAX: usize = 120;
const JOB_KEY_MAX: usize = 200;
const TASK_TEXT_MAX: usize = 300;
/// The note an email leaves on the application it moves.
const NOTE_MAX: usize = 120;
/// A message "received" more than this many days after the import is a mistake (a wrong year or
/// zone), and it would sit at the top of every recent list.
const FUTURE_SLACK_DAYS: i64 = 1;
/// The most an import file may hold: a bigger one is refused whole, before anything is written.
pub const MAX_MESSAGES: usize = 500;
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// The states in which a company may still answer: the ones a company name can match. Silence
/// (`Ghosted`) and an `Offer` are in: a late reply or a withdrawn offer is real.
const LIVE_STATES: [State; 6] = [
    State::Submitted,
    State::Replied,
    State::TechnicalTest,
    State::Interview,
    State::Offer,
    State::Ghosted,
];

/// What one import did. `lines` has one line per message, in the order they were handled.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub imported: usize,
    pub skipped: usize,
    pub rejected: usize,
    pub state_changes: usize,
    pub tasks_created: usize,
    pub unmatched: usize,
    /// Known messages that had no posting and got one in this import.
    pub linked: usize,
    pub lines: Vec<String>,
}

impl ImportReport {
    /// The one-line Spanish summary printed after the per-message lines.
    pub fn summary(&self) -> String {
        format!(
            "Importados: {} · omitidos (ya conocidos): {} · rechazados: {} · cambios de estado: {} · \
             tareas creadas: {} · sin postulación asociada: {}{}",
            self.imported,
            self.skipped,
            self.rejected,
            self.state_changes,
            self.tasks_created,
            self.unmatched,
            if self.linked > 0 {
                format!(" · vinculados a una postulación: {}", self.linked)
            } else {
                String::new()
            }
        )
    }
}

/// Refuses a file bigger than `MAX_FILE_BYTES`; the command asks before it reads the file.
pub fn check_size(bytes: u64) -> Result<()> {
    if bytes > MAX_FILE_BYTES {
        bail!(
            "el archivo pesa más de {} MB: divídelo en archivos más chicos (no se importó nada)",
            MAX_FILE_BYTES / (1024 * 1024)
        );
    }
    Ok(())
}

/// The messages of an import file: a JSON array of at most `MAX_MESSAGES` elements in at most
/// `MAX_FILE_BYTES` bytes. Each element is validated on its own by `import`.
pub fn parse_items(text: &str) -> Result<Vec<Value>> {
    check_size(text.len() as u64)?;
    // Editors and PowerShell like to put a byte-order mark in front of the file.
    let text = text.trim_start_matches('\u{feff}');
    let value: Value = serde_json::from_str(text).map_err(|e| {
        anyhow!(
            "el archivo no es JSON válido (línea {}, columna {})",
            e.line(),
            e.column()
        )
    })?;
    match value {
        Value::Array(items) if items.len() > MAX_MESSAGES => bail!(
            "el archivo tiene {} mensajes y el máximo es {MAX_MESSAGES} por archivo: divídelo (no se importó nada)",
            items.len()
        ),
        Value::Array(items) => Ok(items),
        _ => bail!("el archivo debe ser una lista JSON de mensajes: [ {{ ... }}, {{ ... }} ]"),
    }
}

/// One message that passed validation, with every text already cleaned and cut.
struct Item {
    message_id: String,
    thread_id: String,
    from: String,
    subject: String,
    received_at: DateTime<Utc>,
    company: Option<String>,
    job_key: Option<String>,
    kind: EmailKind,
    summary: String,
    task: Option<ItemTask>,
}

struct ItemTask {
    text: String,
    due_at: Option<DateTime<Utc>>,
}

/// Text from an email on one line: control and invisible characters gone, whitespace collapsed,
/// cut to `max` characters.
fn line(text: &str, max: usize) -> String {
    clean(&plain_line(text), max)
}

/// A text field that may be missing or `null`.
fn field<'a>(obj: &'a Map<String, Value>, name: &str) -> Result<Option<&'a str>, String> {
    match obj.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(format!("«{name}» debe ser texto")),
    }
}

fn required<'a>(obj: &'a Map<String, Value>, name: &str) -> Result<&'a str, String> {
    field(obj, name)?.ok_or_else(|| format!("falta «{name}»"))
}

fn id_field(obj: &Map<String, Value>, name: &str) -> Result<String, String> {
    let id = plain_line(required(obj, name)?);
    if id.is_empty() {
        return Err(format!("«{name}» está vacío"));
    }
    if id.chars().count() > ID_MAX {
        return Err(format!("«{name}» tiene más de {ID_MAX} caracteres"));
    }
    Ok(id)
}

fn date(text: &str, name: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(text.trim())
        .map(|at| at.with_timezone(&Utc))
        .map_err(|_| {
            format!(
                "«{name}» no es una fecha RFC 3339 válida, por ejemplo 2026-10-05T14:30:00Z (recibí «{}»)",
                line(text, 40)
            )
        })
}

fn validate_task(task: &Map<String, Value>, now: DateTime<Utc>) -> Result<ItemTask, String> {
    let text = line(
        required(task, "text").map_err(|why| format!("{why} en «task»"))?,
        TASK_TEXT_MAX,
    );
    if text.is_empty() {
        return Err("«text» de «task» está vacío".to_string());
    }
    let due_at = match field(task, "due_at")? {
        Some(text) => {
            let due = date(text, "due_at")?;
            if due < actions::earliest_date() || due > actions::latest_due(now) {
                return Err(format!(
                    "«due_at» está fuera de rango: debe ser desde el 2000-01-01 hasta cinco años desde hoy (recibí «{}»)",
                    line(text, 40)
                ));
            }
            Some(due)
        }
        None => None,
    };
    Ok(ItemTask { text, due_at })
}

/// Checks one element of the file. The error is the Spanish reason it is rejected.
fn validate(value: &Value, now: DateTime<Utc>) -> Result<Item, String> {
    let Some(obj) = value.as_object() else {
        return Err("no es un objeto JSON".to_string());
    };
    let message_id = id_field(obj, "message_id")?;
    let thread_id = id_field(obj, "thread_id")?;
    let from = line(required(obj, "from")?, FROM_MAX);
    if from.is_empty() {
        return Err("«from» está vacío".to_string());
    }
    let subject = line(required(obj, "subject")?, SUBJECT_MAX);
    let received_text = required(obj, "received_at")?;
    let received_at = date(received_text, "received_at")?;
    if received_at < actions::earliest_date() {
        return Err(format!(
            "«received_at» es demasiado antigua: debe ser desde el 2000-01-01 (recibí «{}»)",
            line(received_text, 40)
        ));
    }
    if received_at > now + TimeDelta::days(FUTURE_SLACK_DAYS) {
        return Err(format!(
            "«received_at» está en el futuro ({})",
            received_at.to_rfc3339()
        ));
    }
    let kind_text = required(obj, "kind")?;
    let kind = EmailKind::parse(&kind_text.trim().to_ascii_lowercase()).ok_or_else(|| {
        let names: Vec<&str> = EmailKind::ALL.iter().map(EmailKind::as_str).collect();
        format!(
            "tipo desconocido «{}»: usa uno de {}",
            line(kind_text, 40),
            names.join(", ")
        )
    })?;
    let summary = line(required(obj, "summary")?, SUMMARY_MAX);
    let company = field(obj, "company")?
        .map(|text| line(text, COMPANY_MAX))
        .filter(|text| !text.is_empty());
    let job_key = field(obj, "job_key")?
        .map(|text| line(text, JOB_KEY_MAX))
        .filter(|text| !text.is_empty());
    let task = match obj.get("task") {
        None | Some(Value::Null) => None,
        Some(Value::Object(task)) => Some(validate_task(task, now)?),
        Some(_) => {
            return Err("«task» debe ser un objeto con «text» y, si quieres, «due_at»".to_string());
        }
    };
    Ok(Item {
        message_id,
        thread_id,
        from,
        subject,
        received_at,
        company,
        job_key,
        kind,
        summary,
        task,
    })
}

/// Where a message belongs.
enum Link {
    Found(String),
    /// Why no application was chosen.
    Unmatched(String),
}

/// The application a message is about: its `job_key` when the tracker knows it; otherwise the one
/// application in a live state whose company has the same `normalize`d name. Zero or several is
/// never guessed.
fn resolve_job(tracker: &dyn Store, item: &Item) -> Result<Link> {
    let mut why = None;
    if let Some(key) = &item.job_key {
        if tracker.get_job(key)?.is_some() {
            return Ok(Link::Found(key.clone()));
        }
        why = Some(format!("job_key «{key}» no existe en la base"));
    }
    let Some(company) = &item.company else {
        return Ok(Link::Unmatched(
            why.unwrap_or_else(|| "sin job_key ni empresa".to_string()),
        ));
    };
    let keys = tracker.job_keys_of_company(&normalize(company), &LIVE_STATES)?;
    let by_company = match keys.as_slice() {
        [only] => return Ok(Link::Found(only.clone())),
        [] => format!("ninguna postulación enviada de «{company}»"),
        several => format!(
            "varias postulaciones de «{company}»: {}; indica el job_key",
            several.join(", ")
        ),
    };
    Ok(Link::Unmatched(match why {
        Some(why) => format!("{why}; {by_company}"),
        None => by_company,
    }))
}

/// The state an email of this kind moves its application to, when it moves it at all.
fn target_state(kind: EmailKind) -> Option<State> {
    match kind {
        EmailKind::Rejection => Some(State::Rejected),
        EmailKind::Recruiter => Some(State::Replied),
        EmailKind::InterviewInvite => Some(State::Interview),
        EmailKind::TechnicalTest => Some(State::TechnicalTest),
        EmailKind::Ack | EmailKind::ActionRequired | EmailKind::Other => None,
    }
}

/// The task an invitation or a test needs when the message brought none. The importer does not
/// invent a deadline.
fn automatic_task(
    tracker: &dyn Store,
    item: &Item,
    job_key: Option<&str>,
) -> Result<Option<String>> {
    if !matches!(
        item.kind,
        EmailKind::InterviewInvite | EmailKind::TechnicalTest
    ) {
        return Ok(None);
    }
    let linked = match job_key {
        Some(key) => tracker.get_job(key)?.map(|job| plain_line(&job.company)),
        None => None,
    };
    let name = item
        .company
        .clone()
        .or(linked.filter(|name| !name.is_empty()))
        .unwrap_or_else(|| item.from.clone());
    let text = if item.kind == EmailKind::InterviewInvite {
        format!("Responder a {name}: entrevista")
    } else {
        format!("Entregar prueba técnica de {name}")
    };
    Ok(Some(clean(&text, TASK_TEXT_MAX)))
}

/// Imports every valid message of `items` and reports the rest. The valid ones are handled
/// oldest first whatever the order of the file (Gmail lists the newest first, and an invitation
/// cannot come before the reply it answers), so the state machine sees them in the order they
/// happened. A message whose id is already stored is skipped without touching anything, unless it
/// was stored without a posting and now has one (`revisit_known`).
#[tracing::instrument(
    name = "inbox_import",
    skip_all,
    fields(
        items = items.len(),
        imported = Empty,
        rejected = Empty,
        state_changes = Empty,
        tasks_created = Empty,
    )
)]
pub fn import(tracker: &dyn Store, items: &[Value], now: DateTime<Utc>) -> Result<ImportReport> {
    let mut report = ImportReport::default();
    let mut valid = Vec::new();
    for (index, value) in items.iter().enumerate() {
        let position = index + 1;
        match validate(value, now) {
            Ok(item) => valid.push((position, item)),
            Err(why) => {
                report.rejected += 1;
                report
                    .lines
                    .push(plain_line(&format!("elemento {position} rechazado: {why}")));
            }
        }
    }
    valid.sort_by_key(|(position, item)| (item.received_at, *position));
    for (position, item) in valid {
        import_one(tracker, position, &item, now, &mut report)?;
    }
    crate::record_counts(&[
        ("imported", report.imported),
        ("rejected", report.rejected),
        ("state_changes", report.state_changes),
        ("tasks_created", report.tasks_created),
    ]);
    Ok(report)
}

/// Moves the application a message belongs to, through `actions::mark_from_email` only, and says what
/// happened in `notes`. Nothing moves for an email from before the application was sent (it
/// answers an earlier process, and its event would come before the application's own
/// `submitted`), or when the move is not legal from the current state: both are reported, never
/// an error.
fn move_state(
    tracker: &dyn Store,
    key: &str,
    mail: &Mail<'_>,
    now: DateTime<Utc>,
    notes: &mut Vec<String>,
    report: &mut ImportReport,
) -> Result<()> {
    let Some(target) = target_state(mail.kind) else {
        return Ok(());
    };
    if let Some(app) = tracker.get_application(key)?
        && let Some(sent_at) = app.submitted_at
        && mail.received_at < sent_at
    {
        notes.push("anterior a la postulación, sin cambio de estado".to_string());
        return Ok(());
    }
    let subject = if mail.subject.is_empty() {
        "correo recibido"
    } else {
        mail.subject
    };
    let note = clean(&format!("correo: {subject}"), NOTE_MAX);
    // The history records when the email arrived, not when it was imported.
    let at = mail.received_at.min(now);
    let outcome = actions::mark_from_email(tracker, key, target.as_str(), Some(&note), at)?;
    if outcome.ok {
        report.state_changes += 1;
        notes.push(format!("estado → {target}"));
    } else {
        let why = outcome
            .message
            .strip_prefix(&format!("No se pudo marcar {key}: "))
            .unwrap_or(&outcome.message);
        notes.push(format!("sin cambio de estado: {why}"));
    }
    Ok(())
}

/// What `move_state` needs to know about a message.
struct Mail<'a> {
    kind: EmailKind,
    subject: &'a str,
    received_at: DateTime<Utc>,
}

fn skipped_line(position: usize, message_id: &str) -> String {
    plain_line(&format!(
        "elemento {position} omitido: ya estaba importado ({message_id})"
    ))
}

/// A message whose id is already stored. It is skipped, with one exception: an email that was
/// stored without a posting (the company matched several, or the application was recorded later)
/// and now resolves to one is linked to it, once, and its state move is made then. The task it
/// made is not created again; it only takes the posting.
fn revisit_known(
    tracker: &dyn Store,
    position: usize,
    item: &Item,
    known: &EmailRow,
    now: DateTime<Utc>,
    report: &mut ImportReport,
) -> Result<()> {
    let key = match (&known.job_key, resolve_job(tracker, item)?) {
        (None, Link::Found(key)) => key,
        _ => {
            report.skipped += 1;
            report.lines.push(skipped_line(position, &item.message_id));
            return Ok(());
        }
    };
    if !tracker.link_email(&known.message_id, &key)? {
        // Another jobbot linked it between the read and the write.
        report.skipped += 1;
        report.lines.push(skipped_line(position, &item.message_id));
        return Ok(());
    }
    report.linked += 1;
    let mut notes = vec![format!("postulación {key}")];
    let mail = Mail {
        kind: known.kind,
        subject: &known.subject,
        received_at: known.received_at,
    };
    move_state(tracker, &key, &mail, now, &mut notes, report)?;
    report.lines.push(plain_line(&format!(
        "elemento {position} vinculado: ya estaba importado ({}) · {}",
        item.message_id,
        notes.join(" · ")
    )));
    Ok(())
}

fn import_one(
    tracker: &dyn Store,
    position: usize,
    item: &Item,
    now: DateTime<Utc>,
    report: &mut ImportReport,
) -> Result<()> {
    if let Some(known) = tracker.get_email(&item.message_id)? {
        return revisit_known(tracker, position, item, &known, now, report);
    }
    let mut notes = Vec::new();
    let job_key = match resolve_job(tracker, item)? {
        Link::Found(key) => {
            notes.push(format!("postulación {key}"));
            Some(key)
        }
        Link::Unmatched(why) => {
            report.unmatched += 1;
            notes.push(format!("sin postulación asociada ({why})"));
            None
        }
    };
    if let Some(key) = &job_key {
        let mail = Mail {
            kind: item.kind,
            subject: &item.subject,
            received_at: item.received_at,
        };
        move_state(tracker, key, &mail, now, &mut notes, report)?;
    }
    let task_text = match &item.task {
        Some(task) => Some((task.text.clone(), task.due_at)),
        None => automatic_task(tracker, item, job_key.as_deref())?.map(|text| (text, None)),
    };
    let task = task_text.map(|(text, due_at)| NewTask {
        job_key: job_key.clone(),
        kind: item.kind.as_str().to_string(),
        text,
        due_at,
        source_message_id: Some(item.message_id.clone()),
        link: None,
    });
    let email = NewEmail {
        message_id: item.message_id.clone(),
        thread_id: item.thread_id.clone(),
        from_addr: item.from.clone().into(),
        subject: item.subject.clone(),
        received_at: item.received_at,
        job_key,
        kind: item.kind,
        summary: item.summary.clone(),
    };
    match tracker.add_email(&email, task.as_ref(), now)? {
        EmailAdded::Stored { task_created } => {
            report.imported += 1;
            if task_created && let Some(task) = &task {
                report.tasks_created += 1;
                notes.push(format!("tarea creada: {}", task.text));
            }
        }
        // Another jobbot stored the same message between the check and the write.
        EmailAdded::AlreadyKnown => {
            report.skipped += 1;
            report.lines.push(skipped_line(position, &item.message_id));
            return Ok(());
        }
    }
    let subject = if item.subject.is_empty() {
        "(sin asunto)"
    } else {
        &item.subject
    };
    report.lines.push(plain_line(&format!(
        "elemento {position} importado: {} «{subject}» · {}",
        item.kind.label(),
        notes.join(" · ")
    )));
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_file_must_be_a_json_array_and_a_byte_order_mark_is_tolerated() {
        assert!(parse_items("esto no es json").is_err());
        let err = parse_items("{\"message_id\": \"m1\"}")
            .unwrap_err()
            .to_string();
        assert!(err.contains("lista"), "{err}");
        let items = parse_items("\u{feff}[{\"a\": 1}, 2]").unwrap();
        assert_eq!(items.len(), 2);
        assert!(parse_items("[]").unwrap().is_empty());
    }

    #[test]
    fn the_summary_counts_everything_in_spanish() {
        let report = ImportReport {
            imported: 4,
            skipped: 1,
            rejected: 2,
            state_changes: 3,
            tasks_created: 2,
            unmatched: 1,
            linked: 0,
            lines: vec![],
        };
        assert_eq!(
            report.summary(),
            "Importados: 4 · omitidos (ya conocidos): 1 · rechazados: 2 · cambios de estado: 3 · \
             tareas creadas: 2 · sin postulación asociada: 1"
        );
        // Links made to messages that were already known are only counted when there are some.
        let linked = ImportReport {
            linked: 2,
            ..report
        };
        assert!(
            linked
                .summary()
                .ends_with("sin postulación asociada: 1 · vinculados a una postulación: 2"),
            "{}",
            linked.summary()
        );
    }

    #[test]
    fn a_file_with_too_many_messages_or_too_many_bytes_is_refused() {
        let many = |n: usize| serde_json::to_string(&vec![json!({}); n]).unwrap();
        assert_eq!(
            parse_items(&many(MAX_MESSAGES)).unwrap().len(),
            MAX_MESSAGES
        );
        let err = parse_items(&many(MAX_MESSAGES + 1))
            .unwrap_err()
            .to_string();
        assert!(err.contains("500") && err.contains("mensajes"), "{err}");

        let big = format!("[\"{}\"]", "x".repeat(MAX_FILE_BYTES as usize));
        let err = parse_items(&big).unwrap_err().to_string();
        assert!(err.contains("2 MB"), "{err}");
    }
}
