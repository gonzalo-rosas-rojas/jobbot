// One row of HOY: LO SIGUIENTE (createNextRow, three buttons) or a line of "Y después" (createLineRow, one button). A row is built once
// per item and PATCHED when its view changes: only the parts that differ are written, and a row whose view and state are the same
// as last time is not touched at all. That is what lets a refresh every 30 s land under a button he has half-pressed (armed,
// focused, hovered) without taking it away from him.
//
//   row.el        the element (li.row, or div.next)       row.view   the last view it was patched with
//   row.patch(view, state)   state = { armed: actionId | null, pending: bool }  -> true when something was written
//   data-card / data-task    the key of what it is (the cursor and the keys read it)       data-row   the id of the view
//   data-actions             every verb the row accepts, in key-table order, even the ones with no button
//   data-pending, data-armed the state that dims the row and says "press again"

import { h, setText } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { setData, setHidden, setTitle, setBusy, nameOf } from "../shared/util.js";
import { opens } from "../shared/opens.js";
import { patchActButtons } from "../shared/act-buttons.js";

const signature = (view, state) => JSON.stringify([
  view.kind, view.company, view.text, view.why, view.instrument, view.open, view.link, view.tone, view.kindId,
  view.actions.map((a) => [a.id, a.key, a.label, a.primary, a.visible]),
  state.armed, state.pending,
]);

/** The parts every row has, as empty nodes: the two shapes put them together in their own way. */
function rowParts() {
  return {
    label: h("span", { class: "gold" }),
    kind: h("span", { class: "kind label" }),
    co: h("strong", { class: "co" }),
    role: h("span", { class: "role" }),
    why: h("span", { class: "why" }),
    meta: h("span", { class: "meta label num" }),
    acts: h("span", { class: "acts" }),
  };
}

/** LO SIGUIENTE: a headline of its own, the reason under it and the three buttons. */
function nextSkeleton() {
  const parts = rowParts();
  const el = h("div", { class: "next", tabindex: "-1" },
    h("p", { class: "next-eyebrow label" }, parts.label, parts.kind, parts.meta),
    h("h2", { class: "next-title" }, parts.co, parts.role),
    h("p", { class: "next-sub" }, parts.why),
    parts.acts);
  return { el, parts };
}

/** A line of "Y después": only a line wraps its text in one box. */
function lineSkeleton() {
  const parts = rowParts();
  parts.what = h("span", { class: "what" }, parts.co, parts.role, parts.why);
  const el = h("li", { class: "row", tabindex: "-1" }, parts.kind, parts.what, parts.meta, parts.acts);
  return { el, parts };
}

/** LO SIGUIENTE as a row: { el, view, patch(view, state), setFresh(on) }. The identity of a row (its key) never changes: a different item is a different row. */
export function createNextRow(first) {
  return buildRow(first, nextSkeleton(), ({ parts }) => setText(parts.label, t("hoy.next.label")));
}

/** A line of "Y después" as a row: same contract as createNextRow. */
export function createLineRow(first) {
  // a line is cut at the edge of its box: the whole of it is one hover away
  return buildRow(first, lineSkeleton(), ({ parts }, view) => setTitle(parts.what, [view.company, view.text, view.why].filter(Boolean).join(" · ")));
}

/** The row both shapes share; `patchOwn(skeleton, view)` writes what only that shape has. */
function buildRow(first, { el, parts }, patchOwn) {
  if (first.data.card) el.dataset.card = first.data.card;
  else el.dataset.task = first.data.task;
  el.dataset.row = first.id;
  let sig = "";
  const buttons = { shape: "" };
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
    setBusy(el, state.pending);
    setData(el, "armed", state.armed);
    setText(parts.kind, view.kind);
    setText(parts.co, view.company || "");
    setHidden(parts.co, !view.company);
    setText(parts.role, view.text);
    setText(parts.why, view.why || "");
    setHidden(parts.why, !view.why);
    setText(parts.meta, view.instrument);
    setHidden(parts.meta, !view.instrument);
    patchOwn({ parts }, view);
    patchActButtons(parts.acts, buttons, view.actions, { armed: state.armed, what: nameOf(view.company, view.text), ariaKey: "hoy.act.aria" });
    return true;
  }

  patch(first);
  return row;
}
