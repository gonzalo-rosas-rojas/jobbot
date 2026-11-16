// Small building blocks for the recipes. Everything takes a BaseAudioContext, so the same code runs live and in an
// OfflineAudioContext. Nothing here keeps state except a noise buffer cache keyed by context.

import { mulberry32 } from "../core/random.js";

// An exponential ramp to 0 throws, so every release ends here and the node is then set to 0 for good.
export const FLOOR = 0.001;

export const STOP_TAIL_S = 0.002;   // a source is stopped this long after its envelope ends, so the release is never cut
const PITCH_JITTER = 0.005;          // the pitch factor of one sound moves by up to this much either way (0.5 %)
const TIMING_JITTER_S = 0.003;      // and each note's start by up to this much (3 ms)
const NOISE_SEED = 20261007;         // the white noise of every context is the same noise

// A number in (-1, 1) from a generator of [0, 1).
const symmetric = (r) => r() * 2 - 1;

// The latest of several end times: when a sound of many notes is over.
export const latestEnd = (...ends) => Math.max(...ends);

// A warm pair of partials at a given level: [type, ratio, peak] for note() in recipes.js and gestures.js.
export const soft = (k) => [["sine", 1, 0.1 * k], ["triangle", 1, 0.06 * k]];

// The only randomness a recipe has: one pitch factor per sound (+-0.5 %, shared by all its notes so intervals stay
// exact) and a timing nudge per note (+-3 ms). Without opts.seed there is no jitter at all.
export function jitter(opts = {}) {
  const live = opts.seed != null;
  const r = mulberry32(live ? opts.seed : 1);
  const pitch = live ? 1 + symmetric(r) * PITCH_JITTER : 1;
  const at = (t) => (live && t > 0 ? t + symmetric(r) * TIMING_JITTER_S : t);
  return { pitch, at, r };
}

// Linear attack from silence (never a click), then an exponential release that lands on FLOOR at t + attack + decay.
export function env(param, t, peak, attack, decay) {
  const end = t + attack + decay;
  param.setValueAtTime(0, t);
  param.linearRampToValueAtTime(peak, t + attack);
  param.exponentialRampToValueAtTime(FLOOR, end);
  param.setValueAtTime(0, end);
  return end;
}

// When a source ends, cut it and its own nodes loose so nothing lingers in the graph.
export function free(src, ...nodes) {
  src.addEventListener("ended", () => {
    try {
      src.disconnect();
      for (const n of nodes) n.disconnect();
    } catch {
      /* already gone */
    }
  });
}

const WARM_Q = 0.7;   // the resonance of the soft low-pass every warm sound goes through

export function filter(ctx, type, f, q, dest) {
  const node = ctx.createBiquadFilter();
  node.type = type;
  node.frequency.value = f;
  node.Q.value = q;
  node.connect(dest);
  return node;
}

/** The soft low-pass in front of `out` that rounds a warm sound: nothing above `cutoff` Hz gets through. */
export const warm = (ctx, out, cutoff) => filter(ctx, "lowpass", cutoff, WARM_Q, out);

// One enveloped oscillator. Returns the time its envelope ends.
export function tone(ctx, dest, { f, t, peak, decay, attack = 0.006, type = "sine", to }) {
  const o = ctx.createOscillator();
  o.type = type;
  o.frequency.setValueAtTime(f, t);
  if (to) o.frequency.exponentialRampToValueAtTime(to, t + attack + decay);
  const g = ctx.createGain();
  const end = env(g.gain, t, peak, attack, decay);
  o.connect(g).connect(dest);
  o.start(t);
  o.stop(end + STOP_TAIL_S);
  free(o, g);
  return end;
}

const noises = new WeakMap();
// One second of seeded white noise per context; bursts read different slices of it.
export function noise(ctx) {
  let buf = noises.get(ctx);
  if (!buf) {
    buf = ctx.createBuffer(1, ctx.sampleRate, ctx.sampleRate);
    const d = buf.getChannelData(0);
    const r = mulberry32(NOISE_SEED);
    for (let i = 0; i < d.length; i++) d[i] = symmetric(r);
    noises.set(ctx, buf);
  }
  return buf;
}

/** A note is a stack of partials sharing pitch and envelope: [type, ratio, peak]. `to` glides the pitch. Returns where it ends. */
export function note(ctx, dest, { f, t, decay, parts, attack, to }) {
  return latestEnd(...parts.map(([type, ratio, peak]) => tone(ctx, dest, { f: f * ratio, t, peak, decay, attack, type, to: to ? to * ratio : undefined })));
}

// A short filtered-noise burst; `to` sweeps the filter frequency. `at` picks the slice of the noise buffer.
export function burst(ctx, dest, { t, dur, f, to, q = 1, peak, type = "bandpass", attack = 0.002, at = 0 }) {
  const src = ctx.createBufferSource();
  src.buffer = noise(ctx);
  const flt = ctx.createBiquadFilter();
  flt.type = type;
  flt.Q.value = q;
  flt.frequency.setValueAtTime(f, t);
  if (to) flt.frequency.exponentialRampToValueAtTime(to, t + dur);
  const g = ctx.createGain();
  const end = env(g.gain, t, peak, attack, dur - attack);
  src.connect(flt).connect(g).connect(dest);
  src.start(t, at, dur + STOP_TAIL_S);
  free(src, flt, g);
  return end;
}
