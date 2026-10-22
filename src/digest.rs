//! Daily digest in Spanish Markdown (spec section 5, item 8): what needs Gonza, what is
//! waiting for his approval, live conversations, the last 24 hours' sends, new matches and
//! weekly metrics. The date in the title is Santiago's, because Gonza reads it there; everything
//! else is computed in UTC. Rejections are counted in the metrics but never listed: they stay in
//! the log, not in the push.

use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};
use chrono_tz::America::Santiago;

use crate::tracker::{ApplicationRow, State, Tracker};

/// One bullet for a job: score, company, title, and the source with its link (attribution).
fn line(r: &ApplicationRow, extra: &str) -> String {
    format!(
        "- [{:.0}] {} — {} ({}: {}){}",
        r.score, r.company, r.title, r.source, r.url, extra
    )
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

pub fn build_digest(tracker: &Tracker, now: DateTime<Utc>, top_n: usize) -> Result<String> {
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use super::*;
    use crate::models::Job;
    use crate::testing::{NOW, make_job, make_job_with};

    /// Adds a job, creates its application and walks it through `states`.
    fn seed(t: &Tracker, job: &Job, states: &[State]) {
        t.add_job(job, 80.0, &BTreeMap::new(), *NOW).unwrap();
        t.create_application(&job.key(), *NOW).unwrap();
        for state in states {
            t.transition(&job.key(), *state, None, *NOW).unwrap();
        }
    }

    #[test]
    fn digest_sections_attribution_and_privacy_of_rejections() {
        let t = Tracker::in_memory().unwrap();
        let new = make_job_with(|j| {
            j.source = "remoteok".into();
            j.external_id = "a".into();
            j.company = "NewCo".into();
            j.title = "New Role".into();
            j.url = "https://remoteok.com/remote-jobs/a".into();
        });
        let rejected = make_job_with(|j| {
            j.external_id = "b".into();
            j.company = "NoCo".into();
            j.title = "Rejected Role".into();
        });
        let interview = make_job_with(|j| {
            j.external_id = "c".into();
            j.company = "YesCo".into();
            j.title = "Interview Role".into();
        });
        let needy = make_job_with(|j| {
            j.external_id = "d".into();
            j.company = "NeedCo".into();
            j.title = "Needy Role".into();
        });
        seed(&t, &new, &[]);
        seed(
            &t,
            &rejected,
            &[State::Prepared, State::Submitted, State::Rejected],
        );
        seed(
            &t,
            &interview,
            &[State::Prepared, State::Submitted, State::Interview],
        );
        seed(&t, &needy, &[]);
        t.transition(&needy.key(), State::NeedsGonza, Some("falta email"), *NOW)
            .unwrap();

        let text = build_digest(&t, *NOW, 5).unwrap();
        assert!(text.contains("Resumen diario — 2026-10-01"), "{text}");
        assert!(
            text.contains("falta email") && text.contains("Interview Role"),
            "{text}"
        );
        // Attribution and the link back to the source.
        assert!(
            text.contains("remoteok: https://remoteok.com/remote-jobs/a"),
            "{text}"
        );
        // Rejections are logged, not pushed.
        assert!(!text.contains("Rejected Role"), "{text}");
        assert!(
            text.contains("Enviadas: 2 · Respuestas: 2 (100%) · Entrevistas: 1"),
            "{text}"
        );
    }

    #[test]
    fn digest_lists_applications_waiting_for_approval() {
        let t = Tracker::in_memory().unwrap();
        let job = make_job();
        seed(&t, &job, &[State::Prepared]);
        t.set_screenshot(&job.key(), Path::new("data/screenshots/x.png"))
            .unwrap();
        let text = build_digest(&t, *NOW, 5).unwrap();
        assert!(
            text.contains("Por aprobar (1)") && text.contains("jobbot approve test:1"),
            "{text}"
        );
    }

    #[test]
    fn empty_digest_is_still_valid() {
        let text = build_digest(&Tracker::in_memory().unwrap(), *NOW, 5).unwrap();
        assert!(
            text.contains("Nada pendiente.") && text.contains("n/a"),
            "{text}"
        );
    }

    #[test]
    fn top_section_respects_top_n() {
        let t = Tracker::in_memory().unwrap();
        for id in ["1", "2", "3"] {
            seed(&t, &make_job_with(|j| j.external_id = id.into()), &[]);
        }
        let text = build_digest(&t, *NOW, 2).unwrap();
        assert!(text.contains("Nuevas coincidencias (top 2)"), "{text}");
        assert_eq!(text.matches("- [80]").count(), 2, "{text}");
    }

    #[test]
    fn the_digest_reports_claude_calls_by_stage_for_the_last_day() {
        let t = Tracker::in_memory().unwrap();
        t.record_claude_call("dossier", *NOW, 0.05, 1, 1, true)
            .unwrap();
        t.record_claude_call("dossier", *NOW - TimeDelta::hours(2), 0.07, 1, 1, true)
            .unwrap();
        t.record_claude_call("dossier", *NOW - TimeDelta::hours(30), 9.0, 1, 1, true)
            .unwrap();
        let text = build_digest(&t, *NOW, 5).unwrap();
        assert!(
            text.contains("## Claude (últimas 24 h)")
                && text.contains("- dossier: 2 llamadas · US$0.12"),
            "{text}"
        );
    }

    #[test]
    fn the_digest_says_when_claude_was_not_used() {
        let text = build_digest(&Tracker::in_memory().unwrap(), *NOW, 5).unwrap();
        assert!(
            text.contains("## Claude (últimas 24 h)\n- Sin llamadas."),
            "{text}"
        );
    }
}
