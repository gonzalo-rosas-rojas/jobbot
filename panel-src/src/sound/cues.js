// The sound vocabulary of the page, as DATA: which sound belongs to which thing that happens. One table, one file, so that "what does this
// sound mean?" has one answer and the sheet "Escuchar los sonidos" (ui/sound-ui.js) can list every one of them. The synthesis is in
// recipes.js (the engine's own recipes) and gestures.js (the ones this vocabulary adds); this file only chooses.
//
// Two buses, controlled apart (sound/player.js):
//   actions     what the user does: moves, opens, presses, approves, puts aside, stops the engine, changes a preference
//   background  what happens without him, found by comparing one snapshot with the next (sound/background.js): quieter, rarer, never at load
//
// A cue is { id, bus, group, recipe, opts?, calm?, mutates?, click?, gain?, max }
//   id        the name callers use: sound.play("approve"), sound.play("arrive", { room: "cola" }) -> "arrive.cola"
//   recipe    the name in recipes.js or gestures.js that the engine plays; `opts` are fixed options of it (a room, a direction), and what the
//             caller passes is merged over them (sound.play("switch", { on: false }))
//   alt       the other direction of a cue that has two (a switch goes up for on and down for off): the sheet plays one, then the other
//   calm      "skip": dropped when motion is asked to be calm (navigation sounds go with the trips they accompany)
//   mutates   it follows a change of data that HE made: the background detector will not sound the snapshot that shows it (it is his own doing)
//   click     built on a noise click, so the spectrum rules of the tests are the click rules (a share above 4 kHz is allowed)
//   gain      level against the recipe (default: the bus's); the background bus is a little quieter still
//   max       the longest it may last, in seconds (micro-interactions under 350 ms, arrivals 420, background under 900): the tests hold it
// The words ("Aprobar una propuesta" / "Aprobar, a, dos pulsaciones") are i18n: `sound.cue.<id>.name` and `.when`, in es and en.

import { ROOM_IDS } from "./recipes.js";

export const BUSES = Object.freeze({
  actions: Object.freeze({ id: "actions", gain: 1 }),
  // a background sound is a hint, not an alert: quieter than his own, at most one a minute (an error excepted), none while the tab is hidden
  background: Object.freeze({ id: "background", gain: 0.6, minGapMs: 60 * 1000, exempt: Object.freeze(["bg.error"]) }),
});

const A = "actions";
const B = "background";

const arrive = (room) => ({ id: `arrive.${room}`, bus: A, group: "move", recipe: "arrive", opts: { room }, calm: "skip", max: 0.42 });

export const CUES = Object.freeze([
  // ---- moving
  { id: "step", bus: A, group: "move", recipe: "tick", click: true, calm: "skip", max: 0.05 },
  ...ROOM_IDS.map(arrive),
  // ---- opening and closing: the drawer, the palette, the help, a menu, the sheet of sounds
  { id: "open", bus: A, group: "open", recipe: "open", click: true, max: 0.12 },
  { id: "close", bus: A, group: "open", recipe: "close", click: true, max: 0.12 },
  // ---- the two presses of what cannot be undone
  { id: "arm", bus: A, group: "twopress", recipe: "arm", max: 0.15 },
  { id: "confirm", bus: A, group: "twopress", recipe: "confirm", click: true, max: 0.15 },
  // ---- going forward (rising, warm)
  { id: "approve", bus: A, group: "advance", recipe: "approve", mutates: true, max: 0.2 },
  { id: "sent", bus: A, group: "advance", recipe: "sent", mutates: true, max: 0.3 },
  { id: "register", bus: A, group: "advance", recipe: "register", mutates: true, max: 0.3 },
  { id: "done", bus: A, group: "advance", recipe: "done", click: true, mutates: true, max: 0.2 },
  { id: "taskadd", bus: A, group: "advance", recipe: "taskadd", mutates: true, max: 0.25 },
  // ---- putting aside and taking back (falling)
  { id: "snooze", bus: A, group: "retreat", recipe: "snooze", mutates: true, max: 0.3 },
  { id: "undo", bus: A, group: "retreat", recipe: "undo", mutates: true, max: 0.15 },
  { id: "withdraw", bus: A, group: "retreat", recipe: "withdraw", mutates: true, max: 0.3 },
  // ---- the engine's switch: low and warm down, clear up
  { id: "stop", bus: A, group: "engine", recipe: "stop", mutates: true, max: 0.35 },
  { id: "resume", bus: A, group: "engine", recipe: "resume", mutates: true, max: 0.3 },
  // ---- preferences
  { id: "switch", bus: A, group: "pref", recipe: "toggle", opts: { on: true }, alt: { on: false }, max: 0.1 },
  { id: "theme", bus: A, group: "pref", recipe: "theme", opts: { to: "dark" }, alt: { to: "light" }, max: 0.3 },
  { id: "lang", bus: A, group: "pref", recipe: "lang", opts: { to: "en" }, alt: { to: "es" }, max: 0.15 },
  // ---- something he asked for failed
  { id: "error", bus: A, group: "fail", recipe: "error", max: 0.15 },
  // ---- the background: what happens by itself
  { id: "bg.run", bus: B, group: "bg", recipe: "run", max: 0.9 },
  { id: "bg.runfail", bus: B, group: "bg", recipe: "late", max: 0.2 },
  { id: "bg.queue", bus: B, group: "bg", recipe: "turn", max: 0.25 },
  { id: "bg.reply", bus: B, group: "bg", recipe: "chord", max: 0.9 },
  { id: "bg.interview", bus: B, group: "bg", recipe: "ceremony", max: 0.9 },
  { id: "bg.gate", bus: B, group: "bg", recipe: "gate", max: 0.9 },
  { id: "bg.stop", bus: B, group: "bg", recipe: "stop", max: 0.9 },
  { id: "bg.go", bus: B, group: "bg", recipe: "resume", max: 0.9 },
  { id: "bg.error", bus: B, group: "bg", recipe: "error", gain: 0.7, max: 0.9 },
].map((cue) => Object.freeze(cue)));

export const CUE_BY_ID = Object.freeze(Object.fromEntries(CUES.map((cue) => [cue.id, cue])));

/** The groups of each bus in the order the sheet lists them (`sound.group.<id>` is the title). */
export const GROUPS = Object.freeze({
  actions: Object.freeze(["move", "open", "twopress", "advance", "retreat", "engine", "pref", "fail"]),
  background: Object.freeze(["bg"]),
});

// Names that older callers used before the vocabulary: accepted and mapped (null: silent on purpose). A name that is neither a cue nor here is no sound.
// ("ceremony" is the recipe of the interview cue: the sound lab and its tests still call that name, and it lands on the cue that is the same sound.)
export const ALIASES = Object.freeze({ tick: "step", press: "confirm", release: null, toggle: "switch", ceremony: "bg.interview" });

/** The cue a call means, or null: `play("arrive", { room })` is "arrive.<room>", an alias is its cue, anything else must be a cue id. */
export function cueOf(name, opts = {}) {
  if (name === "arrive") {
    return CUE_BY_ID[`arrive.${opts.room}`] || null;
  }
  const id = Object.hasOwn(ALIASES, name) ? ALIASES[name] : name;
  return typeof id === "string" && Object.hasOwn(CUE_BY_ID, id) ? CUE_BY_ID[id] : null;
}
