// The palette: one pure function per sound, (ctx, out, t0, opts) -> seconds. Each builds nodes on any
// BaseAudioContext, connects them to `out`, schedules from `t0` and returns how long the sound lasts. No globals,
// no randomness except the seeded jitter of envelopes.js (opts.seed).
//
// One family: sine and triangle partials with a touch of filtered noise, D major pentatonic (D E F# A B), short
// round envelopes, nothing sustained above ~4 kHz. Peaks here are before the master gain (0.25 by default) and
// the compressor, which adds about 2.2x; they are kept at or under 0.3 so that even volume 0.6 stays below 0.5.
// Room motifs: the pitch of the first note falls with the stop's place in the bar (hoy D5, procesos B4, cola F#4, aprender D4, sistema
// D3). The brief's table started aprender on A4, above cola's F#4, which would break that order, so it is a fourth
// lower here (D4 -> A3); the interval, the register logic and the decay are the table's.

import { burst, filter, jitter, tone, free } from "./envelopes.js";

export const ROOM_IDS = Object.freeze(["hoy", "procesos", "cola", "aprender", "sistema"]);

const HZ = { D3: 146.83, Fs3: 185, A3: 220, D4: 293.66, F4: 349.23, Fs4: 369.99, A4: 440, B4: 493.88, D5: 587.33, A5: 880 };
// a warm pair of partials at a given level
const soft = (k) => [["sine", 1, 0.1 * k], ["triangle", 1, 0.06 * k]];

const last = (...ends) => Math.max(...ends);

// A note is a stack of partials sharing pitch and envelope: [type, ratio, peak]. Returns where it ends.
function note(ctx, dest, f, t, decay, parts, attack) {
  return last(...parts.map(([type, ratio, peak]) => tone(ctx, dest, { f: f * ratio, t, peak, decay, attack, type })));
}

const MOTIFS = {
  // the first stop: rising fifth, airy, with a breath of high noise (-30 dB) under it
  hoy(ctx, out, t0, j) {
    const parts = [["sine", 1, 0.16], ["sine", 2, 0.025]];
    return last(
      note(ctx, out, HZ.D5 * j.pitch, t0, 0.2, parts),
      note(ctx, out, HZ.A5 * j.pitch, t0 + j.at(0.09), 0.29, parts),
      burst(ctx, out, { t: t0, dur: 0.12, type: "highpass", f: 5000, peak: 0.005, at: j.r() * 0.8 }),
    );
  },
  // the corridor: the same note twice, an even pulse
  procesos(ctx, out, t0, j) {
    const parts = [["triangle", 1, 0.175]];
    return last(
      note(ctx, out, HZ.B4 * j.pitch, t0, 0.12, parts),
      note(ctx, out, HZ.B4 * j.pitch, t0 + j.at(0.08), 0.12, parts),
    );
  },
  // the gate: a low-to-high step with a tiny dry latch under the second note
  cola(ctx, out, t0, j) {
    const t2 = t0 + j.at(0.085);
    return last(
      note(ctx, out, HZ.Fs4 * j.pitch, t0, 0.16, soft(1.25)),
      note(ctx, out, HZ.A4 * j.pitch, t2, 0.2, soft(1.25)),
      burst(ctx, out, { t: t2, dur: 0.003, f: 3000, q: 3, peak: 0.2, attack: 0.001, at: j.r() * 0.8 }),
    );
  },
  // the archive: a falling fourth, warm and slow
  aprender(ctx, out, t0, j) {
    const lp = filter(ctx, "lowpass", 1200, 0.7, out);
    const parts = [["sine", 1, 0.16], ["triangle", 1, 0.05]];
    return last(
      note(ctx, lp, HZ.D4 * j.pitch, t0, 0.2, parts),
      note(ctx, lp, HZ.A3 * j.pitch, t0 + j.at(0.12), 0.26, parts),
    );
  },
  // the machine room: one low note over a sub (half its frequency), with a 6 Hz hum
  sistema(ctx, out, t0, j) {
    const lp = filter(ctx, "lowpass", 900, 0.7, out);
    const wob = ctx.createGain();
    wob.gain.value = 0.7;
    wob.connect(lp);
    const end = note(ctx, wob, HZ.D3 * j.pitch, t0, 0.3, [["sine", 1, 0.21], ["sine", 0.5, 0.035], ["sine", 2, 0.085], ["sine", 3, 0.03]]);
    const lfo = ctx.createOscillator();
    lfo.frequency.value = 6;
    const depth = ctx.createGain();
    depth.gain.value = 0.3;
    lfo.connect(depth).connect(wob.gain);
    lfo.start(t0);
    lfo.stop(end + 0.005);
    free(lfo, depth, wob);
    return end;
  },
};

function arrive(ctx, out, t0, opts) {
  const room = opts.room;
  if (!Object.hasOwn(MOTIFS, room)) throw new Error(`arrive needs a room, got ${room}`);
  // the most frequent cue of all: about 3 dB under the rare ones (error, approve, ceremony), which must stand out
  const soften = ctx.createGain();
  soften.gain.value = 0.7;
  soften.connect(out);
  return MOTIFS[room](ctx, soften, t0, jitter(opts)) - t0;
}

// 12 ms dry click for the keyboard cursor; constant, never pitched by position
function tick(ctx, out, t0, opts) {
  const j = jitter(opts);
  return burst(ctx, out, { t: t0, dur: 0.012, f: 2500, q: 3, peak: 0.9, attack: 0.001, at: j.r() * 0.8 }) - t0;
}

// the gate latch opens: a warm rising major third over a soft low thump
function approve(ctx, out, t0, opts) {
  const j = jitter(opts);
  return last(
    tone(ctx, out, { f: 90 * j.pitch, t: t0, peak: 0.18, decay: 0.074 }),
    note(ctx, out, HZ.D4 * j.pitch, t0, 0.11, soft(1.8)),
    note(ctx, out, HZ.Fs4 * j.pitch, t0 + j.at(0.07), 0.116, soft(1.8)),
  ) - t0;
}

// a falling semitone, soft
function undo(ctx, out, t0, opts) {
  const j = jitter(opts);
  const parts = [["sine", 1, 0.14], ["triangle", 1, 0.042]];
  return last(
    note(ctx, out, HZ.Fs4 * j.pitch, t0, 0.05, parts),
    note(ctx, out, HZ.F4 * j.pitch, t0 + j.at(0.055), 0.075, parts),
  ) - t0;
}

// a drawer or the palette: noise sweeps up to open, down to close
function sweep(from, to, peakOf) {
  return (ctx, out, t0, opts) => {
    const j = jitter(opts);
    return burst(ctx, out, { t: t0, dur: 0.09, f: from, to, q: 1.4, peak: peakOf, attack: 0.02, at: j.r() * 0.8 }) - t0;
  };
}

// a switch: two close pitches, up for on, down for off
function toggle(ctx, out, t0, opts) {
  const j = jitter(opts);
  const [a, b] = opts.on === false ? [HZ.B4, HZ.A4] : [HZ.A4, HZ.B4];
  const parts = [["sine", 1, 0.117], ["triangle", 1, 0.052]];
  return last(
    note(ctx, out, a * j.pitch, t0, 0.034, parts, 0.004),
    note(ctx, out, b * j.pitch, t0 + j.at(0.04), 0.034, parts, 0.004),
  ) - t0;
}

// something is waiting for you: one soft FM bell (carrier 880, modulator x3.5, index fading), once
function turn(ctx, out, t0, opts) {
  const j = jitter(opts);
  const f = 880 * j.pitch;
  const lp = filter(ctx, "lowpass", 3500, 0.7, out);
  const carrier = ctx.createOscillator();
  const mod = ctx.createOscillator();
  const dev = ctx.createGain();
  const amp = ctx.createGain();
  carrier.frequency.value = f;
  mod.frequency.value = f * 3.5;
  dev.gain.setValueAtTime(f * 3.5 * 0.6, t0);
  dev.gain.exponentialRampToValueAtTime(f * 3.5 * 0.04, t0 + 0.14);
  mod.connect(dev).connect(carrier.frequency);
  amp.gain.setValueAtTime(0, t0);
  amp.gain.linearRampToValueAtTime(0.28, t0 + 0.004);
  amp.gain.exponentialRampToValueAtTime(0.001, t0 + 0.194);
  amp.gain.setValueAtTime(0, t0 + 0.194);
  carrier.connect(amp).connect(lp);
  for (const o of [carrier, mod]) {
    o.start(t0);
    o.stop(t0 + 0.196);
  }
  free(carrier, amp, lp);
  free(mod, dev);
  return 0.196;
}

// overdue: a low, dull double pulse that falls a third (a different shape and pitch from the machine room's note)
function late(ctx, out, t0, opts) {
  const j = jitter(opts);
  const lp = filter(ctx, "lowpass", 800, 0.7, out);
  const parts = [["sine", 1, 0.12], ["sine", 2, 0.07], ["sine", 3, 0.03]];
  return last(
    note(ctx, lp, HZ.A3 * j.pitch, t0, 0.052, parts, 0.008),
    note(ctx, lp, HZ.Fs3 * j.pitch, t0 + j.at(0.11), 0.052, parts, 0.008),
  ) - t0;
}

// an action failed: a short, low, rounded buzz
function error(ctx, out, t0, opts) {
  const j = jitter(opts);
  const lp = filter(ctx, "lowpass", 800, 0.7, out);
  const o = ctx.createOscillator();
  o.type = "sawtooth";
  o.frequency.value = 110 * j.pitch;
  const g = ctx.createGain();
  g.gain.setValueAtTime(0, t0);
  g.gain.linearRampToValueAtTime(0.26, t0 + 0.012);
  g.gain.setValueAtTime(0.26, t0 + 0.07);
  g.gain.exponentialRampToValueAtTime(0.001, t0 + 0.12);
  g.gain.setValueAtTime(0, t0 + 0.12);
  o.connect(g).connect(lp);
  o.start(t0);
  o.stop(t0 + 0.122);
  free(o, g, lp);
  return 0.12;
}

// a real milestone (first approval, interview, offer): D major arpeggio, 900 ms with a soft tail
function ceremony(ctx, out, t0, opts) {
  const j = jitter(opts);
  const parts = [["sine", 1, 0.145], ["triangle", 1, 0.065]];
  return last(
    ...[[HZ.D4, 0, 0.55], [HZ.Fs4, 0.15, 0.55], [HZ.A4, 0.3, 0.55], [HZ.D5, 0.45, 0.42]].map(([f, at, decay]) =>
      note(ctx, out, f * j.pitch, t0 + j.at(at), decay, parts, 0.008),
    ),
  ) - t0;
}

export const RECIPES = {
  arrive, tick, approve, undo,
  open: sweep(400, 1800, 0.75),
  close: sweep(1800, 400, 0.5),
  toggle, turn, late, error, ceremony,
};

// Priority when over budget (higher wins): error > ceremony > approve > turn > late > arrive > the rest.
// `level` is the peak each recipe measures at the default volume (the test keeps these honest); the engine uses it
// to find the quietest voice when more than six are sounding.
export const META = {
  error: { pri: 7, level: 0.144 },
  ceremony: { pri: 6, level: 0.143 },
  approve: { pri: 5, level: 0.147 },
  turn: { pri: 4, level: 0.085 },
  late: { pri: 3, level: 0.095 },
  arrive: { pri: 2, level: 0.07 },
  tick: { pri: 1, level: 0.05 },
  undo: { pri: 1, level: 0.085 },
  open: { pri: 1, level: 0.06 },
  close: { pri: 1, level: 0.065 },
  toggle: { pri: 1, level: 0.07 },
};
