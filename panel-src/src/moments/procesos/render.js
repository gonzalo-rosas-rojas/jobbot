// Draws PROCESOS (spec §5.2, board/relay2/procesos-1920.png) from the view of ./view.js. The page is BUILT ONCE and PATCHED after: every text
// goes through setText (a write only when it differs), the piles and rows are keyed by their id and kept (cells.js), the lists are brought to
// the new order with `sync` (which moves no node that is already where it should be), and a pile that changed lane TRAVELS to its new place
// (420 ms, the shared element of §3.3) while a tombstone keeps the hole it left. The list scrolls INSIDE the sheet: it is `data-lenis-prevent`,
// `overscroll-behavior: contain`, and a visible cue says how many more there are below and that the wheel moves the list, not the camera.
//
//   const page = createProcesos({ ui, run, reduced, raf });
//   page.el                              section.procesos  (host.append(page.el))
//   page.patch(view, { first })          the view of ./view.js; animates the piles in `view.moves` unless `first` or reduced motion
//   page.rowView(id)                     the view of a pile or row by its id ("card:fx:1"), the target of a key
//   page.repaint(id)                     paints one again (its armed or pending state changed)
//   page.armed(text | null)              the line of the foot that says "press again" (null: the legend)
//   page.focusFind() / page.error(retry) / page.destroy()
// `ui` is { get(), set(patch) }: the filter, the order, the query, the mode and whether the closed ones are shown, owned by procesos.js.

import { h, setText } from "../../core/dom.js";
import { t, tn, lang } from "../../i18n/index.js";
import { sync, setData, setHidden, stampChanged } from "../shared/util.js";
import { createPile, createTomb, createRow, tableHead, stackGlyph } from "./cells.js";
import { keyDisplay, tombIdOf, QUERY_MAX } from "./view.js";
import { createScrollCue } from "./scroll-cue.js";
import { positionsBefore, travelTo } from "./travel.js";

const SKELETON_LANES = 3;  // the grey bars drawn until the first view: this many lanes of this many cells
const SKELETON_CELLS = 6;
const LOW_WINDOW_QUERY = "(max-height: 760px) and (min-width: 760px)"; // a window this low gets the one short line of the legend

/** Drops from `map` what `keep` (a Set of keys) does not have. */
function forgetMissing(map, keep) {
  for (const id of [...map.keys()]) if (!keep.has(id)) map.delete(id);
}


const kbds = (id) => keyDisplay(id).split(/\s+/).filter(Boolean).flatMap((k, i) => (i ? [" ", h("kbd", { class: "k" }, k)] : [h("kbd", { class: "k" }, k)]));

export function createProcesos({ ui, run, reduced = () => false, raf = (fn) => requestAnimationFrame(fn) }) {
  const piles = new Map();   // id -> pile of the field (kept while the application is in the board)
  const tombs = new Map();   // tombIdOf(key) -> tombstone
  const rows = new Map();    // id -> row of the table
  const laneEls = new Map(); // lane id -> { el, name, n, you, note, list }
  let last = null;           // the last view patched: the closed ones are looked up in it
  let theadLang = lang();    // the language the header of the table was drawn in

  // ---- the parts that are patched
  const parts = {
    eyebrow: h("h1", { class: "eyebrow label", id: "procesos-title", tabindex: "-1" }),
    run: h("p", { class: "runline label" }),
    mega: h("b", { class: "mega cond num" }),
    pieces: h("span", { class: "pieces label" }),
    filters: h("div", { class: "crow", role: "group", "aria-label": t("procesos.filter.label") }),
    orders: h("div", { class: "crow", role: "group", "aria-label": t("procesos.order.label") }),
    find: h("input", { class: "find-input label", type: "text", autocomplete: "off", spellcheck: "false", maxlength: String(QUERY_MAX) }),
    mode: h("button", { class: "chip mode", type: "button", "data-mode-toggle": "" }),
    field: h("div", { class: "field" }),
    table: h("div", { class: "table", role: "table" }),
    thead: tableHead(),
    tbody: h("div", { class: "tbody", role: "rowgroup" }),
    closed: h("div", { class: "closed" }),
    empty: h("div", { class: "empty", role: "status" }),
    scroller: h("div", { class: "scroller", "data-lenis-prevent": "" }),
    cue: h("p", { class: "scroll-cue label", "aria-hidden": "true" }),
    thumb: h("i", { class: "thumb" }),
    legend: h("p", { class: "legend label" }),
    armed: h("p", { class: "armed label gold", role: "status" }),
  };
  parts.table.append(parts.tbody);
  parts.scroller.append(parts.field, parts.empty, parts.closed);
  const track = h("i", { class: "track", "aria-hidden": "true" }, parts.thumb);
  const viewbox = h("div", { class: "viewbox", "data-can-scroll": "false", "data-at-start": "true", "data-at-end": "true" }, parts.scroller, parts.cue, track);

  const chip = (kind, id, label, n, active) => h("button", { class: "chip", type: "button", ["data-" + kind]: id, "aria-pressed": active },
    h("span", { class: "chip-label" }, label), ...(n == null ? [] : [" ", h("span", { class: "chip-n num" }, String(n))]));

  // the skeleton, until the first view: three lanes of grey bars, so the sheet already has its shape
  const skeleton = h("div", { class: "skeleton", "aria-hidden": "true" }, ...Array.from({ length: SKELETON_LANES }, () => h("div", { class: "skel-lane" }, ...Array.from({ length: SKELETON_CELLS }, () => h("i", { class: "skel-cell" })))));
  parts.scroller.prepend(skeleton);

  const findLabel = h("label", { class: "find" }, h("kbd", { class: "k" }, "/"), " ", h("span", { class: "lab" }, ""), parts.find);
  const filterLab = h("span", { class: "lab-text" });
  const orderLab = h("span", { class: "lab-text" });
  const head = h("header", { class: "head" },
    h("div", { class: "head-line" }, parts.eyebrow, parts.run),
    h("div", { class: "head-main" },
      h("p", { class: "big" }, parts.mega, parts.pieces),
      h("div", { class: "controls label" },
        h("div", { class: "crow-wrap" }, h("span", { class: "lab" }, h("kbd", { class: "k" }, "f"), " ", filterLab), parts.filters),
        h("div", { class: "crow-wrap" }, h("span", { class: "lab" }, orderLab), parts.orders),
        h("div", { class: "crow-wrap tools" }, findLabel, parts.mode))));
  const foot = h("footer", { class: "foot" }, parts.legend, parts.armed);

  const el = h("section", { class: "procesos", "aria-labelledby": "procesos-title", "data-loading": "true", "data-mode": "field", "aria-busy": "true" },
    head, viewbox, foot, h("p", { class: "sr" }, t("procesos.loading")));

  // ---- one delegated listener for the page: nothing is bound per pile, so nothing is lost when one is rebuilt
  const viewOf = (id) => {
    const pile = piles.get(id) || rows.get(id);
    if (pile) return pile.view;
    return (last && last.closed.items.find((c) => c.id === id)) || null;
  };
  el.addEventListener("click", (e) => {
    const target = e.target;
    const filter = target.closest("button[data-filter]");
    if (filter) { ui.setFilter({ filter: filter.dataset.filter }); return; }
    const order = target.closest("button[data-order]");
    if (order) { ui.set({ order: order.dataset.order }); return; }
    if (target.closest("button[data-mode-toggle]")) { ui.set({ mode: ui.get().mode === "table" ? "field" : "table" }); return; }
    if (target.closest("button[data-closed-toggle]")) { ui.set({ showClosed: !ui.get().showClosed }); return; }
    if (target.closest("button[data-reset]")) { ui.setFilter({ filter: "all", query: "" }); return; }
    if (target.closest("button[data-retry]")) { ui.retry(); return; }
    const rowEl = target.closest("[data-row]");
    if (rowEl && !target.closest("a, summary")) {
      const view = viewOf(rowEl.dataset.row);
      if (view) run("open", view);
    }
  });
  parts.find.addEventListener("input", () => ui.set({ query: parts.find.value }));
  parts.find.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.stopPropagation(); // the page's own Esc (the drawer, the palette) is not this one's
      e.preventDefault();
      if (parts.find.value) { parts.find.value = ""; ui.set({ query: "" }); }
      else parts.find.blur();
    } else if (e.key === "Enter" || e.key === "ArrowDown") {
      e.stopPropagation();
      e.preventDefault();
      parts.find.blur();
      ui.toList();
    }
  });

  // ---- the cue under the list: how many are below, and that the wheel moves the list
  const scrollCue = createScrollCue({ scroller: parts.scroller, viewbox, cueEl: parts.cue, thumb: parts.thumb, observed: [parts.field, parts.tbody], reduced, raf });

  // ---- the head
  function patchHead(view) {
    setText(parts.eyebrow, view.eyebrow);
    setText(parts.run, [...view.run, view.stale ? t("procesos.stale", { ago: view.stale }) : ""].filter(Boolean).join(" · "));
    setData(parts.run, "stale", view.stale ? "true" : null);
    setText(parts.mega, String(view.open));
    patchPieces(view);
    patchChips(view);
    patchFind(view);
    patchMode(view);
  }

  /** The line under the big number: what the open ones are made of. */
  function patchPieces(view) {
    const sig = JSON.stringify([view.openLabel, view.pieces.map((p) => [p.id, p.text, p.gold])]);
    if (stampChanged(parts.pieces, sig)) {
      const kids = [h("span", { class: "open-label" }, view.openLabel)];
      for (const p of view.pieces) kids.push(" · ", h("span", { class: p.gold ? "gold" : null, "data-piece": p.id }, p.text));
      parts.pieces.replaceChildren(...kids);
    }
  }

  /** The chips of the filter and of the order. */
  function patchChips(view) {
    setText(filterLab, t("procesos.filter.label"));
    setText(orderLab, t("procesos.order.label"));
    parts.filters.setAttribute("aria-label", t("procesos.filter.label"));
    parts.orders.setAttribute("aria-label", t("procesos.order.label"));
    const fsig = JSON.stringify(view.filters.map((f) => [f.id, f.label, f.n, f.active]));
    if (stampChanged(parts.filters, fsig)) {
      parts.filters.replaceChildren(...view.filters.map((f) => chip("filter", f.id, f.label, f.n, f.active)));
    }
    const osig = JSON.stringify(view.orders.map((o) => [o.id, o.label, o.active]));
    if (stampChanged(parts.orders, osig)) {
      parts.orders.replaceChildren(...view.orders.map((o) => chip("order", o.id, o.label, null, o.active)));
    }
  }

  /** The search field: it keeps what is being typed while it has the focus. */
  function patchFind(view) {
    if (document.activeElement !== parts.find && parts.find.value !== view.query) parts.find.value = view.query;
    parts.find.setAttribute("aria-label", t("procesos.search.aria"));
    parts.find.setAttribute("placeholder", t("procesos.search.placeholder"));
    setText(findLabel.querySelector(".lab"), t("procesos.search.label"));
  }

  /** The button of the mode, which names the view it goes to. */
  function patchMode(view) {
    const goes = t(view.mode === "table" ? "procesos.view.field" : "procesos.view.table"); // the button names the view it goes to
    if (stampChanged(parts.mode, goes)) {
      parts.mode.replaceChildren(h("kbd", { class: "k" }, keyDisplay("field")), " ", h("span", { class: "chip-label" }, goes));
      parts.mode.setAttribute("aria-label", t("procesos.view.label") + ": " + goes);
    }
  }

  // ---- the field
  function laneOf(lane) {
    let ctl = laneEls.get(lane.id);
    if (!ctl) {
      ctl = {
        name: h("span", { class: "lane-name" }),
        n: h("b", { class: "lane-n num" }),
        you: h("b", { class: "gold lane-you" }),
        note: h("span", { class: "lane-note" }),
        list: h("ul", { class: "cells" }),
      };
      ctl.el = h("section", { class: "lane", "data-lane": lane.id }, h("h3", { class: "lane-h label" }, ctl.name, " ", ctl.n, " ", ctl.you, " ", ctl.note), ctl.list);
      laneEls.set(lane.id, ctl);
    }
    return ctl;
  }

  function stateOf(id) { return ui.stateOf(id); }

  function patchField(view, travel) {
    const before = travel ? positionsBefore(view.moves, piles) : new Map();
    const wanted = new Set();
    const twanted = new Set();
    const lanes = view.lanes.map((lane) => {
      const ctl = laneOf(lane);
      const kids = lane.cells.map((cell) => {
        if (cell.tomb != null) {
          const tid = tombIdOf(cell.tomb);
          twanted.add(tid);
          let tomb = tombs.get(tid);
          if (!tomb) { tomb = createTomb(cell); tombs.set(tid, tomb); }
          tomb.patch(cell);
          return tomb.el;
        }
        wanted.add(cell.id);
        let pile = piles.get(cell.id);
        if (!pile) { pile = createPile(cell); piles.set(cell.id, pile); }
        pile.patch(cell, stateOf(cell.id));
        return pile.el;
      });
      sync(ctl.list, kids);
      setText(ctl.name, lane.label);
      const shown = lane.cells.filter((c) => c.tomb == null).length;
      setText(ctl.n, shown === lane.total ? String(lane.total) : t("procesos.lane.count", { n: shown, total: lane.total }));
      setText(ctl.you, lane.you > 0 ? tn("procesos.lane.you", lane.you) : "");
      setHidden(ctl.you, lane.you === 0);
      setText(ctl.note, lane.note);
      setData(ctl.el, "quiet", lane.quiet ? "true" : null);
      return ctl.el;
    });
    sync(parts.field, lanes);
    forgetMissing(piles, wanted);
    forgetMissing(tombs, twanted);
    forgetMissing(laneEls, new Set(view.lanes.map((l) => l.id)));
    if (before.size) travelTo(before, piles, reduced);
  }

  // ---- the table
  function patchTable(view) {
    const wanted = new Set(view.table.map((r) => r.id));
    forgetMissing(rows, wanted);
    const els = view.table.map((row) => {
      let ctl = rows.get(row.id);
      if (!ctl) { ctl = createRow(row); rows.set(row.id, ctl); }
      ctl.patch(row, stateOf(row.id));
      return ctl.el;
    });
    sync(parts.tbody, els);
    if (theadLang !== lang()) { // the language changed
      const next = tableHead();
      theadLang = lang();
      parts.thead.replaceWith(next);
      parts.thead = next;
    }
  }

  // ---- the applications that ended
  function patchClosed(view) {
    const c = view.closed;
    setHidden(parts.closed, c.total === 0);
    if (c.total === 0) return;
    const shown = ui.get().showClosed;
    const sig = JSON.stringify([c.total, c.accepted, c.rejected, c.withdrawn, shown, c.more, c.items.map((i) => i.id + i.state + i.days), t("procesos.closed.show")]);
    if (!stampChanged(parts.closed, sig)) return;
    const line = h("p", { class: "closedline label" },
      h("b", { class: "closed-title" }, tn("procesos.head.closed", c.total)), " · ",
      t("procesos.closed.detail", { rejected: c.rejected, withdrawn: c.withdrawn, accepted: c.accepted }), " · ",
      t("procesos.closed.out") + ": ",
      h("button", { class: "closed-toggle", type: "button", "data-closed-toggle": "", "aria-expanded": shown }, t(shown ? "procesos.closed.hide" : "procesos.closed.show")));
    const kids = [line];
    if (shown) {
      kids.push(h("ul", { class: "cells closed-cells" }, c.items.map((cell) => {
        const pile = createPile(cell);
        pile.btn.dataset.closed = "true";
        pile.btn.title = [cell.company, cell.title, t("procesos.closed.stage", { state: cell.stage, days: cell.age })].filter(Boolean).join(" · ");
        return pile.el;
      })));
      if (c.more > 0) kids.push(h("p", { class: "closed-more label" }, t("procesos.closed.more", { n: c.more })));
    }
    parts.closed.replaceChildren(...kids);
  }

  function patchEmpty(view) {
    setHidden(parts.empty, !view.empty);
    if (!view.empty) return;
    const sig = JSON.stringify([view.empty.kind, view.empty.text, t("procesos.empty.filter.reset")]);
    if (!stampChanged(parts.empty, sig)) return;
    setData(parts.empty, "kind", view.empty.kind);
    // (replaceChildren turns a null into the text "null": only real nodes go in)
    parts.empty.replaceChildren(...[h("p", { class: "empty-text wrap-any" }, view.empty.text), view.empty.kind === "none" ? null : h("button", { class: "chip", type: "button", "data-reset": "" }, t("procesos.empty.filter.reset"))].filter(Boolean));
  }

  // ---- the foot: the key of a pile, and what the piles mean
  const lowWindow = typeof matchMedia === "function" ? matchMedia(LOW_WINDOW_QUERY) : null;
  const onLow = () => { if (last) patchLegend(last); };
  if (lowWindow && lowWindow.addEventListener) lowWindow.addEventListener("change", onLow);

  function patchLegend(view) {
    const low = !!(lowWindow && lowWindow.matches); // a low window gets one short line, so the list keeps its room
    const sig = JSON.stringify([view.mode, low, t("procesos.legend.piles")]);
    if (!stampChanged(parts.legend, sig)) return;
    const youMark = () => h("span", { class: "gold you-mark" }, "▌ " + t("procesos.legend.you"));
    if (view.mode === "table") {
      const base = [t("procesos.legend.table"), " · ", ...kbds("move"), " " + t("procesos.legend.move"), " · ", ...kbds("open"), " " + t("procesos.legend.open")];
      const extra = low ? [] : [" · ", ...kbds("withdraw"), " " + t("procesos.legend.withdraw"), " · ", ...kbds("snooze"), " " + t("procesos.legend.snooze"), " · " + t("procesos.legend.wheel")];
      parts.legend.replaceChildren(...base, ...extra);
    } else if (low) {
      parts.legend.replaceChildren(stackGlyph("demo"), t("procesos.legend.short"), " · ", youMark());
    } else {
      parts.legend.replaceChildren(stackGlyph("demo"), t("procesos.legend.piles"), " · " + t("procesos.legend.tomb"), " · ", youMark());
    }
  }

  function patch(view, { first: isFirst = false } = {}) {
    last = view;
    if (el.dataset.loading) {
      delete el.dataset.loading;
      el.removeAttribute("aria-busy");
      skeleton.remove();
      const sr = el.querySelector(":scope > .sr");
      if (sr) sr.remove();
    }
    patchHead(view);
    setData(el, "mode", view.mode);
    // only the view that is shown is in the page: a hidden one would leave text that nobody sees
    if (view.mode === "table") {
      if (!parts.table.isConnected) parts.field.replaceWith(parts.table);
      if (!parts.thead.isConnected) viewbox.prepend(parts.thead);
    } else {
      if (!parts.field.isConnected) parts.table.replaceWith(parts.field);
      parts.thead.remove();
    }
    // what the other mode drew is not on screen any more: its views must not answer a key or take a repaint (`viewOf`, `repaint`)
    if (view.mode === "table") { piles.clear(); patchTable(view); }
    else { rows.clear(); patchField(view, !isFirst && !reduced()); }
    patchEmpty(view);
    patchClosed(view);
    patchLegend(view);
    scrollCue.queue();
  }

  return {
    el,
    patch,
    rowView: viewOf,
    repaint(id) {
      const ctl = piles.get(id) || rows.get(id);
      if (ctl) ctl.patch(ctl.view, stateOf(id));
    },
    armed(text) {
      setText(parts.armed, text || "");
      setHidden(parts.armed, !text);
      setHidden(parts.legend, !!text);
    },
    focusFind() { parts.find.focus({ preventScroll: true }); parts.find.select(); },
    error(retry) {
      delete el.dataset.loading;
      el.removeAttribute("aria-busy");
      setData(el, "failed", "true");
      parts.thead.remove();
      parts.scroller.replaceChildren(h("div", { class: "fail", role: "alert" }, h("p", { class: "fail-text" }, t("procesos.error")), h("button", { class: "chip", type: "button", "data-retry": "" }, t("procesos.error.retry"))));
      ui.setRetry(retry);
    },
    clearError() {
      if (!el.dataset.failed) return;
      delete el.dataset.failed;
      parts.scroller.replaceChildren(ui.get().mode === "table" ? parts.table : parts.field, parts.empty, parts.closed);
    },
    destroy() {
      scrollCue.destroy();
      if (lowWindow && lowWindow.removeEventListener) lowWindow.removeEventListener("change", onLow);
      piles.clear(); tombs.clear(); rows.clear(); laneEls.clear();
    },
  };
}
