// Small helpers shared by every moment: `clean` is the one filter for what another person wrote (a company, a role, a subject, a note),
// which goes through it before it is shown; `safeHref` decides which links are followed; and the rest patch the DOM only where it differs.

/** How long a changed row carries the `fresh` mark (4 s, the old page's): the session's own value, so the two cannot drift apart. */
export { FRESH_MS } from "../../core/session.js";

/** True when `text` is something a date can be made of: the server's dates are shown only when they are (a missing or broken one is left out). */
export const isTime = (text) => !!text && Number.isFinite(Date.parse(text));

/** A link from the server is followed only when it is http or https; anything else is dropped, never shown as a link. */
export const safeHref = (url) => (/^https?:\/\//i.test(String(url || "")) ? String(url) : null);

// What must never reach the screen from outside: control characters, line and paragraph separators, and the bidirectional marks and
// overrides (U+061C, U+200E/F, U+202A-E, U+2066-9), which can make a line read backwards or hide what is after it. The ranges are
// code points, not escapes: an invisible character in this file would be a trap for the next person who reads it.
const RANGES = [[0x00, 0x1f], [0x7f, 0x9f], [0x061c, 0x061c], [0x2028, 0x2029], [0x200e, 0x200f], [0x202a, 0x202e], [0x2066, 0x2069]];
const UNSAFE = new RegExp("[" + RANGES.map(([from, to]) => String.fromCodePoint(from) + "-" + String.fromCodePoint(to)).join("") + "]", "g");

/** Text from outside, ready to show: no control or bidi characters, one line, spaces collapsed, cut at `max` with an ellipsis. */
export function clean(text, max = 160) {
  const one = String(text == null ? "" : text).replace(UNSAFE, " ").replace(/\s+/g, " ").trim();
  if (one.length <= max) return one;
  let cut = one.slice(0, Math.max(0, max - 1));
  if (/[\ud800-\udbff]$/.test(cut)) cut = cut.slice(0, -1); // never leave half of an emoji
  return cut.trimEnd() + "…";
}

/**
 * Makes the children of `parent` exactly `els`, in order, touching the DOM only where it differs: a node that is already in
 * the right place stays (so it keeps its focus, its open fold and its half-pressed button), a new one is put before the node
 * that should follow it, and what is left over goes.
 */
export function sync(parent, els) {
  let at = parent.firstChild;
  for (const el of els) {
    if (el === at) at = at.nextSibling;
    else parent.insertBefore(el, at);
  }
  while (at) {
    const next = at.nextSibling;
    parent.removeChild(at);
    at = next;
  }
}

/** `el.dataset[key] = value` (or the attribute removed when `value` is null/false) only when it differs: setting an attribute to what it already is still counts as a change for anyone watching the DOM. */
export function setData(el, key, value) {
  const next = value == null || value === false ? undefined : String(value);
  if (el.dataset[key] === next) return;
  if (next === undefined) delete el.dataset[key];
  else el.dataset[key] = next;
}

/** `aria-busy` on `el` while `on`, written only when it differs. */
export function setBusy(el, on) {
  if (on) {
    if (el.getAttribute("aria-busy") !== "true") el.setAttribute("aria-busy", "true");
  } else if (el.hasAttribute("aria-busy")) {
    el.removeAttribute("aria-busy");
  }
}

/** `el.setAttribute(name, value)` only when it differs; a null, undefined or false value removes the attribute (an empty text is a value). */
export function setAttr(el, name, value) {
  if (value === null || value === undefined || value === false) {
    if (el.hasAttribute(name)) el.removeAttribute(name);
    return;
  }
  const next = String(value);
  if (el.getAttribute(name) !== next) el.setAttribute(name, next);
}

/** `el.hidden = on` only when it differs. */
export function setHidden(el, on) {
  if (el.hidden !== !!on) el.hidden = !!on;
}

/** What a row is called aloud: the company and its role, and no stray separator when one of the two is missing. */
export const nameOf = (company, text) => [company, text].filter(Boolean).join(" · ");

/** True when `el` was last drawn with another signature, and stamps this one on it: the caller rebuilds only then. */
export function stampChanged(el, sig) {
  if (el.dataset.sig === sig) return false;
  el.dataset.sig = sig;
  return true;
}

/** `el.title = text` only when it differs (an empty text removes the title). */
export function setTitle(el, text) {
  const next = text || "";
  if ((el.getAttribute("title") || "") !== next) {
    if (next) el.setAttribute("title", next);
    else el.removeAttribute("title");
  }
}

/** What opens a posting when the host gives no `openUrl`: a new tab that cannot reach back to the page (`noopener`) or tell where it came from. */
export const openInNewTab = (url) => window.open(url, "_blank", "noopener,noreferrer");
