// The sound engine. Synthesised with the Web Audio API only (no files, no libraries, no network): the recipes in
// recipes.js build the nodes, this module decides whether a sound may play and keeps the chain safe.
//
// Product rules (all tested in test/sound.test.mjs):
// - Off until he turns it on, and no AudioContext exists before a user gesture. Importing this module creates
//   no AudioContext (it only listens for the tab being hidden or shown). setEnabled(true) is what creates the context, so call it from the M key or the speaker button.
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
import { isHidden } from "./page.js";
import { local } from "../core/store.js";
import { clamp } from "../core/math.js";
import { SECOND_MS, MINUTE_MS } from "../core/time.js";

// The engine plays the recipes of recipes.js and the gestures that the cue vocabulary (cues.js) adds; `names` is still the recipes' own list.
const RECIPES = { ...BASE_RECIPES, ...GESTURES };
const META = { ...BASE_META, ...GESTURE_META };

const SOUND_KEY = "sound";                 // the shelf adds the "panel." prefix: "panel.sound" and "panel.volume" in localStorage
const VOLUME_KEY = "volume";
const DEFAULT_VOLUME = 0.25;
const MAX_VOLUME = 0.6;
const SLOTS = 2;
const WINDOW_MS = 1000;
const DUPLICATE_MS = 80;
const UNSOLICITED_MS = 5 * MINUTE_MS;
const MAX_VOICES = 6;
const GRACE_MS = 1500; // after setEnabled(true) the context may still say "suspended" for a few ms
const CUT_TIME_S = 0.008;          // a displaced voice fades out with this time constant
const SILENT_BELOW = 0.005;       // under this volume nothing is played
const START_LEAD_S = 0.01;        // a sound starts this far ahead of the audio clock
const VOLUME_TIME_S = 0.02;       // the master volume settles with this time constant
const SEED_RANGE = 2 ** 32;       // the seed of a sound with no seed of its own
const END_PAD_MS = 20;            // a voice counts as sounding this long after its recipe says it ended
const DISCONNECT_PAD_S = 0.3;     // its bus is cut loose this long after the recipe's end
const UNSOLICITED = new Set(["turn", "late"]);
const NAMES = Object.freeze(Object.keys(BASE_RECIPES));

const AudioCtor = () => globalThis.AudioContext || globalThis.webkitAudioContext || null;
const clock = () => performance.now();

// Blocked or private storage: core/store.js keeps the choice in memory, so it only lasts for this page.
let enabled = local.get(SOUND_KEY) === "1" && !!AudioCtor();
let volume = (() => {
  const v = parseFloat(local.get(VOLUME_KEY));
  return Number.isFinite(v) ? clamp(v, 0, MAX_VOLUME) : DEFAULT_VOLUME;
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
    const audio = new Ctor({ latencyHint: "interactive" });
    chain = buildChain(audio, volume);
    ctx = audio;
  } catch (e) {
    console.error(e);
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

// A context that is closing or already gone has nothing to suspend.
function suspendQuietly() {
  try {
    ctx.suspend().catch(() => {});
  } catch {
    /* nothing to suspend */
  }
}

function setEnabled(on) {
  on = !!on;
  if (on && !ensure()) on = false;
  enabled = on;
  local.set(SOUND_KEY, on ? "1" : "0");
  if (on) {
    armedUntil = clock() + GRACE_MS;
    wake();
  } else if (ctx) {
    suspendQuietly();
  }
  return enabled;
}

function setVolume(v) {
  v = Number(v);
  if (!Number.isFinite(v)) return volume;
  volume = clamp(v, 0, MAX_VOLUME);
  local.set(VOLUME_KEY, String(volume));
  if (chain) chain.master.gain.setTargetAtTime(volume, ctx.currentTime, VOLUME_TIME_S);
  return volume;
}

// A tab nobody is looking at keeps no audio clock running: the context sleeps while the page is hidden and wakes when it is
// shown again (the page has had its gesture, so the browser lets it).
if (typeof document !== "undefined") {
  document.addEventListener("visibilitychange", () => {
    if (!ctx || ctx.state === "closed") return;
    if (document.hidden) suspendQuietly();
    else if (enabled) wake();
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
    voice.bus.gain.setTargetAtTime(0, at, CUT_TIME_S);
  } catch {
    /* the voice is already gone */
  }
}

function prune(now) {
  while (recent.length && now - recent[0].t > WINDOW_MS) recent.shift();
}

const weakest = (voices, by) => voices.reduce((a, b) => (by(b) < by(a) ? b : a));

// May this sound start now? The voices it replaces (maybe none), or null when the budget says no. `unsolicited` and `forced` as in start().
function admit({ name, meta, key, unsolicited, forced, now }) {
  const victims = [];
  prune(now);

  if (!forced) {
    if (unsolicited) {
      if (isHidden() || busy) return null;
      if (now - (lastUnsolicited.get(name) ?? -Infinity) < UNSOLICITED_MS) return null;
    }
    if (now - (lastStart.get(key) ?? -Infinity) < DUPLICATE_MS) return null;
    const counted = recent.filter((v) => !v.cut && !v.forced); // previews do not use up the budget
    if (counted.length >= SLOTS) {
      const loser = weakest(counted, (v) => v.pri);
      if (meta.pri <= loser.pri) return null;
      victims.push(loser);
    }
  }
  const sounding = recent.filter((v) => !v.cut && v.end > now && !victims.includes(v));
  if (sounding.length >= MAX_VOICES) {
    const quiet = weakest(sounding, (v) => v.level);
    if (meta.level <= quiet.level) return null;
    victims.push(quiet);
  }
  return victims;
}

// The bus of one sound, into the master; the cue (cues.js) may ask for its own level.
function openBus(opts) {
  const bus = ctx.createGain();
  bus.gain.value = Number.isFinite(opts.gain) ? Math.max(0, opts.gain) : 1;
  bus.connect(chain.master);
  return bus;
}

// A started voice counts against the budget; a preview (`forced`) does not touch the rules that limit repeats.
function remember(voice, unsolicited) {
  recent.push(voice);
  if (voice.forced) return;
  lastStart.set(voice.key, voice.t);
  if (unsolicited) lastUnsolicited.set(voice.name, voice.t);
}

function start(name, opts, forced) {
  if (!enabled || !Object.hasOwn(RECIPES, name) || volume < SILENT_BELOW || !ready()) return false;
  const room = name === "arrive" ? opts.room : null;
  if (name === "arrive" && !ROOM_IDS.includes(room)) return false;
  const now = clock();
  const meta = META[name];
  const key = keyOf(name, opts, room);
  const unsolicited = opts.unsolicited ?? UNSOLICITED.has(name);
  const victims = admit({ name, meta, key, unsolicited, forced, now });
  if (!victims) return false;

  const bus = openBus(opts);
  const audioNow = ctx.currentTime; // read once per sound; every time below derives from it
  const t0 = audioNow + START_LEAD_S;
  let seconds;
  try {
    seconds = RECIPES[name](ctx, bus, t0, { ...opts, room, seed: opts.seed ?? (Math.random() * SEED_RANGE) >>> 0 });
  } catch (e) {
    console.error(e);
    bus.disconnect();
    return false;
  }
  victims.forEach((v) => cut(v, audioNow));
  const voice = { name, key, pri: meta.pri, level: meta.level, t: now, end: now + seconds * SECOND_MS + END_PAD_MS, bus, cut: false, forced };
  remember(voice, unsolicited);
  // a timer, not onended: a context that never started must not leave nodes behind either
  setTimeout(() => bus.disconnect(), (seconds + DISCONNECT_PAD_S) * SECOND_MS);
  return true;
}

function guarded(name, opts, forced) {
  try {
    return start(name, opts || {}, forced);
  } catch (e) {
    console.error(e);
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
