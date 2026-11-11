// Draws COLA (spec §5.3, board/relay2/cola-1920.png) from the view of ./view.js. The page is BUILT ONCE and PATCHED after: every text goes
// through setText (a write only when it differs), the rows of the queue are keyed by their id and kept in place, the lists are brought
// to the new order with `sync`, and a part whose view did not change is not touched at all. A refresh that changes nothing writes
// nothing, and one that changes a number leaves the buttons, the focus and the armed press exactly as they were.
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

import { h, sv } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { sync, setText, setData, setHidden, setTitle } from "../shared/util.js";
import { opens } from "../shared/opens.js";
import { actButton } from "../../ui/act.js";

const ENTER_MS = 420;
const SKELETON_BARS = 6;

/** The band between the five figures: half of its height at each one, drawn on a 1000 x 30 box that the page stretches. */
export function bandPath(log) {
  const xs = [0, 250, 500, 750, 1000];
  const top = (i) => (15 - log[i] * 14).toFixed(1);
  const bot = (i) => (15 + log[i] * 14).toFixed(1);
  let d = "M" + xs[0] + " " + top(0);
  for (let i = 1; i < 5; i++) d += " C" + (xs[i - 1] + 125) + " " + top(i - 1) + " " + (xs[i] - 125) + " " + top(i) + " " + xs[i] + " " + top(i);
  d += " L1000 " + bot(4);
  for (let i = 3; i >= 0; i--) d += " C" + (xs[i + 1] - 125) + " " + bot(i + 1) + " " + (xs[i] + 125) + " " + bot(i) + " " + xs[i] + " " + bot(i);
  return d + "Z";
}

/** `el.setAttribute(name, value)` only when it differs (a null or false value removes it): a refresh that changes nothing writes nothing. */
function attr(el, name, value) {
  if (value === null || value === undefined || value === false) { if (el.hasAttribute(name)) el.removeAttribute(name); return; }
  const next = String(value);
  if (el.getAttribute(name) !== next) el.setAttribute(name, next);
}

const button = (a) => actButton({ key: a.key, label: a.label, primary: a.primary, "data-action": a.id, title: a.title || null });

/**
 * Keeps the buttons of one row: rebuilt only when the SET of buttons changed (a verb arrived or left, a label changed with the
 * language); for an armed press only the text of that one button changes, so the node he is pressing is the same node.
 */
function patchActs(box, state, actions, armed, what) {
  const shown = actions.filter((a) => a.visible);
  const shape = JSON.stringify(shown.map((a) => [a.id, a.key, a.label, a.primary]));
  if (shape !== state.shape) {
    state.shape = shape;
    const focused = box.contains(document.activeElement) ? document.activeElement.dataset.action : null;
    box.replaceChildren(...shown.map(button));
    if (focused) { const again = box.querySelector(`[data-action="${focused}"]`); if (again) again.focus({ preventScroll: true }); }
  }
  for (const b of box.querySelectorAll("button[data-action]")) {
    const a = shown.find((x) => x.id === b.dataset.action);
    if (!a) continue;
    const isArmed = armed === a.id;
    const said = isArmed ? t("act.sure") : a.label; // the name a screen reader gives is the one the eyes read
    setText(b.lastChild, said);
    setData(b, "armed", isArmed ? "true" : null);
    const aria = t("cola.act.aria", { action: said, what });
    if (b.getAttribute("aria-label") !== aria) b.setAttribute("aria-label", aria);
  }
}

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
  const state = { shape: "" };
  const row = { el, view: first, patch };
  function patch(view, st = { armed: null, pending: false }) {
    const next = JSON.stringify([view.kind, view.company, view.text, view.meta, view.actions.map((a) => [a.id, a.key, a.label, a.visible]), st.armed, st.pending]);
    if (next === sig) return false;
    sig = next;
    row.view = view;
    // a click on the row looks at a proposal in full, or opens what it is: the hand and the underline say so
    setData(el, "opens", view.kindId === "pending" || opens(view) ? "true" : null);
    setData(el, "pending", st.pending ? "true" : null);
    if (st.pending) { if (el.getAttribute("aria-busy") !== "true") el.setAttribute("aria-busy", "true"); } else if (el.hasAttribute("aria-busy")) el.removeAttribute("aria-busy");
    setText(parts.kind, view.kind);
    setText(parts.co, view.company || "");
    setHidden(parts.co, !view.company);
    setText(parts.text, view.text);
    setText(parts.meta, view.meta);
    setHidden(parts.meta, !view.meta);
    setTitle(parts.text.parentElement, [view.company, view.text].filter(Boolean).join(" · "));
    patchActs(parts.acts, state, view.actions, st.armed, [view.company, view.text].filter(Boolean).join(" · "));
    return true;
  }
  patch(first);
  return row;
}

export function createCola({ actions, shot, on, reduced = () => false }) {
  const rowCtls = new Map();            // queue rows by id
  let focus = null;                     // the row view of the proposal on screen
  let focusKey = null;
  let lastView = null;
  let built = false;                    // false until the first patch: what is there at the start does not animate
  const enterTimers = new Set();
  let enterTimer = 0;
  const sigs = { bars: "", path: "", funnel: "", squares: "", band: "" };
  const focusActs = { shape: "" };

  const parts = {
    eyebrow: h("p", { class: "eyebrow label" }),
    dot: h("span", { class: "dot", "aria-hidden": "true" }),
    engine: h("span", { class: "engine-text" }),
    lead: h("span", { class: "lead" }),
    n: h("b", { class: "n num" }),
    of: h("span", { class: "of" }),
    ask: h("p", { class: "ask serif" }),
    sub: h("p", { class: "sub label" }),
    warn: h("p", { class: "warn label", role: "status" }),
    facts: h("p", { class: "facts label" }),
    role: h("h2", { class: "role-title" }),
    score: h("span", { class: "score cond num" }),
    bars: h("ul", { class: "bars", "aria-label": t("cola.factors.aria") }),
    path: h("ol", { class: "path mono", "aria-label": t("cola.path.aria") }),
    whyLabel: h("span", { class: "why-label label gold" }),
    whyText: h("span", { class: "why-text" }),
    acts: h("div", { class: "acts" }),
    batchBtn: actButton({ key: "", cls: "batch-btn", "data-action": "batch" }),
    step: h("span", { class: "step label" }),
    stepPrev: h("button", { class: "step-btn", type: "button", "data-step": "-1" }, h("kbd", { class: "k" }, "k")),
    stepNext: h("button", { class: "step-btn", type: "button", "data-step": "1" }, h("kbd", { class: "k" }, "j")),
    stepText: h("span", { class: "step-text num" }),
    confirm: h("div", { class: "confirm label", role: "group" }),
    confirmAsk: h("span", { class: "gold" }),
    confirmNames: h("span", { class: "names" }),
    confirmNote: h("span", { class: "note" }),
    confirmOk: h("button", { class: "ck", type: "button", "data-batch": "confirm" }, h("kbd", { class: "k" }), " ", h("span", {})),
    confirmNo: h("button", { class: "ck", type: "button", "data-batch": "cancel" }, h("kbd", { class: "k" }), " ", h("span", {})),
    funnelTitle: h("h2", { class: "block-title label" }),
    figs: h("dl", { class: "fn" }),
    band: sv("svg", { class: "band", viewBox: "0 0 1000 30", preserveAspectRatio: "none", "aria-hidden": "true" }, sv("path", { d: "" })),
    funnelNone: h("p", { class: "none label" }),
    queueTitle: h("h2", { class: "block-title label" }),
    list: h("ul", { class: "q-rows", "data-lenis-prevent": "" }),
    queueNone: h("p", { class: "none label" }),
    more: h("p", { class: "q-more label" }),
    trust: h("div", { class: "trust" }),
    reviewLabel: h("p", { class: "label" }),
    squares: h("p", { class: "sq", role: "img" }),
    reviewSub: h("p", { class: "label dim" }),
    sentLabel: h("p", { class: "label" }),
    meter: h("p", { class: "cap-meter", role: "img" }, h("i", {})),
    sentSub: h("p", { class: "label dim" }),
    footLeft: h("p", { class: "foot-line label" }),
    stopBtn: actButton({ key: "", cls: "stop-btn", "data-engine": "" }),
    stopState: h("span", { class: "stop-state label" }),
  };
  parts.h1 = h("h1", { class: "mega cond", id: "cola-title", tabindex: "-1" }, parts.lead, " ", parts.n, " ", parts.of);
  parts.step.append(parts.stepNext, parts.stepPrev, parts.stepText);
  parts.confirm.append(parts.confirmAsk, " ", parts.confirmNames, " ", parts.confirmNote, " ", parts.confirmOk, " ", parts.confirmNo);
  parts.confirm.hidden = true;
  const figEls = ["read", "above", "cv", "form", "sent"].map((id) => ({ id, n: h("dd", { class: "fig-n cond num" }), label: h("dt", { class: "fig-l label" }), sub: h("dd", { class: "fig-s label" }) }));
  figEls.forEach((f) => parts.figs.append(h("div", { class: "fig", "data-fig": f.id }, f.label, f.n, f.sub)));
  parts.trust.append(
    h("div", { class: "trust-a" }, parts.reviewLabel, parts.squares, parts.reviewSub),
    h("div", { class: "trust-b" }, parts.sentLabel, parts.meter, parts.sentSub));

  const prop = h("article", { class: "prop" },
    parts.facts,
    parts.role,
    h("div", { class: "score-row" }, parts.score, parts.bars),
    parts.path,
    h("p", { class: "why" }, parts.whyLabel, " ", parts.whyText),
    h("div", { class: "verbs" }, parts.acts, parts.batchBtn, parts.step),
    parts.confirm);
  const funnelBox = h("section", { class: "funnel" }, parts.funnelTitle, parts.figs, parts.band, parts.funnelNone);
  const skeleton = h("div", { class: "skeleton", role: "status" }, h("span", { class: "sr" }, t("cola.loading")), h("div", { class: "skel-line wide", "aria-hidden": "true" }), h("div", { class: "skel-line", "aria-hidden": "true" }), h("div", { class: "skel-line", "aria-hidden": "true" }), h("div", { class: "skel-line", "aria-hidden": "true" }));

  const el = h("section", { class: "cola", "aria-labelledby": "cola-title", "data-state": "loading" },
    h("div", { class: "head-row" }, parts.eyebrow, h("p", { class: "engine label" }, parts.dot, parts.engine)),
    h("div", { class: "headline" }, parts.h1, parts.ask),
    parts.sub,
    parts.warn,
    prop,
    skeleton,
    funnelBox,
    h("section", { class: "queue" }, parts.queueTitle, parts.list, parts.more, parts.queueNone),
    parts.trust,
    h("footer", { class: "foot" }, parts.footLeft, h("span", { class: "engine-ctl" }, parts.stopBtn, parts.stopState)),
    shot.el);
  parts.band.querySelector("path").setAttribute("d", "");

  const stateOf = (id) => ({ armed: actions.armedOf(id), pending: actions.isPending(id) });
  const viewOf = (id) => {
    if (focus && focus.id === id) return focus;
    const ctl = rowCtls.get(id);
    return ctl ? ctl.view : null;
  };

  // ---- one delegated listener for every button and row of the page: nothing is bound per row, so nothing is lost when a row is rebuilt
  el.addEventListener("click", (e) => {
    const target = e.target;
    const step = target.closest("button[data-step]");
    if (step) { on.step(Number(step.dataset.step)); return; }
    const sure = target.closest("button[data-batch]");
    if (sure) { if (sure.dataset.batch === "confirm") on.batchConfirm(); else on.batchCancel(); return; }
    if (target.closest("button[data-engine]")) { on.engine(); return; }
    if (target.closest("button[data-retry]")) { on.retryDetail(); return; }
    const button$ = target.closest("button[data-action]");
    if (button$) {
      if (button$.dataset.action === "batch") { on.batchOpen(); return; }
      const rowEl = button$.closest("[data-row]");
      const view = rowEl && viewOf(rowEl.dataset.row);
      if (view) on.run(button$.dataset.action, view);
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

  // ---- the parts
  function patchHead(view) {
    setText(parts.eyebrow, view.eyebrow);
    setText(parts.engine, view.engine.line);
    setData(parts.dot, "engine", view.engine.kind);
    setData(parts.dot, "stopped", view.engine.stopped ? "true" : null);
    setText(parts.lead, view.head.lead);
    setText(parts.n, view.head.n == null ? "" : String(view.head.n));
    setHidden(parts.n, view.head.n == null);
    setText(parts.of, view.head.of);
    setHidden(parts.of, view.empty);
    setText(parts.ask, view.head.ask);
    setHidden(parts.ask, view.empty);
    setText(parts.sub, view.empty ? view.head.sub : "");
    setHidden(parts.sub, !view.empty);
    setText(parts.warn, view.engine.warn.join(" "));
    setHidden(parts.warn, view.engine.warn.length === 0);
  }

  function patchBars(f) {
    const sig = JSON.stringify([f.factors, f.factorsError, f.factorsNone, t("common.retry")]);
    if (sig === sigs.bars) return;
    sigs.bars = sig;
    parts.bars.setAttribute("aria-busy", String(f.factors.state === "loading"));
    if (f.factors.state === "loading") {
      parts.bars.replaceChildren(...Array.from({ length: SKELETON_BARS }, () => h("li", { class: "f-bar skel", "aria-hidden": "true" }, h("span", { class: "f-name" }), h("i", { class: "track" }), h("span", { class: "f-val" }))));
    } else if (f.factors.state === "error") {
      parts.bars.replaceChildren(h("li", { class: "bars-msg label", role: "alert" }, f.factorsError, " ", h("button", { class: "plane-retry label", type: "button", "data-retry": "" }, t("common.retry"))));
    } else if (f.factors.state === "none") {
      parts.bars.replaceChildren(h("li", { class: "bars-msg label" }, f.factorsNone));
    } else {
      parts.bars.replaceChildren(...f.factors.bars.map((b) => h("li", { class: "f-bar", "data-factor": b.id },
        h("span", { class: "f-name label" }, b.label),
        h("i", { class: "track" }, h("b", { style: { "--k": b.k.toFixed(3) } })),
        h("span", { class: "f-val mono num" }, b.text))));
    }
  }

  function patchPath(f) {
    const sig = JSON.stringify([f.path, t("layer.aviso")]);
    if (sig === sigs.path) return;
    sigs.path = sig;
    parts.path.replaceChildren(...f.path.map((l) => h("li", { "data-layer": l.id, "data-state": l.state, "aria-current": l.state === "here" ? "step" : null },
      h("b", { class: "l-name" }, t("layer." + l.id)),
      h("span", { class: "l-note" }, l.note, l.here ? [" · ", h("span", { class: "gold" }, l.here)] : null))));
  }

  function patchFocus(view, first) {
    const f = view.focus;
    prop.hidden = !f;
    setHidden(skeleton, true);
    if (!f) { focus = null; focusKey = null; return; }
    const changed = focusKey !== null && focusKey !== f.key;
    focusKey = f.key;
    focus = { id: f.id, data: f.data, open: f.open, link: f.link, actions: f.actions, kindId: "focus" };
    setData(prop, "row", f.id);
    setData(prop, "card", f.key);
    setData(prop, "pending", actions.isPending(f.id) ? "true" : null);
    setText(parts.facts, f.facts.join(" · "));
    setText(parts.role, f.title);
    setText(parts.score, f.score);
    if (parts.score.getAttribute("aria-label") !== f.scoreAria) { if (f.scoreAria) parts.score.setAttribute("aria-label", f.scoreAria); else parts.score.removeAttribute("aria-label"); }
    patchBars(f);
    patchPath(f);
    setText(parts.whyLabel, f.why.label);
    setText(parts.whyText, f.why.text);
    patchActs(parts.acts, focusActs, f.actions, actions.armedOf(f.id), [f.company, f.title].join(" · "));
    // the lote's key: part of the same line of verbs, one key once
    const b = view.batch;
    setText(parts.batchBtn.firstChild, b.key);
    setText(parts.batchBtn.lastChild, b.label);
    setData(parts.batchBtn, "none", b.can ? null : "true");
    attr(parts.batchBtn, "aria-disabled", !b.can);
    parts.batchBtn.classList.toggle("armed", actions.batch.isOpen());
    setText(parts.stepText, f.step.label);
    setHidden(parts.step, !f.step.many);
    attr(parts.stepNext, "aria-label", f.step.next);
    attr(parts.stepPrev, "aria-label", f.step.prev);
    patchConfirm(view);
    if (changed && !reduced()) {
      prop.dataset.enter = "true";
      clearTimeout(enterTimer);
      enterTimer = setTimeout(() => { delete prop.dataset.enter; }, ENTER_MS);
    }
  }

  function patchConfirm(view) {
    const open = actions.batch.isOpen();
    const b = view.batch;
    const picks = open ? actions.batch.picks() : [];
    // the drawn confirmation names what was drawn when it opened, so what he sees is what ↵ sends
    if (!open && !parts.confirm.hidden && parts.confirm.contains(document.activeElement)) parts.batchBtn.focus({ preventScroll: true });
    setHidden(parts.confirm, !open);
    if (!open) return;
    setText(parts.confirmAsk, t("cola.batch.ask", { n: picks.length }));
    setText(parts.confirmNames, picks.slice(0, 4).map((p) => p.company + " " + p.score).join(" · ") + (picks.length > 4 ? " · " + t("cola.batch.more", { n: picks.length - 4 }) : ""));
    setText(parts.confirmNote, b.note ? " · " + b.note + " · " : " · ");
    setText(parts.confirmOk.firstChild, b.enter);
    setText(parts.confirmOk.lastChild, b.confirm);
    setText(parts.confirmNo.firstChild, b.esc);
    setText(parts.confirmNo.lastChild, b.cancel);
  }

  function patchFunnel(view) {
    const fn = view.funnel;
    setText(parts.funnelTitle, fn.title);
    setHidden(parts.figs, fn.empty);
    if (parts.band.hasAttribute("hidden") !== fn.empty) { if (fn.empty) parts.band.setAttribute("hidden", ""); else parts.band.removeAttribute("hidden"); } // an SVG has no `hidden` property
    setText(parts.funnelNone, fn.empty ? fn.none : "");
    setHidden(parts.funnelNone, !fn.empty);
    fn.figs.forEach((fig, i) => {
      const f = figEls[i];
      setText(f.n, String(fig.n));
      setText(f.label, fig.label);
      setText(f.sub, fig.sub);
      setHidden(f.sub, !fig.sub);
      setData(f.n, "last", fig.last ? "true" : null);
    });
    const d = bandPath(fn.log);
    const path = parts.band.firstChild;
    if (path.getAttribute("d") !== d) path.setAttribute("d", d);
  }

  function enterRow(row) {
    if (reduced()) return;
    row.dataset.enter = "true";
    const timer = setTimeout(() => { enterTimers.delete(timer); delete row.dataset.enter; }, ENTER_MS);
    enterTimers.add(timer);
  }

  function patchQueue(view) {
    const q = view.queue;
    setText(parts.queueTitle, q.title);
    const wanted = new Set();
    const els = [];
    for (const g of q.groups) {
      for (const rowView of g.rows) {
        wanted.add(rowView.id);
        let ctl = rowCtls.get(rowView.id);
        if (!ctl) {
          ctl = createQueueRow(rowView);
          rowCtls.set(rowView.id, ctl);
          if (built && rowView.kindId === "approved") enterRow(ctl.el); // the approved one slides down to "tonight" (spec 5.3); j and k never animate a row
        }
        ctl.patch(rowView, stateOf(rowView.id));
        els.push(ctl.el);
      }
    }
    for (const id of [...rowCtls.keys()]) if (!wanted.has(id)) rowCtls.delete(id);
    sync(parts.list, els);
    setHidden(parts.list, els.length === 0);
    setText(parts.queueNone, q.empty ? q.none : "");
    setHidden(parts.queueNone, !q.empty);
    const more = q.groups.reduce((n, g) => n + g.more, 0);
    setText(parts.more, more > 0 ? t("cola.queue.more", { n: more }) : "");
    setHidden(parts.more, more === 0);
  }

  function patchTrust(view) {
    const tr = view.trust;
    setHidden(parts.trust, !tr);
    if (!tr) return;
    setText(parts.reviewLabel, tr.reviewLabel);
    attr(parts.squares, "aria-label", tr.reviewAria);
    const sig = JSON.stringify(tr.squares);
    if (sig !== sigs.squares) {
      sigs.squares = sig;
      parts.squares.replaceChildren(...tr.squares.map((on) => h("i", { "data-on": on ? "true" : null })));
    }
    setText(parts.reviewSub, tr.reviewSub);
    setText(parts.sentLabel, tr.sentLabel);
    attr(parts.meter, "aria-label", tr.sentAria);
    const bar = parts.meter.firstChild;
    const k = tr.k.toFixed(3);
    if (bar.style.getPropertyValue("--k") !== k) bar.style.setProperty("--k", k);
    setText(parts.sentSub, tr.sentSub);
  }

  function patchFoot(view) {
    setText(parts.footLeft, view.foot.left);
    setHidden(parts.footLeft, !view.foot.left);
    const s = view.foot.stop;
    const armed = actions.engineArmed();
    setText(parts.stopBtn.firstChild, s.key);
    const said = armed ? t("act.sure") : s.label;
    setText(parts.stopBtn.lastChild, said);
    setData(parts.stopBtn, "armed", armed ? "true" : null);
    setData(parts.stopBtn, "pending", actions.isPending("engine") ? "true" : null);
    const aria = said;                                   // the accessible name says what the button shows, "¿Seguro?" included (as SISTEMA does)
    if (parts.stopBtn.getAttribute("aria-label") !== aria) parts.stopBtn.setAttribute("aria-label", aria);
    setText(parts.stopState, s.state);
    setData(parts.stopState, "on", s.on ? "true" : null);
  }

  /** The names that screen readers read for the parts that have no text of their own: they follow the language. */
  function patchNames() {
    const names = [[parts.bars, "cola.factors.aria"], [parts.path, "cola.path.aria"], [funnelBox, "cola.funnel.aria"]];
    for (const [node, key] of names) { const text = t(key); if (node.getAttribute("aria-label") !== text) node.setAttribute("aria-label", text); }
  }

  function patch(view) {
    lastView = view;
    patchNames();
    setData(el, "state", view.empty ? "empty" : "ready");
    patchHead(view);
    patchFocus(view);
    patchFunnel(view);
    patchQueue(view);
    patchTrust(view);
    patchFoot(view);
    const f = view.focus;
    built = true;
    shot.show(f ? (f.hasShot ? { key: f.shotKey, preparedAt: f.preparedAt } : { key: null, preparedAt: f.preparedAt }) : null);
  }

  return {
    el,
    patch,
    rowView: viewOf,
    focusView: () => focus,
    /** The drawn lote just opened: the keyboard goes to its confirm, so ↵ (or Space) is the second press and Esc the way out. */
    inConfirm: (node) => !!node && parts.confirm.contains(node),
    focusConfirm() { if (!parts.confirm.hidden) parts.confirmOk.focus({ preventScroll: true }); },
    repaint(id) {
      if (!lastView) return;
      if (id === "batch") { patchConfirm(lastView); parts.batchBtn.classList.toggle("armed", actions.batch.isOpen()); return; }
      if (id === "engine") { patchFoot(lastView); return; }
      if (focus && focus.id === id) {
        setData(prop, "pending", actions.isPending(id) ? "true" : null);
        patchActs(parts.acts, focusActs, focus.actions, actions.armedOf(id), lastView.focus ? [lastView.focus.company, lastView.focus.title].join(" · ") : "");
        return;
      }
      const ctl = rowCtls.get(id);
      if (ctl) ctl.patch(ctl.view, stateOf(id));
    },
    destroy() { clearTimeout(enterTimer); for (const timer of enterTimers) clearTimeout(timer); enterTimers.clear(); rowCtls.clear(); focus = null; shot.destroy(); },
  };
}
