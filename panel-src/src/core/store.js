// Everything the panel remembers, and the one place that touches web storage.
//
// Two shelves, both under the `panel.` prefix:
//   keep   sessionStorage, JSON. What Gonza was doing in this visit (room, filter, folds); gone with the tab.
//   local  localStorage, plain text. Choices that outlive the visit: theme, language, last visit, sound.
// Storage can be missing or throw (private windows, blocked site data, previews), so every access sits in a
// try/catch and falls back to a Map that lives as long as the page. Nothing here runs at import time.

const PREFIX = "panel.";
const memory = { keep: new Map(), local: new Map() };
let override = null;

/** Tests give fake shelves ({ local, session }, each with getItem/setItem/removeItem); `null` restores the real ones. */
export function useStorage(next) {
  override = next || null;
  memory.keep.clear();
  memory.local.clear();
}

function shelf(kind) {
  try {
    if (override) return override[kind === "keep" ? "session" : "local"] || null;
    return (kind === "keep" ? globalThis.sessionStorage : globalThis.localStorage) || null;
  } catch (e) {
    return null; // reading the property itself can throw when site data is blocked
  }
}

function read(kind, name) {
  try {
    const box = shelf(kind);
    if (box) return box.getItem(PREFIX + name);
  } catch (e) { /* fall through to memory */ }
  return memory[kind].has(name) ? memory[kind].get(name) : null;
}

function write(kind, name, text) {
  try {
    const box = shelf(kind);
    if (box) { box.setItem(PREFIX + name, text); return; }
  } catch (e) { /* fall through to memory */ }
  memory[kind].set(name, text);
}

function drop(kind, name) {
  memory[kind].delete(name);
  try {
    const box = shelf(kind);
    if (box) box.removeItem(PREFIX + name);
  } catch (e) { /* nothing to remove */ }
}

export const keep = {
  get(name) {
    try { return JSON.parse(read("keep", name) || "null"); } catch (e) { return null; }
  },
  set(name, value) {
    write("keep", name, JSON.stringify(value));
  },
};

export const local = {
  get: (name) => read("local", name),
  set: (name, text) => write("local", name, String(text)),
  remove: (name) => drop("local", name),
};

// ---- Where Gonza is, shared by the shell and the rooms

export const ROOM_IDS = ["hoy", "procesos", "cola", "aprender", "sistema"];
export const FILTERS = ["all", "mine", "wait", "stale"];
// The old page called the fourth section "learn"; addresses and saved views from then still open Aprender.
const ALIASES = { learn: "aprender" };
const room = (name) => ALIASES[name] || name;

/**
 * The state that is genuinely shared between the shell and more than one room:
 *   room     the open room (one of ROOM_IDS)
 *   filter   the Procesos filter; the counters of Hoy and the nav jump to Procesos with one set
 *   folds    open/closed state of every fold by id, so a redraw does not close what Gonza opened
 *   cursor   the row the keyboard is on ("c:<key>" or "t:<id>"), used by the shell's j/k and every list
 *   openKey  the application whose drawer is open (null when closed)
 *   note     the half-typed note of the drawer, kept across redraws of the drawer
 * Everything else the old page kept in `state` belongs to one room (the ticket's skipped set and punch count to
 * Hoy, the what-if threshold to Cola, the drill to Aprender, rings and car positions to Procesos) or to the session
 * (the snapshot, the fresh marks), and lives there.
 */
export function createView() {
  return { room: "hoy", filter: "all", folds: {}, cursor: null, openKey: null, note: "" };
}

/** `#prologo`, `#hoy`, `#procesos/<job key>`; the legacy `#learn` is Aprender. Anything else is no route. */
export function routeOfHash(hash) {
  const m = /^#(prologo|hoy|procesos|cola|learn|aprender|sistema)(?:\/(.+))?$/.exec(hash || "");
  if (!m) return null;
  let job = null;
  try { job = m[2] ? decodeURIComponent(m[2]) : null; } catch (e) { /* a broken address is just no job */ }
  return { room: room(m[1]), job };
}

export function hashOf(roomId, jobKey) {
  return "#" + roomId + (jobKey ? "/" + encodeURIComponent(jobKey) : "");
}

/**
 * Applies the saved view and the address to `view`. The address wins over the saved room; a panel shown inside
 * another page (`embedded`, the demo on a site) ignores what was saved. Returns the route read from the address.
 */
export function restoreView(view, { embedded = false, hash = "" } = {}) {
  const start = routeOfHash(hash);
  const saved = embedded ? null : keep.get("view");
  if (saved && FILTERS.includes(saved.filter)) view.filter = saved.filter;
  if (saved && saved.folds && typeof saved.folds === "object") view.folds = saved.folds;
  const savedRoom = saved ? room(saved.room || saved.tab) : null;
  if (start) view.room = start.room;
  else if (savedRoom && ROOM_IDS.includes(savedRoom)) view.room = savedRoom;
  return start;
}

/**
 * An address that names an application (`#procesos/<key>`) opens it with the first snapshot the session accepts, the first read or a later one when the
 * first failed, and only once. `session` is anything with `subscribe` and `snapshot`; `open(job, room)` opens it. Returns the unsubscribe.
 */
export function openRouteOnSnapshot(session, route, open) {
  if (!route || !route.job) return () => {};
  let done = false;
  let off = () => {};
  const go = () => { if (done) return; done = true; off(); open(route.job, route.room); };
  off = session.subscribe((event) => { if (event && event.type === "snapshot") go(); });
  if (done) off();
  else if (session.snapshot) go();
  return off;
}

export function saveView(view) {
  keep.set("view", { room: view.room, filter: view.filter, folds: view.folds });
}

// ---- Theme: the system's until someone chooses; the choice is kept on this machine

export const THEMES = ["auto", "light", "dark"];

export function nextTheme(current) {
  const at = THEMES.indexOf(current);
  return THEMES[(at < 0 ? 0 : at + 1) % THEMES.length];
}

export function loadTheme() {
  const saved = local.get("theme");
  return saved === "light" || saved === "dark" ? saved : "auto";
}

export function saveTheme(name) {
  if (name === "auto") local.remove("theme");
  else local.set("theme", name);
}
