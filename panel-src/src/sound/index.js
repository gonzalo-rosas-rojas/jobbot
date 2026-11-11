// The sound engine. Synthesised with the Web Audio API only (no files, no libraries, no network): the recipes in
// recipes.js build the nodes, this module decides whether a sound may play and keeps the chain safe.
//
// Product rules (all tested in test/sound.test.mjs):
// - Off until he turns it on, and no AudioContext exists before a user gesture. Importing this module creates
//   nothing. setEnabled(true) is what creates the context, so call it from the M key or the speaker button.
// - Persistence: "panel.sound" ("1"/"0") and "panel.volume" in localStorage, always inside try/catch. When the saved
//   state is on, `enabled` is true right after load but there is NO context yet and play() returns false; the shell
//   must call setEnabled(true) on the first pointer or key event after load, which is the gesture that creates it.
// - Budget: at most 2 sounds per rolling second (a more important newcomer replaces the weakest of the two, which
//   is faded out; priority in recipes.js META), identical sounds within 80 ms are dropped, at most 6 voices at once
//   (the quietest loses).
// - Unsolicited sounds (turn, late: not caused by his own action) are dropped while the tab is hidden or
//   while the shell says he is busy (setBusy), and run at most once per 5 minutes per name. His own actions are
//   never dropped for being busy or hidden, only for budget reasons. preview() skips all of that (settings use it).
// - If Web Audio is missing, `sound` still exists and every method is a no-op that returns false or null.
//
// What the shell does (src/ui): on boot nothing; on the first pointerdown or keydown of the page,
//   if (sound.enabled && !sound.ctx) sound.setEnabled(true);   // the saved "on" becomes real with a gesture
// the M key and the speaker button call sound.toggle(); events call sound.play("approve"), sound.play("arrive",
// { room: "cola" }), sound.play("turn", { unsolicited: true }); the schedule calls setBusy().

import { RECIPES as BASE_RECIPES, META as BASE_META, ROOM_IDS } from "./recipes.js";
import { GESTURES, GESTURE_META } from "./gestures.js";
import { buildChain } from "./chain.js";

// The engine plays the recipes of recipes.js and the gestures that the cue vocabulary (cues.js) adds; `names` is still the recipes' own list.
const RECIPES = { ...BASE_RECIPES, ...GESTURES };
const META = { ...BASE_META, ...GESTURE_META };

const SOUND_KEY = "panel.sound";
const VOLUME_KEY = "panel.volume";
const DEFAULT_VOLUME = 0.25;
const MAX_VOLUME = 0.6;
const SLOTS = 2;
const WINDOW_MS = 1000;
const DUPLICATE_MS = 80;
const UNSOLICITED_MS = 5 * 60 * 1000;
const MAX_VOICES = 6;
const GRACE_MS = 1500; // after setEnabled(true) the context may still say "suspended" for a few ms
const UNSOLICITED = new Set(["turn", "late"]);
const NAMES = Object.freeze(Object.keys(BASE_RECIPES));

const AudioCtor = () => globalThis.AudioContext || globalThis.webkitAudioContext || null;
const clock = () => performance.now();
const clamp = (v) => Math.min(MAX_VOLUME, Math.max(0, v));

function read(key) {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key, value) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* blocked or private storage: the choice only lasts for this page */
  }
}

let enabled = read(SOUND_KEY) === "1" && !!AudioCtor();
let volume = (() => {
  const v = parseFloat(read(VOLUME_KEY));
  return Number.isFinite(v) ? clamp(v) : DEFAULT_VOLUME;
})();
let busy = false;
let ctx = null;
let chain = null;
let armedUntil = 0;
const recent = []; // voices started in the last second, oldest first
const lastStart = new Map(); // key -> ms of its last start
const lastUnsolicited = new Map(); // name -> ms

function ensure() {
  if (ctx) return ctx;
  const Ctor = AudioCtor();
  if (!Ctor) return null;
  try {
    const c = new Ctor({ latencyHint: "interactive" });
    chain = buildChain(c, volume);
    ctx = c;
  } catch {
    ctx = chain = null;
  }
  return ctx;
}

function wake() {
  try {
    const p = ctx.resume();
    if (p && p.catch) p.catch(() => {});
  } catch {
    /* the browser refused: the next gesture will try again */
  }
}

function setEnabled(on) {
  on = !!on;
  if (on && !ensure()) on = false;
  enabled = on;
  write(SOUND_KEY, on ? "1" : "0");
  if (on) {
    armedUntil = clock() + GRACE_MS;
    wake();
  } else if (ctx) {
    try {
      ctx.suspend().catch(() => {});
    } catch {
      /* nothing to suspend */
    }
  }
  return enabled;
}

function setVolume(v) {
  v = Number(v);
  if (!Number.isFinite(v)) return volume;
  volume = clamp(v);
  write(VOLUME_KEY, String(volume));
  if (chain) chain.master.gain.setTargetAtTime(volume, ctx.currentTime, 0.02);
  return volume;
}

const hidden = () => typeof document !== "undefined" && document.hidden === true;

// A tab nobody is looking at keeps no audio clock running: the context sleeps while the page is hidden and wakes when it is
// shown again (the page has had its gesture, so the browser lets it).
if (typeof document !== "undefined") {
  document.addEventListener("visibilitychange", () => {
    if (!ctx || ctx.state === "closed") return;
    if (document.hidden) {
      try {
        ctx.suspend().catch(() => {});
      } catch {
        /* nothing to suspend */
      }
    } else if (enabled) wake();
  });
}

function ready() {
  if (!ctx || ctx.state === "closed") return false;
  if (ctx.state === "running") return true;
  wake();
  return ctx.state === "suspended" && clock() < armedUntil;
}

function keyOf(name, opts, room) {
  if (name === "arrive") return `arrive:${room}`;
  return name === "toggle" ? `toggle:${opts.on === false ? "off" : "on"}` : name;
}

function cut(voice, at) {
  voice.cut = true;
  try {
    voice.bus.gain.setTargetAtTime(0, at, 0.008);
  } catch {
    /* the voice is already gone */
  }
}

function prune(now) {
  while (recent.length && now - recent[0].t > WINDOW_MS) recent.shift();
}

const weakest = (voices, by) => voices.reduce((a, b) => (by(b) < by(a) ? b : a));

function start(name, opts, forced) {
  if (!enabled || !Object.hasOwn(RECIPES, name) || volume < 0.005 || !ready()) return false;
  const room = name === "arrive" ? opts.room : null;
  if (name === "arrive" && !ROOM_IDS.includes(room)) return false;
  const now = clock();
  const meta = META[name];
  const key = keyOf(name, opts, room);
  const unsolicited = opts.unsolicited ?? UNSOLICITED.has(name);
  const victims = [];
  prune(now);

  if (!forced) {
    if (unsolicited) {
      if (hidden() || busy) return false;
      if (now - (lastUnsolicited.get(name) ?? -Infinity) < UNSOLICITED_MS) return false;
    }
    if (now - (lastStart.get(key) ?? -Infinity) < DUPLICATE_MS) return false;
    const counted = recent.filter((v) => !v.cut && !v.forced); // previews do not use up the budget
    if (counted.length >= SLOTS) {
      const loser = weakest(counted, (v) => v.pri);
      if (meta.pri <= loser.pri) return false;
      victims.push(loser);
    }
  }
  const sounding = recent.filter((v) => !v.cut && v.end > now && !victims.includes(v));
  if (sounding.length >= MAX_VOICES) {
    const quiet = weakest(sounding, (v) => v.level);
    if (meta.level <= quiet.level) return false;
    victims.push(quiet);
  }

  const bus = ctx.createGain();
  bus.gain.value = Number.isFinite(opts.gain) ? Math.max(0, opts.gain) : 1; // the bus of the cue (cues.js) asks for its own level
  bus.connect(chain.master);
  const audioNow = ctx.currentTime; // read once per sound; every time below derives from it
  const t0 = audioNow + 0.01;
  let seconds;
  try {
    seconds = RECIPES[name](ctx, bus, t0, { ...opts, room, seed: opts.seed ?? (Math.random() * 4294967296) >>> 0 });
  } catch {
    bus.disconnect();
    return false;
  }
  victims.forEach((v) => cut(v, audioNow));
  const voice = { name, key, pri: meta.pri, level: meta.level, t: now, end: now + seconds * 1000 + 20, bus, cut: false, forced };
  recent.push(voice);
  if (!forced) {
    lastStart.set(key, now);
    if (unsolicited) lastUnsolicited.set(name, now);
  }
  // a timer, not onended: a context that never started must not leave nodes behind either
  setTimeout(() => bus.disconnect(), (seconds + 0.3) * 1000);
  return true;
}

function guarded(name, opts, forced) {
  try {
    return start(name, opts || {}, forced);
  } catch {
    return false;
  }
}

export const sound = {
  get enabled() {
    return enabled;
  },
  get volume() {
    return volume;
  },
  get ctx() {
    return ctx;
  },
  get names() {
    return NAMES;
  },
  setEnabled,
  toggle: () => setEnabled(!enabled),
  setVolume,
  setBusy(b) {
    busy = !!b;
  },
  play: (name, opts) => guarded(name, opts, false),
  preview: (name, opts) => guarded(name, opts, true),
  // lab and tests only: connect a node (an AnalyserNode) after the master chain; false if there is no context yet
  tap(node) {
    if (!chain) return false;
    chain.ceiling.connect(node);
    return true;
  },
  // lab and tests only: the voices started in the last second, so rules can be asserted without ears
  recent() {
    prune(clock());
    return recent.map((v) => ({ name: v.name, key: v.key, ageMs: Math.round(clock() - v.t), cut: v.cut }));
  },
};
