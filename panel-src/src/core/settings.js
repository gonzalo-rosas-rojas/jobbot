// The user's settings: three small choices that outlive the visit, with change events.
//
//   sound   true | false           mirrors sound.enabled (the sound engine keeps its own copy; the shell keeps them equal)
//   motion  "auto" | "calm" | "full"             "auto" follows the system's reduced-motion request; "calm" is no trips, no
//                                                gestures, no looping; "full" lets the stage idle even when the system asks
//                                                for reduced motion, but never the sheet (that request is always kept)
//   batchMin  a whole score, 1 to 100 (85)       the score from which COLA offers a proposal in the lote (approve many at once);
//                                                COLA and SISTEMA read this one value and the one DEFAULT_BATCH_MIN
//
// Saved as JSON under `panel.settings` through core/store (try/catch, memory fallback). The address can override the
// motion for tests and screenshots (`?motion=calm|full`): an override is never saved, and the person choosing that setting
// by hand clears it. A key that is not one of the settings above, such as the `world` an earlier page saved, is ignored
// when it is read and left out when the settings are written again.

import { local } from "./store.js";

const MOTION_MODES = ["auto", "calm", "full"];
export const DEFAULT_BATCH_MIN = 85;
export const BATCH_MIN_RANGE = Object.freeze([1, 100]); // scores run 0-100; the engine's own auto_approve_min lives in the same 1..100
export const DEFAULTS = Object.freeze({ sound: false, motion: "auto", batchMin: DEFAULT_BATCH_MIN });

/** A threshold from storage or a field: a number or the text of one becomes a whole score inside the range; anything else is the default. */
function cleanBatchMin(v) {
  const n = typeof v === "number" ? v : typeof v === "string" && v.trim() !== "" ? Number(v) : NaN;
  if (!Number.isFinite(n)) return DEFAULT_BATCH_MIN;
  return Math.min(BATCH_MIN_RANGE[1], Math.max(BATCH_MIN_RANGE[0], Math.round(n)));
}

const CLEAN = {
  sound: (v) => v === true,
  motion: (v) => (MOTION_MODES.includes(v) ? v : DEFAULTS.motion),
  batchMin: cleanBatchMin,
};
const KEYS = Object.keys(DEFAULTS);

function load(store) {
  try {
    const raw = JSON.parse(store.get("settings") || "null");
    if (raw && typeof raw === "object") return Object.fromEntries(KEYS.filter((k) => k in raw).map((k) => [k, CLEAN[k](raw[k])]));
  } catch (e) { /* a broken value is the same as none */ }
  return {};
}

/** The overrides an address asks for: `?motion=calm|full` (only the values that mean something). */
export function overridesOf(search) {
  const params = new URLSearchParams(search || "");
  const out = {};
  const motion = params.get("motion");
  if (motion === "calm" || motion === "full") out.motion = motion;
  return out;
}

export function createSettings({ store = local, search = "" } = {}) {
  const saved = load(store);
  const overrides = overridesOf(search);
  const listeners = new Set();

  const effective = (key) => (key in overrides ? overrides[key] : key in saved ? saved[key] : DEFAULTS[key]);
  const all = () => Object.fromEntries(KEYS.map((k) => [k, effective(k)]));

  function emit(key, value, previous) {
    for (const fn of Array.from(listeners)) {
      try { fn({ key, value, previous, all: all() }); } catch (e) { console.error(e); }
    }
  }

  return {
    /** One effective value, or all of them as a new object. */
    get(key) {
      if (key === undefined) return all();
      return KEYS.includes(key) ? effective(key) : undefined;
    },
    /** `set("motion", "calm")` or `set({ motion: "calm", sound: true })`. Saved; listeners hear every value that changed. */
    set(keyOrPatch, value) {
      const patch = typeof keyOrPatch === "string" ? { [keyOrPatch]: value } : keyOrPatch || {};
      for (const key of Object.keys(patch)) {
        if (!KEYS.includes(key)) continue;
        const next = CLEAN[key](patch[key]);
        const previous = effective(key);
        saved[key] = next;
        delete overrides[key];
        try { store.set("settings", JSON.stringify(saved)); } catch (e) { /* in memory only */ }
        if (next !== previous) emit(key, next, previous);
      }
      return all();
    },
    /** Runs `fn({ key, value, previous, all })` after a setting changed; returns the function that stops it. */
    onChange(fn) {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
    /** Is an address override in force for this key (so the Sistema room can say so)? */
    overridden: (key) => key in overrides,
  };
}
