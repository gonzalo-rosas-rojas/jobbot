// The player (src/sound/player.js): when a cue of the vocabulary may sound. With a fake engine for the rules, and with the REAL engine on a fake
// AudioContext that counts every node it is asked to make, for the promise that matters most on a work machine: a cue that is refused creates nothing.
import test from "node:test";
import assert from "node:assert/strict";
import { createPlayer, LEVELS, isWorkHour, levelOf, OWN_MS } from "../src/sound/player.js";
import { createPrefs } from "../src/sound/prefs.js";
import { CUES } from "../src/sound/cues.js";

const HOUR = 3600000;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
/** A shelf in memory with the two methods of core/store's `local`. */
const createPrefsForTest = () => { const box = new Map(); return { get: (k) => (box.has(k) ? box.get(k) : null), set: (k, v) => box.set(k, String(v)) }; };

/** A player on a fake engine, with a clock and an hour that the test moves. */
function rig({ enabled = true, hour = 20, reduced = false, hidden = false, stored = null } = {}) {
  const clock = { ms: Date.UTC(2026, 9, 9, 23, 0, 0), hour, reduced, hidden };
  const store = stored || createPrefsForTest();
  const engine = {
    enabled, volume: 0.25, ctx: {}, names: [], calls: [],
    setEnabled(on) { engine.enabled = !!on; return engine.enabled; },
    setVolume(v) { engine.volume = v; return v; },
    setBusy() {}, tap() { return false; }, recent() { return []; },
    play(name, opts) { engine.calls.push(["play", name, opts]); return engine.playResult !== false; },
    preview(name, opts) { engine.calls.push(["preview", name, opts]); return engine.enabled; },
  };
  const prefs = createPrefs({ store });
  const player = createPlayer({ engine, prefs, now: () => clock.ms, hora: () => clock.hour, reduced: () => clock.reduced, hidden: () => clock.hidden });
  return { clock, engine, prefs, player, store, names: () => engine.calls.map((c) => c[1]) };
}

test("the sound is off until he turns it on: no cue of the whole vocabulary reaches the engine", () => {
  const r = rig({ enabled: false });
  for (const cue of CUES) assert.equal(r.player.play(cue.id), false, cue.id);
  assert.deepEqual(r.engine.calls, []);
  assert.equal(r.player.status().mode, "off");
});

test("a cue reaches the engine with its recipe, its options and the level of its bus; the player decides, so the engine's own unsolicited rules do not", () => {
  const r = rig();
  assert.equal(r.player.play("approve"), true);
  assert.equal(r.player.play("arrive", { room: "cola" }), true);
  assert.equal(r.player.play("bg.reply"), true);
  assert.deepEqual(r.engine.calls, [
    ["play", "approve", { gain: 1, unsolicited: false }],
    ["play", "arrive", { room: "cola", gain: 1, unsolicited: false }],
    ["play", "chord", { gain: 0.6, unsolicited: false }],
  ]);
  r.clock.ms += 2 * 60000;
  assert.equal(r.player.play("bg.queue"), true);
  assert.deepEqual(r.engine.calls.at(-1), ["play", "turn", { gain: 0.6, unsolicited: false }], "the bell of the engine is unsolicited by name: the player says it is not");
  assert.equal(r.player.play("switch", { on: false }), true);
  assert.deepEqual(r.engine.calls.at(-1), ["play", "toggle", { on: false, gain: 1, unsolicited: false }], "what the caller passes wins over the cue's options");
  assert.equal(r.player.play("bg.error"), true);
  assert.equal(r.engine.calls.at(-1)[2].gain, 0.7, "a cue may have its own level");
});

test("what is not a cue is no sound, and the engine is never asked", () => {
  const r = rig();
  for (const bad of ["nope", "release", "toString", "__proto__", "", undefined, null, "late", "glance"]) assert.equal(r.player.play(bad), false, String(bad));
  assert.equal(r.player.play("arrive", { room: "prologo" }), false);
  assert.deepEqual(r.engine.calls, []);
  assert.equal(r.player.play("tick"), true, "the old name of the cursor's tick is still a cue");
  assert.equal(r.engine.calls[0][1], "tick");
});

test("the two buses are switched apart, and the choice is remembered", () => {
  const r = rig();
  r.player.setBus("background", false);
  assert.equal(r.player.play("bg.run"), false);
  assert.equal(r.player.play("approve"), true);
  assert.equal(r.player.status().mode, "actions");
  r.player.setBus("actions", false);
  assert.equal(r.player.play("approve"), false);
  assert.equal(r.player.status().mode, "none");
  r.player.setBus("background", true);
  assert.equal(r.player.status().mode, "background");
  assert.equal(r.player.play("bg.run"), true);
  r.player.setBus("actions", true);
  assert.equal(r.player.status().mode, "all");
  // a new player on the same storage hears the same choice
  r.player.setBus("background", false);
  const again = rig({ stored: r.store });
  assert.equal(again.player.status().background, false);
  assert.equal(again.player.status().actions, true);
});

test("a silence of an hour ends by itself, is remembered, and can be ended; 'hasta mañana' ends at the next 06:00, at least two hours away", () => {
  const r = rig({ hour: 21 });
  const until = r.player.muteFor(HOUR);
  assert.equal(until, r.clock.ms + HOUR);
  assert.equal(r.player.play("approve"), false);
  assert.equal(r.player.play("bg.run"), false);
  assert.deepEqual([r.player.status().mode, r.player.status().muteUntil], ["muted", until]);
  assert.equal(createPrefs({ store: r.store }).get("muteUntil"), until);
  r.clock.ms = until - 1;
  assert.equal(r.player.play("approve"), false);
  r.clock.ms = until;
  assert.equal(r.player.play("approve"), true, "at the end it sounds again, with no one touching anything");
  assert.equal(r.player.status().mode, "all");
  r.player.muteFor(HOUR);
  r.player.unmute();
  r.clock.ms += 2 * 60000;
  assert.equal(r.player.play("bg.run"), true, "ending the silence by hand");

  for (const [hour, ahead] of [[22, 8], [23.5, 6.5], [0, 6], [3, 3], [4, 2], [5, 25], [5.9, 24.1], [6, 24], [12, 18]]) {
    const q = rig({ hour });
    const t = q.player.muteUntilTomorrow();
    assert.ok(Math.abs((t - q.clock.ms) / HOUR - ahead) < 0.001, `at ${hour} h it should end in ${ahead} h, ends in ${(t - q.clock.ms) / HOUR}`);
  }
});

test("working hours are mute by default, 09:00 to 18:00, and he can allow them; the hours are in the status", () => {
  assert.deepEqual([8.99, 9, 12, 17.99, 18, 18.01, 23, 0].map(isWorkHour), [false, true, true, true, false, false, false, false]);
  const r = rig({ hour: 10 });
  assert.equal(r.player.play("approve"), false);
  assert.equal(r.player.play("arrive", { room: "hoy" }), false);
  assert.equal(r.player.play("bg.reply"), false);
  assert.equal(r.player.play("error"), false, "not even a failure: the page is mute at work");
  assert.deepEqual(r.engine.calls, []);
  const s = r.player.status();
  assert.deepEqual([s.mode, s.work.on, s.work.now, s.work.from, s.work.to], ["work", true, true, "09:00", "18:00"]);
  r.clock.hour = 18;
  assert.equal(r.player.play("approve"), true, "at 18:00 sharp it is the evening");
  r.clock.hour = 9;
  r.clock.ms += 5000;
  assert.equal(r.player.play("close"), false, "at 09:00 sharp the working day has begun");
  r.player.setWork(false);
  assert.equal(r.player.status().mode, "all");
  assert.equal(r.player.play("close"), true);
  assert.equal(createPrefs({ store: r.store }).get("work"), false, "the choice is remembered");
  assert.equal(createPrefs({ store: createPrefsForTest() }).get("work"), true, "and the default is to be mute at work");
});

test("calm motion drops the navigation cues and keeps the rest", () => {
  const r = rig({ reduced: true });
  assert.equal(r.player.play("step"), false);
  assert.equal(r.player.play("tick"), false);
  assert.equal(r.player.play("arrive", { room: "cola" }), false);
  assert.equal(r.player.play("open"), true);
  assert.equal(r.player.play("approve"), true);
  r.clock.reduced = false;
  assert.equal(r.player.play("step"), true);
});

test("the background bus: none while the tab is hidden, one a minute at most, and an error is the exception; his own actions are not limited by it", () => {
  const r = rig();
  r.clock.hidden = true;
  assert.equal(r.player.play("bg.reply"), false);
  assert.equal(r.player.play("approve"), true, "his own action in a hidden tab (a keyboard shortcut) still sounds");
  r.clock.hidden = false;
  assert.equal(r.player.play("bg.reply"), true);
  r.clock.ms += 59000;
  assert.equal(r.player.play("bg.queue"), false, "59 s after the last: too soon");
  assert.equal(r.player.play("approve"), true, "his actions do not wait for the background");
  assert.equal(r.player.play("bg.error"), true, "an error is the exception");
  r.clock.ms += 1000;
  assert.equal(r.player.play("bg.queue"), true, "a minute after the last background sound: the error in between did not move the minute");
});

test("a refusal by the engine (its own budget, a context that is not awake) does not use up the background minute", () => {
  const r = rig();
  r.engine.playResult = false;
  assert.equal(r.player.play("bg.reply"), false);
  r.engine.playResult = true;
  assert.equal(r.player.play("bg.reply"), true, "nothing sounded, so nothing is waited for");
  assert.deepEqual(r.player.history().map((h) => h.id), ["bg.reply"]);
});

test("a cue that follows a change he made is remembered even when it is silent: the next snapshot is his own doing", () => {
  const r = rig({ enabled: false, hour: 10 });
  assert.equal(r.player.ownRecently(), false);
  r.player.play("snooze");
  assert.equal(r.player.ownRecently(), true, "muted and off, and still remembered");
  r.clock.ms += OWN_MS + 1;
  assert.equal(r.player.ownRecently(), false);
  r.player.play("step");
  assert.equal(r.player.ownRecently(), false, "moving is not a change of data");
  r.player.play("arm");
  r.player.play("confirm");
  assert.equal(r.player.ownRecently(), false, "pressing is not either: the cue of the outcome is");
  r.player.preview("approve");
  assert.equal(r.player.ownRecently(), false, "listening to a sound is not an action");
});

test("preview is for the sheet of sounds: it ignores the silence, the hours, the buses and calm, but not the master, and it plays the other direction on request", () => {
  const r = rig({ hour: 11, reduced: true });
  r.player.setBus("actions", false);
  r.player.setBus("background", false);
  r.player.muteFor(HOUR);
  assert.equal(r.player.play("step"), false);
  assert.equal(r.player.preview("step"), true);
  assert.equal(r.player.preview("bg.reply"), true);
  assert.equal(r.player.preview("arrive.cola"), true, "a cue by its id");
  assert.equal(r.player.preview("switch"), true);
  assert.equal(r.player.preview("switch", { alt: true }), true);
  assert.equal(r.player.preview("theme", { alt: true }), true);
  assert.equal(r.player.preview("approve", { alt: true }), true, "a cue with one direction plays as it is");
  assert.deepEqual(r.engine.calls.map((c) => [c[0], c[1], c[2].on ?? c[2].to ?? c[2].room ?? null]), [
    ["preview", "tick", null], ["preview", "chord", null], ["preview", "arrive", "cola"], ["preview", "toggle", true], ["preview", "toggle", false],
    ["preview", "theme", "light"], ["preview", "approve", null],
  ]);
  assert.deepEqual(r.player.history().map((h) => h.preview), Array(7).fill(true));
  r.engine.enabled = false;
  assert.equal(r.player.preview("approve"), false, "with the sound off a preview is nothing (the sheet turns it on first)");
  assert.equal(r.player.preview("nope"), false);
});

test("the status says one word: off, muted, work, none, actions, background or all; the level is one of three", () => {
  const r = rig();
  const mode = () => r.player.status().mode;
  assert.equal(mode(), "all");
  r.player.setEnabled(false);
  assert.equal(mode(), "off");
  r.player.setEnabled(true);
  r.clock.hour = 12;
  assert.equal(mode(), "work");
  r.player.muteFor(HOUR);
  assert.equal(mode(), "muted", "an explicit silence is the more specific word");
  assert.equal(levelOf(0.25), "mid");
  assert.deepEqual(Object.values(LEVELS), [0.12, 0.25, 0.4]);
  r.clock.hour = 20;
  r.player.unmute();
  assert.equal(r.player.status().level, "mid");
  assert.equal(r.player.cycleLevel(), "high");
  assert.equal(r.engine.volume, 0.4);
  assert.equal(r.player.cycleLevel(), "low");
  assert.equal(r.player.cycleLevel(), "mid");
  r.player.setLevel("nonsense");
  assert.equal(r.engine.volume, 0.25);
});

test("the player is the engine to the rest of the page: same members, and it tells whoever listens when something changed", () => {
  const r = rig({ enabled: false });
  let changes = 0;
  const off = r.player.onChange(() => { changes += 1; });
  assert.equal(r.player.enabled, false);
  r.player.setEnabled(true);
  r.player.toggle();
  r.player.setVolume(0.3);
  r.player.setBus("actions", false);
  r.player.muteFor(1000);
  r.player.unmute();
  assert.equal(changes, 6);
  off();
  r.player.toggle();
  assert.equal(changes, 6);
  assert.deepEqual([r.player.volume, r.player.ctx !== undefined, Array.isArray(r.player.names), r.player.tap({}), r.player.recent()], [0.3, true, true, false, []]);
  const heard = [];
  r.player.onPlay((h) => heard.push(h.id));
  r.engine.enabled = true;
  r.player.setBus("actions", true);
  r.clock.hour = 20;
  r.player.play("approve");
  assert.deepEqual(heard, ["approve"]);
});

test("an engine that throws never reaches the page", () => {
  const r = rig();
  r.engine.play = () => { throw new Error("boom"); };
  r.engine.preview = () => { throw new Error("boom"); };
  assert.equal(r.player.play("approve"), false);
  assert.equal(r.player.preview("approve"), false);
});

// ---- the real engine on a fake AudioContext that counts nodes -----------------------------------------------------------------------

test("a refused cue creates NOTHING in Web Audio, and an allowed one does: off, silence, hours, buses and calm, counted in nodes", async () => {
  let made = 0;
  const param = () => ({ value: 0, setValueAtTime() { return this; }, linearRampToValueAtTime() { return this; }, exponentialRampToValueAtTime() { return this; }, setTargetAtTime() { return this; } });
  const node = () => new Proxy({ connect: (d) => d, disconnect() {}, addEventListener() {}, start() {}, stop() {} }, {
    get(t, p) { if (p in t) return t[p]; if (typeof p === "symbol") return undefined; t[p] = param(); return t[p]; },
    set(t, p, v) { t[p] = v; return true; },
  });
  class FakeContext {
    constructor() { this.state = "running"; this.currentTime = 0; this.sampleRate = 44100; this.destination = node(); }
    resume() { return Promise.resolve(); }
    suspend() { return Promise.resolve(); }
    createBuffer(c, n) { return { getChannelData: () => new Float32Array(n) }; }
  }
  for (const make of ["createGain", "createOscillator", "createBiquadFilter", "createBufferSource", "createDynamicsCompressor", "createWaveShaper"]) {
    FakeContext.prototype[make] = function () { made += 1; return node(); };
  }
  globalThis.AudioContext = FakeContext;
  const { sound } = await import("../src/sound/index.js");
  const clock = { ms: Date.UTC(2026, 9, 9, 23, 0, 0), hour: 20, reduced: false };
  const prefs = createPrefs({ store: createPrefsForTest() });
  const player = createPlayer({ engine: sound, prefs, now: () => clock.ms, hora: () => clock.hour, reduced: () => clock.reduced, hidden: () => false });

  // the sound is off: nothing, not even the context
  for (const cue of CUES) player.play(cue.id);
  assert.equal(made, 0, "off: not one node");
  assert.equal(sound.ctx, null, "off: not even a context");

  player.setEnabled(true);
  const chain = made;
  assert.ok(chain > 0, "turning it on builds the master chain");
  assert.equal(sound.ctx.state, "running");

  const refused = async (label, setup, undo, names) => {
    setup();
    for (const name of names) assert.equal(player.play(name), false, `${label}: ${name}`);
    assert.equal(made, chain, `${label}: ${made - chain} nodes were created`);
    undo();
  };
  await refused("silence", () => player.muteFor(HOUR), () => player.unmute(), ["approve", "step", "bg.reply"]);
  await refused("working hours", () => { clock.hour = 12; }, () => { clock.hour = 20; }, ["approve", "error", "bg.run"]);
  await refused("actions off", () => player.setBus("actions", false), () => player.setBus("actions", true), ["approve", "open"]);
  await refused("background off", () => player.setBus("background", false), () => player.setBus("background", true), ["bg.reply", "bg.error"]);
  await refused("calm", () => { clock.reduced = true; }, () => { clock.reduced = false; }, ["step", "arrive"]);
  await refused("not a cue", () => {}, () => {}, ["release", "nope", "turn"]);

  assert.equal(player.play("approve"), true, "allowed");
  assert.ok(made > chain, "an allowed cue builds its nodes");
  const afterOne = made;
  player.setEnabled(false);
  await sleep(100);
  assert.equal(player.play("open"), false);
  assert.equal(made, afterOne, "turned off again: nothing more");
});

test("isWorkHour and the working-day label are the ones the sheet shows", () => {
  assert.equal(isWorkHour(9), true);
  assert.equal(isWorkHour(18), false);
});
