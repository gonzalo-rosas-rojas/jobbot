// The one delegated click listener of HOY: every button, row and link of the page goes through it, so nothing is bound per row and nothing is lost when a row is
// rebuilt.
//
//   bindClicks(el, { viewOf, run, more, unsnooze })
// `viewOf(rowId)` is the row view of that id (or null); `run(actionId, view)` does a verb on it ("open" for a click on the row itself); `more()` is the button
// that opens PROCESOS; `unsnooze(key)` takes a snoozed card back.

export function bindClicks(el, { viewOf, run, more, unsnooze }) {
  el.addEventListener("click", (e) => {
    const target = e.target;
    const button = target.closest("button[data-action]");
    if (button) {
      const rowEl = button.closest("[data-row]");
      const view = rowEl && viewOf(rowEl.dataset.row);
      if (view) run(button.dataset.action, view);
      return;
    }
    const back = target.closest("button[data-unsnooze]");
    if (back) { unsnooze(back.dataset.unsnooze); return; }
    if (target.closest("button[data-more]")) { more(); return; }
    const rowEl = target.closest("[data-row]");
    if (rowEl && !target.closest("a, button, summary")) {
      const view = viewOf(rowEl.dataset.row);
      if (view) run("open", view);
    }
  });
}
