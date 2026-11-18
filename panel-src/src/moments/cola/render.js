// Draws COLA (spec §5.3, board/relay2/cola-1920.png) from the view of ./view.js. The page is BUILT ONCE and PATCHED after: every text goes
// through setText (a write only when it differs), the rows of the queue are keyed by their id and kept in place, the lists are brought
// to the new order with `sync`, and a part whose view did not change is not touched at all. A refresh that changes nothing writes
// nothing, and one that changes a number leaves the buttons, the focus and the armed press exactly as they were.
//
// This file joins the parts and owns the one delegated listener; each part draws itself:
//   head.js     the eyebrow, the headline and the engine's warnings         prop.js     the proposal in focus (factors.js: its bars and path; confirm.js: the lote's yes)
//   funnel.js   the five figures and the band                              queue.js    the rows under it
//   trust.js    the squares and the meter                                  foot.js     the engine's switch
//
//   const cola = createCola({ actions, shot, on });
//   cola.el                       section.cola  (host.append(cola.el))
//   cola.patch(view)              the whole page from a view of colaView
//   cola.rowView(id)              the row of a proposal or a queue row by its id (what a key or a button acts on), or null
//   cola.focusView()              the proposal in focus as a row, or null
//   cola.repaint(id)              paints one row, the lote's confirmation ("batch") or the engine's switch ("engine") again
//   cola.destroy()
// `on` are the handlers of what the page itself offers: run(actionId, row), step(dir), focus(key), batchOpen(), batchConfirm(),
// batchCancel(), engine(), retryDetail().

import { h } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { setData, setHidden } from "../shared/util.js";
import { ENGINE_REF } from "../shared/engine.js";
import { createHead } from "./head.js";
import { createProp } from "./prop.js";
import { createFunnel } from "./funnel.js";
import { createQueue } from "./queue.js";
import { createTrust } from "./trust.js";
import { createFoot } from "./foot.js";

export function createCola({ actions, shot, on, reduced = () => false }) {
  let lastView = null;
  let built = false;                    // false until the first patch: what is there at the start does not animate

  const head = createHead();
  const prop = createProp({ actions, reduced });
  const funnel = createFunnel();
  const queue = createQueue({ actions, reduced });
  const trust = createTrust();
  const foot = createFoot({ actions });
  const skeleton = h("div", { class: "skeleton", role: "status" }, h("span", { class: "sr" }, t("cola.loading")), h("div", { class: "skel-line wide", "aria-hidden": "true" }), h("div", { class: "skel-line", "aria-hidden": "true" }), h("div", { class: "skel-line", "aria-hidden": "true" }), h("div", { class: "skel-line", "aria-hidden": "true" }));

  const el = h("section", { class: "cola", "aria-labelledby": "cola-title", "data-state": "loading" },
    ...head.els,
    prop.el,
    skeleton,
    funnel.el,
    queue.el,
    trust.el,
    foot.el,
    shot.el);

  const viewOf = (id) => prop.viewOf(id) || queue.viewOf(id);

  // ---- one delegated listener for every button and row of the page: nothing is bound per row, so nothing is lost when a row is rebuilt
  el.addEventListener("click", (e) => {
    const target = e.target;
    const step = target.closest("button[data-step]");
    if (step) { on.step(Number(step.dataset.step)); return; }
    const sure = target.closest("button[data-batch]");
    if (sure) { if (sure.dataset.batch === "confirm") on.batchConfirm(); else on.batchCancel(); return; }
    if (target.closest("button[data-engine]")) { on.engine(); return; }
    if (target.closest("button[data-retry]")) { on.retryDetail(); return; }
    const actionButton = target.closest("button[data-action]");
    if (actionButton) {
      if (actionButton.dataset.action === "batch") { on.batchOpen(); return; }
      const rowEl = actionButton.closest("[data-row]");
      const view = rowEl && viewOf(rowEl.dataset.row);
      if (view) on.run(actionButton.dataset.action, view);
      return;
    }
    const rowEl = target.closest(".q-row[data-row]");
    if (rowEl && !target.closest("a, button, summary")) {
      const view = viewOf(rowEl.dataset.row);
      if (!view) return;
      if (view.kindId === "pending") on.focus(view.data.card); // a proposal that waits is looked at in full first
      else on.run("open", view);
    }
  });

  function patch(view) {
    lastView = view;
    prop.relabel();
    funnel.relabel();
    setData(el, "state", view.empty ? "empty" : "ready");
    head.patch(view);
    setHidden(skeleton, true);
    prop.patch(view);
    funnel.patch(view);
    queue.patch(view, { animate: built });
    trust.patch(view);
    foot.patch(view);
    const f = view.focus;
    built = true;
    shot.show(f ? { key: f.hasShot ? f.shotKey : null, preparedAt: f.preparedAt } : null);
  }

  return {
    el,
    patch,
    rowView: viewOf,
    focusView: prop.focusView,
    inConfirm: prop.inConfirm,
    focusConfirm: prop.focusConfirm,
    repaint(id) {
      if (!lastView) return;
      if (id === "batch") { prop.repaintLote(lastView); return; }
      if (id === ENGINE_REF) { foot.patch(lastView); return; }
      if (prop.repaintRow(id, lastView)) return;
      queue.repaint(id);
    },
    destroy() { prop.destroy(); queue.destroy(); shot.destroy(); },
  };
}
