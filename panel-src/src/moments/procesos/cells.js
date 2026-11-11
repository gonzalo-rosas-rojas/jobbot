// The two ways one application is drawn in PROCESOS, each built once per application and PATCHED after (as HOY's rows are): a pile of the
// field (five bars, a name, an age) and a row of the table. A piece whose view and state are the same as last time is not touched at all,
// which is what lets a refresh land under a button that is half pressed, focused or hovered. Text from outside goes in as text nodes only.
//
//   createPile(view) -> { el: li.slot, btn, view, patch(view, state), id }        createTomb(tomb) -> { el: li.slot.tomb, patch(tomb) }
//   createRow(view)  -> { el: div.trow, view, patch(view, state), id }
// `state` is { armed: actionId | null, pending: bool } from the actions. The element that carries `data-card` is the one the cursor of the
// keyboard walks over; nothing nested inside it carries one, and a tombstone carries none (it is not a row to stand on).

import { h } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { setText, setData, setTitle } from "../shared/util.js";

const NO_STATE = Object.freeze({ armed: null, pending: false });

/** The text of a name with the letters that matched marked: [{ text, hit }] becomes text nodes and <mark>s; no parts is plain text. */
function fillName(el, text, parts) {
  const sig = JSON.stringify(parts ? parts : text);
  if (el.dataset.sig === sig) return;
  el.dataset.sig = sig;
  if (!parts) { el.textContent = text; return; }
  el.replaceChildren(...parts.map((p) => (p.hit ? h("mark", { class: "hit" }, p.text) : document.createTextNode(p.text))));
}

const stateSig = (view, state) => JSON.stringify([
  view.company, view.title, view.state, view.turn, view.days, view.layers.map((l) => [l.state, l.w]), view.gate, view.label,
  view.hl || null, view.actions.map((a) => a.id), view.stage, view.last, view.next, view.cadence, view.turnCell || null, state.armed, state.pending,
]);

function common(el, first, view, state) {
  setData(el, "turn", view.turn);
  setData(el, "state", view.state);
  setData(el, "actions", view.actions.map((a) => a.id).join(","));
  setData(el, "pending", state.pending ? "true" : null);
  if (state.pending) { if (el.getAttribute("aria-busy") !== "true") el.setAttribute("aria-busy", "true"); } else if (el.hasAttribute("aria-busy")) el.removeAttribute("aria-busy");
  setData(el, "armed", state.armed);
  const label = view.label;
  if (el.getAttribute("aria-label") !== label) el.setAttribute("aria-label", label);
}

/** The five bars of a pile: their state, the second as long as the score, the fifth as the gate says (clear, dim, hollow or still to come). */
function patchBars(stk, view) {
  const bars = stk.children;
  view.layers.forEach((layer, i) => {
    setData(bars[i], "s", layer.state);
    if (i === 1) {
      const w = layer.w + "%";
      if (bars[i].style.getPropertyValue("--w") !== w) bars[i].style.setProperty("--w", w);
    }
  });
  setData(bars[4], "gate", view.gate);
}

export const stackGlyph = (extra = "") => h("span", { class: "stk " + extra, "aria-hidden": "true" }, h("i"), h("i"), h("i"), h("i"), h("i"));

// ---- a pile of the field

export function createPile(first) {
  const stk = stackGlyph();
  const nm = h("span", { class: "nm" });
  const age = h("span", { class: "age num mono" });
  const btn = h("button", { class: "cell", type: "button", "data-card": first.key, "data-row": first.id }, stk, nm, age);
  const el = h("li", { class: "slot", "data-slot": first.id }, btn);
  let sig = "";
  const pile = { el, btn, view: first, id: first.id, patch };

  function patch(view, state = NO_STATE) {
    const next = stateSig(view, state);
    if (next === sig) return false;
    sig = next;
    pile.view = view;
    common(btn, false, view, state);
    patchBars(stk, view);
    fillName(nm, view.company, view.hl ? view.hl.company : null);
    setText(age, view.age);
    setTitle(btn, [view.company, view.title].filter(Boolean).join(" · "));
    return true;
  }

  patch(first);
  return pile;
}

/** The place of a pile that left: a dotted gap of the same size that says where it went, until the page is loaded again. */
export function createTomb(first) {
  const text = h("span", { class: "tomb-text label" });
  const el = h("li", { class: "slot tomb", "data-slot": "tomb:" + first.tomb, "aria-label": t("procesos.tomb.aria") }, text);
  let sig = "";
  const tomb = { el, id: "tomb:" + first.tomb, patch };
  function patch(view) {
    const next = JSON.stringify([view.text, t("procesos.tomb.aria")]);
    if (next === sig) return false;
    sig = next;
    setText(text, view.text);
    el.setAttribute("aria-label", t("procesos.tomb.aria") + ": " + view.text);
    return true;
  }
  patch(first);
  return tomb;
}

// ---- a row of the table

const COLS = ["process", "stage", "age", "last", "next", "cadence", "turn"];

/** The header of the table: one cell per column. It sits ABOVE the list (not inside it), so what scrolls never shows through it; the rows
 *  carry their own full label, so for a reader of the screen it is a repeat and stays out of the way. */
export function tableHead() {
  return h("div", { class: "trow thead label", "aria-hidden": "true" }, COLS.map((c) => h("span", { class: "c-" + c }, t("procesos.col." + c))));
}

function turnNodes(cell) {
  if (cell.kind === "you") return [h("span", { class: "turn-open" }, cell.key ? h("kbd", { class: "k" }, cell.key) : null, cell.key ? " " : null, cell.text)];
  return [h("span", { class: "dimtxt" }, cell.text)];
}

export function createRow(first) {
  const co = h("b", { class: "co" });
  const role = h("span", { class: "role" });
  const parts = {
    process: h("span", { class: "c-process p", role: "cell" }, co, role),
    stage: h("span", { class: "c-stage dim", role: "cell" }),
    age: h("span", { class: "c-age num mono", role: "cell" }),
    last: h("span", { class: "c-last dim", role: "cell" }),
    next: h("span", { class: "c-next", role: "cell" }),
    cadence: h("span", { class: "c-cadence dim label", role: "cell" }),
    turn: h("span", { class: "c-turn turn", role: "cell" }),
  };
  const el = h("div", { class: "trow", role: "row", tabindex: "-1", "data-card": first.key, "data-row": first.id }, ...COLS.map((c) => parts[c]));
  let sig = "";
  let turnSig = "";
  const row = { el, view: first, id: first.id, patch };

  function patch(view, state = NO_STATE) {
    const next = stateSig(view, state);
    if (next === sig) return false;
    sig = next;
    row.view = view;
    common(el, false, view, state);
    fillName(co, view.company, view.hl ? view.hl.company : null);
    fillRole(role, view);
    setText(parts.stage, view.stage);
    setText(parts.age, view.age.replace(/d$/, " d"));
    setText(parts.last, view.last);
    setText(parts.next, view.next);
    setText(parts.cadence, view.cadence);
    const ts = JSON.stringify(view.turnCell);
    if (ts !== turnSig) { turnSig = ts; parts.turn.replaceChildren(...turnNodes(view.turnCell)); }
    setData(parts.turn, "kind", view.turnCell.kind);
    setTitle(el, [view.company, view.title, view.last].filter(Boolean).join(" · "));
    return true;
  }

  patch(first);
  return row;
}

function fillRole(el, view) {
  // " — " and the role go in one node so that the cut of a long line is at the end of the role, not in the middle of the name
  const parts = view.hl ? view.hl.title : null;
  const sig = JSON.stringify(parts ? ["p", parts] : ["t", view.title]);
  if (el.dataset.sig === sig) return;
  el.dataset.sig = sig;
  const dash = document.createTextNode(view.title ? " — " : "");
  el.replaceChildren(dash, ...(parts ? parts.map((p) => (p.hit ? h("mark", { class: "hit" }, p.text) : document.createTextNode(p.text))) : [document.createTextNode(view.title)]));
}
