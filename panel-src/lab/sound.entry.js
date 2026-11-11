// Sound lab: audition every recipe, watch the live analyser, try the engine's rules, and measure each recipe in an
// OfflineAudioContext through the same master chain. Labs are exempt from i18n, so the labels are English.
//
// window.__sound     the engine (src/sound/index.js)
// window.__renderAll({ master, seed })   renders every recipe offline and returns the measurements (see measure())
// window.__renderCues({ master, seed })  the same for every cue of the vocabulary (src/sound/cues.js), at the level of its bus
// window.__renderPileup(master, everything, copies)   renders many sounds at once through the chain
// window.__ready     true when the page is built and the offline renders are done

import { sound } from "../src/sound/index.js";
import { RECIPES, ROOM_IDS } from "../src/sound/recipes.js";
import { GESTURES } from "../src/sound/gestures.js";
import { CUES, BUSES } from "../src/sound/cues.js";
import { buildChain } from "../src/sound/chain.js";

const ALL = { ...RECIPES, ...GESTURES };

const SR = 44100;
const T0 = 0.02;
// Chrome's compressor looks ahead 6 ms, so the chain delays every sound by that much; the tail is measured after it.
const LATENCY = 0.006;

// ---- what to render: every recipe, toggle both ways, arrive once per room (top floor first) ----
const VARIANTS = [
  ...ROOM_IDS.map((room) => ({ id: `arrive:${room}`, name: "arrive", opts: { room } })),
  ...sound.names
    .filter((n) => n !== "arrive" && n !== "toggle")
    .map((n) => ({ id: n, name: n, opts: {} })),
  { id: "toggle", name: "toggle", opts: { on: true } },
  { id: "toggle:off", name: "toggle", opts: { on: false } },
];

// ---- what the vocabulary plays: every cue at the level of its bus, plus the other direction of the ones that have two ----
const levelOf = (cue) => cue.gain ?? BUSES[cue.bus].gain;
const CUE_VARIANTS = [
  ...CUES.map((cue) => ({ id: `cue:${cue.id}`, cue, name: cue.recipe, opts: cue.opts || {}, gain: levelOf(cue) })),
  ...CUES.filter((cue) => cue.alt).map((cue) => ({ id: `cue:${cue.id}:alt`, cue, name: cue.recipe, opts: { ...cue.opts, ...cue.alt }, gain: levelOf(cue) })),
];

// ---- measurement ----
function fft(re, im) {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) {
      [re[i], re[j]] = [re[j], re[i]];
      [im[i], im[j]] = [im[j], im[i]];
    }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const ang = (-2 * Math.PI) / len;
    const wr = Math.cos(ang);
    const wi = Math.sin(ang);
    for (let i = 0; i < n; i += len) {
      let cr = 1;
      let ci = 0;
      for (let k = 0; k < len / 2; k++) {
        const a = i + k;
        const b = a + len / 2;
        const tr = re[b] * cr - im[b] * ci;
        const ti = re[b] * ci + im[b] * cr;
        re[b] = re[a] - tr;
        im[b] = im[a] - ti;
        re[a] += tr;
        im[a] += ti;
        [cr, ci] = [cr * wr - ci * wi, cr * wi + ci * wr];
      }
    }
  }
}

// Hann-windowed magnitude spectrum of d[start, start+len), zero-padded to `size`.
function magnitudes(d, start, len, size) {
  const re = new Float64Array(size);
  const im = new Float64Array(size);
  const n = Math.min(len, d.length - start);
  for (let i = 0; i < n; i++) re[i] = d[start + i] * (0.5 - 0.5 * Math.cos((2 * Math.PI * i) / (n - 1)));
  fft(re, im);
  const m = new Float64Array(size / 2);
  for (let i = 0; i < m.length; i++) m[i] = Math.hypot(re[i], im[i]);
  return m;
}

// fundamentalHz: the strongest spectral peak between 60 Hz and 3 kHz (Hann window, 16384-point FFT, parabolic
// interpolation). For the tonal recipes the strongest partial is the fundamental, which the tests also check
// against the nominal tuning; for the noise clicks it is only the dominant band.
function dominantHz(d, start, len) {
  const size = 16384;
  const m = magnitudes(d, start, len, size);
  const bin = SR / size;
  let best = Math.ceil(60 / bin);
  for (let i = best; i < 3000 / bin; i++) if (m[i] > m[best]) best = i;
  const [a, b, c] = [m[best - 1], m[best], m[best + 1]];
  const shift = a - 2 * b + c === 0 ? 0 : (0.5 * (a - c)) / (a - 2 * b + c);
  return (best + shift) * bin;
}

// Onsets: the envelope is the loudest |sample| of the last 8 ms (no ripple for tones down to 60 Hz, unlike an RMS
// over a few ms). An onset is a frame within 26 dB of the loudest that is 6 dB above the quietest of the previous
// 15 ms, at least 25 ms after the last one. Returns frame times in ms.
function onsetsOf(d) {
  const hop = Math.round(SR / 1000);
  const win = 8 * hop;
  const db = [];
  let ref = 0;
  const env = [];
  for (let end = 0; end < d.length; end += hop) {
    let m = 0;
    for (let i = Math.max(0, end - win + 1); i <= end; i++) m = Math.max(m, Math.abs(d[i]));
    env.push(m);
    ref = Math.max(ref, m);
  }
  for (const m of env) db.push(20 * Math.log10(Math.max(m, 1e-9) / ref));
  const out = [];
  let lastK = -1e9;
  for (let k = 0; k < db.length; k++) {
    if (db[k] < -26 || k - lastK < 25) continue;
    let low = -120;
    if (k > 0) low = Math.min(...db.slice(Math.max(0, k - 15), k));
    if (db[k] - low >= 6) {
      out.push(k);
      lastK = k;
    }
  }
  return out;
}

// Spectral centroid (amplitude-weighted, 20 Hz to 12 kHz) and the share of power above 4 kHz and below 100 Hz.
function spectralShape(d) {
  const size = 65536;
  const m = magnitudes(d, 0, d.length, size);
  const bin = SR / size;
  let num = 0;
  let den = 0;
  let hi = 0;
  let lo = 0;
  let all = 0;
  for (let i = Math.ceil(20 / bin); i < 20000 / bin; i++) {
    const f = i * bin;
    all += m[i] * m[i];
    if (f > 4000) hi += m[i] * m[i];
    if (f < 100) lo += m[i] * m[i];
    if (f <= 12000) {
      num += f * m[i];
      den += m[i];
    }
  }
  return { centroidHz: den ? num / den : 0, hiFrac: all ? hi / all : 0, loFrac: all ? lo / all : 0 };
}

const plots = {};

function columns(d, upto, n) {
  const out = [];
  const step = Math.max(1, Math.floor(upto / n));
  for (let c = 0; c < n; c++) {
    let lo = 0;
    let hi = 0;
    for (let i = c * step; i < Math.min(d.length, (c + 1) * step); i++) {
      lo = Math.min(lo, d[i]);
      hi = Math.max(hi, d[i]);
    }
    out.push([lo, hi]);
  }
  return out;
}

async function measure(variant, master, seed) {
  const oc = new OfflineAudioContext(1, Math.round(SR * 1.2), SR);
  const chain = buildChain(oc, master);
  const bus = oc.createGain();
  bus.gain.value = variant.gain ?? 1;
  bus.connect(chain.master);
  const duration = ALL[variant.name](oc, bus, T0, { ...variant.opts, seed });
  const d = (await oc.startRendering()).getChannelData(0);

  let peak = 0;
  let sum = 0;
  let nan = 0;
  let lastAudible = 0;
  for (let i = 0; i < d.length; i++) {
    if (!Number.isFinite(d[i])) {
      nan++;
      continue;
    }
    const a = Math.abs(d[i]);
    peak = Math.max(peak, a);
    if (a > 0.002) lastAudible = i;
  }
  const span = Math.round(duration * SR);
  for (let i = Math.round(T0 * SR); i < Math.round(T0 * SR) + span; i++) sum += d[i] * d[i];
  const from = Math.round((T0 + duration + LATENCY) * SR);
  let tailPeak = 0;
  for (let i = from; i < Math.min(d.length, from + Math.round(0.1 * SR)); i++) tailPeak = Math.max(tailPeak, Math.abs(d[i]));

  // the first millisecond of the sound and the last sample of the buffer (a click is a jump from or to silence), and the steepest step against the peak
  const head = Math.round((T0 + LATENCY) * SR);
  let headPeak = 0;
  for (let i = Math.max(0, head - 20); i < head + Math.round(0.001 * SR); i++) headPeak = Math.max(headPeak, Math.abs(d[i]));
  let maxStep = 0;
  for (let i = 1; i < d.length; i++) maxStep = Math.max(maxStep, Math.abs(d[i] - d[i - 1]));

  const starts = onsetsOf(d);
  const notesHz = starts.map((ms, i) => {
    const room = i + 1 < starts.length ? starts[i + 1] - ms : 80;
    return +dominantHz(d, Math.round((ms / 1000) * SR), Math.round((Math.min(80, room) / 1000) * SR)).toFixed(1);
  });
  const shape = spectralShape(d);
  plots[variant.id] = columns(d, Math.round((T0 + duration + 0.04) * SR), 240);
  return {
    name: variant.id,
    duration,
    peak,
    rms: Math.sqrt(sum / Math.max(1, span)),
    tailPeak,
    nan,
    fundamentalHz: notesHz[0] ?? 0,
    onsetsMs: starts,
    notesHz,
    centroidHz: shape.centroidHz,
    hiFrac: shape.hiFrac,
    loFrac: shape.loFrac,
    lastAudibleMs: (lastAudible / SR - T0) * 1000,
    headPeak,
    lastSample: Math.abs(d[d.length - 1]),
    stepRel: peak ? maxStep / peak : 0,
  };
}

async function renderAll({ master = 0.25, seed = 7 } = {}) {
  const out = [];
  for (const v of VARIANTS) out.push(await measure(v, master, seed));
  return out;
}

async function renderCues({ master = 0.25, seed = 7 } = {}) {
  const out = [];
  for (const v of CUE_VARIANTS) out.push({ ...(await measure(v, master, seed)), bus: v.cue.bus, cue: v.cue.id });
  return out;
}

// Many sounds at once through the chain (the engine never does this, which is why the chain is tested for it): six
// of the loudest, or with `everything` every recipe, `copies` times over.
async function renderPileup(master = 0.6, everything = false, copies = 1) {
  const oc = new OfflineAudioContext(1, Math.round(SR * 1.2), SR);
  const chain = buildChain(oc, master);
  const bus = oc.createGain();
  bus.connect(chain.master);
  const picks = everything ? VARIANTS : ["ceremony", "error", "approve", "turn", "late", "undo"].map((name) => ({ name, opts: {} }));
  for (let c = 0; c < copies; c++) for (const v of picks) RECIPES[v.name](oc, bus, T0, { ...v.opts, seed: 3 + c });
  const d = (await oc.startRendering()).getChannelData(0);
  return d.reduce((p, x) => Math.max(p, Math.abs(x)), 0);
}

// ---- page ----
function h(tag, props = {}, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (k === "class") el.className = v;
    else if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
    else el.setAttribute(k, v);
  }
  el.append(...kids);
  return el;
}

const css = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim() || "#888";
const root = document.getElementById("lab");
const logBox = h("pre", { id: "log", "aria-live": "off" });
const logLines = [];
const stamp = () => (performance.now() / 1000).toFixed(3).padStart(8);

function log(line) {
  logLines.unshift(`${stamp()}  ${line}`);
  logLines.length = Math.min(logLines.length, 14);
  logBox.textContent = logLines.join("\n");
}

let analyser = null;
const refs = { enable: null, volume: null, volumeText: null, enforce: null };

function hookAnalyser() {
  if (analyser || !sound.ctx) return;
  analyser = sound.ctx.createAnalyser();
  analyser.fftSize = 2048;
  analyser.smoothingTimeConstant = 0.55;
  sound.tap(analyser);
}

function syncEnable() {
  refs.enable.textContent = sound.enabled ? "Sound is ON (click to turn off)" : "Enable sound (needs a click)";
  refs.enable.setAttribute("aria-pressed", String(sound.enabled));
  hookAnalyser();
}

function audition(v) {
  const enforce = refs.enforce.checked;
  const call = enforce ? sound.play : sound.preview;
  const ok = call(v.name, v.gain != null ? { ...v.opts, gain: v.gain } : v.opts);
  log(`${enforce ? "play   " : "preview"} ${v.id.padEnd(16)} -> ${ok}`);
}

function card(v) {
  const meta = h("div", { class: "lab-meta", "data-meta": v.id }, h("div", {}, "rendering..."), h("div", {}, " "));
  const plot = h("canvas", { width: "420", height: "78", "data-plot": v.id });
  return h("div", { class: "lab-card" }, h("button", { "data-play": v.id, onclick: () => audition(v) }, v.id), plot, meta);
}

function paintCard(r) {
  const meta = root.querySelector(`[data-meta="${r.name}"]`);
  const plot = root.querySelector(`[data-plot="${r.name}"]`);
  if (!meta || !plot) return;
  meta.replaceChildren(
    h("div", {}, `${Math.round(r.duration * 1000)} ms, peak ${r.peak.toFixed(3)}`),
    h("div", {}, `${Math.round(r.fundamentalHz)} Hz, ${r.onsetsMs.length} onset${r.onsetsMs.length === 1 ? "" : "s"}`),
  );
  const g = plot.getContext("2d");
  g.clearRect(0, 0, plot.width, plot.height);
  const mid = plot.height / 2;
  const cols = plots[r.name];
  const k = (mid * 0.9) / Math.max(r.peak, 1e-6); // shapes are normalised to each sound's own peak; the numbers are real
  g.fillStyle = css("--track");
  cols.forEach(([lo, hi], i) => {
    const x = (i * plot.width) / cols.length;
    g.fillRect(x, mid - hi * k, Math.max(1, plot.width / cols.length - 0.5), Math.max(1, (hi - lo) * k));
  });
  g.fillStyle = css("--line");
  g.fillRect(0, mid, plot.width, 1);
  // the gold line is where the recipe says it ends
  g.fillStyle = css("--gold");
  g.fillRect(((T0 + r.duration) / (T0 + r.duration + 0.04)) * plot.width, 0, 1, plot.height);
}

// live scope: a rolling strip of the loudest sample per frame (about 10 s), and a log-frequency spectrum with hold
const strip = new Float32Array(600);
const hold = new Float32Array(1024).fill(-120);
let tdata = null;
let fdata = null;

function drawLive(scope, spec) {
  requestAnimationFrame(() => drawLive(scope, spec));
  hookAnalyser();
  for (const c of [scope, spec]) {
    if (c.width !== c.clientWidth && c.clientWidth) c.width = c.clientWidth; // crisp text at any window width
  }
  const a = scope.getContext("2d");
  const b = spec.getContext("2d");
  a.clearRect(0, 0, scope.width, scope.height);
  b.clearRect(0, 0, spec.width, spec.height);
  const mid = scope.height / 2;
  a.fillStyle = css("--line");
  for (const level of [0.5, 0.25]) {
    a.fillRect(0, mid - level * mid * 2, scope.width, 1);
    a.fillRect(0, mid + level * mid * 2, scope.width, 1);
  }
  a.fillStyle = css("--muted");
  a.font = "11px system-ui";
  a.fillText("last ~10 s, loudest sample per frame; 0.5 is the ceiling nothing passes", 8, 14);
  b.fillStyle = css("--muted");
  b.font = "11px system-ui";
  b.fillText("spectrum 50 Hz to 12 kHz (log), peak hold", 8, 14);
  if (!analyser) return;
  tdata = tdata || new Float32Array(analyser.fftSize);
  fdata = fdata || new Float32Array(analyser.frequencyBinCount);
  analyser.getFloatTimeDomainData(tdata);
  analyser.getFloatFrequencyData(fdata);
  strip.copyWithin(0, 1);
  strip[strip.length - 1] = tdata.reduce((p, x) => Math.max(p, Math.abs(x)), 0);
  a.fillStyle = css("--track");
  for (let i = 0; i < strip.length; i++) {
    const hgt = Math.min(0.5, strip[i]) * mid * 2;
    a.fillRect((i * scope.width) / strip.length, mid - hgt, scope.width / strip.length + 0.5, Math.max(0.5, hgt * 2));
  }
  const bin = sound.ctx.sampleRate / analyser.fftSize;
  const lo = Math.log(50);
  const hi = Math.log(12000);
  for (let x = 0; x < spec.width; x += 3) {
    const f0 = Math.exp(lo + ((hi - lo) * x) / spec.width);
    const f1 = Math.exp(lo + ((hi - lo) * (x + 3)) / spec.width);
    let db = -140;
    for (let i = Math.floor(f0 / bin); i <= Math.max(Math.floor(f0 / bin), Math.floor(f1 / bin)); i++) db = Math.max(db, fdata[i]);
    const k = x / 3;
    hold[k] = Math.max(db, hold[k] - 0.4);
    const y = (v) => spec.height - Math.max(0, Math.min(1, (v + 95) / 80)) * (spec.height - 20); // -95 dB to -15 dB
    b.fillStyle = css("--track-soft");
    b.fillRect(x, y(hold[k]), 2, spec.height);
    b.fillStyle = css("--track");
    b.fillRect(x, y(db), 2, spec.height);
  }
}

function tour() {
  ROOM_IDS.forEach((room, i) =>
    setTimeout(() => {
      const ok = sound.play("arrive", { room });
      log(`play    arrive:${room.padEnd(9)} -> ${ok}`);
    }, i * 600),
  );
}

// seven sounds in 200 ms: the budget keeps two, and more important newcomers replace the weakest
function stress() {
  const names = ["tick", "open", "close", "undo", "toggle", "approve", "error"];
  const results = [];
  names.forEach((name, i) =>
    setTimeout(() => {
      results.push([name, sound.play(name)]);
      if (i === names.length - 1) {
        const kept = sound.recent().filter((v) => !v.cut).map((v) => v.key);
        log(`stress: accepted ${results.filter((r) => r[1]).map((r) => r[0]).join(", ") || "none"}; still audible: ${kept.join(", ") || "none"}`);
      }
    }, i * 33),
  );
}

function buildPage() {
  const grid = (title, items) => [h("h2", {}, title), h("div", { class: "lab-grid" }, ...items.map(card))];
  refs.enable = h("button", { class: "lab-primary", id: "enable", onclick: () => { sound.toggle(); syncEnable(); log(`sound ${sound.enabled ? "on" : "off"} (context ${sound.ctx ? sound.ctx.state : "none"})`); } });
  refs.volumeText = h("span", { class: "lab-num" }, sound.volume.toFixed(2));
  refs.volume = h("input", { id: "volume", type: "range", min: "0", max: "0.6", step: "0.01", value: String(sound.volume), "aria-label": "Master volume", oninput: (e) => { refs.volumeText.textContent = sound.setVolume(e.target.value).toFixed(2); } });
  refs.enforce = h("input", { id: "enforce", type: "checkbox" });
  const busy = h("input", { id: "busy", type: "checkbox", onchange: (e) => sound.setBusy(e.target.checked) });
  const hide = h("input", { id: "hide", type: "checkbox", onchange: (e) => Object.defineProperty(document, "hidden", { configurable: true, get: () => e.target.checked }) });
  const scope = h("canvas", { id: "scope", width: "540", height: "150" });
  const spec = h("canvas", { id: "spectrum", width: "540", height: "150" });
  const actions = VARIANTS.filter((v) => v.name !== "arrive");
  root.append(
    h("h1", {}, "Sound lab"),
    h("p", { class: "lab-note" }, "Every sound is synthesised with the Web Audio API (no files). Nothing plays until you enable sound with a click. The cards show each recipe rendered offline through the same master chain at volume 0.25: the numbers are real, the waveforms are normalised to each sound's own peak, and the gold line is where the recipe says it ends."),
    h("div", { class: "lab-bar" }, refs.enable, h("label", {}, "Volume", refs.volume, refs.volumeText), h("label", {}, refs.enforce, "obey the engine's rules (budget, duplicates, unsolicited)")),
    h("div", { class: "lab-bar" }, h("button", { id: "tour", onclick: tour }, "Play the arrival tour (top to bottom)"), h("button", { id: "stress", onclick: stress }, "Stress: 7 sounds in 200 ms"), h("label", {}, busy, "owner is busy (drops turn, late)"), h("label", {}, hide, "simulate a hidden tab")),
    h("div", { class: "lab-scopes" }, scope, spec),
    ...grid("Arrival motifs: pitch rises with the floor", VARIANTS.filter((v) => v.name === "arrive")),
    ...grid("Action sounds", actions),
    ...grid("The vocabulary (cues.js): every cue at the level of its bus, one card each", CUE_VARIANTS),
    h("h2", {}, "Log"),
    logBox,
  );
  drawLive(scope, spec);
  syncEnable();
}

window.__sound = sound;
window.__renderAll = renderAll;
window.__renderCues = renderCues;
window.__renderPileup = renderPileup;
window.__ready = false;

buildPage();
renderAll()
  .then((all) => all.forEach(paintCard))
  .then(() => renderCues())
  .then((all) => {
    all.forEach(paintCard);
    window.__ready = true;
  });
