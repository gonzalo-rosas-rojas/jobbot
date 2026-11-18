//! What a run leaves for Gonza and how he is told: the hand-back result, the controls marked for
//! him to finish, the reasons recorded and the line `--assist` prints.

use std::path::PathBuf;

use jobbot_domain::models::Job;
use jobbot_domain::outbound::plain_line;
use playwright_rs::Page;

use super::shots::take_shot;
use super::{ASSIST_TIMEOUT_MS, FieldInfo, FillPlan, SubmitResult, SubmitStatus, choice_group};

/// The reason when the submit click call returned an error: the form may have been sent anyway.
pub(super) fn click_error_reason(err: &impl std::fmt::Display) -> String {
    format!(
        "unconfirmed_submit: el clic de envío devolvió un error: {}",
        first_line(err)
    )
}

pub(crate) fn needs_gonza(reason: impl Into<String>, screenshot: Option<PathBuf>) -> SubmitResult {
    SubmitResult {
        status: SubmitStatus::NeedsGonza,
        reason: Some(reason.into()),
        screenshot,
    }
}

/// Takes the screenshot and hands the form back to Gonza with `reason`; the shot is how he sees
/// where the run stopped.
pub(super) async fn stop_for_gonza(
    page: &Page,
    shot: PathBuf,
    reason: impl Into<String>,
) -> anyhow::Result<SubmitResult> {
    take_shot(page, &shot).await?;
    Ok(needs_gonza(reason, Some(shot)))
}

/// First line of an error: Playwright errors carry a long call log after it.
pub(super) fn first_line(err: &impl std::fmt::Display) -> String {
    err.to_string()
        .lines()
        .next()
        .unwrap_or_default()
        .to_string()
}

/// Whether `field` stands for a required question nobody answered. A choice (a radio or a group of
/// two or more controls) is one question, represented by its first control: it is open when one
/// of its options is required and none was given an answer. A single control is open when it is
/// required and has no answer.
fn question_unanswered(
    fields: &[FieldInfo],
    field: &FieldInfo,
    answered: impl Fn(usize) -> bool,
) -> bool {
    let group = choice_group(fields, field);
    if group.len() >= 2 || field.kind == "radio" {
        group.first().is_some_and(|first| first.idx == field.idx)
            && group.iter().any(|member| member.required)
            && !group.iter().any(|member| answered(member.idx))
    } else {
        field.required && !answered(field.idx)
    }
}

/// The `data-jb` indexes of the controls a run leaves for Gonza, in page order: every required
/// control the plan has no answer for (one option per radio or checkbox group, the first), every
/// pre-ticked box he has to untick and every control whose fill failed. When the plan misses
/// nothing, only the failed ones.
pub(super) fn controls_left(fields: &[FieldInfo], plan: &FillPlan, failed: &[usize]) -> Vec<usize> {
    let answered = |idx: usize| {
        plan.actions.iter().any(|action| action.idx == idx) || plan.kept.contains(&idx)
    };
    let mut left = failed.to_vec();
    left.extend(&plan.to_untick);
    if !plan.missing.is_empty() {
        for field in fields {
            if question_unanswered(fields, field, answered) {
                left.push(field.idx);
            }
        }
    }
    left.sort_unstable();
    left.dedup();
    left
}

/// What `FLAG_JS` marks: for each control left for Gonza, the `data-jb` numbers of the whole
/// question it belongs to, itself first (it is the one outlined). The mark goes away as soon as any
/// of them is chosen, so it must watch every button of a radio group, not only the outlined one.
pub(super) fn flag_groups(fields: &[FieldInfo], left: &[usize]) -> Vec<Vec<usize>> {
    left.iter()
        .map(|&idx| {
            let mut group = vec![idx];
            if let Some(field) = fields.iter().find(|field| field.idx == idx) {
                group.extend(
                    choice_group(fields, field)
                        .iter()
                        .map(|member| member.idx)
                        .filter(|&member| member != idx),
                );
            }
            group
        })
        .collect()
}

/// Why a form that was filled as far as it could be still needs Gonza: the required answers the
/// bank does not have, then the fields that would not take their value. `None` when nothing is left.
pub(super) fn leftover_reason(missing: &[String], failures: &[String]) -> Option<String> {
    let missing =
        (!missing.is_empty()).then(|| format!("faltan respuestas: {}", missing.join("; ")));
    let failures = (!failures.is_empty()).then(|| failures.join("; "));
    match (missing, failures) {
        (Some(missing), Some(failures)) => Some(format!("{missing} — {failures}")),
        (missing, failures) => missing.or(failures),
    }
}

/// `reason`, plus what was still pending for Gonza when the program stopped waiting for him.
fn with_pending(reason: String, leftover: Option<&str>) -> String {
    match leftover {
        Some(left) => format!("{reason}. Quedaba pendiente: {left}"),
        None => reason,
    }
}

/// The reason recorded when Gonza never confirmed an assisted form.
pub(super) fn assist_timeout_reason(minutes: u64, leftover: Option<&str>) -> String {
    with_pending(
        format!("assist_timeout: sin confirmación en {minutes} minutos"),
        leftover,
    )
}

/// The reason recorded when Gonza closed the tab of an assisted form: the way to skip a form
/// without answering it. If he had sent it first, `confirm` records that.
pub(super) fn assist_closed_reason(leftover: Option<&str>) -> String {
    with_pending(
        "assist_closed: cerraste la pestaña sin que la página confirmara; si sí la enviaste, `jobbot confirm KEY` la marca"
            .to_string(),
        leftover,
    )
}

/// The line `--assist` prints on standard error when the form is in the person's hands: which
/// posting, what is left to answer, and how long the program waits.
pub(super) fn assist_banner(job: &Job, leftover: Option<&str>) -> String {
    let waiting = format!(
        "Espero la confirmación hasta {} minutos.",
        ASSIST_TIMEOUT_MS / 60_000
    );
    match leftover {
        Some(left) => format!(
            "[assist] {} — {}: formulario llenado hasta donde pude. Te quedó por completar (marcado en rojo en la página): {}. \
             Complétalo, resuelve el captcha si aparece y envía tú. {waiting}",
            plain_line(&job.company),
            plain_line(&job.title),
            plain_line(left)
        ),
        None => format!(
            "[assist] {} — {}: formulario listo en el navegador. Revisa, resuelve el captcha si aparece y envía tú. {waiting}",
            plain_line(&job.company),
            plain_line(&job.title)
        ),
    }
}

/// Marks the controls Gonza has to finish: a red outline, a `data-jb-todo` attribute and the first
/// one scrolled into view. The script gets only `data-jb` numbers, never text from the page, and
/// writes no markup. Each entry is one question: the control to outline first, then the other
/// options of the same question (the other buttons of a radio group). The mark goes away once any
/// of them has a value; a toggle button is watched through its `aria-pressed`, because the page
/// updates it after the click event has passed. A box listed in `untick` is flagged the other way
/// round: the page ticked it, and the mark goes away once he unticks that box.
const FLAG_JS: &str = r#"({ groups, untick }) => {
  const filled = (el) => {
    if (el.tagName === 'BUTTON') return el.getAttribute('aria-pressed') === 'true';
    const type = (el.getAttribute('type') || '').toLowerCase();
    if (type === 'checkbox' || type === 'radio') return el.checked;
    if (type === 'file') return el.files.length > 0;
    return el.value.trim() !== '';
  };
  let first = null;
  groups.forEach((indices) => {
    const members = indices
      .map((idx) => document.querySelector('[data-jb="' + idx + '"]'))
      .filter(Boolean);
    const el = members[0];
    if (!el) return;
    // A hidden control (a styled file picker) shows no outline: mark what Gonza can see.
    const box = el.getClientRects().length > 0 ? el : (el.closest('label') || el.parentElement || el);
    box.style.outline = '3px solid #d93025';
    box.style.outlineOffset = '2px';
    box.setAttribute('data-jb-todo', '1');
    first = first || box;
    const unticking = untick.includes(indices[0]);
    const clear = () => {
      if (unticking ? el.checked : !members.some(filled)) return;
      box.style.outline = '';
      box.style.outlineOffset = '';
      box.removeAttribute('data-jb-todo');
    };
    members.forEach((member) => {
      ['input', 'change'].forEach((event) => member.addEventListener(event, clear));
      if (member.tagName === 'BUTTON') {
        new MutationObserver(clear).observe(member, { attributes: true, attributeFilter: ['aria-pressed'] });
      }
    });
  });
  if (first) first.scrollIntoView({ block: 'center' });
  return first !== null;
}"#;

/// Best effort: a page that rejects the script, or has already re-rendered the form, just goes
/// without marks. `groups` as `flag_groups` builds them; `untick` are the pre-ticked boxes among
/// them (`FillPlan::to_untick`).
pub(super) async fn flag_for_gonza(page: &Page, groups: &[Vec<usize>], untick: &[usize]) {
    if groups.is_empty() {
        return;
    }
    let arg = serde_json::json!({ "groups": groups, "untick": untick });
    let _: Result<bool, _> = page.evaluate(FLAG_JS, Some(&arg)).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::form::plan_fields;
    use crate::form::support_tests::{bank_with_gaps, field, radio};
    use std::path::Path;

    #[test]
    fn the_assist_line_prints_the_postings_company_and_title_as_plain_text() {
        let job = Job {
            company: "Acme\u{1b}[2J\u{202e}Corp".into(),
            title: "Engineer\u{1b}]8;;https://evil.example\u{7}\nnext".into(),
            ..Job::default()
        };
        for line in [
            assist_banner(&job, None),
            assist_banner(&job, Some("Why\u{202e} us?\u{1b}[1A")),
        ] {
            assert!(
                !line
                    .chars()
                    .any(|c| c.is_control() || c == '\u{202e}' || c == '\u{1b}'),
                "{line:?}"
            );
            assert!(
                line.contains("Acme [2JCorp") && line.contains("Engineer"),
                "{line}"
            );
        }
    }

    #[test]
    fn a_label_split_over_two_lines_keeps_its_words_apart_in_the_assist_line() {
        let job = Job::default();
        let line = assist_banner(&job, Some("faltan respuestas: Phone\nNumber *"));
        assert!(line.contains("faltan respuestas: Phone Number *"), "{line}");
    }

    #[test]
    fn controls_left_are_the_required_ones_without_an_answer() {
        let fields = [
            field(0, "input", "text", "First name *", true), // answered
            field(1, "input", "email", "Email *", true),     // required, no value in the bank
            field(2, "input", "text", "Nickname", false),    // optional: never flagged
            radio(3, "remote", "Open to remote work?", "Yes", true),
            radio(4, "remote", "Open to remote work?", "No", true),
            radio(
                5,
                "sponsor",
                "Do you require visa sponsorship? *",
                "Yes",
                true,
            ),
            radio(
                6,
                "sponsor",
                "Do you require visa sponsorship? *",
                "No",
                true,
            ),
            field(
                7,
                "input",
                "checkbox",
                "I agree to the privacy policy",
                true,
            ),
            field(8, "input", "file", "Portfolio PDF", true),
            field(9, "input", "file", "Resume/CV *", true), // gets the CV
        ];
        let plan = plan_fields(&fields, &bank_with_gaps(), Path::new("cv.pdf"));
        // The unanswered radio group is flagged once, on its first button; the answered one
        // (sponsorship) and the optional field are left out.
        assert_eq!(controls_left(&fields, &plan, &[]), vec![1, 3, 7, 8]);
        // A control whose fill failed is left for Gonza too, once, in page order.
        assert_eq!(controls_left(&fields, &plan, &[0, 7]), vec![0, 1, 3, 7, 8]);
    }

    #[test]
    fn a_flagged_radio_group_is_watched_through_all_its_buttons() {
        let fields = [
            field(0, "input", "email", "Email *", true),
            radio(1, "remote", "Open to remote work?", "Yes", true),
            radio(2, "remote", "Open to remote work?", "No", true),
        ];
        // The control to outline leads its group; the mark goes away when any button is chosen.
        assert_eq!(flag_groups(&fields, &[0, 1]), vec![vec![0], vec![1, 2]]);
        // A button whose fill failed leads the group it belongs to.
        assert_eq!(flag_groups(&fields, &[2]), vec![vec![2, 1]]);
    }

    #[test]
    fn without_missing_answers_only_failed_controls_are_left() {
        let fields = [field(0, "input", "text", "First name *", true)];
        let plan = plan_fields(&fields, &bank_with_gaps(), Path::new("cv.pdf"));
        assert!(plan.missing.is_empty());
        assert_eq!(controls_left(&fields, &plan, &[]), Vec::<usize>::new());
        assert_eq!(controls_left(&fields, &plan, &[0]), vec![0]);
    }

    #[test]
    fn leftover_reason_lists_missing_answers_first_and_failures_after() {
        let missing = vec!["Email *".to_string(), "Why this role? *".to_string()];
        let failures = vec![
            "opción no encontrada: Country = Atlantis".to_string(),
            "no se pudo completar Phone: timeout".to_string(),
        ];
        assert_eq!(leftover_reason(&[], &[]), None);
        assert_eq!(
            leftover_reason(&missing, &[]).as_deref(),
            Some("faltan respuestas: Email *; Why this role? *")
        );
        assert_eq!(
            leftover_reason(&[], &failures[..1]).as_deref(),
            Some("opción no encontrada: Country = Atlantis")
        );
        assert_eq!(
            leftover_reason(&missing, &failures).as_deref(),
            Some(
                "faltan respuestas: Email *; Why this role? * — opción no encontrada: Country = Atlantis; no se pudo completar Phone: timeout"
            )
        );
    }

    #[test]
    fn assist_timeout_says_what_was_still_pending() {
        assert_eq!(
            assist_timeout_reason(10, None),
            "assist_timeout: sin confirmación en 10 minutos"
        );
        assert_eq!(
            assist_timeout_reason(10, Some("faltan respuestas: Email *")),
            "assist_timeout: sin confirmación en 10 minutos. Quedaba pendiente: faltan respuestas: Email *"
        );
    }

    #[test]
    fn assist_closed_points_to_confirm_and_says_what_was_pending() {
        let base = "assist_closed: cerraste la pestaña sin que la página confirmara; si sí la enviaste, `jobbot confirm KEY` la marca";
        assert_eq!(assist_closed_reason(None), base);
        assert_eq!(
            assist_closed_reason(Some("faltan respuestas: Email *")),
            format!("{base}. Quedaba pendiente: faltan respuestas: Email *")
        );
    }
}
