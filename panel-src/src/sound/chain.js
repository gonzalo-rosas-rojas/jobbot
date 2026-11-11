// The one master chain, shared by the live engine and the offline renders, so the tests measure what the ears get:
//   recipes -> master gain (volume) -> compressor (a safety limiter, not an effect) -> ceiling -> destination
//
// The ceiling is the one addition to the specified chain. Measured in Chrome, the compressor alone (-18 dB, 8:1,
// 12 dB knee) adds about 2.2x make-up gain and still lets a full-scale input out at 0.62; with volume 0.6 and
// three overlapping voices that breaks the 0.5 rule. The ceiling is a WaveShaper that is the identity up to 0.3
// and tops out below 0.5, so "peak <= 0.5 at the destination" holds for any combination of sounds, not by luck.

const CEILING = 0.5;
const KNEE = 0.3;

function ceilingCurve() {
  const n = 2049;
  const c = new Float32Array(n);
  const room = CEILING - KNEE;
  for (let i = 0; i < n; i++) {
    const x = Math.abs(-1 + (2 * i) / (n - 1));
    c[i] = Math.sign(-1 + (2 * i) / (n - 1)) * (x <= KNEE ? x : KNEE + room * Math.tanh((x - KNEE) / room));
  }
  return c;
}

export function buildChain(ctx, volume) {
  const master = ctx.createGain();
  master.gain.value = volume;
  const comp = ctx.createDynamicsCompressor();
  comp.threshold.value = -18;
  comp.knee.value = 12;
  comp.ratio.value = 8;
  comp.attack.value = 0.003;
  comp.release.value = 0.12;
  const ceiling = ctx.createWaveShaper();
  ceiling.curve = ceilingCurve();
  ceiling.oversample = "none";
  master.connect(comp);
  comp.connect(ceiling);
  ceiling.connect(ctx.destination);
  return { master, comp, ceiling };
}
