// COLA's queue (spec §5.3): the proposals that wait, the approved ones, the ones blocked by answers and what went out, as rows. A row is built once per
// id and PATCHED when its view or its state (armed, pending) changes; the list is brought to the new order with `sync`, which moves no node that is already
// where it should be.
//
//   const queue = createQueue({ actions, reduced });
//   queue.el                         section.queue
//   queue.patch(view, { animate })   `animate`: false for the first drawing (what is there at the start does not slide in)
//   queue.viewOf(id)                 the row view of a queue row by its id, or null
//   queue.repaint(id)                paints one row again (its armed or pending state changed)
//   queue.destroy()

import { h, setText } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { sync, setData, setHidden, setTitle, setBusy, nameOf } from "../shared/util.js";
import { opens } from "../shared/opens.js";
import { patchActButtons } from "../shared/act-buttons.js";
import { ENTER_MS } from "./enter.js";

const ACT_ARIA = "cola.act.aria"; // the sentence a button says to a screen reader (shared/act-buttons.js keeps the buttons)

function createQueueRow(first) {
  const parts = {
    kind: h("span", { class: "q-kind label" }),
    co: h("strong", { class: "co" }),
    text: h("span", { class: "role" }),
    meta: h("span", { class: "meta label num" }),
    acts: h("span", { class: "acts" }),
  };
  const el = h("li", { class: "q-row", "data-row": first.id, "data-kind": first.kindId }, parts.kind, h("span", { class: "what" }, parts.co, parts.text), parts.meta, parts.acts);
  if (first.data.card) el.dataset.card = first.data.card;
  let sig = "";
  const buttons = { shape: "" };
  const row = { el, view: first, patch };
  function patch(view, st = { armed: null, pending: false }) {
    row.view = view; // what a button acts on is always the latest view, also when nothing the signature reads changed (the link, say)
    const next = JSON.stringify([view.kind, view.company, view.text, view.meta, view.actions.map((a) => [a.id, a.key, a.label, a.visible]), st.armed, st.pending]);
    if (next === sig) return false;
    sig = next;
    // a click on the row looks at a proposal in full, or opens what it is: the hand and the underline say so
    setData(el, "opens", view.kindId === "pending" || opens(view) ? "true" : null);
    setData(el, "pending", st.pending ? "true" : null);
    setBusy(el, st.pending);
    setText(parts.kind, view.kind);
    setText(parts.co, view.company || "");
    setHidden(parts.co, !view.company);
    setText(parts.text, view.text);
    setText(parts.meta, view.meta);
    setHidden(parts.meta, !view.meta);
    const name = nameOf(view.company, view.text);
    setTitle(parts.text.parentElement, name);
    patchActButtons(parts.acts, buttons, view.actions, { armed: st.armed, what: name, ariaKey: ACT_ARIA });
    return true;
  }
  patch(first);
  return row;
}

export function createQueue({ actions, reduced = () => false }) {
  const rowCtls = new Map();            // queue rows by id
  const enterTimers = new Set();
  const parts = {
    title: h("h2", { class: "block-title label" }),
    list: h("ul", { class: "q-rows", "data-lenis-prevent": "" }),
    none: h("p", { class: "none label" }),
    more: h("p", { class: "q-more label" }),
  };
  const el = h("section", { class: "queue" }, parts.title, parts.list, parts.more, parts.none);
  const stateOf = (id) => ({ armed: actions.armedOf(id), pending: actions.isPending(id) });

  function enterRow(row) {
    if (reduced()) return;
    row.dataset.enter = "true";
    const timer = setTimeout(() => { enterTimers.delete(timer); delete row.dataset.enter; }, ENTER_MS);
    enterTimers.add(timer);
  }

  return {
    el,
    patch(view, { animate }) {
      const q = view.queue;
      setText(parts.title, q.title);
      const wanted = new Set();
      const els = [];
      for (const g of q.groups) {
        for (const rowView of g.rows) {
          wanted.add(rowView.id);
          let ctl = rowCtls.get(rowView.id);
          if (!ctl) {
            ctl = createQueueRow(rowView);
            rowCtls.set(rowView.id, ctl);
            if (animate && rowView.kindId === "approved") enterRow(ctl.el); // the approved one slides down to "tonight" (spec 5.3); j and k never animate a row
          }
          ctl.patch(rowView, stateOf(rowView.id));
          els.push(ctl.el);
        }
      }
      for (const id of [...rowCtls.keys()]) if (!wanted.has(id)) rowCtls.delete(id);
      sync(parts.list, els);
      setHidden(parts.list, els.length === 0);
      setText(parts.none, q.empty ? q.none : "");
      setHidden(parts.none, !q.empty);
      const more = q.groups.reduce((n, g) => n + g.more, 0);
      setText(parts.more, more > 0 ? t("cola.queue.more", { n: more }) : "");
      setHidden(parts.more, more === 0);
    },
    viewOf(id) {
      const ctl = rowCtls.get(id);
      return ctl ? ctl.view : null;
    },
    repaint(id) {
      const ctl = rowCtls.get(id);
      if (ctl) ctl.patch(ctl.view, stateOf(id));
    },
    destroy() {
      for (const timer of enterTimers) clearTimeout(timer);
      enterTimers.clear();
      rowCtls.clear();
    },
  };
}
