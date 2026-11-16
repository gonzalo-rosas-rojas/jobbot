// A small menu next to what it is about (spec §3.4, §6.2): "postpone until when", "which answer", "new task". It is NOT a modal dialog: nothing behind
// it goes `inert`, the page keeps scrolling, a click outside closes it. It is a box that reads its own keys (`data-local-keys`): the global shortcuts
// are quiet while the keyboard is inside it (ui/keyfilter.js), each item has a key of its own (a digit or a letter) and Esc closes it and gives the
// focus back to where it came from. `isOpen()` is what the shell hands to `installKeys` as `locked`, so a menu owns the keyboard while it is open.
//
//   const menu = createMenu({ sound, root: () => document.body });
//   menu.open({ title, items: [{ key: "1", label, hint?, run }], body?, from: element, onClose?(why) })     why: "pick" | "cancel"
//   menu.close(why = "cancel")   menu.isOpen()   menu.el
// With `body` (a node, a form) the box holds it instead of the list of items and focuses its first field.

import { h } from "../core/dom.js";
import { t } from "../i18n/index.js";
import { KEYS } from "./keys.js";

const EDGE = 12;
const GAP = 6; // between the box and what it sits beside
const NO_ANCHOR = { left: 80, bottom: 140, top: 100 }; // where the box goes when it has nothing to sit beside: near the top left of the sheet

const ESC = KEYS.find((k) => k.id === "close").display;

export function createMenu({ sound, root = () => document.body }) {
  let el = null;
  let origin = null;
  let onClose = null; // what the opener wants to hear when the box closes: onClose(why)
  let items = [];
  let itemButtons = [];

  const isOpen = () => el !== null;

  function close(why = "cancel") {
    if (!el) return;
    const box = el;
    const back = origin;
    const heard = onClose;
    el = null;
    origin = null;
    onClose = null;
    items = [];
    itemButtons = [];
    document.removeEventListener("pointerdown", outside, true);
    box.remove();
    if (back && back.isConnected) back.focus({ preventScroll: true });
    if (heard) heard(why);
  }

  function outside(e) {
    if (el && !el.contains(e.target)) close("cancel");
  }

  function pick(item) {
    const run = item.run;
    close("pick");
    if (run) run();
  }

  function move(step) {
    if (itemButtons.length === 0) return;
    const at = itemButtons.findIndex((b) => b === document.activeElement);
    const last = itemButtons.length - 1;
    const next = at < 0 ? (step > 0 ? 0 : last) : (at + step + itemButtons.length) % itemButtons.length; // with no item focused, down is the first and up the last
    itemButtons[next].focus({ preventScroll: true });
    sound.play("step");
  }

  function onKey(e) {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation(); // the page's own Esc (close the drawer) must not also run
      close("cancel");
      return;
    }
    if (items.length === 0) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      move(e.key === "ArrowDown" ? 1 : -1);
      return;
    }
    const item = items.find((i) => i.key.toLowerCase() === String(e.key).toLowerCase());
    if (item) {
      e.preventDefault();
      e.stopPropagation();
      pick(item);
    }
  }

  /** Puts the box beside `from` (below it, left aligned, or above it when there is no room), always inside the window, even when `from` is not; with no `from`, near the top left of the sheet. */
  function place(box, from) {
    const r = from ? from.getBoundingClientRect() : NO_ANCHOR;
    const w = box.offsetWidth;
    const hh = box.offsetHeight;
    const left = Math.min(Math.max(EDGE, r.left), window.innerWidth - w - EDGE);
    let top = r.bottom + GAP;
    if (top + hh > window.innerHeight - EDGE) top = r.top - hh - GAP;
    top = Math.min(Math.max(EDGE, top), Math.max(EDGE, window.innerHeight - hh - EDGE)); // a row below the fold must not take the box with it
    box.style.left = Math.round(left) + "px";
    box.style.top = Math.round(top) + "px";
  }

  function open({ title, items: list = [], body = null, from = null, onClose: whenClosed = null }) {
    if (el) close("cancel");
    origin = from;
    onClose = whenClosed;
    items = list;
    const buttons = list.map((item) => h("button", { class: "menu-item", type: "button", role: "menuitem", "data-key": item.key, onclick: () => pick(item) },
      h("kbd", { class: "k" }, item.key), h("span", { class: "menu-label" }, item.label), item.hint ? h("span", { class: "menu-hint label" }, item.hint) : null));
    itemButtons = buttons;
    // The box holds a title and a foot besides the items, which a `menu` may not: the box is a labelled group (a dialog with a form), the
    // list is the menu and its <li> are `none`, so each item is a direct child of the menu for a screen reader.
    const box = h("div", { class: "menu", role: body ? "dialog" : "group", "aria-label": title, "data-local-keys": "" },
      h("p", { class: "menu-title label" }, title),
      body || h("ul", { class: "menu-items", role: "menu", "aria-label": title }, ...buttons.map((b) => h("li", { role: "none" }, b))),
      h("p", { class: "menu-foot label" }, h("kbd", { class: "k" }, ESC), " ", t("menu.close")));
    box.addEventListener("keydown", onKey);
    el = box;
    root().append(box);
    place(box, from);
    document.addEventListener("pointerdown", outside, true);
    sound.play("open");
    const first = body ? box.querySelector("input, select, textarea, button") : buttons[0];
    if (first) first.focus({ preventScroll: true });
    return box;
  }

  return { open, close, isOpen, get el() { return el; } };
}
