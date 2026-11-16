//! Reading the form's controls from the page, and following them to their new numbers when the
//! answers open more questions.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use anyhow::Context;
use playwright_rs::Page;

use super::{FieldInfo, FillAction, FillPlan, POLL_MS};

/// How long a hosted board gets to render its form after `DOMContentLoaded`.
const FORM_WAIT_MS: u64 = 10_000;

/// Lists the form controls of the page, tagging each one with `data-jb="<idx>"` so the Rust side
/// can address it again. The script lives in `form/field_scan.js` (its header explains what each
/// field reports and how labels, groups and the required flag are worked out).
const FIELD_JS: &str = include_str!("field_scan.js");

/// True when `a` and `b` are the same control seen in two reads of the page: the same tag, kind,
/// label and option text (the numbers a read gives them change when controls appear).
fn same_control(a: &FieldInfo, b: &FieldInfo) -> bool {
    a.tag == b.tag
        && a.kind == b.kind
        && a.label.trim() == b.label.trim()
        && a.option.trim() == b.option.trim()
}

/// For each control of an earlier read, the number it has in a later one (`after`), matching equal
/// controls in page order. A control that is gone has no entry.
pub(super) fn carried_over(before: &[FieldInfo], after: &[FieldInfo]) -> HashMap<usize, usize> {
    let mut taken = vec![false; after.len()];
    let mut carried = HashMap::new();
    for old in before {
        let found = after
            .iter()
            .enumerate()
            .find(|(at, new)| !taken[*at] && same_control(old, new));
        if let Some((at, new)) = found {
            taken[at] = true;
            carried.insert(old.idx, new.idx);
        }
    }
    carried
}

/// The controls of `after` that no control of the earlier read was carried over to.
pub(super) fn new_controls(after: &[FieldInfo], carried: &HashMap<usize, usize>) -> Vec<FieldInfo> {
    let known: HashSet<usize> = carried.values().copied().collect();
    after
        .iter()
        .filter(|field| !known.contains(&field.idx))
        .cloned()
        .collect()
}

/// `plan` with its control numbers moved to the numbers of a later read; what is gone is dropped.
pub(super) fn renumbered(plan: FillPlan, carried: &HashMap<usize, usize>) -> FillPlan {
    let moved = |idx: &usize| carried.get(idx).copied();
    FillPlan {
        actions: plan
            .actions
            .into_iter()
            .filter_map(|action| {
                carried
                    .get(&action.idx)
                    .map(|&idx| FillAction { idx, ..action })
            })
            .collect(),
        missing: plan.missing,
        kept: plan.kept.iter().filter_map(moved).collect(),
        to_untick: plan.to_untick.iter().filter_map(moved).collect(),
    }
}

/// The plan for the controls that were there, plus the plan for the ones that appeared.
pub(super) fn merged(mut plan: FillPlan, extra: FillPlan) -> FillPlan {
    plan.actions.extend(extra.actions);
    for label in extra.missing {
        if !plan.missing.contains(&label) {
            plan.missing.push(label);
        }
    }
    plan.kept.extend(extra.kept);
    plan.to_untick.extend(extra.to_untick);
    plan
}

/// Scans the form, waiting up to `FORM_WAIT_MS` for controls to appear: Greenhouse's and Ashby's
/// hosted boards render the form from JavaScript after `DOMContentLoaded`, so an immediate scan
/// would report `no_form_found` on a page that is still loading.
pub(super) async fn scan_fields(page: &Page) -> anyhow::Result<Vec<FieldInfo>> {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(FORM_WAIT_MS);
    loop {
        let fields: Vec<FieldInfo> = page
            .evaluate(FIELD_JS, None::<&()>)
            .await
            .context("scanning the form fields")?;
        if !fields.is_empty() || tokio::time::Instant::now() >= deadline {
            return Ok(fields);
        }
        tokio::time::sleep(Duration::from_millis(POLL_MS)).await;
    }
}

/// One read of the form, without waiting for it to appear (see `scan_fields`).
pub(super) async fn scan_once(page: &Page) -> anyhow::Result<Vec<FieldInfo>> {
    page.evaluate(FIELD_JS, None::<&()>)
        .await
        .context("reading the form fields again")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::form::FillValue;
    use crate::form::support_tests::field;

    #[test]
    fn a_second_read_carries_the_old_controls_to_their_new_numbers() {
        let first = [
            field(0, "input", "text", "First name *", true),
            field(1, "input", "tel", "Phone", false),
            field(2, "input", "file", "Resume/CV *", true),
        ];
        // A question opened in the middle: everything below it moves down one number.
        let second = [
            field(0, "input", "text", "First name *", true),
            field(1, "textarea", "textarea", "Describe it *", true),
            field(2, "input", "tel", "Phone", false),
            field(3, "input", "file", "Resume/CV *", true),
        ];
        let carried = carried_over(&first, &second);
        assert_eq!(carried, HashMap::from([(0, 0), (1, 2), (2, 3)]));
        let appeared = new_controls(&second, &carried);
        assert_eq!(appeared.len(), 1);
        assert_eq!(appeared[0].label, "Describe it *");
        // Two controls with the same text are matched one by one, in page order.
        let twins = [
            field(0, "input", "text", "Reference", false),
            field(1, "input", "text", "Reference", false),
        ];
        let three = [
            field(0, "input", "text", "Reference", false),
            field(1, "input", "text", "Reference", false),
            field(2, "input", "text", "Reference", false),
        ];
        let carried = carried_over(&twins, &three);
        assert_eq!(carried, HashMap::from([(0, 0), (1, 1)]));
        assert_eq!(new_controls(&three, &carried)[0].idx, 2);
    }

    #[test]
    fn a_control_that_went_away_has_no_new_number() {
        let twins = [
            field(0, "input", "text", "Reference", false),
            field(1, "input", "text", "Reference", false),
        ];
        let three = [
            field(0, "input", "text", "Reference", false),
            field(1, "input", "text", "Reference", false),
            field(2, "input", "text", "Reference", false),
        ];
        assert_eq!(
            carried_over(&three, &twins),
            HashMap::from([(0, 0), (1, 1)])
        );
    }

    #[test]
    fn the_plan_follows_the_controls_to_their_new_numbers_and_takes_the_new_ones() {
        let plan = FillPlan {
            actions: vec![
                FillAction {
                    idx: 1,
                    label: "Phone".into(),
                    value: FillValue::Text("+56".into()),
                },
                FillAction {
                    idx: 5,
                    label: "Gone".into(),
                    value: FillValue::Check,
                },
            ],
            missing: vec!["Salary".into()],
            kept: vec![0, 5],
            to_untick: vec![1],
        };
        let carried = HashMap::from([(0, 0), (1, 2)]);
        let plan = renumbered(plan, &carried);
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].idx, 2);
        assert_eq!(plan.kept, vec![0]);
        assert_eq!(plan.to_untick, vec![2]);
        let extra = FillPlan {
            actions: vec![FillAction {
                idx: 1,
                label: "Describe it".into(),
                value: FillValue::Text("x".into()),
            }],
            missing: vec!["Salary".into(), "Describe it *".into()],
            ..FillPlan::default()
        };
        let plan = merged(plan, extra);
        assert_eq!(plan.actions.len(), 2);
        assert_eq!(plan.missing, vec!["Salary", "Describe it *"]);
    }
}
