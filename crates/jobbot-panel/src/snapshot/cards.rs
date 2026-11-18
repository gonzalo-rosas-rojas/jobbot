//! The cards of the board and of the queues: one application as the page draws it, the columns
//! and listings that hold cards, and the sentence that says why a waiting card waits.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;

use super::iso;
use crate::Panel;
use jobbot_app::actions::{UNDO_WINDOW, marks_from};
use jobbot_app::discovery::MAY_HAVE_BEEN_SENT;
use jobbot_app::learn::dossier::dossier_path;
use jobbot_app::ports::store::{ApplicationRow, UndoableMark};
use jobbot_domain::gate::REASON_LISTING_ONLY;
use jobbot_domain::handoff::sheet_path;
use jobbot_domain::rules::Criteria;
use jobbot_domain::state::State;

/// Cards shown per board column, and in the queue of things that need Gonza (plan ruling R8).
pub const COLUMN_CAP: usize = 25;
pub const QUEUE_CAP: usize = 50;

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
    /// Gonza's last mark, while `POST /api/mark/undo` can still take it back.
    pub undo: Option<UndoCard>,
}

/// A mark that can be taken back until `until`.
#[derive(Debug, Serialize, PartialEq)]
pub struct UndoCard {
    pub from: &'static str,
    pub to: &'static str,
    pub at: String,
    pub until: String,
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

pub(super) fn label(state: State) -> &'static str {
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
    if reason.starts_with(REASON_LISTING_ONLY) {
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

/// What a card needs besides its row: the clock, the dossier and hand-over folders, the age limit
/// and the marks that can still be undone.
pub(crate) struct CardContext<'a> {
    now: DateTime<Utc>,
    dossiers: &'a Path,
    handoffs: &'a Path,
    /// `max_posting_age_days` of `profile/criteria.toml`; `None` when it cannot be read.
    max_age_days: Option<i64>,
    /// `undoable_marks` within `UNDO_WINDOW`, by key.
    undoable: BTreeMap<String, UndoableMark>,
}

/// Whole days from `since` to `now`, never negative (a posting dated in the future is 0 days old).
fn days_since(now: DateTime<Utc>, since: DateTime<Utc>) -> i64 {
    (now - since).num_days().max(0)
}

/// A snooze that still holds at `now`.
fn snoozed_at(row: &ApplicationRow, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    row.snoozed_until.filter(|until| *until > now)
}

/// True when a stored screenshot path names a file that is still there: only then is there
/// something for Gonza to review.
pub(super) fn has_screenshot_on_disk(screenshot_path: Option<&str>) -> bool {
    file_on_disk(screenshot_path)
}

/// Prepared, with the screenshot of its filled form on disk and not yet approved: the one rule
/// behind the `approve` action of a card and the "awaiting approval" queue.
pub(super) fn awaits_approval(row: &ApplicationRow) -> bool {
    row.state == State::Prepared
        && !row.approved
        && has_screenshot_on_disk(row.screenshot_path.as_deref())
}

/// True when a stored path names a file that is still there.
pub(super) fn file_on_disk(path: Option<&str>) -> bool {
    path.is_some_and(|p| Path::new(p).is_file())
}

pub(crate) fn card(row: &ApplicationRow, ctx: &CardContext<'_>) -> Card {
    let has_screenshot = has_screenshot_on_disk(row.screenshot_path.as_deref());
    let mut actions = Vec::new();
    match row.state {
        State::NeedsGonza => {
            actions.push("resolve");
            actions.push("confirm");
        }
        State::Prepared if awaits_approval(row) => {
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
        has_screenshot,
        has_cv: file_on_disk(row.cv_path.as_deref()),
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
        undo: ctx
            .undoable
            .get(&row.job_key)
            .filter(|mark| mark.to == row.state)
            .map(|mark| UndoCard {
                from: mark.from.as_str(),
                to: mark.to.as_str(),
                at: iso(mark.at),
                until: iso(mark.at + UNDO_WINDOW),
            }),
    }
}

pub(super) fn listing(rows: &[ApplicationRow], cap: usize, ctx: &CardContext<'_>) -> Listing {
    Listing {
        total: rows.len(),
        cards: rows.iter().take(cap).map(|r| card(r, ctx)).collect(),
    }
}

pub(crate) fn card_context<'a>(
    panel: &Panel,
    now: DateTime<Utc>,
    dossiers: &'a Path,
    handoffs: &'a Path,
    criteria: Option<&Criteria>,
) -> Result<CardContext<'a>> {
    Ok(CardContext {
        now,
        dossiers,
        handoffs,
        max_age_days: criteria.map(|c| c.max_posting_age_days),
        undoable: panel.tracker.undoable_marks(now - UNDO_WINDOW)?,
    })
}

/// "Company — Title" of a posting, or the key itself when the posting is unknown.
pub fn offer_label(panel: &Panel, key: &str) -> String {
    match panel.tracker.get_job(key) {
        Ok(Some(job)) => format!("{} — {}", job.company, job.title),
        Ok(None) => key.to_string(),
        Err(err) => {
            tracing::warn!(key, error = %format!("{err:#}"), "offer label unavailable, the key is shown");
            key.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;
    use crate::fixtures::{add, panel};
    use crate::snapshot::support_tests::{column, profile_with, shipped_criteria};
    use crate::snapshot::{Snapshot, build_job_detail, build_snapshot};
    use jobbot_testkit::{NOW, make_job_with};

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
    fn a_card_says_whether_its_answer_sheet_for_a_hand_over_is_on_disk() {
        let (panel, dir) = panel();
        let with_sheet = add(&panel, "1", 80.0, &[State::Prepared, State::NeedsGonza]);
        add(&panel, "2", 80.0, &[State::Prepared, State::NeedsGonza]);
        let sent = add(&panel, "3", 80.0, &[State::Prepared, State::Submitted]);
        // Where `handoff::write_sheet` puts it: `<data>/handoff/<key as a file name>.md`.
        let handoffs = dir.path().join("data").join("handoff");
        std::fs::create_dir_all(&handoffs).unwrap();
        for key in [&with_sheet, &sent] {
            std::fs::write(jobbot_domain::handoff::sheet_path(&handoffs, key), "# hoja").unwrap();
        }
        // A folder with the sheet's name is not a sheet.
        let folder = add(&panel, "4", 80.0, &[State::Prepared, State::NeedsGonza]);
        std::fs::create_dir(jobbot_domain::handoff::sheet_path(&handoffs, &folder)).unwrap();

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
}
