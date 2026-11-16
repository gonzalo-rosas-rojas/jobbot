//! Choosing an option in a list: a native `<select>` or a drop-down the page draws by hand.

use std::time::Duration;

use playwright_rs::{ClickOptions, Page, SelectOption, SelectOptions};

use super::{NoOption, POLL_MS, best_option, exact_option, pick_one, search_text};

/// A drop-down list is settled once it shows options and stopped changing for a moment. An empty
/// list never is: a typeahead that answers late shows nothing at first, and that is not "no
/// options". The wait for an empty list ends only with `OPTIONS_MAX_MS`.
fn menu_settled(listed: usize, quiet: Duration) -> bool {
    listed > 0 && quiet >= Duration::from_millis(OPTIONS_SETTLE_MS)
}

const SELECT_TIMEOUT_MS: f64 = 3_000.0;

/// What kind of control `locator` is: `select`, `combobox` or `text`.
const CONTROL_KIND_JS: &str = r#"(el) => el.tagName === 'SELECT' ? 'select' : (el.getAttribute('role') === 'combobox' ? 'combobox' : 'text')"#;

/// The visible options of the open menu of a combobox: the listbox the input points at with
/// `aria-controls` when it does (the page may also hold hidden lists, such as a phone country
/// picker), any visible `role="option"` otherwise.
const VISIBLE_OPTIONS_JS: &str = r#"(controls) => {
  const root = controls ? document.getElementById(controls) : null;
  return Array.from((root || document).querySelectorAll('[role="option"]'))
    .filter((o) => o.getClientRects().length > 0)
    .map((o) => (o.innerText || o.textContent || '').replace(/\s+/g, ' ').trim());
}"#;

/// Tags the `index`-th visible option (same scope as `VISIBLE_OPTIONS_JS`) with `data-jb-opt`, but
/// only if its text is `arg.text`, the option that was judged (read the same way as
/// `VISIBLE_OPTIONS_JS`). A list redrawn between the reading and this call would otherwise tag
/// another option, and the click would answer with it. False when nothing is tagged.
pub(super) const TAG_OPTION_JS: &str = r#"(arg) => {
  document.querySelectorAll('[data-jb-opt]').forEach((el) => el.removeAttribute('data-jb-opt'));
  const root = arg.controls ? document.getElementById(arg.controls) : null;
  const options = Array.from((root || document).querySelectorAll('[role="option"]'))
    .filter((o) => o.getClientRects().length > 0);
  const hit = options[arg.index];
  if (!hit) return false;
  const text = (hit.innerText || hit.textContent || '').replace(/\s+/g, ' ').trim();
  if (text !== arg.text) return false;
  hit.setAttribute('data-jb-opt', '1');
  return true;
}"#;

/// How long a drop-down list may keep changing (a school list fetched while typing) before the
/// filler decides from what it shows, and the most it waits in all.
const OPTIONS_SETTLE_MS: u64 = 900;
const OPTIONS_MAX_MS: u64 = 4_000;

/// The option labels of a native `<select>`, in order (the label attribute, else the text: what
/// Playwright matches by label).
const SELECT_OPTIONS_JS: &str = r#"(el) => Array.from(el.options).map((o) => (o.label || o.text || '').replace(/\s+/g, ' ').trim())"#;

/// How long a combobox that was just opened gets to show its whole list before it is typed into.
const PEEK_MS: u64 = 400;

/// Gives `candidates` (the bank value, then its alternatives) to the control in `locator`.
pub(super) async fn choose(
    page: &Page,
    locator: &playwright_rs::Locator,
    candidates: &[String],
) -> anyhow::Result<()> {
    let kind: String = locator.evaluate(CONTROL_KIND_JS, None::<()>).await?;
    match kind.as_str() {
        "select" => {
            // The same matching as every other list (case, punctuation and place rules, the bare
            // yes/no rule, the Hispanic rule), then the option it names is selected by position:
            // "Job board" finds the page's "Job Board".
            let labels: Vec<String> = locator.evaluate(SELECT_OPTIONS_JS, None::<()>).await?;
            let index = pick_one(&labels, candidates).map_err(anyhow::Error::new)?;
            let options = SelectOptions::builder().timeout(SELECT_TIMEOUT_MS).build();
            locator
                .select_option(SelectOption::Index(index), options)
                .await?;
            Ok(())
        }
        "combobox" => pick_option(page, locator, candidates).await,
        _ => match candidates.first() {
            Some(first) => Ok(locator.fill(first, None).await?),
            None => Ok(()),
        },
    }
}

/// The visible options of a combobox that has just been opened: waits up to `PEEK_MS` for the list
/// to show something (a typeahead shows nothing until typed into, and then this is empty).
async fn peek_options(page: &Page, controls: &str) -> anyhow::Result<Vec<String>> {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(PEEK_MS);
    loop {
        let options: Vec<String> = page.evaluate(VISIBLE_OPTIONS_JS, Some(&controls)).await?;
        if !options.is_empty() || tokio::time::Instant::now() >= deadline {
            return Ok(options);
        }
        tokio::time::sleep(Duration::from_millis(POLL_MS / 8)).await;
    }
}

/// Opens a custom drop-down, types each candidate in turn to filter (or load) its list and clicks
/// the option that is the candidate (or starts with it as a whole word), never one that claims
/// "(Not Hispanic or Latino)". The whole list is read before anything is typed (typing filters it
/// down): a list that offers Hispanic or Latino is never answered, however it was filtered.
/// Errors with `NoOption` when no candidate matches any option or the list is reserved; the
/// control is then left closed and empty.
async fn pick_option(
    page: &Page,
    locator: &playwright_rs::Locator,
    candidates: &[String],
) -> anyhow::Result<()> {
    for (round, candidate) in candidates.iter().enumerate() {
        locator.click(None).await?;
        if round == 0 {
            let opened = locator
                .get_attribute("aria-controls")
                .await?
                .unwrap_or_default();
            let whole = peek_options(page, &opened).await?;
            if !whole.is_empty() && pick_one(&whole, candidates) == Err(NoOption::Reserved) {
                locator.press("Escape", None).await?;
                return Err(NoOption::Reserved.into());
            }
        }
        locator
            .press_sequentially(search_text(candidate), None)
            .await?;
        let controls = locator
            .get_attribute("aria-controls")
            .await?
            .unwrap_or_default();
        if click_match(page, &controls, candidate).await? {
            return Ok(());
        }
        // Nothing for this candidate: clear what was typed and close the list before the next.
        locator.fill("", None).await?;
        locator.press("Escape", None).await?;
    }
    Err(NoOption::NoMatch.into())
}

/// How long the click on a chosen option may take, and how many times the option is chosen again.
/// A list can redraw between being read and being clicked (the late answer to an earlier key
/// replaces its nodes); the tagged node is then gone and the click would wait for it in vain.
const OPTION_CLICK_MS: f64 = 2_000.0;
const OPTION_ATTEMPTS: usize = 3;

/// Clicks the option for `wanted` once the menu settles, choosing it again from the redrawn list
/// when the click times out. False when the menu has no option for `wanted`.
async fn click_match(page: &Page, controls: &str, wanted: &str) -> anyhow::Result<bool> {
    for attempt in 1..=OPTION_ATTEMPTS {
        let Some((index, text)) = settled_match(page, controls, wanted).await? else {
            return Ok(false);
        };
        let arg = serde_json::json!({ "controls": controls, "index": index, "text": text });
        let tagged: bool = page.evaluate(TAG_OPTION_JS, Some(&arg)).await?;
        if !tagged {
            // The list changed since it was read: read it again and decide again.
            continue;
        }
        let click = ClickOptions::builder().timeout(OPTION_CLICK_MS).build();
        match page
            .locator(r#"[data-jb-opt="1"]"#)
            .click(Some(click))
            .await
        {
            Ok(()) => return Ok(true),
            Err(playwright_rs::Error::Timeout(_)) if attempt < OPTION_ATTEMPTS => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(false)
}

/// Waits for the menu to settle (an exact match ends the wait at once) and returns the index and
/// the text of the option for `wanted`: an exact one if there is one, otherwise the first that
/// starts with it. The text goes with the index so that the click can check it is still that option.
/// An empty list keeps being waited for up to `OPTIONS_MAX_MS` (see `menu_settled`).
async fn settled_match(
    page: &Page,
    controls: &str,
    wanted: &str,
) -> anyhow::Result<Option<(usize, String)>> {
    let start = tokio::time::Instant::now();
    let mut last: Vec<String> = Vec::new();
    let mut last_change = start;
    loop {
        let options: Vec<String> = page.evaluate(VISIBLE_OPTIONS_JS, Some(&controls)).await?;
        if options != last {
            last = options;
            last_change = tokio::time::Instant::now();
        }
        let judged = |index: Option<usize>| index.map(|index| (index, last[index].clone()));
        let exact = exact_option(&last, wanted);
        if exact.is_some() {
            return Ok(judged(exact));
        }
        let now = tokio::time::Instant::now();
        if menu_settled(last.len(), now.duration_since(last_change))
            || now.duration_since(start) >= Duration::from_millis(OPTIONS_MAX_MS)
        {
            return Ok(judged(best_option(&last, wanted)));
        }
        tokio::time::sleep(Duration::from_millis(POLL_MS / 3)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_drop_down_list_is_never_settled_before_the_most_it_waits() {
        let quiet = |ms: u64| Duration::from_millis(ms);
        // A typeahead that answers late shows nothing for a while: that is not "no options".
        assert!(!menu_settled(0, quiet(OPTIONS_SETTLE_MS + 100)));
        assert!(!menu_settled(0, quiet(OPTIONS_MAX_MS)));
        // A list with options is settled once it stopped changing for a moment.
        assert!(!menu_settled(3, quiet(OPTIONS_SETTLE_MS - 100)));
        assert!(menu_settled(3, quiet(OPTIONS_SETTLE_MS)));
    }
}
