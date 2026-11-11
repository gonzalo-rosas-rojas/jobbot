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

import { h } from "../../core/dom.js";
import { t, tn } from "../../i18n/index.js";
import { sync, setText, setData, setHidden } from "../shared/util.js";
import { createPile, createTomb, createRow, tableHead, stackGlyph } from "./cells.js";
import { keyDisplay } from "./view.js";

export const TRAVEL_MS = 420;
const TRAVEL_EASE = "cubic-bezier(0.2, 0, 0, 1)"; // --ease-jump

const kbds = (id) => keyDisplay(id).split(/\s+/).filter(Boolean).flatMap((k, i) => (i ? [" ", h("kbd", { class: "k" }, k)] : [h("kbd", { class: "k" }, k)]));

export function createProcesos({ ui, run, reduced = () => false, raf = (fn) => requestAnimationFrame(fn) }) {
  const piles = new Map();   // id -> pile of the field (kept while the application is in the board)
  const tombs = new Map();   // "tomb:key" -> tombstone
  const rows = new Map();    // id -> row of the table
  const laneEls = new Map(); // lane id -> { el, name, n, you, note, list }
  let last = null;           // the last view patched: the closed ones are looked up in it

  // ---- the parts that are patched
  const parts = {
    eyebrow: h("h1", { class: "eyebrow label", id: "procesos-title", tabindex: "-1" }),
    run: h("p", { class: "runline label" }),
    mega: h("b", { class: "mega cond num" }),
    pieces: h("span", { class: "pieces label" }),
    filters: h("div", { class: "crow", role: "group", "aria-label": t("procesos.filter.label") }),
    orders: h("div", { class: "crow", role: "group", "aria-label": t("procesos.order.label") }),
    find: h("input", { class: "find-input label", type: "text", autocomplete: "off", spellcheck: "false", maxlength: "60" }),
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

  const chip = (kind, id, label, n, active) => {
    const b = h("button", { class: "chip", type: "button", ["data-" + kind]: id, "aria-pressed": active }, h("span", { class: "chip-label" }, label), n == null ? null : " ", n == null ? null : h("span", { class: "chip-n num" }, String(n)));
    return b;
  };

  // the skeleton, until the first view: three lanes of grey bars, so the sheet already has its shape
  const skeleton = h("div", { class: "skeleton", "aria-hidden": "true" }, ...[0, 1, 2].map(() => h("div", { class: "skel-lane" }, ...Array.from({ length: 6 }, () => h("i", { class: "skel-cell" })))));
  parts.scroller.prepend(skeleton);

  const find = h("label", { class: "find" }, h("kbd", { class: "k" }, "/"), " ", h("span", { class: "lab" }, ""), parts.find);
  const head = h("header", { class: "head" },
    h("div", { class: "head-line" }, parts.eyebrow, parts.run),
    h("div", { class: "head-main" },
      h("p", { class: "big" }, parts.mega, parts.pieces),
      h("div", { class: "controls label" },
        h("div", { class: "crow-wrap" }, h("span", { class: "lab" }, h("kbd", { class: "k" }, "f"), " ", h("span", { class: "lab-text" })), parts.filters),
        h("div", { class: "crow-wrap" }, h("span", { class: "lab" }, h("span", { class: "lab-text" })), parts.orders),
        h("div", { class: "crow-wrap tools" }, find, parts.mode))));
  const filterLab = head.querySelectorAll(".lab-text")[0];
  const orderLab = head.querySelectorAll(".lab-text")[1];
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
    if (filter) { ui.set({ filter: filter.dataset.filter }); return; }
    const order = target.closest("button[data-order]");
    if (order) { ui.set({ order: order.dataset.order }); return; }
    if (target.closest("button[data-mode-toggle]")) { ui.set({ mode: ui.get().mode === "table" ? "field" : "table" }); return; }
    if (target.closest("button[data-closed-toggle]")) { ui.set({ showClosed: !ui.get().showClosed }); return; }
    if (target.closest("button[data-reset]")) { ui.set({ filter: "all", query: "" }); return; }
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
  let cueQueued = false;
  // All the reads first, all the writes after: a write between two reads makes the next read recalculate styles on the spot, and a pile of PROCESOS
  // has about a thousand elements (that was 16 ms at CPU x4 for each read that followed a write, in the frame right after a trip's swap: ACEPTACION A7).
  function measure() {
    cueQueued = false;
    const box = parts.scroller;
    const max = box.scrollHeight - box.clientHeight;
    const can = max > 2;
    const top = box.scrollTop;
    const atEnd = !can || top >= max - 2;
    let below = 0;
    let thumb = null;
    if (can) {
      const bottom = box.getBoundingClientRect().bottom;
      for (const node of box.querySelectorAll("[data-card]")) if (node.getBoundingClientRect().top >= bottom - 2) below += 1;
      thumb = { top: ((top / box.scrollHeight) * 100).toFixed(2) + "%", height: Math.max(6, (box.clientHeight / box.scrollHeight) * 100).toFixed(2) + "%" };
    }
    setData(viewbox, "canScroll", can ? "true" : "false");
    setData(viewbox, "atStart", top <= 2 ? "true" : "false");
    setData(viewbox, "atEnd", atEnd ? "true" : "false");
    if (can) {
      setText(parts.cue, [below > 0 ? t("procesos.cue.below", { n: below }) : "", t("procesos.legend.wheel")].filter(Boolean).join(" · "));
      parts.thumb.style.setProperty("top", thumb.top);
      parts.thumb.style.setProperty("height", thumb.height);
    }
  }
  const queueMeasure = () => { if (!cueQueued) { cueQueued = true; raf(measure); } };
  parts.scroller.addEventListener("scroll", queueMeasure, { passive: true });
  const resizer = typeof ResizeObserver === "function" ? new ResizeObserver(queueMeasure) : null;
  if (resizer) { resizer.observe(parts.scroller); resizer.observe(parts.field); resizer.observe(parts.tbody); }

  // The bottom of the list fades out (css) and the shared cursor only brings a row to the EDGE of the list, which is inside the fade: so the
  // row under the cursor is lifted out of it, and a row that is walked to is always clear to read.
  const FADE = 64;
  const FADE_TOP = 22;
  const lifter = typeof MutationObserver === "function" ? new MutationObserver(() => {
    if (viewbox.dataset.canScroll !== "true") return;
    const row = parts.scroller.querySelector('[data-cursor="true"]');
    if (!row) return;
    const at = row.getBoundingClientRect();
    const box = parts.scroller.getBoundingClientRect();
    const under = box.bottom - FADE - at.bottom; // negative: the row is in the fade at the bottom
    const above = box.top + FADE_TOP - at.top;   // positive: the row is in the fade at the top
    const top = under < -1 ? -under : above > 1 ? -above : 0;
    if (top) parts.scroller.scrollBy({ top, behavior: reduced() ? "auto" : "smooth" });
  }) : null;
  if (lifter) lifter.observe(parts.scroller, { subtree: true, attributes: true, attributeFilter: ["data-cursor"] });

  // ---- the head
  function patchHead(view) {
    setText(parts.eyebrow, view.eyebrow);
    setText(parts.run, [...view.run, view.stale ? t("procesos.stale", { ago: view.stale }) : ""].filter(Boolean).join(" · "));
    setData(parts.run, "stale", view.stale ? "true" : null);
    setText(parts.mega, String(view.open));
    const sig = JSON.stringify([view.openLabel, view.pieces.map((p) => [p.id, p.text, p.gold])]);
    if (parts.pieces.dataset.sig !== sig) {
      parts.pieces.dataset.sig = sig;
      const kids = [h("span", { class: "open-label" }, view.openLabel)];
      for (const p of view.pieces) kids.push(" · ", h("span", { class: p.gold ? "gold" : null, "data-piece": p.id }, p.text));
      parts.pieces.replaceChildren(...kids);
    }
    setText(filterLab, t("procesos.filter.label"));
    setText(orderLab, t("procesos.order.label"));
    parts.filters.setAttribute("aria-label", t("procesos.filter.label"));
    parts.orders.setAttribute("aria-label", t("procesos.order.label"));
    const fsig = JSON.stringify(view.filters.map((f) => [f.id, f.label, f.n, f.active]));
    if (parts.filters.dataset.sig !== fsig) {
      parts.filters.dataset.sig = fsig;
      parts.filters.replaceChildren(...view.filters.map((f) => chip("filter", f.id, f.label, f.n, f.active)));
    }
    const osig = JSON.stringify(view.orders.map((o) => [o.id, o.label, o.active]));
    if (parts.orders.dataset.sig !== osig) {
      parts.orders.dataset.sig = osig;
      parts.orders.replaceChildren(...view.orders.map((o) => chip("order", o.id, o.label, null, o.active)));
    }
    if (document.activeElement !== parts.find && parts.find.value !== view.query) parts.find.value = view.query;
    parts.find.setAttribute("aria-label", t("procesos.search.aria"));
    parts.find.setAttribute("placeholder", t("procesos.search.placeholder"));
    setText(find.querySelector(".lab"), t("procesos.search.label"));
    const goes = t(view.mode === "table" ? "procesos.view.field" : "procesos.view.table"); // the button names the view it goes to
    if (parts.mode.dataset.sig !== goes) {
      parts.mode.dataset.sig = goes;
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
    const before = new Map();
    if (travel) for (const m of view.moves) { const p = piles.get("card:" + m.key); if (p && p.el.isConnected) before.set(p.id, p.el.getBoundingClientRect()); }
    const wanted = new Set();
    const twanted = new Set();
    const lanes = view.lanes.map((lane) => {
      const ctl = laneOf(lane);
      const kids = lane.cells.map((cell) => {
        if (cell.tomb != null) {
          const tid = "tomb:" + cell.tomb;
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
      setData(ctl.el, "quiet", lane.id === "ghosted" ? "true" : null);
      return ctl.el;
    });
    sync(parts.field, lanes);
    for (const id of [...piles.keys()]) if (!wanted.has(id)) piles.delete(id);
    for (const id of [...tombs.keys()]) if (!twanted.has(id)) tombs.delete(id);
    for (const id of [...laneEls.keys()]) if (!view.lanes.some((l) => l.id === id)) laneEls.delete(id);
    if (before.size) travelTo(before);
  }

  /** The pile that changed lane goes from where it was to where it is (a shared element): measured after the move, played with the page's own clock. */
  function travelTo(before) {
    if (reduced()) return;
    for (const [id, from] of before) {
      const pile = piles.get(id);
      if (!pile || !pile.el.isConnected || typeof pile.el.animate !== "function") continue;
      const to = pile.el.getBoundingClientRect();
      const dx = from.left - to.left;
      const dy = from.top - to.top;
      if ((dx === 0 && dy === 0) || Math.abs(dy) > window.innerHeight * 1.5) continue; // too far to follow with the eye: it simply arrives
      pile.el.dataset.travel = "true";
      const anim = pile.el.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], { duration: TRAVEL_MS, easing: TRAVEL_EASE });
      const done = () => { delete pile.el.dataset.travel; };
      anim.addEventListener("finish", done);
      anim.addEventListener("cancel", done);
    }
  }

  // ---- the table
  function patchTable(view) {
    const wanted = new Set(view.table.map((r) => r.id));
    for (const id of [...rows.keys()]) if (!wanted.has(id)) rows.delete(id);
    const els = view.table.map((row) => {
      let ctl = rows.get(row.id);
      if (!ctl) { ctl = createRow(row); rows.set(row.id, ctl); }
      ctl.patch(row, stateOf(row.id));
      return ctl.el;
    });
    sync(parts.tbody, els);
    if (parts.thead.firstElementChild.textContent !== t("procesos.col.process")) { // the language changed
      const next = tableHead();
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
    if (parts.closed.dataset.sig === sig) return;
    parts.closed.dataset.sig = sig;
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
    if (parts.empty.dataset.sig === sig) return;
    parts.empty.dataset.sig = sig;
    setData(parts.empty, "kind", view.empty.kind);
    // (replaceChildren turns a null into the text "null": only real nodes go in)
    parts.empty.replaceChildren(...[h("p", { class: "empty-text wrap-any" }, view.empty.text), view.empty.kind === "none" ? null : h("button", { class: "chip", type: "button", "data-reset": "" }, t("procesos.empty.filter.reset"))].filter(Boolean));
  }

  // ---- the foot: the key of a pile, and what the piles mean
  function patchLegend(view) {
    const low = !!(lowWindow && lowWindow.matches); // a low window gets one short line, so the list keeps its room
    const sig = JSON.stringify([view.mode, low, t("procesos.legend.piles")]);
    if (parts.legend.dataset.sig === sig) return;
    parts.legend.dataset.sig = sig;
    if (view.mode === "table") {
      parts.legend.replaceChildren(t("procesos.legend.table"), " · ", ...kbds("move"), " " + t("procesos.legend.move"), " · ", ...kbds("open"), " " + t("procesos.legend.open"),
        ...(low ? [] : [" · ", ...kbds("withdraw"), " " + t("procesos.legend.withdraw"), " · ", ...kbds("snooze"), " " + t("procesos.legend.snooze"), " · " + t("procesos.legend.wheel")]));
    } else if (low) {
      parts.legend.replaceChildren(stackGlyph("demo"), t("procesos.legend.short"), " · ", h("span", { class: "gold you-mark" }, "▌ " + t("procesos.legend.you")));
    } else {
      parts.legend.replaceChildren(stackGlyph("demo"), t("procesos.legend.piles"), " · " + t("procesos.legend.tomb"), " · ", h("span", { class: "gold you-mark" }, "▌ " + t("procesos.legend.you")));
    }
  }
  const lowWindow = typeof matchMedia === "function" ? matchMedia("(max-height: 760px) and (min-width: 760px)") : null;
  const onLow = () => { if (last) patchLegend(last); };
  if (lowWindow && lowWindow.addEventListener) lowWindow.addEventListener("change", onLow);

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
    queueMeasure();
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
    measure,
    destroy() {
      if (resizer) resizer.disconnect();
      if (lifter) lifter.disconnect();
      if (lowWindow && lowWindow.removeEventListener) lowWindow.removeEventListener("change", onLow);
      piles.clear(); tombs.clear(); rows.clear(); laneEls.clear();
    },
  };
}
