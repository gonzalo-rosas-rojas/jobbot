//! Gonza's job emails, imported from a JSON file. jobbot never reads Gmail: a separate step (Claude
//! reading the mailbox in a chat) writes the file, and this module only validates it and writes the
//! tracker. Everything in the file comes from other people's emails, so it is untrusted data: each
//! text is stripped of control and invisible characters and cut to a fixed length, and nothing in it
//! is ever interpreted.

use anyhow::{Result, anyhow, bail};
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::{Map, Value};

use crate::actions;
use crate::outbound::{clean, plain_line};
use crate::text::normalize;
use crate::tracker::{EmailAdded, EmailKind, EmailRow, NewEmail, NewTask, State, Tracker};

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
fn resolve_job(tracker: &Tracker, item: &Item) -> Result<Link> {
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
fn automatic_task(tracker: &Tracker, item: &Item, job_key: Option<&str>) -> Result<Option<String>> {
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
pub fn import(tracker: &Tracker, items: &[Value], now: DateTime<Utc>) -> Result<ImportReport> {
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
    Ok(report)
}

/// Moves the application a message belongs to, through `actions::mark` only, and says what
/// happened in `notes`. Nothing moves for an email from before the application was sent (it
/// answers an earlier process, and its event would come before the application's own
/// `submitted`), or when the move is not legal from the current state: both are reported, never
/// an error.
fn move_state(
    tracker: &Tracker,
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
    let outcome = actions::mark(tracker, key, target.as_str(), Some(&note), at)?;
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
    tracker: &Tracker,
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
    tracker: &Tracker,
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
        from_addr: item.from.clone(),
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
    use std::collections::BTreeMap;

    use chrono::{TimeDelta, TimeZone};
    use serde_json::json;

    use super::*;
    use crate::models::Job;
    use crate::testing::{NOW, make_job_with};
    use crate::tracker::{EmailKind, State};

    /// When the applications of these tests were sent: before every email.
    fn sent_at() -> DateTime<Utc> {
        *NOW - TimeDelta::days(3)
    }

    /// Adds an application of `company` (job key `test:<id>`) walked through `states`.
    fn app(t: &Tracker, id: &str, company: &str, states: &[State]) -> String {
        let job: Job = make_job_with(|j| {
            j.external_id = id.into();
            j.company = company.into();
        });
        t.add_job(&job, 80.0, &BTreeMap::new(), sent_at()).unwrap();
        t.create_application(&job.key(), sent_at()).unwrap();
        for state in states {
            t.transition(&job.key(), *state, None, sent_at()).unwrap();
        }
        job.key()
    }

    fn sent(t: &Tracker, id: &str, company: &str) -> String {
        app(t, id, company, &[State::Prepared, State::Submitted])
    }

    /// A valid message of `kind` received an hour before `NOW`.
    fn msg(id: &str, kind: &str) -> Value {
        json!({
            "message_id": id,
            "thread_id": format!("th-{id}"),
            "from": "Ana <ana@acme.example>",
            "subject": format!("Asunto {id}"),
            "received_at": "2026-10-01T11:00:00Z",
            "kind": kind,
            "summary": format!("Resumen {id}"),
        })
    }

    /// `base` with the keys of `extra` added or replaced.
    fn with(mut base: Value, extra: Value) -> Value {
        let target = base.as_object_mut().unwrap();
        for (key, value) in extra.as_object().unwrap() {
            target.insert(key.clone(), value.clone());
        }
        base
    }

    fn state_of(t: &Tracker, key: &str) -> State {
        t.get_application(key).unwrap().unwrap().state
    }

    fn run(t: &Tracker, items: &[Value]) -> ImportReport {
        import(t, items, *NOW).unwrap()
    }

    fn emails(t: &Tracker) -> Vec<crate::tracker::EmailRow> {
        t.recent_emails(*NOW - TimeDelta::days(365)).unwrap()
    }

    #[test]
    fn a_message_is_stored_linked_by_its_job_key_and_moves_the_application() {
        let t = Tracker::in_memory().unwrap();
        let key = sent(&t, "1", "Acme AI");
        let item = with(msg("m1", "rejection"), json!({ "job_key": key }));

        let report = run(&t, &[item]);

        assert_eq!(
            (report.imported, report.skipped, report.rejected),
            (1, 0, 0)
        );
        assert_eq!((report.state_changes, report.unmatched), (1, 0));
        let stored = &emails(&t)[0];
        assert_eq!(stored.message_id, "m1");
        assert_eq!(stored.job_key.as_deref(), Some(key.as_str()));
        assert_eq!(stored.kind, EmailKind::Rejection);
        assert_eq!(stored.company.as_deref(), Some("Acme AI"));
        assert_eq!(stored.imported_at, *NOW);
        assert_eq!(state_of(&t, &key), State::Rejected);
        // The history carries the real time of the email, not the time of the import.
        let last = t.events_for(&key).unwrap().pop().unwrap();
        assert_eq!(
            last.at,
            Utc.with_ymd_and_hms(2026, 10, 1, 11, 0, 0).unwrap()
        );
        assert_eq!(last.note.as_deref(), Some("correo: Asunto m1"));
    }

    #[test]
    fn received_at_is_stored_in_utc_whatever_offset_it_came_with() {
        let t = Tracker::in_memory().unwrap();
        let item = with(
            msg("m1", "other"),
            json!({ "received_at": "2026-10-01T08:30:00-03:00" }),
        );

        run(&t, &[item]);

        assert_eq!(
            emails(&t)[0].received_at,
            Utc.with_ymd_and_hms(2026, 10, 1, 11, 30, 0).unwrap()
        );
    }

    #[test]
    fn each_kind_moves_the_application_only_to_its_own_state() {
        for (kind, expected) in [
            ("rejection", State::Rejected),
            ("recruiter", State::Replied),
            ("interview_invite", State::Interview),
            ("technical_test", State::TechnicalTest),
            ("ack", State::Submitted),
            ("other", State::Submitted),
            ("action_required", State::Submitted),
        ] {
            let t = Tracker::in_memory().unwrap();
            let key = sent(&t, "1", "Acme AI");
            let report = run(&t, &[with(msg("m1", kind), json!({ "job_key": key }))]);
            assert_eq!(state_of(&t, &key), expected, "{kind}");
            assert_eq!(
                report.state_changes,
                usize::from(expected != State::Submitted),
                "{kind}"
            );
        }
    }

    #[test]
    fn an_illegal_move_is_reported_and_the_message_is_still_stored() {
        let t = Tracker::in_memory().unwrap();
        let key = app(
            &t,
            "1",
            "Acme AI",
            &[State::Prepared, State::Submitted, State::Interview],
        );
        let item = with(msg("m1", "recruiter"), json!({ "job_key": key }));

        let report = run(&t, &[item]);

        assert_eq!((report.imported, report.state_changes), (1, 0));
        assert_eq!(report.rejected, 0, "an illegal move is not an error");
        assert_eq!(state_of(&t, &key), State::Interview);
        let line = report.lines.join("\n");
        assert!(
            line.contains("sin cambio de estado")
                && line.contains("no se puede pasar de interview a replied"),
            "{line}"
        );
        assert_eq!(emails(&t).len(), 1);
    }

    #[test]
    fn a_message_for_an_application_that_does_not_exist_changes_nothing_but_is_kept() {
        let t = Tracker::in_memory().unwrap();
        // A discovered posting that was never applied to.
        let job = make_job_with(|j| j.external_id = "9".into());
        t.add_job(&job, 80.0, &BTreeMap::new(), sent_at()).unwrap();
        let item = with(msg("m1", "rejection"), json!({ "job_key": job.key() }));

        let report = run(&t, &[item]);

        assert_eq!((report.imported, report.state_changes), (1, 0));
        assert!(report.lines.join("\n").contains("sin cambio de estado"));
    }

    #[test]
    fn importing_the_same_file_twice_changes_nothing() {
        let t = Tracker::in_memory().unwrap();
        let key = sent(&t, "1", "Acme AI");
        let items = [
            with(msg("m1", "interview_invite"), json!({ "job_key": key })),
            with(
                msg("m2", "action_required"),
                json!({ "task": { "text": "Enviar el video", "due_at": "2026-10-03T15:00:00Z" } }),
            ),
        ];
        let first = run(&t, &items);
        assert_eq!((first.imported, first.tasks_created), (2, 2));
        let events = t.events_for(&key).unwrap().len();

        let second = run(&t, &items);

        assert_eq!(
            (second.imported, second.skipped, second.state_changes),
            (0, 2, 0)
        );
        assert_eq!(second.tasks_created, 0);
        assert_eq!(emails(&t).len(), 2);
        assert_eq!(t.open_tasks().unwrap().len(), 2);
        assert_eq!(t.events_for(&key).unwrap().len(), events, "no second move");
    }

    #[test]
    fn a_message_id_repeated_inside_one_file_is_skipped() {
        let t = Tracker::in_memory().unwrap();
        let report = run(&t, &[msg("m1", "other"), msg("m1", "other")]);
        assert_eq!((report.imported, report.skipped), (1, 1));
        assert_eq!(emails(&t).len(), 1);
    }

    #[test]
    fn the_company_picks_the_one_application_in_a_live_state() {
        let t = Tracker::in_memory().unwrap();
        let key = sent(&t, "1", "Acme AI");
        // Same name, different case and suffix: it is the same company.
        let item = with(
            msg("m1", "recruiter"),
            json!({ "company": "ACME AI, Inc." }),
        );

        let report = run(&t, &[item]);

        assert_eq!(report.unmatched, 0);
        assert_eq!(emails(&t)[0].job_key.as_deref(), Some(key.as_str()));
        assert_eq!(state_of(&t, &key), State::Replied);
    }

    #[test]
    fn a_company_with_no_live_application_is_reported_and_left_unlinked() {
        let t = Tracker::in_memory().unwrap();
        // Applications that are not waiting for an answer do not count.
        app(&t, "1", "Acme AI", &[State::Prepared]);
        app(
            &t,
            "2",
            "Acme AI",
            &[State::Prepared, State::Submitted, State::Rejected],
        );
        let item = with(msg("m1", "recruiter"), json!({ "company": "Acme AI" }));

        let report = run(&t, &[item]);

        assert_eq!((report.imported, report.unmatched), (1, 1));
        assert_eq!(emails(&t)[0].job_key, None);
        assert!(
            report.lines.join("\n").contains("ninguna postulación"),
            "{:?}",
            report.lines
        );
    }

    #[test]
    fn a_company_with_several_live_applications_is_not_guessed() {
        let t = Tracker::in_memory().unwrap();
        let a = sent(&t, "1", "Acme AI");
        let b = app(
            &t,
            "2",
            "Acme AI",
            &[State::Prepared, State::Submitted, State::Replied],
        );
        let item = with(msg("m1", "rejection"), json!({ "company": "Acme AI" }));

        let report = run(&t, &[item]);

        assert_eq!(
            (report.imported, report.unmatched, report.state_changes),
            (1, 1, 0)
        );
        assert_eq!(emails(&t)[0].job_key, None);
        let text = report.lines.join("\n");
        assert!(text.contains("varias postulaciones") && text.contains(&a) && text.contains(&b));
        assert_eq!(state_of(&t, &a), State::Submitted);
        assert_eq!(state_of(&t, &b), State::Replied);
    }

    #[test]
    fn a_job_key_that_does_not_exist_falls_back_to_the_company() {
        let t = Tracker::in_memory().unwrap();
        let key = sent(&t, "1", "Acme AI");
        let item = with(
            msg("m1", "recruiter"),
            json!({ "job_key": "nope:404", "company": "Acme AI" }),
        );

        let report = run(&t, &[item]);

        assert_eq!(report.unmatched, 0);
        assert_eq!(emails(&t)[0].job_key.as_deref(), Some(key.as_str()));
    }

    #[test]
    fn a_message_with_neither_key_nor_company_is_kept_and_counted_as_unmatched() {
        let t = Tracker::in_memory().unwrap();
        let report = run(&t, &[msg("m1", "other")]);
        assert_eq!((report.imported, report.unmatched), (1, 1));
        assert_eq!(emails(&t)[0].job_key, None);
    }

    #[test]
    fn a_given_task_is_created_with_its_due_date_and_the_message_it_came_from() {
        let t = Tracker::in_memory().unwrap();
        let key = sent(&t, "1", "Acme AI");
        let item = with(
            msg("m1", "action_required"),
            json!({
                "job_key": key,
                "task": { "text": "Grabar el video de Acme AI", "due_at": "2026-10-03T15:00:00Z" }
            }),
        );

        let report = run(&t, &[item]);

        assert_eq!(report.tasks_created, 1);
        let task = &t.open_tasks().unwrap()[0];
        assert_eq!(task.text, "Grabar el video de Acme AI");
        assert_eq!(task.kind, "action_required");
        assert_eq!(task.job_key.as_deref(), Some(key.as_str()));
        assert_eq!(task.source_message_id.as_deref(), Some("m1"));
        assert_eq!(
            task.due_at,
            Some(Utc.with_ymd_and_hms(2026, 10, 3, 15, 0, 0).unwrap())
        );
        assert_eq!(task.created_at, *NOW);
    }

    #[test]
    fn an_interview_or_a_test_without_a_task_gets_one_automatically() {
        let t = Tracker::in_memory().unwrap();
        sent(&t, "1", "Acme AI");
        sent(&t, "2", "Globex");

        let report = run(
            &t,
            &[
                with(
                    msg("m1", "interview_invite"),
                    json!({ "company": "Acme AI" }),
                ),
                with(msg("m2", "technical_test"), json!({ "company": "Globex" })),
                msg("m3", "ack"),
                msg("m4", "recruiter"),
            ],
        );

        assert_eq!(report.tasks_created, 2);
        let mut texts: Vec<String> = t
            .open_tasks()
            .unwrap()
            .into_iter()
            .map(|r| r.text)
            .collect();
        texts.sort();
        assert_eq!(
            texts,
            [
                "Entregar prueba técnica de Globex",
                "Responder a Acme AI: entrevista"
            ]
        );
        assert!(
            t.open_tasks()
                .unwrap()
                .iter()
                .all(|task| task.due_at.is_none()),
            "the importer does not invent a deadline"
        );
    }

    #[test]
    fn the_automatic_task_names_the_company_of_the_linked_application_when_none_is_given() {
        let t = Tracker::in_memory().unwrap();
        let key = sent(&t, "1", "Acme AI");

        run(
            &t,
            &[with(
                msg("m1", "interview_invite"),
                json!({ "job_key": key }),
            )],
        );

        assert_eq!(
            t.open_tasks().unwrap()[0].text,
            "Responder a Acme AI: entrevista"
        );
        // And with nothing to name it, the sender is the best there is.
        run(&t, &[msg("m2", "technical_test")]);
        let texts: Vec<String> = t
            .open_tasks()
            .unwrap()
            .into_iter()
            .map(|r| r.text)
            .collect();
        assert!(
            texts.contains(&"Entregar prueba técnica de Ana <ana@acme.example>".to_string()),
            "{texts:?}"
        );
    }

    #[test]
    fn a_task_the_message_brings_replaces_the_automatic_one() {
        let t = Tracker::in_memory().unwrap();
        let item = with(
            msg("m1", "interview_invite"),
            json!({ "company": "Acme AI", "task": { "text": "Elegir hora el martes" } }),
        );

        let report = run(&t, &[item]);

        assert_eq!(report.tasks_created, 1);
        let tasks = t.open_tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].text, "Elegir hora el martes");
        assert_eq!(tasks[0].due_at, None);
    }

    #[test]
    fn a_task_of_null_is_no_task() {
        let t = Tracker::in_memory().unwrap();
        let report = run(&t, &[with(msg("m1", "other"), json!({ "task": null }))]);
        assert_eq!((report.imported, report.tasks_created), (1, 0));
    }

    #[test]
    fn a_bad_message_is_rejected_by_position_and_the_others_still_import() {
        let t = Tracker::in_memory().unwrap();
        let items = [
            msg("ok-1", "other"),
            with(msg("x", "spam"), json!({})),
            with(
                msg("y", "other"),
                json!({ "received_at": "ayer por la tarde" }),
            ),
            with(msg("", "other"), json!({})),
            with(msg("   ", "other"), json!({})),
            json!("no soy un objeto"),
            with(msg("z", "other"), json!({ "from": 42 })),
            with(
                msg("w", "other"),
                json!({ "task": { "text": "x", "due_at": "mañana" } }),
            ),
            with(
                msg("v", "other"),
                json!({ "task": { "due_at": "2026-10-03T15:00:00Z" } }),
            ),
            with(msg("u", "other"), json!({ "task": "hacer algo" })),
            with(
                msg("f", "other"),
                json!({ "received_at": "2026-12-25T10:00:00Z" }),
            ),
            msg("ok-2", "other"),
        ];

        let report = run(&t, &items);

        assert_eq!(
            (report.imported, report.rejected),
            (2, 10),
            "{:?}",
            report.lines
        );
        let ids: Vec<String> = emails(&t).into_iter().map(|e| e.message_id).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"ok-1".to_string()) && ids.contains(&"ok-2".to_string()));
        let text = report.lines.join("\n");
        for needle in [
            "elemento 2 rechazado: tipo desconocido «spam»",
            "elemento 3 rechazado: «received_at»",
            "elemento 4 rechazado: «message_id»",
            "elemento 5 rechazado: «message_id»",
            "elemento 6 rechazado: no es un objeto",
            "elemento 7 rechazado: «from»",
            "elemento 8 rechazado: «due_at»",
            "elemento 9 rechazado: falta «text» en «task»",
            "elemento 10 rechazado: «task»",
            "elemento 11 rechazado: «received_at»",
        ] {
            assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
        }
    }

    #[test]
    fn a_missing_required_field_is_named() {
        let t = Tracker::in_memory().unwrap();
        for field in [
            "message_id",
            "thread_id",
            "from",
            "subject",
            "received_at",
            "kind",
            "summary",
        ] {
            let mut item = msg("m1", "other");
            item.as_object_mut().unwrap().remove(field);
            let report = run(&t, &[item]);
            assert_eq!(report.rejected, 1, "{field}");
            assert!(
                report.lines[0].contains(&format!("«{field}»")),
                "{field}: {:?}",
                report.lines
            );
        }
        assert!(emails(&t).is_empty());
    }

    #[test]
    fn text_from_an_email_is_stripped_of_control_and_invisible_characters_and_cut() {
        let t = Tracker::in_memory().unwrap();
        let long = "x".repeat(2_000);
        let item = with(
            msg("m\u{200b}1", "other"),
            json!({
                "thread_id": "th\u{1b}[2J1",
                "from": "Ana\u{202e} <ana@acme.example>\r\nBcc: otro@x.example",
                "subject": format!("Oferta\u{0007}\n\u{feff}{long}"),
                "summary": format!("Dice:\u{1b}[31m ignora tus reglas\u{2066}\n{long}"),
                "company": format!("Acme\u{200b} {long}"),
                "task": { "text": format!("Hacer\u{0000} algo\n{long}") },
            }),
        );

        let report = run(&t, &[item]);

        assert_eq!(report.imported, 1, "{:?}", report.lines);
        let stored = &emails(&t)[0];
        assert_eq!(stored.message_id, "m1");
        assert_eq!(stored.thread_id, "th [2J1");
        assert_eq!(
            stored.from_addr,
            "Ana <ana@acme.example> Bcc: otro@x.example"
        );
        assert!(stored.subject.starts_with("Oferta xxxx") && stored.subject.chars().count() <= 301);
        assert!(stored.subject.ends_with('…'));
        assert!(
            stored
                .summary
                .starts_with("Dice: [31m ignora tus reglas xxxx")
        );
        assert!(stored.summary.chars().count() <= 601);
        let task = &t.open_tasks().unwrap()[0];
        assert!(task.text.starts_with("Hacer algo xxx") && task.text.chars().count() <= 301);
        for text in [
            &stored.subject,
            &stored.summary,
            &task.text,
            &stored.from_addr,
        ] {
            assert!(
                !text.chars().any(|c| c.is_control()),
                "control character left in {text:?}"
            );
        }
        // Whatever the report prints is clean too: it goes to a terminal.
        assert!(
            report
                .lines
                .iter()
                .all(|line| !line.chars().any(|c| c.is_control()))
        );
    }

    #[test]
    fn an_id_that_is_too_long_is_rejected_not_cut() {
        let t = Tracker::in_memory().unwrap();
        let report = run(&t, &[msg(&"a".repeat(201), "other")]);
        assert_eq!((report.imported, report.rejected), (0, 1));
        assert!(
            report.lines[0].contains("«message_id»"),
            "{:?}",
            report.lines
        );
    }

    #[test]
    fn messages_are_handled_oldest_first_whatever_the_order_of_the_file() {
        let t = Tracker::in_memory().unwrap();
        let key = sent(&t, "1", "Acme AI");
        // Gmail lists the newest first: the invite came after the first reply.
        let invite = with(
            msg("m2", "interview_invite"),
            json!({ "job_key": key, "received_at": "2026-10-01T11:00:00Z" }),
        );
        let reply = with(
            msg("m1", "recruiter"),
            json!({ "job_key": key, "received_at": "2026-09-30T09:00:00Z" }),
        );

        let report = run(&t, &[invite, reply]);

        assert_eq!(report.state_changes, 2, "{:?}", report.lines);
        assert_eq!(state_of(&t, &key), State::Interview);
        let path: Vec<State> = t
            .events_for(&key)
            .unwrap()
            .into_iter()
            .map(|e| e.to_state)
            .collect();
        assert_eq!(
            path,
            [
                State::Scored,
                State::Prepared,
                State::Submitted,
                State::Replied,
                State::Interview
            ]
        );
    }

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
    fn dates_out_of_range_reject_the_message_and_leave_the_database_readable() {
        let t = Tracker::in_memory().unwrap();
        let key = sent(&t, "1", "Acme AI");
        let task = |due: &str| json!({ "task": { "text": "x", "due_at": due } });
        let items = [
            with(
                msg("far-past", "rejection"),
                json!({ "job_key": key, "received_at": "0000-01-01T00:00:00+05:00" }),
            ),
            with(
                msg("pre-2000", "other"),
                json!({ "received_at": "1999-12-31T23:59:59Z" }),
            ),
            with(
                msg("due-far", "action_required"),
                task("9999-12-31T23:59:59Z"),
            ),
            with(
                msg("due-old", "action_required"),
                task("1999-12-31T23:59:59Z"),
            ),
            with(
                msg("due-6y", "action_required"),
                task("2032-10-02T12:00:00Z"),
            ),
        ];

        let report = run(&t, &items);

        assert_eq!(
            (report.imported, report.rejected),
            (0, 5),
            "{:?}",
            report.lines
        );
        let text = report.lines.join("\n");
        for needle in [
            "elemento 1 rechazado: «received_at»",
            "elemento 2 rechazado: «received_at»",
            "elemento 3 rechazado: «due_at»",
            "elemento 4 rechazado: «due_at»",
            "elemento 5 rechazado: «due_at»",
        ] {
            assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
        }
        // Nothing was written, and everything that reads the tracker still works.
        assert_eq!(state_of(&t, &key), State::Submitted);
        assert_eq!(t.events_for(&key).unwrap().len(), 3);
        assert!(t.open_tasks().unwrap().is_empty());
        assert!(emails(&t).is_empty());
        assert!(crate::today::build_today(&t, *NOW).is_ok());
        assert_eq!(t.list_by_state(State::Submitted).unwrap().len(), 1);

        // The limits themselves are inside: 2000-01-01 and exactly five years from now.
        let edge = run(
            &t,
            &[
                with(
                    msg("edge-old", "other"),
                    json!({ "received_at": "2000-01-01T00:00:00Z" }),
                ),
                with(
                    msg("edge-due", "action_required"),
                    task("2031-10-01T12:00:00Z"),
                ),
                with(
                    msg("edge-old-due", "action_required"),
                    task("2000-01-01T00:00:00Z"),
                ),
            ],
        );
        assert_eq!((edge.imported, edge.rejected), (3, 0), "{:?}", edge.lines);
    }

    #[test]
    fn an_email_first_imported_unmatched_is_linked_when_a_later_import_names_the_posting() {
        let t = Tracker::in_memory().unwrap();
        let a = sent(&t, "1", "Acme AI");
        let b = sent(&t, "2", "Acme AI");
        let by_company = with(msg("m1", "rejection"), json!({ "company": "Acme AI" }));

        let first = run(&t, std::slice::from_ref(&by_company));
        assert_eq!(
            (first.imported, first.unmatched, first.state_changes),
            (1, 1, 0)
        );
        assert_eq!(emails(&t)[0].job_key, None);

        let named = with(by_company, json!({ "job_key": a }));
        let second = run(&t, std::slice::from_ref(&named));

        assert_eq!(
            (
                second.imported,
                second.skipped,
                second.linked,
                second.state_changes
            ),
            (0, 0, 1, 1),
            "{:?}",
            second.lines
        );
        assert_eq!(emails(&t)[0].job_key.as_deref(), Some(a.as_str()));
        assert_eq!(state_of(&t, &a), State::Rejected);
        assert_eq!(state_of(&t, &b), State::Submitted);
        assert!(second.lines.join("\n").contains("vinculado"));

        // The third time it is a plain skip: no second move, no second link.
        let events = t.events_for(&a).unwrap().len();
        let third = run(&t, &[named]);
        assert_eq!(
            (third.skipped, third.linked, third.state_changes),
            (1, 0, 0)
        );
        assert_eq!(t.events_for(&a).unwrap().len(), events);
    }

    #[test]
    fn linking_a_known_invitation_keeps_its_one_task_and_gives_it_the_posting() {
        let t = Tracker::in_memory().unwrap();
        let a = sent(&t, "1", "Acme AI");
        sent(&t, "2", "Acme AI");
        let invite = with(
            msg("m1", "interview_invite"),
            json!({ "company": "Acme AI" }),
        );
        run(&t, std::slice::from_ref(&invite));
        assert_eq!(t.open_tasks().unwrap()[0].job_key, None);

        let report = run(&t, &[with(invite, json!({ "job_key": a }))]);

        assert_eq!((report.linked, report.tasks_created), (1, 0));
        assert_eq!(state_of(&t, &a), State::Interview);
        let tasks = t.open_tasks().unwrap();
        assert_eq!(tasks.len(), 1, "the task is not created twice");
        assert_eq!(tasks[0].job_key.as_deref(), Some(a.as_str()));
    }

    #[test]
    fn a_known_message_is_only_skipped_when_it_is_already_linked_or_still_matches_nothing() {
        let t = Tracker::in_memory().unwrap();
        let a = sent(&t, "1", "Acme AI");
        let b = sent(&t, "2", "Globex");
        let linked = with(msg("m1", "rejection"), json!({ "job_key": a }));
        let lost = with(msg("m2", "other"), json!({ "company": "Nadie" }));
        run(&t, &[linked.clone(), lost.clone()]);

        // Another key for a message that is already linked: it keeps the first.
        let other_key = with(linked, json!({ "job_key": b }));
        let report = run(&t, &[other_key, lost]);

        assert_eq!(
            (report.skipped, report.linked, report.unmatched),
            (2, 0, 0),
            "{:?}",
            report.lines
        );
        assert_eq!(state_of(&t, &b), State::Submitted);
        let by_id = |id: &str| t.get_email(id).unwrap().unwrap().job_key;
        assert_eq!(by_id("m1").as_deref(), Some(a.as_str()));
        assert_eq!(by_id("m2"), None);
    }

    #[test]
    fn an_email_from_before_the_application_was_sent_is_kept_but_moves_nothing() {
        let t = Tracker::in_memory().unwrap();
        // Sent on 2026-09-28 at 12:00 UTC.
        let key = sent(&t, "1", "Acme AI");
        let events = t.events_for(&key).unwrap().len();
        let old = with(
            msg("m1", "rejection"),
            json!({ "company": "Acme AI", "received_at": "2026-08-01T10:00:00Z" }),
        );

        let report = run(&t, &[old]);

        assert_eq!(
            (report.imported, report.state_changes),
            (1, 0),
            "{:?}",
            report.lines
        );
        assert_eq!(state_of(&t, &key), State::Submitted);
        assert_eq!(t.events_for(&key).unwrap().len(), events);
        assert_eq!(emails(&t)[0].job_key.as_deref(), Some(key.as_str()));
        assert!(
            report
                .lines
                .join("\n")
                .contains("anterior a la postulación, sin cambio de estado"),
            "{:?}",
            report.lines
        );

        // An answer at the very moment it was sent is not earlier than it.
        let same = with(
            msg("m2", "recruiter"),
            json!({ "job_key": key, "received_at": "2026-09-28T12:00:00Z" }),
        );
        assert_eq!(run(&t, &[same]).state_changes, 1);
    }

    #[test]
    fn a_late_answer_after_silence_is_matched_by_job_key_and_by_company() {
        for (kind, expected) in [
            ("rejection", State::Rejected),
            ("technical_test", State::TechnicalTest),
            ("recruiter", State::Replied),
            ("interview_invite", State::Interview),
        ] {
            for by_key in [true, false] {
                let t = Tracker::in_memory().unwrap();
                let key = app(
                    &t,
                    "1",
                    "Acme AI",
                    &[State::Prepared, State::Submitted, State::Ghosted],
                );
                let how = if by_key {
                    json!({ "job_key": key })
                } else {
                    json!({ "company": "Acme AI" })
                };

                let report = run(&t, &[with(msg("m1", kind), how)]);

                assert_eq!(
                    state_of(&t, &key),
                    expected,
                    "{kind}, by key: {by_key}: {:?}",
                    report.lines
                );
                assert_eq!(report.state_changes, 1, "{kind}, by key: {by_key}");
            }
        }
    }

    #[test]
    fn an_application_with_an_offer_is_still_found_by_its_company() {
        let t = Tracker::in_memory().unwrap();
        let key = app(
            &t,
            "1",
            "Acme AI",
            &[
                State::Prepared,
                State::Submitted,
                State::Interview,
                State::Offer,
            ],
        );

        let report = run(
            &t,
            &[with(
                msg("m1", "rejection"),
                json!({ "company": "Acme AI" }),
            )],
        );

        assert_eq!(report.unmatched, 0, "{:?}", report.lines);
        assert_eq!(state_of(&t, &key), State::Rejected);
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
