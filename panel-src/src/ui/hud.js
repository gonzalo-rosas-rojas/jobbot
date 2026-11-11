// The top strip of the stage (spec §4.6, §6.1), 56 px: the mark and the name of the stop on the left; in the middle the stops
// bar (stops.js puts itself into `center`); on the right the state of the night window and of the engine, the palette key and
// the hour. It says what the snapshot says and nothing it cannot know: the run is only ever worded from `controls.last_run_at`
// (A15), and a title bar that names the job search is not here: the mark says "Panel" (spec §10.4).

import { h } from "../core/dom.js";
import { t } from "../i18n/index.js";
import { ago, nextWindowLabel } from "../core/format.js";
import { serverDay } from "../core/time.js";
import { WINDOW_END } from "../scene/clock.js";
import { KEYS } from "./keys.js";
import { icon } from "./icons.js";

const two = (n) => String(n).padStart(2, "0");

/**
 * The two phrases of the right side, from a snapshot and "now": { window: "Cerrada · abre 18:00", run: "Corrida hace 7 h" }.
 * `window` also says when the STOP flag is on. `run` is "Sin corridas todavía" only when the engine says it never ran, and is
 * left empty when the snapshot says nothing about it. Before the first snapshot both are empty: nothing is better than a guess.
 */
export function statusParts(snap, nowMs) {
  const c = snap && snap.controls;
  if (!c) return { window: "", run: "" };
  let window;
  if (c.window_open === true) window = t("hud.window.open", { until: two(WINDOW_END) + ":00" });
  else if (c.window_open === false) window = c.next_window ? t("hud.window.closed", { when: nextWindowLabel(c.next_window, serverDay(nowMs)) }) : t("hud.window.closedNoNext");
  else window = t("hud.window.unknown");
  if (c.stopped === true) window = t("run.stopped") + " · " + window;
  let run = "";
  if (c.last_run_at) run = t("hud.run.ago", { ago: ago(c.last_run_at, nowMs) });
  else if (c.engine === "never") run = t("hud.run.never");
  return { window, run };
}

/**
 * createHud({ onSearch, extra, brand }) -> { el, center, setStop(id | null), setStatus(snap, nowMs), setClock(text), relabel() }
 * `brand` is the name the mark shows when the page is not the real panel (the public demo calls itself so); without it the mark says "Panel".
 * `extra` is one more button of the right side that another module owns (the sound's indicator, ui/sound-ui.js): it sits between the state and the key of the palette.
 * `setStop` takes any of the six stop ids; `setStatus` keeps what it was given so that `relabel()` (a change of language)
 * can word it again.
 */
export function createHud({ onSearch = () => {}, extra = null, brand } = {}) {
  const search = KEYS.find((k) => k.id === "search");
  let stop = null;
  let last = { snap: null, nowMs: 0 };

  const mark = h("span", { class: "hud-brand label" });
  const name = h("span", { class: "hud-stop label" });
  const center = h("div", { class: "hud-center" });
  const status = h("p", { class: "hud-status label" });
  // the keys and the word, like "? Atajos" below: two key caps alone did not say what they open (2026-10-24)
  const word = h("span", { class: "hud-search-word" });
  const button = h("button", { class: "hud-search", type: "button", "aria-haspopup": "dialog", "aria-keyshortcuts": "Control+K", onclick: () => onSearch() },
    icon("search", "icon-lg"), ...search.display.split(" ").map((k) => h("kbd", {}, k)), word);      // the icon shows only where there is no keyboard (stage.css)
  const clock = h("span", { class: "hud-clock mono num" });
  const el = h("header", { id: "hud", class: "hud" },
    h("div", { class: "hud-left" }, mark, name),
    center,
    h("div", { class: "hud-right" }, status, extra, button, clock));

  function paintStatus() {
    const { window, run } = statusParts(last.snap, last.nowMs);
    status.textContent = [window, run].filter(Boolean).join(" · ");
    status.hidden = status.textContent === "";
  }

  function relabel() {
    mark.textContent = brand || t("hud.brand");
    name.textContent = stop ? t("tab." + stop) : "";
    button.setAttribute("aria-label", t("ctrl.search"));
    word.textContent = t("ctrl.search");
    button.title = t("ctrl.search.hint");
    paintStatus();
  }
  relabel();

  return {
    el,
    center,
    setStop(id) { stop = id; name.textContent = id ? t("tab." + id) : ""; },
    setStatus(snap, nowMs) { last = { snap, nowMs }; paintStatus(); },
    setClock(text) { if (clock.textContent !== text) clock.textContent = text; },
    relabel,
  };
}
