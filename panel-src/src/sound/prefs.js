// What he chose about the sound beyond "on or off" (that one is the engine's, sound/index.js), remembered on this machine:
//
//   actions      true | false     the bus of what he does (default on)
//   background   true | false     the bus of what happens by itself (default on)
//   muteUntil    0 | a time in ms  a silence he asked for ("1 hora", "hasta mañana") ends at this moment; 0: none
//   work         true | false     mute in working hours (09:00 to 18:00): the default, because this is a work machine and the search is nobody's business
//
// Saved as JSON under `panel.soundprefs` (core/store: try/catch, memory when storage is blocked). Bad values fall back to the defaults. `set` returns
// the new values and tells the listeners which key changed. Nothing here makes a sound or reads a clock: sound/player.js decides what the values mean.

import { local } from "../core/store.js";

const DEFAULT_PREFS = Object.freeze({ actions: true, background: true, muteUntil: 0, work: true });

const CLEAN = {
  actions: (v) => v !== false,
  background: (v) => v !== false,
  work: (v) => v !== false,
  muteUntil: (v) => (Number.isFinite(v) && v > 0 ? Math.round(v) : 0),
};
const KEYS = Object.keys(DEFAULT_PREFS);

function load(store) {
  try {
    const raw = JSON.parse(store.get("soundprefs") || "null");
    if (raw && typeof raw === "object") return Object.fromEntries(KEYS.filter((k) => k in raw).map((k) => [k, CLEAN[k](raw[k])]));
  } catch (e) { /* a broken value is the same as none */ }
  return {};
}

/** createPrefs({ store }) -> { get(key?), set(key | patch, value), onChange(fn) }. `store` is { get(name), set(name, text) } (core/store's `local`). */
export function createPrefs({ store = local } = {}) {
  const saved = load(store);
  const listeners = new Set();
  const all = () => ({ ...DEFAULT_PREFS, ...saved });

  return {
    get(key) {
      if (key === undefined) return all();
      return KEYS.includes(key) ? all()[key] : undefined;
    },
    set(keyOrPatch, value) {
      const patch = typeof keyOrPatch === "string" ? { [keyOrPatch]: value } : keyOrPatch || {};
      const changed = [];
      for (const key of Object.keys(patch)) {
        if (!KEYS.includes(key)) continue;
        const next = CLEAN[key](patch[key]);
        const previous = all()[key];
        saved[key] = next;
        if (next !== previous) changed.push([key, next, previous]);
      }
      try { store.set("soundprefs", JSON.stringify(saved)); } catch (e) { /* in memory only */ }
      for (const [key, next, previous] of changed) {
        for (const fn of Array.from(listeners)) {
          try { fn({ key, value: next, previous, all: all() }); } catch (e) { console.error(e); }
        }
      }
      return all();
    },
    onChange(fn) {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
  };
}
