//! What Gonza marks by hand: approving, resolving and confirming applications, recording a
//! company's answer, undoing a mark, and recording an application that went out outside jobbot.

use std::io::Write;

use anyhow::Result;
use chrono::{DateTime, TimeDelta, Utc};

use super::{EXIT_ERROR, EXIT_OK, RecordArgs, open_tracker, parse_flag_time};
use crate::actions;
use crate::paths::Paths;

/// How far ahead of the clock `--at` may be before it is refused: a little slack for a clock that
/// runs fast, none for a date that has not happened.
const FUTURE_TOLERANCE: TimeDelta = TimeDelta::minutes(5);

/// Runs `act` on each key, in order, and prints what it answered. The exit code is the one `mark` gives for
/// a refusal when any key was refused, `EXIT_OK` when all were accepted.
fn act_on_each(
    keys: &[String],
    out: &mut dyn Write,
    mut act: impl FnMut(&str) -> Result<actions::Outcome>,
) -> Result<i32> {
    let mut code = EXIT_OK;
    for key in keys {
        let outcome = act(key)?;
        writeln!(out, "{}", outcome.message)?;
        if !outcome.ok {
            code = EXIT_ERROR;
        }
    }
    Ok(code)
}

pub(super) fn cmd_approve(paths: &Paths, keys: &[String], out: &mut dyn Write) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    act_on_each(keys, out, |key| actions::approve(&tracker, key))
}

pub(super) fn cmd_resolve(
    paths: &Paths,
    keys: &[String],
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    act_on_each(keys, out, |key| actions::resolve(&tracker, key, now))
}

/// For `unconfirmed_submit` and assisted forms Gonza finished himself.
pub(super) fn cmd_confirm(
    paths: &Paths,
    keys: &[String],
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    act_on_each(keys, out, |key| actions::confirm(&tracker, key, now))
}

/// Records a company's answer by hand: nothing reads the mailbox for it.
pub(super) fn cmd_mark(
    paths: &Paths,
    key: &str,
    state_text: &str,
    note: Option<&str>,
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    let outcome = actions::mark(&tracker, key, state_text, note, now)?;
    writeln!(out, "{}", outcome.message)?;
    Ok(if outcome.ok { EXIT_OK } else { EXIT_ERROR })
}

/// Takes back the last mark of an application, the same way the panel's undo does.
pub(super) fn cmd_unmark(
    paths: &Paths,
    key: &str,
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let tracker = open_tracker(paths)?;
    let outcome = actions::unmark(&tracker, key, now)?;
    writeln!(out, "{}", outcome.message)?;
    Ok(if outcome.ok { EXIT_OK } else { EXIT_ERROR })
}

/// Records an application that went out outside jobbot. Like `mark` and `confirm` it writes only
/// the tracker, so it ignores STOP and the execution window: refusing to write down a form that
/// was already sent would only leave the tracker wrong.
pub(super) fn cmd_record(
    paths: &Paths,
    args: RecordArgs,
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> Result<i32> {
    let at = match args.at.as_deref() {
        None => now,
        Some(text) => match parse_flag_time("--at", text, "2026-10-05T05:21:42Z", out)? {
            Some(time) => time,
            None => return Ok(EXIT_ERROR),
        },
    };
    if at > now + FUTURE_TOLERANCE {
        writeln!(
            out,
            "--at está en el futuro ({}): una postulación no se registra antes de enviarse.",
            at.to_rfc3339()
        )?;
        return Ok(EXIT_ERROR);
    }
    let spec = actions::RecordSpec {
        key: args.key,
        company: args.company.unwrap_or_default(),
        title: args.title.unwrap_or_default(),
        url: args.url.unwrap_or_default(),
        location: args.location.unwrap_or_default(),
        cv_path: args.cv,
        salary_min_monthly_usd: args.salary_min,
        salary_max_monthly_usd: args.salary_max,
        note: args.note,
        origin: None,
    };
    let tracker = open_tracker(paths)?;
    let outcome = actions::record(&tracker, &spec, at)?;
    writeln!(out, "{}", outcome.message)?;
    Ok(if outcome.ok { EXIT_OK } else { EXIT_ERROR })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::{TimeZone, Utc};

    use crate::cli::test_support::{
        busy_now, cli, fake_deps, open_now, plain, seed, seed_prepared, temp_root, tracker_of,
    };
    use crate::form::SubmitMode;
    use crate::testing::{make_job, make_job_with};
    use crate::tracker::State;

    #[tokio::test]
    async fn approve_unknown_key_is_reported() {
        let root = temp_root();
        let (code, out) = plain(root.path(), open_now(), &["approve", "nope:1"]).await;
        assert_eq!(code, 1, "a refused approval exits like a refused mark");
        assert!(out.contains("No existe: nope:1"), "{out}");
    }

    #[tokio::test]
    async fn approve_and_confirm_exit_with_an_error_only_when_a_key_was_refused() {
        let root = temp_root();
        seed(root.path(), &make_job(), &[State::Prepared]);
        // Prepared but never filled in (no screenshot to review): the approval is refused.
        let (code, out) = plain(root.path(), open_now(), &["approve", "test:1"]).await;
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("No hay captura que revisar"), "{out}");
        // A scored application is not waiting for Gonza: it cannot be confirmed.
        seed(
            root.path(),
            &make_job_with(|j| j.external_id = "2".into()),
            &[],
        );
        let (code, out) = plain(root.path(), open_now(), &["confirm", "test:2"]).await;
        assert_eq!(code, 1, "{out}");
        // Every key accepted: exit 0.
        tracker_of(root.path())
            .set_screenshot("test:1", Path::new("shots/a.png"))
            .unwrap();
        let (code, out) = plain(root.path(), open_now(), &["approve", "test:1"]).await;
        assert_eq!(code, 0, "{out}");
    }

    #[tokio::test]
    async fn approve_marks_the_application() {
        let root = temp_root();
        seed(root.path(), &make_job(), &[State::Prepared]);
        tracker_of(root.path())
            .set_screenshot("test:1", Path::new("shots/a.png"))
            .unwrap();
        let (_, out) = plain(root.path(), open_now(), &["approve", "test:1"]).await;
        assert!(out.contains("Aprobada: test:1"), "{out}");
        assert!(
            tracker_of(root.path())
                .get_application("test:1")
                .unwrap()
                .unwrap()
                .approved
        );
    }

    #[tokio::test]
    async fn resolve_sends_needs_gonza_back_to_the_queue() {
        let root = temp_root();
        seed(root.path(), &make_job(), &[State::NeedsGonza]);
        tracker_of(root.path())
            .set_cv_path("test:1", Path::new("cv.pdf"))
            .unwrap();
        let (code, out) = plain(root.path(), open_now(), &["resolve", "test:1", "nope:1"]).await;
        assert_eq!(code, 1, "one key was refused: {out}");
        assert!(out.contains("De vuelta a la cola: test:1"), "{out}");
        assert!(out.contains("No existe: nope:1"), "{out}");
        assert_eq!(
            tracker_of(root.path())
                .get_application("test:1")
                .unwrap()
                .unwrap()
                .state,
            State::Prepared
        );
    }

    #[tokio::test]
    async fn resolve_without_cv_goes_back_to_scored() {
        let root = temp_root();
        // Below-floor exceptions reach needs_gonza straight from scored, without a PDF: sending
        // them to prepared would make submit_all hand them straight back.
        seed(root.path(), &make_job(), &[State::NeedsGonza]);
        let (code, out) = plain(root.path(), open_now(), &["resolve", "test:1"]).await;
        assert_eq!(code, 0);
        assert!(
            out.contains("De vuelta a puntuadas (sin CV, prepare le hará el PDF): test:1"),
            "{out}"
        );
        assert_eq!(
            tracker_of(root.path())
                .get_application("test:1")
                .unwrap()
                .unwrap()
                .state,
            State::Scored
        );
    }

    #[tokio::test]
    async fn confirm_marks_a_needs_gonza_application_as_submitted() {
        let root = temp_root();
        seed(root.path(), &make_job(), &[State::NeedsGonza]);
        let (code, out) = plain(root.path(), open_now(), &["confirm", "test:1", "nope:1"]).await;
        assert_eq!(code, 1, "one key was refused: {out}");
        assert!(out.contains("Confirmada como enviada: test:1"), "{out}");
        assert!(out.contains("No existe: nope:1"), "{out}");
        let row = tracker_of(root.path())
            .get_application("test:1")
            .unwrap()
            .unwrap();
        assert_eq!(row.state, State::Submitted);
        assert!(
            row.approved,
            "a submission Gonza checked himself counts as reviewed"
        );
        // Only applications waiting for Gonza can be confirmed.
        seed(
            root.path(),
            &make_job_with(|j| j.external_id = "2".into()),
            &[],
        );
        let (_, out) = plain(root.path(), open_now(), &["confirm", "test:2"]).await;
        assert!(out.contains("No se pudo confirmar test:2"), "{out}");
    }

    #[tokio::test]
    async fn resolve_clears_the_old_screenshot_so_the_next_dry_run_takes_a_fresh_one() {
        let root = temp_root();
        let job = make_job();
        seed_prepared(root.path(), &job);
        // A dry run stopped for a missing answer and left its screenshot behind.
        let tracker = tracker_of(root.path());
        tracker
            .set_screenshot(&job.key(), Path::new("old.png"))
            .unwrap();
        tracker
            .transition(
                &job.key(),
                State::NeedsGonza,
                Some("faltan respuestas: Email"),
                open_now(),
            )
            .unwrap();
        drop(tracker);
        let (code, out) = plain(root.path(), open_now(), &["resolve", "test:1"]).await;
        assert_eq!(code, 0, "{out}");
        let row = tracker_of(root.path())
            .get_application("test:1")
            .unwrap()
            .unwrap();
        assert_eq!(row.state, State::Prepared);
        assert_eq!(row.screenshot_path, None);
        // The corrected form gets a fresh dry run instead of "Nada que enviar.".
        let (deps, calls) = fake_deps(vec![]);
        let (code, out) = cli(root.path(), open_now(), deps, &["submit"]).await;
        assert_eq!(code, 0, "{out}");
        assert_eq!(*calls.lock().unwrap(), vec![SubmitMode::DryRun], "{out}");
    }

    #[tokio::test]
    async fn mark_explains_in_spanish_and_points_to_confirm_for_needs_gonza() {
        let root = temp_root();
        seed(
            root.path(),
            &make_job(),
            &[State::Prepared, State::NeedsGonza],
        );
        let (code, out) = plain(root.path(), open_now(), &["mark", "test:1", "replied"]).await;
        assert_eq!(code, 1, "{out}");
        assert!(
            out.contains("no se puede pasar de needs_gonza a replied") && out.contains("confirm"),
            "{out}"
        );
        assert!(!out.contains("illegal transition"), "{out}");
    }

    #[tokio::test]
    async fn mark_records_a_reply_by_hand_and_it_shows_in_the_status() {
        let root = temp_root();
        seed(
            root.path(),
            &make_job(),
            &[State::Prepared, State::Submitted],
        );
        let (code, out) = plain(root.path(), busy_now(), &["mark", "test:1", "replied"]).await;
        assert_eq!(code, 0, "mark needs no window: {out}");
        assert!(out.contains("test:1 → replied"), "{out}");
        let (code, _) = plain(
            root.path(),
            busy_now(),
            &["mark", "test:1", "interview", "--note", "viernes 19:00"],
        )
        .await;
        assert_eq!(code, 0);
        let row = tracker_of(root.path())
            .get_application("test:1")
            .unwrap()
            .unwrap();
        assert_eq!(row.state, State::Interview);
        assert!(
            plain(root.path(), busy_now(), &["status"])
                .await
                .1
                .contains("interview")
        );
    }

    #[tokio::test]
    async fn unmark_takes_back_the_last_mark_once() {
        let root = temp_root();
        seed(
            root.path(),
            &make_job(),
            &[State::Prepared, State::Submitted],
        );
        let (code, _) = plain(root.path(), busy_now(), &["mark", "test:1", "rejected"]).await;
        assert_eq!(code, 0);
        let (code, out) = plain(root.path(), busy_now(), &["unmark", "test:1"]).await;
        assert_eq!(code, 0, "unmark needs no window: {out}");
        assert!(out.contains("Deshecho: test:1 vuelve a submitted"), "{out}");
        let row = tracker_of(root.path())
            .get_application("test:1")
            .unwrap()
            .unwrap();
        assert_eq!(row.state, State::Submitted);
        let (code, out) = plain(root.path(), busy_now(), &["unmark", "test:1"]).await;
        assert_eq!(code, 1, "{out}");
        assert!(
            out.contains("No hay una marca reciente que deshacer"),
            "{out}"
        );
    }

    #[tokio::test]
    async fn record_registers_a_hand_sent_application_with_its_real_time_inside_work_hours() {
        let root = temp_root();
        let (code, out) = plain(
            root.path(),
            busy_now(),
            &[
                "record",
                "--company",
                "Toku",
                "--title",
                "Senior Software Engineer",
                "--url",
                "https://jobs.ashbyhq.com/toku/abc/application",
                "--at",
                "2026-10-05T05:21:42Z",
                "--cv",
                "data/cv/toku.pdf",
                "--note",
                "enviada con Playwright",
                "--salary-min",
                "5000",
            ],
        )
        .await;
        assert_eq!(code, 0, "record needs no window: {out}");
        assert!(out.contains("Registrada como enviada: Toku"), "{out}");
        let tracker = tracker_of(root.path());
        let row = tracker
            .get_application("manual:jobs.ashbyhq.com_toku_abc")
            .unwrap()
            .unwrap();
        assert_eq!(row.state, State::Submitted);
        assert_eq!(
            row.submitted_at,
            Some(Utc.with_ymd_and_hms(2026, 10, 5, 5, 21, 42).unwrap())
        );
        assert_eq!(row.cv_path.as_deref(), Some("data/cv/toku.pdf"));
        assert_eq!(row.note.as_deref(), Some("enviada con Playwright"));
        assert!(!row.approved);
        assert!(
            plain(root.path(), busy_now(), &["status"])
                .await
                .1
                .contains("submitted")
        );
    }

    #[tokio::test]
    async fn record_ignores_stop_and_stamps_now_when_no_time_is_given() {
        let root = temp_root();
        assert_eq!(plain(root.path(), open_now(), &["stop"]).await.0, 0);
        let (code, out) = plain(
            root.path(),
            open_now(),
            &[
                "record",
                "--company",
                "Railway",
                "--title",
                "Senior Full-Stack Engineer",
                "--url",
                "https://jobs.ashbyhq.com/railway/xyz",
            ],
        )
        .await;
        assert_eq!(code, 0, "recording a fact is not blocked by STOP: {out}");
        let row = tracker_of(root.path())
            .get_application("manual:jobs.ashbyhq.com_railway_xyz")
            .unwrap()
            .unwrap();
        assert_eq!(row.submitted_at, Some(open_now()));
    }

    #[tokio::test]
    async fn the_refusals_of_a_record_date_say_the_moment_as_utc_with_a_plus_zero_offset() {
        let root = temp_root();
        let record =
            |at: &'static str| ["record", "--company", "Toku", "--title", "SE", "--at", at];
        let said = |out: String| out.trim_end().to_string();
        let (_, out) = plain(root.path(), open_now(), &record("ayer")).await;
        assert_eq!(
            said(out),
            "--at debe ser una fecha RFC 3339, por ejemplo 2026-10-05T05:21:42Z (recibí «ayer»)"
        );
        for (at, shown) in [
            ("2027-01-01T00:00:00Z", "2027-01-01T00:00:00+00:00"),
            ("2026-12-31T21:00:00-03:00", "2027-01-01T00:00:00+00:00"),
            ("2027-01-01T00:00:00.25Z", "2027-01-01T00:00:00.250+00:00"),
        ] {
            let (code, out) = plain(root.path(), open_now(), &record(at)).await;
            assert_eq!(code, 1, "{at}");
            assert_eq!(
                said(out),
                format!(
                    "--at está en el futuro ({shown}): una postulación no se registra antes de enviarse."
                ),
                "{at}"
            );
        }
    }

    #[tokio::test]
    async fn the_record_date_is_stored_as_the_utc_second_whatever_shape_it_came_in() {
        let sent = Utc.with_ymd_and_hms(2026, 10, 14, 15, 0, 0).unwrap();
        for (i, at) in [
            "2026-10-14T15:00:00Z",
            "2026-10-14T15:00:00+00:00",
            "2026-10-14T12:00:00-03:00",
            "2026-10-14T15:00:00.999999999Z",
        ]
        .into_iter()
        .enumerate()
        {
            let root = temp_root();
            let url = format!("https://jobs.ashbyhq.com/toku/{i}");
            let (code, out) = plain(
                root.path(),
                open_now(),
                &[
                    "record",
                    "--company",
                    "Toku",
                    "--title",
                    "SE",
                    "--url",
                    &url,
                    "--at",
                    at,
                ],
            )
            .await;
            assert_eq!(code, 0, "{at}: {out}");
            let tracker = tracker_of(root.path());
            assert_eq!(tracker.submitted_since(sent).unwrap(), 1, "{at}");
            assert_eq!(
                tracker
                    .submitted_since(sent + chrono::TimeDelta::seconds(1))
                    .unwrap(),
                0,
                "{at}: stored at 15:00:00, not rounded up"
            );
        }
    }

    #[tokio::test]
    async fn record_refuses_a_bad_or_future_time_and_a_key_mixed_with_company() {
        let root = temp_root();
        let base = [
            "record",
            "--company",
            "Toku",
            "--title",
            "SE",
            "--url",
            "https://jobs.ashbyhq.com/toku/abc",
        ];
        let with = |extra: &[&'static str]| {
            let mut parts: Vec<&str> = base.to_vec();
            parts.extend_from_slice(extra);
            parts
        };
        let (code, out) = plain(root.path(), open_now(), &with(&["--at", "ayer"])).await;
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("--at") && out.contains("RFC 3339"), "{out}");
        let (code, out) = plain(
            root.path(),
            open_now(),
            &with(&["--at", "2027-01-01T00:00:00Z"]),
        )
        .await;
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("--at") && out.contains("futuro"), "{out}");
        let (code, out) = plain(
            root.path(),
            open_now(),
            &["record", "--key", "test:1", "--company", "Toku"],
        )
        .await;
        assert_eq!(code, 1, "{out}");
        assert!(
            tracker_of(root.path()).counts().unwrap().is_empty(),
            "none of the refusals stored anything"
        );
    }

    #[tokio::test]
    async fn record_of_a_stored_key_works_once_and_a_replay_exits_with_an_error() {
        let root = temp_root();
        seed(root.path(), &make_job(), &[State::Prepared]);
        let args = ["record", "--key", "test:1"];
        let (code, out) = plain(root.path(), open_now(), &args).await;
        assert_eq!(code, 0, "{out}");
        let (code, out) = plain(root.path(), open_now(), &args).await;
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("ya está registrada"), "{out}");
    }

    #[tokio::test]
    async fn mark_refuses_states_it_does_not_own_illegal_moves_and_unknown_keys() {
        let root = temp_root();
        seed(
            root.path(),
            &make_job(),
            &[State::Prepared, State::Submitted],
        );
        let (code, out) = plain(root.path(), open_now(), &["mark", "test:1", "prepared"]).await;
        assert_eq!(code, 1);
        assert!(
            out.contains("Estado no válido") && out.contains("replied"),
            "{out}"
        );
        let (code, out) = plain(root.path(), open_now(), &["mark", "test:1", "offer"]).await;
        assert_eq!(code, 1, "submitted cannot jump to an offer");
        assert!(out.contains("No se pudo marcar"), "{out}");
        let (code, out) = plain(root.path(), open_now(), &["mark", "nope:1", "replied"]).await;
        assert_eq!(code, 1);
        assert!(out.contains("No se pudo marcar"), "{out}");
    }
}
