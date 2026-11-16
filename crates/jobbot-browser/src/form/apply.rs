//! Applying a fill plan to the controls of the page, one action at a time.

use std::time::Duration;

use playwright_rs::Page;

use super::dropdown::choose;
use super::leftover::{first_line, plain};
use super::{FieldInfo, FillAction, FillValue, NoOption};

/// True when a failed fill only means "this list has no option for the answer" and the field is
/// optional: the field is skipped and left as it was. A required field keeps failing loudly, and
/// so does anything that is not a missing option (a page that did not answer).
fn is_skippable(err: &anyhow::Error, required: bool) -> bool {
    !required && err.downcast_ref::<NoOption>().is_some()
}

/// Does every action; one that fails (an option the form does not have, a control that refuses the
/// value) must not hide the others, so every action is tried and the failures are reported
/// together. An OPTIONAL field whose list has no option for the answer is not a failure: it is
/// skipped and stays as the page had it. `fields` are the controls numbered as the actions are.
pub(super) async fn apply_all(
    page: &Page,
    actions: &[FillAction],
    fields: &[FieldInfo],
    failures: &mut Vec<String>,
    failed: &mut Vec<usize>,
) {
    for action in actions {
        if let Err(err) = apply_action(page, action).await {
            let required = fields
                .iter()
                .any(|field| field.idx == action.idx && field.required);
            if is_skippable(&err, required) {
                eprintln!(
                    "[omitido] {}: {}; es opcional y queda como estaba",
                    plain(&action.label),
                    skip_note(&err)
                );
                continue;
            }
            failures.push(failure_reason(action, &err));
            failed.push(action.idx);
        }
    }
}

/// Performs one planned action on the control tagged `data-jb`. A choice (`Select`) or a text
/// answer goes to whatever the control turns out to be: a native `<select>` takes the first
/// option label that exists, a custom drop-down (an input with `role="combobox"`, Greenhouse's
/// react-select) is opened and an option is clicked, and a plain input is simply filled.
pub(super) async fn apply_action(page: &Page, action: &FillAction) -> anyhow::Result<()> {
    let locator = page.locator(format!(r#"[data-jb="{}"]"#, action.idx));
    match &action.value {
        FillValue::File(path) => locator.set_input_files(path, None).await?,
        FillValue::Select(candidates) => choose(page, &locator, candidates).await?,
        FillValue::Text(text) => choose(page, &locator, std::slice::from_ref(text)).await?,
        FillValue::Check => select_option(&locator).await?,
    }
    Ok(())
}

/// True for the toggle buttons Ashby draws its Yes/No questions with.
const IS_BUTTON_JS: &str = r#"(el) => el.tagName === 'BUTTON'"#;
const PRESSED_JS: &str = r#"(el) => el.getAttribute('aria-pressed') === 'true'"#;
/// How long a toggle button gets to show it is pressed after the click (React redraws it a moment
/// later).
const PRESS_SETTLE_MS: u64 = 1_000;
/// How often a toggle button is asked whether it shows as pressed while it settles.
const TOGGLE_POLL_MS: u64 = 100;

/// Selects the option behind `locator`. A radio button or a checkbox is checked. A toggle button
/// is clicked unless it is already pressed (a second click may release it again) and must show as
/// pressed afterwards: a click the page ignored is a failure, not a filled field.
async fn select_option(locator: &playwright_rs::Locator) -> anyhow::Result<()> {
    let is_button: bool = locator.evaluate(IS_BUTTON_JS, None::<()>).await?;
    if !is_button {
        locator.check(None).await?;
        return Ok(());
    }
    let pressed = || async { locator.evaluate::<bool, ()>(PRESSED_JS, None).await };
    if pressed().await? {
        return Ok(());
    }
    locator.click(None).await?;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(PRESS_SETTLE_MS);
    while !pressed().await? {
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("el botón no quedó seleccionado");
        }
        tokio::time::sleep(Duration::from_millis(TOGGLE_POLL_MS)).await;
    }
    Ok(())
}

/// What a skipped optional field says on the terminal.
fn skip_note(err: &anyhow::Error) -> &'static str {
    match err.downcast_ref::<NoOption>() {
        Some(NoOption::Reserved) => "la lista ofrece Hispanic o Latino: esa respuesta es tuya",
        _ => "ninguna opción de la lista coincide con la respuesta",
    }
}

fn failure_reason(action: &FillAction, err: &anyhow::Error) -> String {
    if err.downcast_ref::<NoOption>() == Some(&NoOption::Reserved) {
        return format!(
            "pregunta reservada para ti (la lista ofrece Hispanic o Latino): {}",
            action.label
        );
    }
    match &action.value {
        FillValue::Select(candidates) => {
            format!(
                "opción no encontrada: {} = {}",
                action.label,
                candidates.join(" | ")
            )
        }
        _ => format!("no se pudo completar {}: {}", action.label, first_line(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_pick_of_an_optional_field_is_a_skip_and_a_required_one_a_failure() {
        let no_match = anyhow::Error::new(NoOption::NoMatch);
        let reserved = anyhow::Error::new(NoOption::Reserved);
        let broken = anyhow::anyhow!("Timeout 10000ms exceeded");
        assert!(is_skippable(&no_match, false));
        assert!(is_skippable(&reserved, false));
        assert!(!is_skippable(&no_match, true));
        assert!(!is_skippable(&reserved, true));
        // A page that did not answer is a failure whatever the field.
        assert!(!is_skippable(&broken, false));
        assert!(!is_skippable(&broken, true));
    }
}
