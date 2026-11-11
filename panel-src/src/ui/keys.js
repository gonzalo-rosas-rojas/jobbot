// The keyboard layer (spec §3.4): ONE table, `KEYS`, is the source of the bindings, of the help sheet and of the A19 test.
// A global key has one meaning in the whole page and is never reassigned; a row or stop key has one meaning per stop and
// is only active in the stops it lists; a local key belongs to a box that reads its own (APRENDER's drill). The page's
// owner of each verb is the handler `installKeys` is given: a verb with no handler does nothing and is not prevented.

import { tinykeys } from "tinykeys";
import { ignoreKey, takesEnter } from "./keyfilter.js";

const WORK = ["hoy", "procesos", "cola"];
const NAV = new Set(["go", "prologue"]);                   // blocked while the detail is open (§3.6)

// id, tinykeys syntax, what the help shows, the dictionary key of what it does, scope and stops (spec §3.4, same order)
export const KEYS = [
  { id: "go", keys: ["1", "2", "3", "4", "5"], display: "1 – 5", label: "keys.rooms", scope: "global" },
  { id: "prologue", keys: ["0"], display: "0", label: "keys.prologue", scope: "global" },
  { id: "search", keys: ["$mod+k"], display: "Ctrl K", label: "keys.search", scope: "global" },
  { id: "help", keys: ["[Shift]+?"], display: "?", label: "keys.help", scope: "global" },
  { id: "theme", keys: ["t"], display: "t", label: "keys.theme", scope: "global" },
  { id: "sound", keys: ["m"], display: "m", label: "keys.sound", scope: "global" },
  { id: "lang", keys: ["l"], display: "l", label: "keys.lang", scope: "global" },
  { id: "close", keys: ["Escape"], display: "Esc", label: "keys.close", scope: "global" },
  { id: "move", keys: ["j", "k"], display: "j  k", label: "keys.list", scope: "row", stops: [...WORK, "aprender"] },
  { id: "open", keys: ["Enter"], display: "Enter", label: "keys.open", scope: "row", stops: [...WORK, "aprender"] },
  { id: "approve", keys: ["a"], display: "a", label: "keys.approve", scope: "row", stops: ["hoy", "cola"] },
  { id: "confirm", keys: ["c"], display: "c", label: "keys.confirm", scope: "row", stops: WORK },
  { id: "resolve", keys: ["u"], display: "u", label: "keys.resolve", scope: "row", stops: ["hoy", "cola", "aprender"] },
  { id: "withdraw", keys: ["x"], display: "x", label: "keys.withdraw", scope: "row", stops: ["hoy", "procesos"] },
  { id: "snooze", keys: ["s"], display: "s", label: "keys.snooze", scope: "row", stops: WORK },
  { id: "register", keys: ["r"], display: "r", label: "keys.register", scope: "row", stops: ["hoy", "procesos"] },
  { id: "done", keys: ["h"], display: "h", label: "keys.done", scope: "row", stops: ["hoy"] },
  { id: "newtask", keys: ["n"], display: "n", label: "keys.newtask", scope: "stop", stops: ["hoy", "procesos"] },
  { id: "link", keys: ["o"], display: "o", label: "keys.link", scope: "row", stops: [...WORK, "aprender"] },
  { id: "filter", keys: ["f 1", "f 2", "f 3", "f 4"], display: "f 1–4", label: "keys.filter", scope: "stop", stops: ["procesos", "cola"] },
  { id: "find", keys: ["[Shift]+/"], display: "/", label: "keys.find", scope: "stop", stops: ["procesos", "cola"] },
  { id: "field", keys: ["v"], display: "v", label: "keys.field", scope: "stop", stops: ["procesos"] },
  { id: "batch", keys: ["Shift+A"], display: "Shift A", label: "keys.batch", scope: "stop", stops: ["cola"] },
  { id: "engine", keys: ["p"], display: "p", label: "keys.engine", scope: "stop", stops: ["cola", "sistema"] },
  { id: "reveal", keys: ["Enter", "Space"], display: "Enter  Space", label: "keys.reveal", scope: "local", stops: ["aprender"] },
  { id: "grade", keys: ["0", "1", "2", "3", "4", "5"], display: "0 – 5", label: "keys.grade", scope: "local", stops: ["aprender"] },
];

export const isSequence = (key) => key.trim().includes(" ");
const norm = (key) => key.trim().toLowerCase();
const firstPress = (key) => norm(key).split(" ")[0];

/** The keys that work at a stop (null or "prologo": the global ones), in table order. `live`, a Set of ids, hides what is not wired yet. */
export function keysFor(stopId, { live = null } = {}) {
  return KEYS.filter((k) => k.scope === "global" || ((k.stops || []).includes(stopId) && (!live || live.has(k.id))));
}

/** Every way the table breaks A19, as sentences; [] when it is sound. Local keys are the one allowed overlap (§3.4). */
export function conflicts(table = KEYS) {
  const out = [];
  const globals = table.filter((k) => k.scope === "global");
  for (const k of table) {
    if (k.scope === "global" || k.scope === "local") continue;
    for (const key of k.keys) for (const g of globals) for (const gk of g.keys) {
      if (norm(key) === norm(gk)) out.push(`${k.id} (${key}) redefines the global ${g.id} (${gk})`);
    }
  }
  const stops = new Set(table.flatMap((k) => k.stops || []));
  for (const stop of stops) {
    const live = table.filter((k) => k.scope !== "local" && (k.scope === "global" || (k.stops || []).includes(stop)));
    for (let i = 0; i < live.length; i++) for (let j = i + 1; j < live.length; j++) {
      const [a, b] = [live[i], live[j]];
      if (a.scope === "global" && b.scope === "global") continue;
      for (const ka of a.keys) for (const kb of b.keys) {
        const same = norm(ka) === norm(kb);
        const prefix = (!isSequence(ka) && firstPress(kb) === norm(ka) && isSequence(kb)) || (!isSequence(kb) && firstPress(ka) === norm(kb) && isSequence(ka));
        if (same || prefix) out.push(`${stop}: ${a.id} and ${b.id} both start with ${prefix ? firstPress(isSequence(ka) ? ka : kb) : ka}`);
      }
    }
  }
  return out;
}

/**
 * Binds the table. handlers[id](key, event); stop() is the stop on screen (null in the prologue); locked() says a dialog or
 * menu owns the keyboard; drawerOpen() blocks 0-5 (§3.6). Returns detach().
 */
export function installKeys({ target = window, handlers, stop = () => null, locked = () => false, drawerOpen = () => false, typing, chordMs = 1000 }) {
  const byKey = new Map();
  for (const entry of KEYS) {
    if (entry.scope === "local") continue;
    for (const key of entry.keys) byKey.set(key, [...(byKey.get(key) || []), entry]);
  }
  // tinykeys fires the first binding that matches: a chord ("f 1") must come before the single key that shares its last press
  // ("1"). Sorting is not enough: JavaScript lists integer-like property names first, in numeric order, whatever the insertion
  // order, so a digit is bound as the regex form "(1)", which is an ordinary string key and keeps its place.
  const ordered = [...byKey.entries()].sort(([a], [b]) => Number(isSequence(b)) - Number(isSequence(a)));
  function fire(key, event) {
    const at = stop();
    const entry = (byKey.get(key) || []).find((e) => e.scope === "global" || (e.stops || []).includes(at));
    const run = entry && handlers[entry.id];
    // tinykeys has already swallowed the press as the end of a chord. A chord this stop does not have was never started
    // (a stray "f" in HOY), so its last press is an ordinary key again and still does its own work.
    if (!run) return isSequence(key) ? fire(key.trim().split(" ").slice(1).join(" "), event) : undefined;
    if (drawerOpen() && NAV.has(entry.id)) return;
    if (key === "Enter" && takesEnter(event.target)) return;
    event.preventDefault();
    run(key, event);
  }
  const bindings = {};
  for (const [key] of ordered) bindings[/^\d+$/.test(key) ? `(${key})` : key] = (event) => fire(key, event);
  return tinykeys(target, bindings, { ignore: (event) => ignoreKey(event, { typing, locked }), timeout: chordMs });
}
