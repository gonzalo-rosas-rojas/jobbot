//! Daily digest in Spanish Markdown: what needs Gonza, what is
//! waiting for his approval, live conversations, the last 24 hours' sends, new matches and
//! weekly metrics. The date in the title is Santiago's, because Gonza reads it there; everything
//! else is computed in UTC. Rejections are counted in the metrics but never listed: they stay in
//! the log, not in the push.

use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};
use chrono_tz::America::Santiago;
use jobbot_domain::outbound::plain_line;
use jobbot_domain::state::State;

use crate::ports::store::{ApplicationRow, Store};

/// One bullet for a job: score, company, title, and the source with its link (attribution). The
/// posting's fields are a stranger's text: the bullet stays one plain line, in the file too.
fn line(r: &ApplicationRow, extra: &str) -> String {
    plain_line(&format!(
        "- [{:.0}] {} — {} ({}: {}){}",
        r.score, r.company, r.title, r.source, r.url, extra
    ))
}

/// Appends a `##` section: the heading, the rows (or one fallback bullet when there are none)
/// and a blank line.
fn section(lines: &mut Vec<String>, heading: String, rows: Vec<String>, when_empty: &str) {
    lines.push(format!("## {heading}"));
    if rows.is_empty() {
        lines.push(format!("- {when_empty}"));
    } else {
        lines.extend(rows);
    }
    lines.push(String::new());
}

#[tracing::instrument(name = "digest", skip_all)]
pub fn build_digest(tracker: &dyn Store, now: DateTime<Utc>, top_n: usize) -> Result<String> {
    let today = now.with_timezone(&Santiago).format("%Y-%m-%d");
    let mut lines = vec![format!("# Resumen diario — {today}"), String::new()];

    let mut needs: Vec<String> = tracker
        .list_by_state(State::NeedsGonza)?
        .iter()
        .map(|r| {
            let reason = r.waiting_reason().unwrap_or("sin motivo");
            line(r, &format!(": {reason} · `{}`", r.job_key))
        })
        .collect();
    let pending = needs.len();
    if pending > 0 {
        needs.push(
            "- Para cada una: `jobbot resolve KEY` la devuelve a la cola; `jobbot confirm KEY` la marca como \
             enviada si la mandaste tú (o si la captura `-after` muestra que sí se envió)."
                .to_string(),
        );
    }
    section(
        &mut lines,
        format!("Necesitan a Gonza ({pending})"),
        needs,
        "Nada pendiente.",
    );

    let awaiting: Vec<String> = tracker
        .list_by_state(State::Prepared)?
        .iter()
        .filter(|r| r.screenshot_path.is_some() && !r.approved)
        .map(|r| {
            let shot = r.screenshot_path.as_deref().unwrap_or_default();
            line(
                r,
                &format!(" · captura: {shot} · `jobbot approve {}`", r.job_key),
            )
        })
        .collect();
    section(
        &mut lines,
        format!("Por aprobar ({})", awaiting.len()),
        awaiting,
        "Nada por aprobar.",
    );

    let mut live = Vec::new();
    for state in [
        State::Offer,
        State::Interview,
        State::TechnicalTest,
        State::Replied,
    ] {
        for r in tracker.list_by_state(state)? {
            live.push(line(&r, &format!(" [{}]", state.as_str())));
        }
    }
    section(
        &mut lines,
        format!("Entrevistas y respuestas ({})", live.len()),
        live,
        "Sin novedades.",
    );

    // A rolling day, like the cap in the gate: the evening window crosses midnight in every zone.
    lines.push(format!(
        "## Enviadas en las últimas 24 h ({})",
        tracker.submitted_since(now - TimeDelta::hours(24))?
    ));
    lines.push(String::new());

    let top: Vec<String> = tracker
        .top_scored(top_n)?
        .iter()
        .map(|r| line(r, ""))
        .collect();
    section(
        &mut lines,
        format!("Nuevas coincidencias (top {top_n})"),
        top,
        "Sin coincidencias nuevas.",
    );

    let week = now - TimeDelta::days(7);
    let sent = tracker.submitted_since(week)?;
    let responses = tracker.count_events(
        &[
            State::Replied,
            State::TechnicalTest,
            State::Interview,
            State::Offer,
            State::Rejected,
        ],
        week,
    )?;
    let interviews = tracker.count_events(&[State::Interview], week)?;
    let rate = if sent > 0 {
        format!("{:.0}%", 100.0 * responses as f64 / sent as f64)
    } else {
        "n/a".to_string()
    };
    lines.push("## Métricas de la semana".to_string());
    lines.push(format!(
        "- Enviadas: {sent} · Respuestas: {responses} ({rate}) · Entrevistas: {interviews}"
    ));
    // What the headless Claude runs cost: the digest is where Gonza sees it.
    lines.push(String::new());
    lines.push("## Claude (últimas 24 h)".to_string());
    let by_stage = tracker.claude_calls_by_stage(now - TimeDelta::hours(24))?;
    if by_stage.is_empty() {
        lines.push("- Sin llamadas.".to_string());
    }
    for (stage, (calls, dollars)) in &by_stage {
        lines.push(format!("- {stage}: {calls} llamadas · US${dollars:.2}"));
    }
    Ok(lines.join("\n") + "\n")
}
