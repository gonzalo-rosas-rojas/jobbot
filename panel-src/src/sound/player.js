// The player: the one object the page calls to make a sound. It stands between the page and the engine (sound/index.js) and decides, for a
// cue of the vocabulary (sound/cues.js), whether it may sound NOW. The engine keeps its own rules (a context only after a gesture, two sounds a
// second, no duplicates within 80 ms, six voices); the player adds the ones that are about him and about this machine:
//
//   master       the engine is on (apagado por defecto; `m`). Off, nothing exists: no cue creates a node.
//   bus          Acciones and Fondo are switched apart (sound/prefs.js).
//   silence      "Silenciar 1 hora" / "hasta mañana" ends at a moment that is remembered.
//   work hours   from 09:00 to 18:00 the page is mute, unless he allows it: this is a work machine and the search is secret. The hours are shown.
//   calm         with calm motion, the navigation cues (the tick of the cursor, the arrival motifs) are dropped with the trips they go with.
//   background   one a minute at most (an error excepted), none while the tab is hidden, and the rest of its rules live in sound/background.js.
//
// A call that the gates refuse creates NOTHING (no oscillator, no gain, no buffer): the tests count the nodes of a fake context to prove it.
// `preview` is the other thing: "Escuchar los sonidos" plays a cue because he pressed its button, so it ignores the silence, the hours, the
// buses and the calm (but not the master: the sheet turns the sound on first, with his click).
//
// The player has the same interface as the engine (play, preview, enabled, setEnabled, toggle, volume, setVolume, setBusy, ctx, names, tap,
// recent), so the shell and every moment take it as their `sound` and need no change but the names they ask for.

import { BUSES, cueOf } from "./cues.js";
import { horaOf, WINDOW_START, WINDOW_END } from "../scene/clock.js";

export const LEVELS = Object.freeze({ low: 0.12, mid: 0.25, high: 0.4 });
export const LEVEL_IDS = Object.freeze(["low", "mid", "high"]);
const WORK_FROM = WINDOW_END;                  // the bot's window is 18:00 to 09:00; the rest of the day is the working day
const WORK_TO = WINDOW_START;
export const OWN_MS = 90 * 1000;                      // how long a change he made explains the changes the next snapshot shows
const HISTORY = 24;
const HOUR = 3600000;
const TOMORROW_AT = 6;                                 // "hasta mañana" ends at the next 06:00, at least two hours away
const two = (n) => String(n).padStart(2, "0");

export const isWorkHour = (hora) => hora >= WORK_FROM && hora < WORK_TO;
const workLabel = () => ({ from: `${two(WORK_FROM)}:00`, to: `${two(WORK_TO)}:00` });

/** The level (low, mid, high) nearest to a volume of the engine. */
export function levelOf(volume) {
  let best = "mid";
  for (const id of LEVEL_IDS) if (Math.abs(LEVELS[id] - volume) < Math.abs(LEVELS[best] - volume)) best = id;
  return best;
}

/**
 * createPlayer({ engine, prefs, now, hora, reduced, hidden }) -> the engine's interface plus
 *   status() -> { enabled, mode, actions, background, muted, muteUntil, work: { on, now, from, to }, level, volume }
 *     mode: "off" | "muted" | "work" | "none" | "actions" | "background" | "all", the one word the indicator shows
 *   setBus("actions" | "background", on), muteFor(ms), muteUntilTomorrow(), unmute(), setWork(on), setLevel(low | mid | high), cycleLevel()
 *   play(name, opts) -> whether a sound started, preview(name | id, opts) -> the same without the gates, history(), ownRecently(ms), onChange(fn), onPlay(fn)
 * `now()` is the clock of the page (ms), `hora(ms)` the decimal hour of the working day's zone, `reduced()` whether motion is asked to be calm.
 */
export function createPlayer({ engine, prefs, now = Date.now, hora = (ms) => horaOf(ms), reduced = () => false, hidden = () => typeof document !== "undefined" && document.hidden === true }) {
  const changeListeners = new Set();
  const playListeners = new Set();
  const history = [];
  let ownAt = -Infinity;       // the last time he did something that changes data (cue.mutates), heard or not
  let lastBackground = -Infinity;

  const tell = (set, arg) => {
    for (const fn of Array.from(set)) {
      try { fn(arg); } catch (e) { console.error(e); }
    }
  };
  const emit = () => tell(changeListeners);
  prefs.onChange(() => emit());

  const muted = (t) => prefs.get("muteUntil") > t;

  /** Why a cue may not sound now, or null when it may. */
  function why(cue, t) {
    if (!engine.enabled) return "off";
    const p = prefs.get();
    if (!p[cue.bus]) return "bus";
    if (p.muteUntil > t) return "muted";
    if (p.work && isWorkHour(hora(t))) return "work";
    if (cue.calm === "skip" && reduced()) return "calm";
    if (cue.bus === "background") {
      if (hidden()) return "hidden";
      const bus = BUSES.background;
      if (!bus.exempt.includes(cue.id) && t - lastBackground < bus.minGapMs) return "gap";
    }
    return null;
  }

  const optionsOf = (cue, opts) => ({ ...cue.opts, ...opts, gain: cue.gain ?? BUSES[cue.bus].gain, unsolicited: false });

  function remember(cue, t, preview) {
    history.push({ id: cue.id, bus: cue.bus, at: t, preview });
    if (history.length > HISTORY) history.shift();
    tell(playListeners, history[history.length - 1]);
  }

  function play(name, opts) {
    try {
      const cue = cueOf(name, opts || {});
      if (!cue) return false;
      const t = now();
      if (cue.mutates) ownAt = t;                         // the data changed whether it made a sound or not
      if (why(cue, t)) return false;
      if (!engine.play(cue.recipe, optionsOf(cue, opts))) return false;
      if (cue.bus === "background" && !BUSES.background.exempt.includes(cue.id)) lastBackground = t;      // an error does not move the minute
      remember(cue, t, false);
      return true;
    } catch (e) {
      return false;
    }
  }

  /** `alt`: the other direction of a cue that has two (on and off, light and dark, English and Spanish). */
  function preview(name, opts) {
    try {
      const cue = cueOf(name, opts || {});
      if (!cue || !engine.enabled) return false;
      const direction = opts && opts.alt && cue.alt ? cue.alt : null;
      const { alt, ...rest } = opts || {};
      if (!engine.preview(cue.recipe, optionsOf(cue, { ...rest, ...direction }))) return false;
      remember(cue, now(), true);
      return true;
    } catch (e) {
      return false;
    }
  }

  function status() {
    const t = now();
    const p = prefs.get();
    const isMuted = p.muteUntil > t;
    const workNow = isWorkHour(hora(t));
    let mode;
    if (!engine.enabled) mode = "off";
    else if (isMuted) mode = "muted";
    else if (p.work && workNow) mode = "work";
    else if (p.actions && p.background) mode = "all";
    else if (p.actions) mode = "actions";
    else if (p.background) mode = "background";
    else mode = "none";
    return {
      enabled: !!engine.enabled, mode, actions: p.actions, background: p.background,
      muted: isMuted, muteUntil: isMuted ? p.muteUntil : 0,
      work: { on: p.work, now: workNow, ...workLabel() },
      level: levelOf(engine.volume), volume: engine.volume,
    };
  }

  function setEnabled(on) {
    const result = engine.setEnabled(on);
    emit();
    return result;
  }

  function setLevel(id) {
    if (!Object.hasOwn(LEVELS, id)) return engine.volume;
    const v = engine.setVolume(LEVELS[id]);
    emit();
    return v;
  }

  return {
    get enabled() { return engine.enabled; },
    get volume() { return engine.volume; },
    get ctx() { return engine.ctx; },
    get names() { return engine.names; },
    setEnabled,
    toggle: () => setEnabled(!engine.enabled),
    setVolume(v) {
      const r = engine.setVolume(v);
      emit();
      return r;
    },
    setBusy: (b) => engine.setBusy(b),
    tap: (node) => engine.tap(node),
    recent: () => engine.recent(),
    play,
    preview,
    status,
    setBus(bus, on) {
      if (bus === "actions" || bus === "background") prefs.set(bus, !!on);
    },
    muteFor(ms) {
      const until = now() + Math.max(0, ms);
      prefs.set("muteUntil", until);
      return until;
    },
    muteUntilTomorrow() {
      const t = now();
      let ahead = (TOMORROW_AT - hora(t) + 24) % 24;
      if (ahead < 2) ahead += 24;
      const until = t + Math.round(ahead * HOUR);
      prefs.set("muteUntil", until);
      return until;
    },
    unmute: () => prefs.set("muteUntil", 0),
    setWork: (on) => prefs.set("work", !!on),
    setLevel,
    cycleLevel() {
      const next = LEVEL_IDS[(LEVEL_IDS.indexOf(levelOf(engine.volume)) + 1) % LEVEL_IDS.length];
      setLevel(next);
      return next;
    },
    /** The sounds that started, oldest first (the last 24). */
    history: () => history.slice(),
    /** Did he change data in the last `ms`? (A snapshot that shows his own doing is not news.) */
    ownRecently: (ms = OWN_MS) => now() - ownAt <= ms,
    onChange(fn) {
      changeListeners.add(fn);
      return () => changeListeners.delete(fn);
    },
    onPlay(fn) {
      playListeners.add(fn);
      return () => playListeners.delete(fn);
    },
  };
}
