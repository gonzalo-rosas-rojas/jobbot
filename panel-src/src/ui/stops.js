// The stops bar, the middle of the HUD (spec §6.1): `1 HOY · 2 PROCESOS 74 · 3 COLA 2 · 4 APRENDER 12 · 5 SISTEMA ●`. A tab list of
// five tabs (the prologue is not a tab: it has no number to press but 0). The active one is gold with "estás aquí" under it; in
// the prologue none is lit and the caption says "ANOCHE" where "estás aquí" would be. Click or Enter selects, the arrows, Home
// and End walk the tabs and select as they go, like the tab lists of every toolkit.
// The flat page (under 760 px, spec §3.11) has no stage and no number keys on the screen: there the prologue is a sixth tab,
// "Anoche", first in the strip (`setFlat(true)`), and lit like any other.
//
// What a number means (the same ones `stopCounts(model)` writes; a zero says nothing, so it is not drawn):
//   PROCESOS  live processes        COLA  proposals waiting for approval        APRENDER  reviews due
//   SISTEMA   a dot: filled when the engine ran recently, hollow when it is behind or never ran (never mint: spec §4.1, §6.1)

import { h } from "../core/dom.js";
import { t, tn } from "../i18n/index.js";
import { STOP_IDS } from "../scene/poses.js";
import { KEYS } from "./keys.js";

const TABS = STOP_IDS.slice(1);                                  // the five: the prologue is not a tab
const KEY_OF = { ...Object.fromEntries(TABS.map((id, i) => [id, KEYS.find((k) => k.id === "go").keys[i]])), prologo: KEYS.find((k) => k.id === "prologue").keys[0] };
const COUNTED = ["procesos", "cola", "aprender"];

/** The words a count is read as ("74 procesos vivos", "motor atrasado"); "" when there is nothing to say. */
export function countWords(id, counts) {
  const c = counts || {};
  if (id === "sistema") return c.sistema ? t("stops.engine." + c.sistema) : "";
  return COUNTED.includes(id) && c[id] > 0 ? tn("stops.count." + id, c[id]) : "";
}

export function createStops({ onSelect }) {
  const counts = {};
  const parts = new Map();
  const tabs = new Map();
  let active = null;
  let order = TABS;                                              // the tabs that exist now: five, or six on the flat page

  function build(id) {
    const count = h("span", { class: "stop-count num", hidden: true, "aria-hidden": "true" });
    const dot = h("span", { class: "stop-dot", hidden: true, "aria-hidden": "true" });
    const name = h("span", { class: "stop-name" });
    const here = h("span", { class: "stop-here", "aria-hidden": "true" });
    const tab = h("button", {
      class: "stop label", type: "button", role: "tab", id: "tab-" + id, "data-stop": id,
      "aria-selected": false, "aria-controls": "app", "aria-keyshortcuts": KEY_OF[id], tabindex: -1,
      onclick: () => onSelect(id),
    }, h("span", { class: "stop-key", "aria-hidden": "true" }, KEY_OF[id]), name, count, dot, here);
    tabs.set(id, tab);
    parts.set(id, { name, count, dot, here });
    return tab;
  }
  for (const id of TABS) build(id);

  const list = h("div", { class: "stops-list", role: "tablist", "aria-orientation": "horizontal" }, ...tabs.values());
  const night = h("span", { class: "stops-night label", "aria-hidden": "true" });
  const el = h("nav", { id: "stops", class: "stops" }, list, night);

  list.addEventListener("keydown", (e) => {
    const at = order.findIndex((id) => tabs.get(id) === document.activeElement);
    if (at < 0) return;
    const next = e.key === "ArrowRight" ? (at + 1) % order.length : e.key === "ArrowLeft" ? (at + order.length - 1) % order.length
      : e.key === "Home" ? 0 : e.key === "End" ? order.length - 1 : -1;
    if (next < 0) return;
    e.preventDefault();
    onSelect(order[next]);
    tabs.get(order[next]).focus();
  });

  function paintCounts() {
    for (const id of TABS) {
      const { count, dot } = parts.get(id);
      const n = counts[id];
      count.hidden = !(COUNTED.includes(id) && n > 0);
      count.textContent = count.hidden ? "" : String(n);
      dot.hidden = id !== "sistema" || !counts.sistema;
      if (!dot.hidden) dot.dataset.state = counts.sistema;
    }
  }

  function relabel() {
    list.setAttribute("aria-label", t("stops.aria"));
    el.setAttribute("aria-label", t("stops.aria"));
    night.textContent = t("tab.prologo");
    for (const id of order) {
      const { name, here } = parts.get(id);
      const words = countWords(id, counts);
      name.textContent = t("tab." + id);
      here.textContent = t("stops.here");
      tabs.get(id).setAttribute("aria-label", words ? t("tab." + id) + ". " + words : t("tab." + id));
    }
  }

  function setActive(id) {
    active = order.includes(id) ? id : null;
    el.dataset.at = id || "prologo";
    for (const tabId of order) {
      const on = tabId === active;
      const tab = tabs.get(tabId);
      tab.setAttribute("aria-selected", String(on));
      tab.tabIndex = on || (active === null && tabId === order[0]) ? 0 : -1;          // one tab stop in the list, whatever the stop
    }
    // The strip of a narrow screen scrolls to the lit tab by its own scrollLeft and nothing else: scrollIntoView would also move every
    // scrolling ancestor, and on the flat page the sticky strip sits inside the document, which must stay where the person left it (on a
    // desktop it is the document Lenis is scrolling). Only when the strip really overflows.
    const box = el.parentElement;
    if (active && box && box.scrollWidth > box.clientWidth) {
      const a = tabs.get(active).getBoundingClientRect();
      const b = box.getBoundingClientRect();
      const room = 16;
      if (a.left < b.left + room) box.scrollLeft += a.left - b.left - room;
      else if (a.right > b.right - room) box.scrollLeft += a.right - b.right + room;
    }
  }

  /** The flat page has the prologue as a sixth tab, "Anoche", first in the strip; the desktop one has five. Light the right tab afterwards. */
  function setFlat(on) {
    if (on === order.includes("prologo")) return;
    if (on) { list.prepend(build("prologo")); order = STOP_IDS; }
    else { tabs.get("prologo").remove(); tabs.delete("prologo"); parts.delete("prologo"); order = TABS; if (active === "prologo") active = null; }
    relabel();
    setActive(active);
  }

  relabel();
  setActive(null);
  return {
    el,
    tabs,
    get active() { return active; },
    setActive,
    setFlat,
    setCounts(next) { Object.assign(counts, next); paintCounts(); relabel(); },
    relabel,
  };
}
