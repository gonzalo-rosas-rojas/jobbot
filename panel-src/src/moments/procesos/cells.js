// The two ways one application is drawn in PROCESOS, each built once per application and PATCHED after (as HOY's rows are): a pile of the
// field (five bars, a name, an age) and a row of the table. A piece whose view and state are the same as last time is not touched at all,
// which is what lets a refresh land under a button that is half pressed, focused or hovered. Text from outside goes in as text nodes only.
//
//   createPile(view) -> { el: li.slot, btn, view, patch(view, state), id }        createTomb(tomb) -> { el: li.slot.tomb, patch(tomb) }
//   createRow(view)  -> { el: div.trow, view, patch(view, state), id }
// `state` is { armed: actionId | null, pending: bool } from the actions. The element that carries `data-card` is the one the cursor of the
// keyboard walks over; nothing nested inside it carries one, and a tombstone carries none (it is not a row to stand on).

import { h, setText } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { setData, setTitle, setBusy, nameOf, stampChanged } from "../shared/util.js";
import { tombIdOf } from "./view.js";

const NO_STATE = Object.freeze({ armed: null, pending: false });

/** [{ text, hit }] as nodes: a <mark> for the letters that matched, a text node for the rest. */
const hitNodes = (parts) => parts.map((p) => (p.hit ? h("mark", { class: "hit" }, p.text) : document.createTextNode(p.text)));

/** The text of a name with the letters that matched marked: [{ text, hit }] becomes text nodes and <mark>s; no parts is plain text. */
function fillName(el, text, parts) {
  if (!stampChanged(el, JSON.stringify(parts ? parts : text))) return;
  if (!parts) { el.textContent = text; return; }
  el.replaceChildren(...hitNodes(parts));
}

const stateSig = (view, state) => JSON.stringify([
  view.company, view.title, view.state, view.turn, view.days, view.layers.map((l) => [l.state, l.w]), view.gate, view.label,
  view.hl || null, view.actions.map((a) => a.id), view.stage, view.last, view.next, view.cadence, view.turnCell || null, state.armed, state.pending,
]);

function common(el, view, state) {
  setData(el, "turn", view.turn);
  setData(el, "state", view.state);
  setData(el, "actions", view.actions.map((a) => a.id).join(","));
  setData(el, "pending", state.pending ? "true" : null);
  setBusy(el, state.pending);
  setData(el, "armed", state.armed);
  const label = view.label;
  if (el.getAttribute("aria-label") !== label) el.setAttribute("aria-label", label);
}

const SCORE_BAR = 1; // the bar that is as long as the score
const GATE_BAR = 4;  // the bar that says how the pile passed the gate

/** The five bars of a pile: their state, the second as long as the score, the fifth as the gate says (clear, dim, hollow or still to come). */
function patchBars(stk, view) {
  const bars = stk.children;
  view.layers.forEach((layer, i) => {
    setData(bars[i], "s", layer.state);
    if (i === SCORE_BAR) {
      const w = layer.w + "%";
      if (bars[i].style.getPropertyValue("--w") !== w) bars[i].style.setProperty("--w", w);
    }
  });
  setData(bars[GATE_BAR], "gate", view.gate);
}

export const stackGlyph = (extra = "") => h("span", { class: "stk " + extra, "aria-hidden": "true" }, h("i"), h("i"), h("i"), h("i"), h("i"));

// ---- a pile of the field

export function createPile(initial) {
  const stk = stackGlyph();
  const nm = h("span", { class: "nm" });
  const age = h("span", { class: "age num mono" });
  const btn = h("button", { class: "cell", type: "button", "data-card": initial.key, "data-row": initial.id }, stk, nm, age);
  const el = h("li", { class: "slot", "data-slot": initial.id }, btn);
  let sig = "";
  const pile = { el, btn, view: initial, id: initial.id, patch };

  function patch(view, state = NO_STATE) {
    const next = stateSig(view, state);
    if (next === sig) return false;
    sig = next;
    pile.view = view;
    common(btn, view, state);
    patchBars(stk, view);
    fillName(nm, view.company, view.hl ? view.hl.company : null);
    setText(age, view.age);
    setTitle(btn, nameOf(view.company, view.title));
    return true;
  }

  patch(initial);
  return pile;
}

/** The place of a pile that left: a dotted gap of the same size that says where it went, until the page is loaded again. */
export function createTomb(initial) {
  const text = h("span", { class: "tomb-text label" });
  const el = h("li", { class: "slot tomb", "data-slot": tombIdOf(initial.tomb), "aria-label": t("procesos.tomb.aria") }, text);
  let sig = "";
  const tomb = { el, id: tombIdOf(initial.tomb), patch };
  function patch(view) {
    const next = JSON.stringify([view.text, t("procesos.tomb.aria")]);
    if (next === sig) return false;
    sig = next;
    setText(text, view.text);
    el.setAttribute("aria-label", t("procesos.tomb.aria") + ": " + view.text);
    return true;
  }
  patch(initial);
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

export function createRow(initial) {
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
  const el = h("div", { class: "trow", role: "row", tabindex: "-1", "data-card": initial.key, "data-row": initial.id }, ...COLS.map((c) => parts[c]));
  let sig = "";
  let turnSig = "";
  const row = { el, view: initial, id: initial.id, patch };

  function patch(view, state = NO_STATE) {
    const next = stateSig(view, state);
    if (next === sig) return false;
    sig = next;
    row.view = view;
    common(el, view, state);
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

  patch(initial);
  return row;
}

function fillRole(el, view) {
  // the role is one span: " — " as a text node of its own, then the role (or its highlighted pieces), all replaced together; being one span,
  // a long line is cut at the end of the span, not in the middle of the name
  const parts = view.hl ? view.hl.title : null;
  const sig = JSON.stringify(parts ? ["p", parts] : ["t", view.title]);
  if (!stampChanged(el, sig)) return;
  const dash = document.createTextNode(view.title ? " — " : "");
  el.replaceChildren(dash, ...(parts ? hitNodes(parts) : [document.createTextNode(view.title)]));
}
