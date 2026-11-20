// Draws SISTEMA (spec §5.5, board/relay2/sistema-1920.png) from the view of ./view.js. The page is BUILT ONCE and PATCHED after, like HOY:
// every text goes through setText (a write only when it differs), the lists are brought to the new order with `sync` (which moves no node
// that is already where it should be), so a refresh that changes nothing writes nothing and one that changes a number leaves the button,
// the focus and the cursor exactly where they were. Hierarchy, from the biggest down: the headline (when the engine last ran), two figures
// (the next run, the sends of 24 h; what Claude cost is said in the account's row) next to the night's window with its marks, then the instruments in two columns in the
// reading face (Inter) with their labels in mono, the limits as one band, and the keys of the page at the foot.
// This file only joins the blocks, one module each (top, engine, account, sources, errors, limits, foot), and listens for the click of the button, of "open" and of the funnel's "view by" switch.
//
//   const ui = createSistema({ run, open, funnelBy });
//   ui.el                    section.sistema  (host.append(ui.el))
//   ui.patch(view, state)    state = { pending, armed, foot }: what the STOP button is doing right now, and whether the foot of the page has room

import { h } from "../../core/dom.js";
import { setData } from "../shared/util.js";
import { createTopBlock } from "./top-block.js";
import { createEngineBlock } from "./engine-block.js";
import { createAccountBlock } from "./account-block.js";
import { createSourcesBlock } from "./sources-block.js";
import { createErrorsBlock } from "./errors-block.js";
import { createLimitsBlock } from "./limits-block.js";
import { createFootBlock } from "./foot-block.js";

export function createSistema({ run, open, funnelBy }) {
  const skeleton = () => h("div", { class: "row skeleton", "aria-hidden": "true" });

  // ---- the blocks: the top across the sheet, then two columns
  const top = createTopBlock();
  const engine = createEngineBlock();
  const account = createAccountBlock();
  const sources = createSourcesBlock();
  const errors = createErrorsBlock();
  const limits = createLimitsBlock();
  const foot = createFootBlock();

  const colLeft = h("div", { class: "col" }, engine.el, account.el, foot.el);
  const colRight = h("div", { class: "col" }, sources.el, errors.el, limits.el);
  // until the first snapshot: grey lines where the instruments will be (CSS hides the real columns meanwhile), never a spinner
  const loading = h("div", { class: "loading", "aria-hidden": "true" }, ...Array.from({ length: 6 }, skeleton));

  const el = h("section", { class: "sistema", "aria-labelledby": "sistema-title", "data-loading": "true", "aria-busy": "true" },
    ...top.els,
    loading,
    h("div", { class: "cols" }, colLeft, colRight));

  // ---- one delegated listener for the button and the "open" of an error: nothing is bound per row, so nothing is lost when a row is rebuilt
  el.addEventListener("click", (e) => {
    const target = e.target;
    const by = target.closest("button[data-funnel-by]");
    if (by) { funnelBy(by.dataset.funnelBy); return; }
    const act = target.closest("button[data-action]");
    if (act) { run(act.dataset.action); return; }
    const go = target.closest("button[data-open]");
    if (go) {
      const row = go.closest("[data-row-id]");
      open(go.dataset.open, row ? row.dataset.rowId : null);
    }
  });

  function patch(view, state = { pending: false, armed: false }) {
    setData(el, "nofoot", state.foot === false ? "true" : null);       // the keys of the page give way before a list does, when there is no room
    if (el.dataset.loading) { delete el.dataset.loading; el.removeAttribute("aria-busy"); loading.remove(); }
    setData(el, "engine", view.engine);
    setData(el, "stopped", view.chip.stop ? "true" : "false");
    top.patch(view);
    engine.patch(view, state);
    account.patch(view);
    sources.patch(view);
    errors.patch(view);
    limits.patch(view);
    foot.patch(view);
  }

  return {
    el,
    patch,
  };
}
