// The proposal in focus (spec §5.3): the facts, the title, the score with its bars, the path through the five layers, why it waits for his yes, its verbs, the key of
// the lote, the steps j and k, and the lote's drawn confirmation. Built once, patched after: a write only when it differs, and the armed press, the focus and the
// node he is pressing are the ones that were there.
//
//   const prop = createProp({ actions, reduced });
//   prop.el                        article.prop
//   prop.patch(view)               from the view of colaView: the proposal, its lote key, its steps and the confirmation
//   prop.focusView()               the proposal as a row (what a key or a button acts on), or null    prop.viewOf(id)  the same when its id is `id`, else null
//   prop.repaintRow(id, view)      paints its verbs and dim state again when `id` is the proposal's: true when it was
//   prop.repaintLote(view)         the lote opened or closed: its confirmation and the lote key; a keyboard left inside a closing confirmation goes back to the lote key
//   prop.inConfirm(node)  prop.focusConfirm()  prop.relabel()  prop.destroy()

import { h, setText } from "../../core/dom.js";
import { setData, setHidden, setAttr, nameOf } from "../shared/util.js";
import { patchActButtons } from "../shared/act-buttons.js";
import { actButton, setActKey, setActLabel } from "../../ui/act.js";
import { createFactors } from "./factors.js";
import { createConfirm } from "./confirm.js";
import { ENTER_MS } from "./enter.js";

const ACT_ARIA = "cola.act.aria"; // the sentence a button says to a screen reader (shared/act-buttons.js keeps the buttons)

export function createProp({ actions, reduced = () => false }) {
  let focus = null;                     // the row view of the proposal on screen
  let focusKey = null;
  let enterTimer = 0;
  const focusActs = { shape: "" };
  const factors = createFactors();
  const confirm = createConfirm({ actions });

  const parts = {
    facts: h("p", { class: "facts label" }),
    role: h("h2", { class: "role-title" }),
    score: h("span", { class: "score cond num" }),
    whyLabel: h("span", { class: "why-label label gold" }),
    whyText: h("span", { class: "why-text" }),
    acts: h("div", { class: "acts" }),
    batchBtn: actButton({ key: "", cls: "batch-btn", "data-action": "batch" }),
    step: h("span", { class: "step label" }),
    stepPrev: h("button", { class: "step-btn", type: "button", "data-step": "-1" }, h("kbd", { class: "k" }, "k")),
    stepNext: h("button", { class: "step-btn", type: "button", "data-step": "1" }, h("kbd", { class: "k" }, "j")),
    stepText: h("span", { class: "step-text num" }),
  };
  parts.step.append(parts.stepNext, parts.stepPrev, parts.stepText);
  const el = h("article", { class: "prop" },
    parts.facts,
    parts.role,
    h("div", { class: "score-row" }, parts.score, factors.bars),
    factors.path,
    h("p", { class: "why" }, parts.whyLabel, " ", parts.whyText),
    h("div", { class: "verbs" }, parts.acts, parts.batchBtn, parts.step),
    confirm.el);

  /** The key of the lote, in the same line of verbs: one key once. */
  function patchLoteKey(b) {
    setActKey(parts.batchBtn, b.key);
    setActLabel(parts.batchBtn, b.label);
    setData(parts.batchBtn, "none", b.can ? null : "true");
    setAttr(parts.batchBtn, "aria-disabled", !b.can);
    parts.batchBtn.classList.toggle("armed", actions.batch.isOpen());
  }

  return {
    el,
    patch(view) {
      const f = view.focus;
      el.hidden = !f;
      if (!f) { focus = null; focusKey = null; return; }
      const changed = focusKey !== null && focusKey !== f.key;
      focusKey = f.key;
      focus = { id: f.id, data: f.data, open: f.open, link: f.link, actions: f.actions, kindId: "focus" };
      setData(el, "row", f.id);
      setData(el, "card", f.key);
      setData(el, "pending", actions.isPending(f.id) ? "true" : null);
      setText(parts.facts, f.facts.join(" · "));
      setText(parts.role, f.title);
      setText(parts.score, f.score);
      setAttr(parts.score, "aria-label", f.scoreAria || null);
      factors.patchBars(f);
      factors.patchPath(f);
      setText(parts.whyLabel, f.why.label);
      setText(parts.whyText, f.why.text);
      patchActButtons(parts.acts, focusActs, f.actions, { armed: actions.armedOf(f.id), what: nameOf(f.company, f.title), ariaKey: ACT_ARIA });
      patchLoteKey(view.batch);
      setText(parts.stepText, f.step.label);
      setHidden(parts.step, !f.step.many);
      setAttr(parts.stepNext, "aria-label", f.step.next);
      setAttr(parts.stepPrev, "aria-label", f.step.prev);
      confirm.patch(view.batch);
      if (changed && !reduced()) {
        el.dataset.enter = "true";
        clearTimeout(enterTimer);
        enterTimer = setTimeout(() => { delete el.dataset.enter; }, ENTER_MS);
      }
    },
    focusView: () => focus,
    viewOf: (id) => (focus && focus.id === id ? focus : null),
    repaintRow(id, view) {
      if (!focus || focus.id !== id) return false;
      setData(el, "pending", actions.isPending(id) ? "true" : null);
      patchActButtons(parts.acts, focusActs, focus.actions, { armed: actions.armedOf(id), what: view.focus ? nameOf(view.focus.company, view.focus.title) : "", ariaKey: ACT_ARIA });
      return true;
    },
    repaintLote(view) {
      confirm.returnFocusTo(parts.batchBtn);
      confirm.patch(view.batch);
      parts.batchBtn.classList.toggle("armed", actions.batch.isOpen());
    },
    /** The drawn lote just opened: the keyboard goes to its confirm, so ↵ (or Space) is the second press and Esc the way out. */
    inConfirm: (node) => confirm.contains(node),
    focusConfirm: () => confirm.focusOk(),
    relabel: () => factors.relabel(),
    destroy() { clearTimeout(enterTimer); focus = null; },
  };
}
