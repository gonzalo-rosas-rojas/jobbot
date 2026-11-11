// The help sheet (the "?" key and the bar's button): how the page is used, then the keys, generated from the table in keys.js for the stop on screen, in
// the active language. Esc, a click outside or the close button close it, and the focus goes back to where it was. A key that no stop has
// wired yet (`live`) is not listed: a shortcut that does nothing is not announced. The sheet is a modal dialog, so the page behind it must
// not be reachable: `onToggle(open)` is how the shell makes it `inert` while the sheet is open (it runs after the focus is inside the
// sheet when it opens, and before the focus goes back when it closes: an inert element cannot take the focus).

import { h } from "../core/dom.js";
import { t } from "../i18n/index.js";
import { STOPS_META } from "../moments/registry.js";
import { keysFor } from "./keys.js";

/** [{ id, display, label, scope }] for a stop, in table order. */
export const helpRows = (stopId, live = null) => keysFor(stopId, { live }).map((k) => ({ id: k.id, display: k.display, label: k.label, scope: k.scope }));

/**
 * The sheet as data: { title, ask, usage[], groups[{ title, rows[{ key, label }] }] }. `usage` is how the page is used, in words and before the keys; on the flat
 * page (a phone, `flat`) it is said for the finger, has no line about keys, and `groups` is empty: a hand with no keyboard has no key to be told.
 * Without a stop, or with one the table of stops does not have, `ask` is null and the keys are the ones of the whole page.
 */
export function helpModel(stopId, { live = null, flat = false } = {}) {
  const rows = flat ? [] : helpRows(stopId, live);
  const group = (title, list) => ({ title, rows: list.map((r) => ({ key: r.display, label: t(r.label) })) });
  const stop = STOPS_META.find((s) => s.id === stopId);
  return {
    title: t("keys.title"),
    ask: stop ? t(stop.ask) : null,
    usage: flat ? [t("keys.usage.row.flat"), t("keys.usage.buttons.flat")] : [t("keys.usage.row"), t("keys.usage.buttons"), t("keys.usage.keys")],
    groups: [group(t("keys.group.global"), rows.filter((r) => r.scope === "global")), group(t("keys.group.stop"), rows.filter((r) => r.scope !== "global"))].filter((g) => g.rows.length > 0),
  };
}

export function createHelp({ root, sound, getStop = () => null, live = () => null, flat = () => false, onToggle = () => {} }) {
  let box = null;
  let from = null;
  const isOpen = () => box !== null;

  function close() {
    if (!box) return;
    box.remove();
    box = null;
    onToggle(false);
    sound.play("close");
    if (from && from.isConnected && from.focus) from.focus({ preventScroll: true });
    from = null;
  }

  const group = (g) => h("section", { class: "help-group" },
    h("h4", { class: "help-title" }, g.title),
    h("dl", {}, g.rows.map((r) => [h("dt", {}, h("kbd", { class: "k" }, r.key)), h("dd", {}, r.label)])));

  const body = () => {
    const sheet = helpModel(getStop(), { live: live(), flat: flat() });
    return [
      sheet.ask ? h("p", { class: "help-ask" }, sheet.ask) : null,
      h("ul", { class: "help-usage" }, sheet.usage.map((line) => h("li", {}, line))),
      sheet.groups.map(group),
    ];
  };

  /** Draws the open sheet again under its head (the close button keeps the focus): the page changed between the desktop and the flat page. */
  function refresh() {
    if (!box) return;
    const head = box.querySelector(".help-head");
    while (head.nextSibling) head.nextSibling.remove();
    head.parentNode.append(...body().flat().filter(Boolean));
  }

  function open() {
    if (box) return;
    from = document.activeElement;
    const closeBtn = h("button", { class: "help-close", type: "button", onclick: close }, t("common.close"));
    const dialog = h("div", { class: "help", role: "dialog", "aria-modal": "true", "aria-labelledby": "help-title", tabindex: -1 },
      h("div", { class: "help-head" }, h("h3", { id: "help-title" }, t("keys.title")), closeBtn),
      body());
    box = h("div", { class: "help-back", "data-lenis-prevent": "", onmousedown: (e) => { if (e.target === box) close(); } }, dialog);
    // the wheel is the sheet's or nobody's: over the veil, or over a sheet that has nothing to scroll, it must not move the page behind (the stage's camera)
    box.addEventListener("wheel", (e) => { if (!dialog.contains(e.target) || dialog.scrollHeight <= dialog.clientHeight) e.preventDefault(); }, { passive: false });
    dialog.addEventListener("keydown", (e) => { if (e.key === "Tab") { e.preventDefault(); closeBtn.focus(); } });
    root.append(box);
    sound.play("open");
    closeBtn.focus();
    onToggle(true);
  }

  return { open, close, isOpen, refresh, toggle: () => (box ? close() : open()) };
}
