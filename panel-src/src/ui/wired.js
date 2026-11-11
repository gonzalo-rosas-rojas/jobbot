// What each stop has wired of the key table (spec §3.4, A19). ui/keys.js says which keys a stop COULD have; this says which ones it
// really answers today, and the help sheet lists only those (ui/help.js, `live`): a shortcut that does nothing is not announced.
// The shell binds every row and stop key of the table to the moment on screen (`moment.onKey(id, row, event)`), so a stop is wired
// the day its moment handles the id. Whoever wires a key of a stop adds its id to the list of that stop in the same commit; the test
// (test/ui.wired.test.mjs) refuses an id the table does not give to that stop, refuses a HOY key of the table that is missing here,
// and refuses a list for a stop whose moment has no `onKey` yet (a placeholder promises nothing).
//
// `move` and `open` are answered by every stop that lists them even when its moment lets them pass: the shell moves the cursor on `j`
// and `k` and opens the application under it on Enter (ui/shell-keys.js).

import { STOP_IDS } from "../scene/poses.js";
import { KEYS } from "./keys.js";

export const WIRED = {
  prologo: [],
  hoy: ["move", "open", "approve", "confirm", "resolve", "withdraw", "snooze", "register", "done", "newtask", "link"],
  procesos: ["move", "open", "confirm", "withdraw", "snooze", "register", "newtask", "link", "filter", "find", "field"],
  cola: ["move", "open", "approve", "confirm", "resolve", "snooze", "link", "batch", "engine"],
  aprender: ["move", "open", "resolve", "link"],
  sistema: ["engine"],
};

/** The ids a stop answers, as the Set that `keysFor(stop, { live })` and the help sheet take. An unknown stop answers nothing. */
export const wiredSet = (stopId) => new Set(STOP_IDS.includes(stopId) ? WIRED[stopId] : []);

const LOCAL = KEYS.filter((k) => k.scope === "local").map((k) => k.id);

/** What the help of a stop lists: what it answers, plus the keys a box reads for itself (the drill of APRENDER: no handler of the shell, they are the box's own). */
export const liveSet = (stopId) => new Set([...wiredSet(stopId), ...LOCAL]);
