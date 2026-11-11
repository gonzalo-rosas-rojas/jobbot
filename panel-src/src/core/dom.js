// The only place that builds DOM, and the only core module that touches it. Data never becomes markup: every child
// that is not a node is a text node, nothing here parses a string as HTML, and the attributes that could carry code
// are refused or dropped (see `h`). A refusal throws: it is always a mistake in our own code, never in the data.

import { t } from "../i18n/index.js";
import { parseLaunch } from "./session.js";
import { keep } from "./store.js";

const SVG_NS = "http://www.w3.org/2000/svg";

// Elements that load or run something, or restyle the page: nothing in the panel needs them.
const BANNED_TAGS = new Set(["script", "style", "iframe", "object", "embed", "link", "meta", "base", "frame", "frameset", "animate", "set"]);
const URL_ATTRS = new Set(["href", "src", "xlink:href", "action", "formaction", "poster"]);

// A link goes to http(s), mail or a place in the same page. An image comes from a blob (the dry-run capture) or a data image.
function safeUrl(name, value) {
  const url = String(value).trim();
  if (name === "src" || name === "poster") return /^(blob:|data:image\/(png|jpeg|gif|webp)[;,])/i.test(url);
  return /^(https?:|mailto:|#)/i.test(url);
}

function setStyle(el, style) {
  if (typeof style !== "object" || Array.isArray(style)) {
    throw new TypeError("h(): `style` takes an object of CSS properties ({ '--i': 3 }), never a string");
  }
  for (const [name, value] of Object.entries(style)) {
    if (value == null || value === false) continue;
    const property = name.startsWith("--") ? name : name.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase());
    el.style.setProperty(property, String(value));
  }
}

/**
 * Sets the attributes of a new element:
 *   class      a string, or an array whose false and empty parts are left out
 *   dataset    { name: value } as data-* attributes
 *   style      an OBJECT, set with style.setProperty (so custom properties like "--i" work); a string throws
 *   on*        event handlers, only as functions (onclick: fn); anything else throws
 *   href, src  only http(s), mailto or #hash (src: a blob or data image); otherwise the attribute is dropped
 *   aria-*     true and false are the words "true" and "false"
 * Other names are plain attributes; `true` makes an empty (boolean) attribute and false, null and undefined leave it out.
 */
function setAttrs(el, attrs, svg) {
  for (const [name, value] of Object.entries(attrs || {})) {
    if (name.startsWith("aria-") && typeof value === "boolean") {
      el.setAttribute(name, String(value)); // "false" means something for aria-expanded and friends, so it is kept
      continue;
    }
    if (value === false || value == null) continue;
    if (name === "class") {
      const text = Array.isArray(value) ? value.filter(Boolean).join(" ") : String(value);
      if (svg) el.setAttribute("class", text); else el.className = text;
    } else if (name === "style") {
      setStyle(el, value);
    } else if (name === "dataset") {
      for (const [key, data] of Object.entries(value)) if (data != null && data !== false) el.dataset[key] = String(data);
    } else if (name.startsWith("on")) {
      if (typeof value !== "function") throw new TypeError("h(): `" + name + "` must be a function, never a string");
      el.addEventListener(name.slice(2), value);
    } else if (URL_ATTRS.has(name)) {
      if (safeUrl(name, value)) el.setAttribute(name, String(value));
    } else {
      el.setAttribute(name, value === true ? "" : String(value));
    }
  }
  if (el.getAttribute("target") === "_blank" && !el.hasAttribute("rel")) el.setAttribute("rel", "noopener noreferrer");
}

function addKids(el, kids) {
  for (const kid of kids.flat(Infinity)) {
    if (kid == null || kid === false || kid === true) continue;
    el.append(kid instanceof Node ? kid : document.createTextNode(String(kid)));
  }
}

function checkTag(tag) {
  if (BANNED_TAGS.has(String(tag).toLowerCase())) throw new TypeError("h(): <" + tag + "> is not allowed in the panel");
}

/** Builds an element. Children: strings and numbers become text, null/undefined/false/true are dropped, arrays are flattened, nodes are appended. */
export function h(tag, attrs, ...kids) {
  checkTag(tag);
  const el = document.createElement(tag);
  setAttrs(el, attrs, false);
  addKids(el, kids);
  return el;
}

/** The same for SVG elements (created in the SVG namespace). */
export function sv(tag, attrs, ...kids) {
  checkTag(tag);
  const el = document.createElementNS(SVG_NS, tag);
  setAttrs(el, attrs, true);
  addKids(el, kids);
  return el;
}

/**
 * Replaces the children of a box with the parts that exist. `replaceChildren` alone would print a missing part (a
 * `null` from a condition) as the text "null".
 */
export function fill(box, ...parts) {
  box.replaceChildren(...parts.flat(Infinity).filter((part) => part != null && part !== false));
}

/** A link only when the address is http or https; anything else is plain text. */
export function link(url, text) {
  if (!/^https?:\/\//i.test(url || "")) return h("span", { class: "muted" }, text || url || "");
  return h("a", { href: url, target: "_blank", rel: "noopener noreferrer" }, text || url);
}

// A button for something that cannot be undone. The first click only arms it and says so; a second, deliberate click
// within a few seconds does the thing. Two clicks in the same instant (a double click by habit) count as one, so that
// cannot confirm by accident.
export const ARM_MIN_MS = 350;
export const ARM_MAX_MS = 5000;

// A key held down repeats, and on a button every repeat of Enter is a click: that must never be the second, deliberate one.
// The repeat is seen in the capture phase, before the click it causes, so `heldKey()` is true inside that click.
let heldAt = 0;
if (typeof document !== "undefined") {
  document.addEventListener("keydown", (e) => { if (e.repeat) heldAt = Date.now(); }, true);
  document.addEventListener("keyup", () => { heldAt = 0; }, true); // letting go ends it: the next press is a new one
}
export const heldKey = () => Date.now() - heldAt < 150;

// Where a button keeps its words: in its .act-label when it has one (its key stays), else the whole button.
const wordsOf = (button) => button.querySelector(".act-label") || button;

// `onArm(button)` and `onConfirm(button)` hear the two presses (the page makes its two sounds there; this module knows no sound).
export function twoStep(label, run, { onArm = null, onConfirm = null } = {}) {
  let armedAt = 0;
  let timer = 0;
  return (e) => {
    if (heldKey()) return;
    const button = e.currentTarget;
    if (armedAt === 0) {
      armedAt = Date.now();
      wordsOf(button).textContent = t("act.sure");
      timer = setTimeout(() => { armedAt = 0; wordsOf(button).textContent = label; }, ARM_MAX_MS);
      if (onArm) onArm(button);
      return;
    }
    if (Date.now() - armedAt < ARM_MIN_MS) return;
    clearTimeout(timer);
    armedAt = 0;
    wordsOf(button).textContent = label;
    if (onConfirm) onConfirm(button);
    run(button);
  };
}

// ---- Keeping the keyboard where it was across a redraw

// The data attributes that name what an element does inside its card; rooms may add theirs.
const FOCUS_ATTRS = ["data-done", "data-open", "data-action", "data-filter", "data-grade", "data-ticket-act", "data-ticket-open", "data-ticket-skip", "data-car"];

const hostOf = () => document.querySelector("#app, #room");

/**
 * How to find an element of the page again after it is drawn anew: by its id, or by the card it sits in and the one
 * data attribute that names what it does (FOCUS_ATTRS), or a fold's summary. Null when it is not inside `root` or
 * cannot be named. Keeps the caret of a text field.
 */
export function focusMark(el, root = hostOf()) {
  if (el == null || root == null || !root.contains(el)) return null;
  let selector = null;
  if (el.id) {
    selector = "#" + CSS.escape(el.id);
  } else {
    const card = el.closest("[data-card]");
    const scope = card ? '[data-card="' + CSS.escape(card.dataset.card) + '"] ' : "";
    for (const attr of FOCUS_ATTRS) {
      if (el.hasAttribute(attr)) selector = scope + "[" + attr + '="' + CSS.escape(el.getAttribute(attr)) + '"]';
    }
    const fold = el.closest("details[data-fold]");
    if (selector == null && el.tagName === "SUMMARY" && fold) selector = 'details[data-fold="' + CSS.escape(fold.dataset.fold) + '"] > summary';
  }
  if (selector == null) return null;
  const caret = typeof el.selectionStart === "number" ? [el.selectionStart, el.selectionEnd] : null;
  return { selector, caret };
}

export function restoreFocus(mark, root = hostOf()) {
  if (mark == null || root == null) return;
  const el = root.querySelector(mark.selector);
  if (el == null) return;
  el.focus({ preventScroll: true });
  if (mark.caret && "setSelectionRange" in el) {
    // legacy quirk: the old page asked every input for its caret, and a range slider throws; it has none to restore
    try { el.setSelectionRange(mark.caret[0], mark.caret[1]); } catch (e) { /* no caret */ }
  }
}

/** True while the person is typing in a field inside `root` (a redraw would take the half-typed text away). */
export function isTypingIn(root) {
  const a = document.activeElement;
  return a != null && root != null && root.contains(a) && /^(TEXTAREA|INPUT|SELECT)$/.test(a.tagName);
}

/**
 * Reads the launch address once: the token (`?t=`) and whether the panel is embedded, and takes the token out of the
 * address bar so it is not left in the history or on a screenshot. The token is kept in sessionStorage (it dies with the
 * tab), so a reload or a restored tab, which come back without `?t=`, still find it; a new one in the address replaces it.
 * The public demo has no token and saves nothing. `env` is the window, for tests.
 */
export function readLaunch(env = window) {
  const launch = parseLaunch(env.location.search, env.top !== env);
  env.history.replaceState(null, "", env.location.pathname + env.location.hash);
  if (env.__DEMO__) return launch;
  if (launch.token) keep.set("token", launch.token);
  else {
    const saved = keep.get("token");
    if (typeof saved === "string") launch.token = saved;
  }
  return launch;
}
