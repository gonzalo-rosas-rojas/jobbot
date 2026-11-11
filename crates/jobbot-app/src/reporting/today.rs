//! `jobbot today`: what Gonza has to do, as plain Spanish text. Read-only. It lists his open tasks
//! (soonest due first), the applications that wait for his decision (the same list and reason the
//! digest and the panel show) and the answers of the last days. Times are Santiago's, because
//! that is where he reads them.

use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};
use chrono_tz::America::Santiago;
use jobbot_domain::outbound::plain_line;
use jobbot_domain::state::State;

use crate::ports::store::{EmailRow, Store, TaskRow};

/// How far back "recent answers" looks.
pub const RECENT_DAYS: i64 = 7;
/// Lines shown of the decision list and of the recent answers; the rest is counted.
pub const LIST_CAP: usize = 50;

fn santiago(at: DateTime<Utc>) -> String {
    at.with_timezone(&Santiago)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

/// "vence en 5 h" up to two days away, "vence en 3 días" beyond, "VENCIDA" once it has passed; the
/// Santiago time follows.
fn due_phrase(task: &TaskRow, due: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let when = santiago(due);
    if task.is_overdue(now) {
        return format!("VENCIDA (vencía el {when})");
    }
    let hours = (due - now).num_hours();
    let left = match hours {
        0 => "vence en menos de 1 h".to_string(),
        1..48 => format!("vence en {hours} h"),
        _ => format!("vence en {} días", hours / 24),
    };
    format!("{left} ({when})")
}

fn task_line(task: &TaskRow, now: DateTime<Utc>) -> String {
    let key = task
        .job_key
        .as_deref()
        .map(|key| format!(" [{}]", plain_line(key)))
        .unwrap_or_default();
    let link = task
        .link
        .as_deref()
        .map(|link| format!(" · {}", plain_line(link)))
        .unwrap_or_default();
    let due = match task.due_at {
        Some(due) => due_phrase(task, due, now),
        None => "sin fecha".to_string(),
    };
    format!(
        "  #{} {}{link}{key} — {due}",
        task.id,
        plain_line(&task.text)
    )
}

fn email_line(email: &EmailRow) -> String {
    let who = email.company.as_deref().unwrap_or(email.from_addr.expose());
    let subject = match plain_line(&email.subject) {
        subject if subject.is_empty() => "(sin asunto)".to_string(),
        subject => subject,
    };
    let summary = match plain_line(&email.summary) {
        summary if summary.is_empty() => String::new(),
        summary => format!(" — {summary}"),
    };
    format!(
        "  {} · {} · {} · «{subject}»{summary}",
        santiago(email.received_at),
        plain_line(who),
        email.kind.label()
    )
}

/// A heading with the total, the rows (cut at `cap`, with the count of the rest) or the sentence
/// for "nothing", and a blank line.
fn section(
    lines: &mut Vec<String>,
    heading: &str,
    mut rows: Vec<String>,
    cap: usize,
    when_empty: &str,
) {
    lines.push(format!("{heading} ({})", rows.len()));
    if rows.is_empty() {
        lines.push(format!("  {when_empty}"));
    } else if rows.len() > cap {
        let rest = rows.len() - cap;
        rows.truncate(cap);
        lines.extend(rows);
        lines.push(format!("  … y {rest} más."));
    } else {
        lines.extend(rows);
    }
    lines.push(String::new());
}

/// The text of `jobbot today`. Everything that came from an email goes through `plain_line` again
/// here, because this text is printed on a terminal.
#[tracing::instrument(name = "today", skip_all)]
pub fn build_today(tracker: &dyn Store, now: DateTime<Utc>) -> Result<String> {
    let day = now.with_timezone(&Santiago).format("%Y-%m-%d");
    let mut lines = vec![
        format!("Tu lista de hoy — {day} (hora de Santiago)"),
        String::new(),
    ];

    // The tasks are never cut: they are the list.
    let tasks: Vec<String> = tracker
        .open_tasks()?
        .iter()
        .map(|task| task_line(task, now))
        .collect();
    section(
        &mut lines,
        "Tareas abiertas",
        tasks,
        usize::MAX,
        "Sin tareas abiertas.",
    );

    let waiting: Vec<String> = tracker
        .list_by_state(State::NeedsGonza)?
        .iter()
        .map(|row| {
            format!(
                "  {} — {} · {}: {}",
                plain_line(&row.company),
                plain_line(&row.title),
                plain_line(&row.job_key),
                plain_line(row.waiting_reason().unwrap_or("sin motivo"))
            )
        })
        .collect();
    section(
        &mut lines,
        "Esperan tu decisión",
        waiting,
        LIST_CAP,
        "Nada pendiente.",
    );

    let recent: Vec<String> = tracker
        .recent_emails(now - TimeDelta::days(RECENT_DAYS))?
        .iter()
        .map(email_line)
        .collect();
    section(
        &mut lines,
        &format!("Respuestas recientes, últimos {RECENT_DAYS} días"),
        recent,
        LIST_CAP,
        "Sin respuestas nuevas.",
    );

    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    Ok(lines.join("\n") + "\n")
}
