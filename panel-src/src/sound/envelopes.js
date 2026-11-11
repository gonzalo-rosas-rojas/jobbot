// Small building blocks for the recipes. Everything takes a BaseAudioContext, so the same code runs live and in an
// OfflineAudioContext. Nothing here keeps state except a noise buffer cache keyed by context.

// An exponential ramp to 0 throws, so every release ends here and the node is then set to 0 for good.
export const FLOOR = 0.001;

// mulberry32: tiny seeded generator, so tests are deterministic and repeats can still differ.
function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// The only randomness a recipe has: one pitch factor per sound (+-0.5 %, shared by all its notes so intervals stay
// exact) and a timing nudge per note (+-3 ms). Without opts.seed there is no jitter at all.
export function jitter(opts = {}) {
  const live = opts.seed != null;
  const r = rng(live ? opts.seed : 1);
  const pitch = live ? 1 + (r() * 2 - 1) * 0.005 : 1;
  const at = (t) => (live && t > 0 ? t + (r() * 2 - 1) * 0.003 : t);
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

export function filter(ctx, type, f, q, dest) {
  const node = ctx.createBiquadFilter();
  node.type = type;
  node.frequency.value = f;
  node.Q.value = q;
  node.connect(dest);
  return node;
}

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
  o.stop(end + 0.002);
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
    const r = rng(20261007);
    for (let i = 0; i < d.length; i++) d[i] = r() * 2 - 1;
    noises.set(ctx, buf);
  }
  return buf;
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
  src.start(t, at, dur + 0.002);
  free(src, flt, g);
  return end;
}
