// Small helpers shared by every moment: `clean` is the one filter for what another person wrote (a company, a role, a subject, a note),
// which goes through it before it is shown; `safeHref` decides which links are followed; and the rest patch the DOM only where it differs.

/** How long a changed row carries the `fresh` mark (4 s, the old page's). */
export const FRESH_MS = 4000;

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

/** `el.textContent = text` only when it differs, so a refresh that changes nothing writes nothing. */
export function setText(el, text) {
  const next = String(text == null ? "" : text);
  if (el.textContent !== next) el.textContent = next;
}

/** `el.dataset[key] = value` (or the attribute removed when `value` is null/false) only when it differs: setting an attribute to what it already is still counts as a change for anyone watching the DOM. */
export function setData(el, key, value) {
  const next = value === null || value === undefined || value === false ? undefined : String(value);
  if (el.dataset[key] === next) return;
  if (next === undefined) delete el.dataset[key];
  else el.dataset[key] = next;
}

/** `el.hidden = on` only when it differs. */
export function setHidden(el, on) {
  if (el.hidden !== !!on) el.hidden = !!on;
}

/** `el.title = text` only when it differs (an empty text removes the title). */
export function setTitle(el, text) {
  const next = text || "";
  if ((el.getAttribute("title") || "") !== next) {
    if (next) el.setAttribute("title", next);
    else el.removeAttribute("title");
  }
}
