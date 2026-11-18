// The gestures that the cue vocabulary (cues.js) adds to the engine's recipes: same family, same rules. Sine and triangle partials in
// D major pentatonic (D E F# A B), short round envelopes with a linear attack from silence (never a click), nothing sustained above
// ~4 kHz, peaks under 0.3 before the master (recipes.js says why). Each one is a pure function (ctx, out, t0, opts) -> seconds, exactly
// like the recipes, and runs on any BaseAudioContext, so the lab renders them offline through the real master chain.
//
// The vocabulary they belong to (one gesture, one meaning, so that he can learn which sound is which action):
//   lifted single note ........ arm      the first press of a two-press action ("¿Seguro?")
//   two dry taps .............. confirm  the second press, or any press the server is now answering
//   rising, warm .............. sent, register, taskadd, done        something moves forward
//   falling, warm ............. snooze, withdraw (and undo, in recipes.js)   something is put aside or taken back
//   a long glide, low or clear  stop, resume      the engine's switch: low and warm down, clear up
//   two close pitches ......... theme, lang      a preference changed (up or down with the direction of the change)
//   slow, quiet, longer ....... run, chord, gate  what happens by itself (the background bus plays these a little quieter still)
//
// The pitch factor and the timing nudge of every sound come from jitter(opts), as in recipes.js: without opts.seed there is none.

import { burst, jitter, latestEnd, note, soft, warm } from "./envelopes.js";

const HZ = { D3: 146.83, A3: 220, B3: 246.94, D4: 293.66, E4: 329.63, Fs4: 369.99, A4: 440, B4: 493.88, D5: 587.33, E5: 659.25 };

// the first press of a two-press action: one note that lifts a whole tone and stops, a question mark
function arm(ctx, out, t0, opts) {
  const jit = jitter(opts);
  return note(ctx, out, { f: HZ.A4 * jit.pitch, t: t0, decay: 0.1, parts: [["sine", 1, 0.15], ["triangle", 1, 0.06]], attack: 0.008, to: HZ.B4 * jit.pitch }) - t0;
}

// the second press: two dry taps on the same pitch, with a hair of click under the first
function confirm(ctx, out, t0, opts) {
  const jit = jitter(opts);
  const parts = soft(1);
  return latestEnd(
    burst(ctx, out, { t: t0, dur: 0.006, f: 2400, q: 3, peak: 0.25, attack: 0.001, at: jit.r() * 0.8 }),
    note(ctx, out, { f: HZ.E4 * jit.pitch, t: t0, decay: 0.04, parts, attack: 0.003 }),
    note(ctx, out, { f: HZ.E4 * jit.pitch, t: t0 + jit.at(0.05), decay: 0.05, parts, attack: 0.003 }),
  ) - t0;
}

// "ya la envié": it went out; a rising fourth, softer than the approval and with no thump under it
function sent(ctx, out, t0, opts) {
  const jit = jitter(opts);
  return latestEnd(
    note(ctx, out, { f: HZ.Fs4 * jit.pitch, t: t0, decay: 0.1, parts: soft(1.4) }),
    note(ctx, out, { f: HZ.B4 * jit.pitch, t: t0 + jit.at(0.075), decay: 0.14, parts: soft(1.4) }),
  ) - t0;
}

// the company's answer written down: three steps up, a small staircase
function register(ctx, out, t0, opts) {
  const jit = jitter(opts);
  return latestEnd(
    note(ctx, out, { f: HZ.A4 * jit.pitch, t: t0, decay: 0.06, parts: soft(1.2) }),
    note(ctx, out, { f: HZ.B4 * jit.pitch, t: t0 + jit.at(0.06), decay: 0.06, parts: soft(1.2) }),
    note(ctx, out, { f: HZ.D5 * jit.pitch, t: t0 + jit.at(0.12), decay: 0.14, parts: soft(1.2) }),
  ) - t0;
}

// a task is done: one bright note and a speck of click, a tick on a list
function done(ctx, out, t0, opts) {
  const jit = jitter(opts);
  return latestEnd(
    burst(ctx, out, { t: t0, dur: 0.005, f: 3000, q: 3, peak: 0.2, attack: 0.001, at: jit.r() * 0.8 }),
    note(ctx, out, { f: HZ.D5 * jit.pitch, t: t0, decay: 0.11, parts: [["sine", 1, 0.2], ["sine", 2, 0.04]], attack: 0.004 }),
  ) - t0;
}

// a new task: two small steps up
function taskadd(ctx, out, t0, opts) {
  const jit = jitter(opts);
  return latestEnd(
    note(ctx, out, { f: HZ.E4 * jit.pitch, t: t0, decay: 0.06, parts: soft(1.3) }),
    note(ctx, out, { f: HZ.A4 * jit.pitch, t: t0 + jit.at(0.055), decay: 0.11, parts: soft(1.3) }),
  ) - t0;
}

// put aside until later: a falling third, soft and unhurried (the lowpass keeps it round)
function snooze(ctx, out, t0, opts) {
  const jit = jitter(opts);
  const lp = warm(ctx, out, 1500);
  const parts = [["sine", 1, 0.1], ["triangle", 1, 0.03]];
  return latestEnd(
    note(ctx, lp, { f: HZ.A4 * jit.pitch, t: t0, decay: 0.1, parts }),
    note(ctx, lp, { f: HZ.Fs4 * jit.pitch, t: t0 + jit.at(0.09), decay: 0.17, parts }),
  ) - t0;
}

// withdrawn: a falling fifth, low and closed
function withdraw(ctx, out, t0, opts) {
  const jit = jitter(opts);
  const lp = warm(ctx, out, 900);
  const parts = [["sine", 1, 0.16], ["triangle", 1, 0.05]];
  return latestEnd(
    note(ctx, lp, { f: HZ.Fs4 * jit.pitch, t: t0, decay: 0.1, parts }),
    note(ctx, lp, { f: HZ.B3 * jit.pitch, t: t0 + jit.at(0.085), decay: 0.17, parts }),
  ) - t0;
}

// the engine stops: one low, warm note that sinks a fourth and fades
function stop(ctx, out, t0, opts) {
  const jit = jitter(opts);
  const lp = warm(ctx, out, 700);
  return note(ctx, lp, { f: HZ.A3 * jit.pitch, t: t0, decay: 0.28, parts: [["sine", 1, 0.2], ["sine", 2, 0.07]], attack: 0.02, to: HZ.D3 * jit.pitch }) - t0;
}

// the engine runs again: the opposite, a clear note that climbs a fifth
function resume(ctx, out, t0, opts) {
  const jit = jitter(opts);
  return note(ctx, out, { f: HZ.D4 * jit.pitch, t: t0, decay: 0.24, parts: [["sine", 1, 0.13], ["triangle", 1, 0.05], ["sine", 2, 0.03]], attack: 0.012, to: HZ.A4 * jit.pitch }) - t0;
}

// the theme changed: a fifth, up toward Papel and down toward Tinta
function theme(ctx, out, t0, opts) {
  const jit = jitter(opts);
  const [first, second] = opts.to === "light" ? [HZ.A4, HZ.D5] : [HZ.D5, HZ.A4];
  return latestEnd(
    note(ctx, out, { f: first * jit.pitch, t: t0, decay: 0.09, parts: soft(1.1) }),
    note(ctx, out, { f: second * jit.pitch, t: t0 + jit.at(0.085), decay: 0.14, parts: soft(1.1) }),
  ) - t0;
}

// the language changed: a quick pair a fourth apart, dry triangles, up toward English and down toward Spanish
function lang(ctx, out, t0, opts) {
  const jit = jitter(opts);
  const [first, second] = opts.to === "en" ? [HZ.B4, HZ.E5] : [HZ.E5, HZ.B4];
  const parts = [["triangle", 1, 0.16]];
  return latestEnd(
    note(ctx, out, { f: first * jit.pitch, t: t0, decay: 0.04, parts, attack: 0.003 }),
    note(ctx, out, { f: second * jit.pitch, t: t0 + jit.at(0.05), decay: 0.07, parts, attack: 0.003 }),
  ) - t0;
}

// ---- what happens by itself: longer, with a slow attack, so that nothing about them is sudden

// a run of the bot ended well: a fifth that swells and goes, like a door that closes softly
function run(ctx, out, t0, opts) {
  const jit = jitter(opts);
  const lp = warm(ctx, out, 1600);
  const parts = [["sine", 1, 0.09], ["triangle", 1, 0.03]];
  return latestEnd(
    note(ctx, lp, { f: HZ.D4 * jit.pitch, t: t0, decay: 0.42, parts, attack: 0.03 }),
    note(ctx, lp, { f: HZ.A4 * jit.pitch, t: t0 + jit.at(0.03), decay: 0.4, parts, attack: 0.03 }),
  ) - t0;
}

// a company answered: a discreet D major chord, the three notes a hair apart like a soft strum
function chord(ctx, out, t0, opts) {
  const jit = jitter(opts);
  const lp = warm(ctx, out, 2200);
  const parts = [["sine", 1, 0.07], ["triangle", 1, 0.025]];
  return latestEnd(
    note(ctx, lp, { f: HZ.D4 * jit.pitch, t: t0, decay: 0.5, parts, attack: 0.012 }),
    note(ctx, lp, { f: HZ.Fs4 * jit.pitch, t: t0 + jit.at(0.012), decay: 0.5, parts, attack: 0.012 }),
    note(ctx, lp, { f: HZ.A4 * jit.pitch, t: t0 + jit.at(0.024), decay: 0.5, parts, attack: 0.012 }),
  ) - t0;
}

// the gate of 18:00 is half an hour away: the same soft note twice, like a clock that clears its throat
function gate(ctx, out, t0, opts) {
  const jit = jitter(opts);
  const lp = warm(ctx, out, 1800);
  const parts = [["triangle", 1, 0.11], ["sine", 1, 0.05]];
  return latestEnd(
    note(ctx, lp, { f: HZ.Fs4 * jit.pitch, t: t0, decay: 0.22, parts, attack: 0.015 }),
    note(ctx, lp, { f: HZ.Fs4 * jit.pitch, t: t0 + jit.at(0.26), decay: 0.22, parts, attack: 0.015 }),
  ) - t0;
}

export const GESTURES = { arm, confirm, sent, register, done, taskadd, snooze, withdraw, stop, resume, theme, lang, run, chord, gate };

// Priority when the engine is over budget (higher wins) and the peak each gesture measures at the default volume (the lab test keeps
// `level` honest): the engine uses it to find the quietest voice. Switches and changes of the engine outrank the small taps.
export const GESTURE_META = {
  arm: { pri: 1, level: 0.058 },
  confirm: { pri: 1, level: 0.073 },
  sent: { pri: 3, level: 0.117 },
  register: { pri: 3, level: 0.101 },
  done: { pri: 3, level: 0.058 },
  taskadd: { pri: 3, level: 0.099 },
  snooze: { pri: 3, level: 0.071 },
  withdraw: { pri: 3, level: 0.116 },
  stop: { pri: 5, level: 0.1 },
  resume: { pri: 5, level: 0.062 },
  theme: { pri: 1, level: 0.094 },
  lang: { pri: 1, level: 0.072 },
  run: { pri: 3, level: 0.102 },
  chord: { pri: 4, level: 0.107 },
  gate: { pri: 3, level: 0.085 },
};
