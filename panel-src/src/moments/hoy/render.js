// Draws HOY (spec §5.1, board/relay2/hoy-1920.png) from the view of ./view.js. The page is BUILT ONCE and PATCHED after: every text
// goes through setText (a write only when it differs), the rows are keyed by their id and kept in place (rows.js), the lists are
// brought to the new order with `sync` (which moves no node that is already where it should be). A refresh that changes nothing
// writes nothing, and one that changes a number leaves the buttons, the focus and the open fold exactly as they were.
//
//   const hoy = createHoy({ actions, run, more, unsnooze, reduced, raf });
//   hoy.el                       section.hoy  (host.append(hoy.el))
//   hoy.patch(view, { fresh, first })      fresh: the marks of what is new ("task:101", "card:fx:12", "mail:…") for 4 s
//   hoy.rowView(id) / hoy.nextView()       the view of a row by its id, and of LO SIGUIENTE (the default target of a key)
//   hoy.repaint(id)              paints one row again (its armed or pending state changed)
//   hoy.destroy()

import { h, sv } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { sync, setText, setData, setHidden, setTitle, FRESH_MS } from "../shared/util.js";
import { createRow } from "./rows.js";
import { actButton } from "../../ui/act.js";

const COUNT_MS = 700;
let counted = false; // the receipts count up once per session of the page (§5.1 "Anima"), never again and never in gold

const SLOT_W = 3;
const STRIP_H = 40;

function stripSvg(strip) {
  const slots = strip.slots;
  const width = slots.length * SLOT_W;
  const defs = sv("defs", {}, sv("pattern", { id: "hoy-hatch", width: 2, height: 2, patternUnits: "userSpaceOnUse", patternTransform: "rotate(45)" }, sv("line", { x1: 0, y1: 0, x2: 0, y2: 2 })));
  const marks = [];
  slots.forEach((slot, i) => {
    if (slot.busy) marks.push(sv("rect", { class: "busy", x: i * SLOT_W, y: 0, width: SLOT_W, height: STRIP_H, fill: "url(#hoy-hatch)" }));
    const bar = slot.seen > 0 ? Math.max(1.5, slot.h * (STRIP_H - 2)) : 0;
    if (bar > 0) marks.push(sv("rect", { class: "seen", x: i * SLOT_W + 0.5, y: STRIP_H - bar, width: SLOT_W - 1, height: bar }));
  });
  return sv("svg", { class: "strip", viewBox: `0 0 ${width} ${STRIP_H}`, preserveAspectRatio: "none", role: "img", "aria-label": t("hoy.strip.aria") }, defs, ...marks);
}

export function createHoy({ actions, run, more, unsnooze, reduced = () => false, raf = (fn) => requestAnimationFrame(fn) }) {
  const rowCtls = new Map(); // "Y después": id -> row
  let nextCtl = null;
  const timers = new Set();
  let stripSig = "";
  let sinceSig = "";
  let snoozedSig = "";
  let aboutSig = "null";

  const parts = {
    about: h("p", { class: "about label" }),
    eyebrow: h("p", { class: "eyebrow label" }),
    ask: h("p", { class: "ask label gold" }),
    figs: h("dl", { class: "figs" }),
    stripTitle: h("p", { class: "strip-title label" }),
    stripBox: h("div", { class: "strip-box" }),
    base: h("p", { class: "base label" }),
    nextSlot: h("div", { class: "next-slot" }),
    after: h("p", { class: "after label" }),
    rest: h("div", { class: "rest" }),
    restLabel: h("h2", { class: "rest-label label" }),
    restHint: h("span", { class: "rest-hint label" }),
    list: h("ul", { class: "rows", "data-lenis-prevent": "" }),
    more: h("button", { class: "more label", type: "button", "data-more": "" }),
    sinceTitle: h("h2", { class: "since-title label" }),
    sinceList: h("ul", { class: "since-rows" }),
    foot: h("p", { class: "foot-line label" }),
    snoozed: h("details", { class: "snoozed", "data-fold": "snoozed" }),
    snoozedTitle: h("summary", { class: "snoozed-title label" }),
    snoozedList: h("ul", { class: "snoozed-rows" }),
  };
  parts.h1 = h("h1", { class: "mega cond", id: "hoy-title", tabindex: "-1" });   // one run of the display face: no accent word, no serif
  const figEls = ["read", "above", "complete", "awaiting"].map((id) => ({
    id, n: h("dd", { class: "fig-n cond num" }), label: h("dt", { class: "fig-l label" }),
  }));
  // dl > div > dt + dd: the label first in the markup (what a screen reader reads first), the number first on the screen (CSS order)
  figEls.forEach((f) => parts.figs.append(h("div", { class: "fig", "data-fig": f.id }, f.label, f.n)));
  parts.snoozed.append(parts.snoozedTitle, parts.snoozedList);
  parts.rest.append(h("header", { class: "rest-head" }, parts.restLabel, parts.restHint, parts.more), parts.list);

  const skeleton = () => h("li", { class: "row skeleton", "aria-hidden": "true" });
  parts.list.append(skeleton(), skeleton(), skeleton());

  const el = h("section", { class: "hoy", "aria-labelledby": "hoy-title", "data-loading": "true" },
    parts.eyebrow,
    h("div", { class: "headline" }, parts.h1, parts.ask),
    // what to do comes before the night's figures (2026-10-24): the squint test kept the headline, the figures and the clock and lost the action
    parts.nextSlot,
    parts.rest,
    h("div", { class: "receipts" }, parts.figs, h("div", { class: "strip-wrap" }, parts.stripTitle, parts.stripBox, parts.base)),
    h("div", { class: "since" }, parts.sinceTitle, parts.sinceList),
    h("footer", { class: "foot" }, parts.foot, parts.snoozed));

  function viewOf(id) {
    if (nextCtl && nextCtl.view.id === id) return nextCtl.view;
    const ctl = rowCtls.get(id);
    return ctl ? ctl.view : null;
  }
  const stateOf = (id) => ({ armed: actions.armedOf(id), pending: actions.isPending(id) });
  const ctlOf = (id) => (nextCtl && nextCtl.view.id === id ? nextCtl : rowCtls.get(id) || null);

  // ---- one delegated listener for every button, row and link of the page: nothing is bound per row, so nothing is lost when a row is rebuilt
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

  function markFresh(ctl, fresh) {
    if (!fresh.has(ctl.view.mark) || ctl.el.dataset.fresh) return;
    ctl.setFresh(true);
    const id = setTimeout(() => { timers.delete(id); ctl.setFresh(false); }, FRESH_MS);
    timers.add(id);
  }

  function countUp(node, n) {
    const start = performance.now();
    node.dataset.counting = "true";
    const step = (now) => {
      const p = Math.min(1, (now - start) / COUNT_MS);
      node.textContent = String(Math.round(n * (1 - Math.pow(1 - p, 3))));
      if (p < 1 && node.isConnected) raf(step);
      else { node.textContent = String(n); delete node.dataset.counting; }
    };
    raf(step);
  }

  function patchFigures(view, first) {
    const animate = first && !counted && !reduced();
    view.figures.forEach((fig, i) => {
      const f = figEls[i];
      setData(f.n, "gold", fig.gold ? "true" : "false");
      setText(f.label, fig.label);
      if (f.n.dataset.counting) return;
      if (animate && !fig.gold && fig.n > 0) countUp(f.n, fig.n);
      else setText(f.n, String(fig.n));
    });
    if (first) counted = true;
  }

  function patchStrip(strip) {
    setText(parts.stripTitle, strip.title || "");
    setHidden(parts.stripTitle, !strip.title);
    const sig = JSON.stringify(strip.slots.map((s) => [s.seen, s.busy, Math.round(s.h * 100)]));
    if (sig === stripSig) return;
    stripSig = sig;
    parts.stripBox.replaceChildren(...(strip.max > 0 ? [stripSvg(strip)] : []));
  }

  function patchNext(view, fresh) {
    if (!view.next) {
      nextCtl = null;
      parts.nextSlot.replaceChildren();
      return;
    }
    if (!nextCtl || nextCtl.view.id !== view.next.id) {
      nextCtl = createRow(view.next, "next");
      parts.nextSlot.replaceChildren(nextCtl.el, parts.after);
    }
    nextCtl.patch(view.next, stateOf(view.next.id));
    setText(parts.after, view.after || "");
    markFresh(nextCtl, fresh);
  }

  function patchRows(view, fresh) {
    const wanted = new Set(view.rows.map((r) => r.id));
    for (const id of [...rowCtls.keys()]) if (!wanted.has(id)) rowCtls.delete(id);
    const els = view.rows.map((row) => {
      let ctl = rowCtls.get(row.id);
      if (!ctl) {
        ctl = createRow(row, "row");
        rowCtls.set(row.id, ctl);
      }
      ctl.patch(row, stateOf(row.id));
      markFresh(ctl, fresh);
      return ctl.el;
    });
    sync(parts.list, els);
    setText(parts.restLabel, view.restLabel);
    setText(parts.restHint, view.restHint);
    setHidden(parts.rest, els.length === 0 && view.more === 0);
    setText(parts.more, view.moreLabel || "");
    setHidden(parts.more, !view.moreLabel);
    setTitle(parts.more, view.moreLabel ? t("hoy.rest.more.hint") : "");
  }

  // The demo's line about itself, the first thing of the sheet. Absent from the page without `about`; drawn again only when what it says changes (a language).
  function patchAbout(about) {
    const sig = JSON.stringify(about);
    if (sig === aboutSig) return;
    aboutSig = sig;
    if (!about) { parts.about.remove(); return; }
    const links = about.links.flatMap((l, i) => [i ? " · " : "", h("a", { href: l.href, target: "_blank", rel: "noopener noreferrer" }, l.text)]);
    parts.about.replaceChildren(h("span", { class: "about-line" }, about.line), " ", h("span", { class: "about-note" }, about.note), " ", h("span", { class: "about-links" }, ...links));
    if (!parts.about.parentNode) el.prepend(parts.about);
  }

  function patchSince(view, fresh) {
    const since = view.since;
    setHidden(parts.sinceTitle.parentElement, !since);
    if (!since) return;
    setText(parts.sinceTitle, t("hoy.since.title") + ", " + since.head);
    const sig = JSON.stringify([since.rows.map((r) => [r.mark, r.hour, r.company, r.type, r.line]), since.rows.map((r) => fresh.has(r.mark)), t("hoy.since.none")]);
    if (sig === sinceSig) return;
    sinceSig = sig;
    const lines = since.rows.map((r) => {
      const li = h("li", { class: "srow", "data-type": r.typeId },
        h("time", { class: "s-hour mono num" }, r.hour), h("span", { class: "s-co label" }, r.company), h("span", { class: "s-type label" }, r.type), h("span", { class: "s-line" }, r.line));
      if (fresh.has(r.mark)) li.dataset.fresh = "true";
      return li;
    });
    parts.sinceList.replaceChildren(...(lines.length ? lines : [h("li", { class: "srow none label" }, t("hoy.since.none"))]));
  }

  function patchFoot(view) {
    setText(parts.foot, view.foot.join(" · "));
    const total = view.snoozed.total;
    setHidden(parts.snoozed, total === 0);
    setText(parts.snoozedTitle, t("hoy.snoozed.title", { n: total }));
    const sig = JSON.stringify([view.snoozed.items, t("hoy.snoozed.back")]);
    if (sig === snoozedSig) return;
    snoozedSig = sig;
    parts.snoozedList.replaceChildren(...view.snoozed.items.map((x) => h("li", { class: "snoozed-row" },
      h("span", { class: "co" }, x.company), h("span", { class: "role" }, x.role), h("span", { class: "meta label" }, x.until),
      actButton({ label: t("hoy.snoozed.back"), "data-unsnooze": x.key }))));
  }

  function patch(view, { fresh = new Set(), first = false } = {}) {
    if (el.dataset.loading) { delete el.dataset.loading; parts.list.replaceChildren(); }
    setData(el, "calm", view.calm ? "true" : "false");
    patchAbout(view.about);
    setText(parts.eyebrow, view.eyebrow);
    setText(parts.h1, view.headline);
    // beside the headline only on a calm day: the count of what waits for his approval is already the gold figure below
    const ask = view.calm ? view.calmLine : "";
    setText(parts.ask, ask);
    setHidden(parts.ask, !ask);
    patchFigures(view, first);
    patchStrip(view.strip);
    setText(parts.base, view.base || "");
    setHidden(parts.base, !view.base);
    patchNext(view, fresh);
    patchRows(view, fresh);
    patchSince(view, fresh);
    patchFoot(view);
  }

  return {
    el,
    patch,
    rowView: viewOf,
    nextView: () => (nextCtl ? nextCtl.view : null),
    repaint(id) { const ctl = ctlOf(id); if (ctl) ctl.patch(ctl.view, stateOf(id)); },
    destroy() { for (const id of timers) clearTimeout(id); timers.clear(); rowCtls.clear(); nextCtl = null; },
  };
}
