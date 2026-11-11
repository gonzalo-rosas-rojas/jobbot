// One row of HOY: LO SIGUIENTE (shape "next", three buttons) or a line of "Y después" (shape "row", one button). A row is built once
// per item and PATCHED when its view changes: only the parts that differ are written, and a row whose view and state are the same
// as last time is not touched at all. That is what lets a refresh every 30 s land under a button he has half-pressed (armed,
// focused, hovered) without taking it away from him.
//
//   row.el        the element (li.row, or div.next)       row.view   the last view it was patched with
//   row.patch(view, state)   state = { armed: actionId | null, pending: bool }  -> true when something was written
//   data-card / data-task    the key of what it is (the cursor and the keys read it)       data-row   the id of the view
//   data-actions             every verb the row accepts, in key-table order, even the ones with no button
//   data-pending, data-armed the state that dims the row and says "press again"

import { h } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { setText, setData, setHidden, setTitle } from "../shared/util.js";
import { opens } from "../shared/opens.js";
import { actButton } from "../../ui/act.js";

const signature = (view, state) => JSON.stringify([
  view.kind, view.company, view.text, view.why, view.instrument, view.open, view.link, view.tone, view.kindId,
  view.actions.map((a) => [a.id, a.key, a.label, a.primary, a.visible]),
  state.armed, state.pending,
]);

const button = (action) => actButton({ key: action.key, label: action.label, primary: action.primary, "data-action": action.id });

function skeleton(shape) {
  const parts = {
    what: h("span", { class: "what" }),
    label: h("span", { class: "gold" }),
    kind: h("span", { class: "kind label" }),
    co: h("strong", { class: "co" }),
    role: h("span", { class: "role" }),
    why: h("span", { class: "why" }),
    meta: h("span", { class: "meta label num" }),
    acts: h("span", { class: "acts" }),
  };
  const el = shape === "next"
    ? h("div", { class: "next", tabindex: "-1" },
      h("p", { class: "next-eyebrow label" }, parts.label, parts.kind, parts.meta),
      h("h2", { class: "next-title" }, parts.co, parts.role),
      h("p", { class: "next-sub" }, parts.why),
      parts.acts)
    : h("li", { class: "row", tabindex: "-1" }, parts.kind, parts.what, parts.meta, parts.acts);
  if (shape !== "next") parts.what.append(parts.co, parts.role, parts.why);
  return { el, parts };
}

/** createRow(view, shape) -> { el, view, patch(view, state), setFresh(on) }. The identity of a row (its key) never changes: a different item is a different row. */
export function createRow(first, shape) {
  const { el, parts } = skeleton(shape);
  if (first.data.card) el.dataset.card = first.data.card;
  else el.dataset.task = first.data.task;
  el.dataset.row = first.id;
  let sig = "";
  let structure = "";
  const row = { el, view: first, patch, setFresh: (on) => setData(el, "fresh", on ? "true" : null) };

  function patch(view, state = { armed: null, pending: false }) {
    const next = signature(view, state);
    if (next === sig) return false;
    sig = next;
    row.view = view;
    setData(el, "tone", view.tone);
    setData(el, "kind", view.kindId);
    setData(el, "actions", view.actions.map((a) => a.id).join(","));
    setData(el, "opens", opens(view) ? "true" : null);   // a click on the row opens it: the hand and the underline say so
    setData(el, "pending", state.pending ? "true" : null);
    if (state.pending) { if (el.getAttribute("aria-busy") !== "true") el.setAttribute("aria-busy", "true"); } else if (el.hasAttribute("aria-busy")) el.removeAttribute("aria-busy");
    setData(el, "armed", state.armed);
    if (shape === "next") setText(parts.label, t("hoy.next.label"));
    setText(parts.kind, view.kind);
    setText(parts.co, view.company || "");
    setHidden(parts.co, !view.company);
    setText(parts.role, view.text);
    setText(parts.why, view.why || "");
    setHidden(parts.why, !view.why);
    setText(parts.meta, view.instrument);
    setHidden(parts.meta, !view.instrument);
    // a line of "Y después" is cut at the edge of its box: the whole of it is one hover away
    if (shape !== "next") setTitle(parts.what, [view.company, view.text, view.why].filter(Boolean).join(" · "));

    // The buttons: rebuilt only when the SET of buttons changed (a verb arrived or left, a label changed with the language);
    // for an armed press only the text of that one button changes, so the node he is pressing is the same node.
    const shown = view.actions.filter((a) => a.visible);
    const shape2 = JSON.stringify(shown.map((a) => [a.id, a.key, a.label, a.primary]));
    if (shape2 !== structure) {
      structure = shape2;
      const focused = parts.acts.contains(document.activeElement) ? document.activeElement.dataset.action : null;
      parts.acts.replaceChildren(...shown.map(button));
      if (focused) { const again = parts.acts.querySelector(`[data-action="${focused}"]`); if (again) again.focus({ preventScroll: true }); }
    }
    const what = [view.company, view.text].filter(Boolean).join(" · ");
    for (const b of parts.acts.children) {
      const action = shown.find((a) => a.id === b.dataset.action);
      const armed = state.armed === action.id;
      const said = armed ? t("act.sure") : action.label; // the name a screen reader gives is the one the eyes read
      setText(b.lastChild, said);
      setData(b, "armed", armed ? "true" : null);
      const aria = t("hoy.act.aria", { action: said, what });
      if (b.getAttribute("aria-label") !== aria) b.setAttribute("aria-label", aria);
    }
    return true;
  }

  patch(first);
  return row;
}
