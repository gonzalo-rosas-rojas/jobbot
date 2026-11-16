// Where the person is: the route the address names (`#procesos/<job key>`, with the old names it still accepts) and the view the shell and the rooms share
// (room, filter, folds, cursor, open application, note). What the panel remembers between visits is in store.js; this module reads and writes the saved view through it.

import { keep } from "./store.js";

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

// The rooms an address may name: the prologue, every room and the old names (ALIASES), so a new room is written once, in ROOM_IDS.
const ROUTE = new RegExp("^#(" + ["prologo", ...ROOM_IDS, ...Object.keys(ALIASES)].join("|") + ")(?:/(.+))?$");

/** `#prologo`, `#hoy`, `#procesos/<job key>`; the legacy `#learn` is Aprender. Anything else is no route. */
export function routeOfHash(hash) {
  const m = ROUTE.exec(hash || "");
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
