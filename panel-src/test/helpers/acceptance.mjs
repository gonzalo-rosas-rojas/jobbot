// What the acceptance matrix (test/acceptance.*.browser.test.mjs, spec A1 to A19) needs on top of test/helpers/page.mjs: keys with their codes and
// modifiers, a frame recorder for the trips, the words of a missing value, every request the page made, and the measure of what a text is
// painted on (a capture of the mounted page with the glyphs made transparent) so that contrast is read on pixels, never on tokens.
// Everything here runs against the MOUNTED page (the demo build, or the real build answered by the test), never against a lab.
import { openPage, wheel, trace } from "./page.mjs";
import { decodePng } from "./png.mjs";
import { contrast, luminance } from "./color.mjs";

export { openPage, wheel, trace, decodePng, contrast, luminance };

export const STOPS = ["prologo", "hoy", "procesos", "cola", "aprender", "sistema"];
export const DIGIT = { prologo: "0", hoy: "1", procesos: "2", cola: "3", aprender: "4", sistema: "5" };
/** The five windows of the spec (A2, A13, A14) and the laptop that matters (1366x768). */
export const SIZES = { fhd: [1920, 1080], uwqhd: [3440, 1440], laptop: [1440, 900], small: [1366, 768], phone: [390, 844] };
/** The words of a missing value that no page may show (A2). `⟦` is how i18n marks a key it does not know. */
export const BAD_WORDS = /(?:^|[^\p{L}\p{N}_])(null|undefined|NaN)(?![\p{L}\p{N}_])|\[object|⟦/u;

import { readFileSync } from "node:fs";
export const ev = (t, expr) => t.page.eval(expr);
/** One of the snapshots the tests of the page share (test/fixtures/snapshot.<name>.json: rich, heavy, empty), as a fresh object. */
export const fixture = (name) => JSON.parse(readFileSync(new URL(`./../fixtures/snapshot.${name}.json`, import.meta.url), "utf8"));
/** The `answer` of a "real" page (the demo's server cut out): /api/snapshot gets `snapshot`, whatever else is a 404. */
export const answerWith = (snapshot, calls = []) => async (p) => { calls.push(p); return p.startsWith("/api/snapshot") ? { status: 200, body: snapshot } : undefined; };
/** Runs a function IN the page (it must not close over anything): run(t, (a, b) => a + b, 1, 2). */
export const run = (t, fn, ...args) => t.page.eval(`(${fn.toString()})(...${JSON.stringify(args)})`);

/** A real key: `code` is what tinykeys reads for a letter or a digit; `modifiers` is the CDP bit set (1 Alt, 2 Ctrl, 4 Meta, 8 Shift). */
export async function press(t, k, { modifiers = 0, code, text } = {}) {
  const c = code || (/^\d$/.test(k) ? "Digit" + k : k.length === 1 ? "Key" + k.toUpperCase() : k);
  const txt = text !== undefined ? text : k.length === 1 && !(modifiers & 6) ? k : k === "Enter" ? "\r" : undefined;
  await t.page.send("Input.dispatchKeyEvent", { type: txt ? "keyDown" : "rawKeyDown", key: k, code: c, text: txt, modifiers });
  await t.page.send("Input.dispatchKeyEvent", { type: "keyUp", key: k, code: c, modifiers });
}
export const pressHelp = (t) => press(t, "?", { code: "Slash", modifiers: 8 });
export const pressCtrlK = (t) => press(t, "k", { modifiers: 2 });

/** Waits until the page says it is at `id`: the moment mounted, the address written, no trip in the air and the sheet solid. Needs ?debug=1. */
export const arrived = (t, id, timeout = 6000) => t.page.waitFor(`(() => { const a = document.getElementById("app"); return a.dataset.moment === ${JSON.stringify(id)} && location.hash.startsWith("#${id}") && !__app.engine?.S.jump && a.style.opacity === ""; })()`, { timeout });
/** Presses the digit of `id` and waits for the arrival. */
export async function travel(t, id, timeout) { await press(t, DIGIT[id]); await arrived(t, id, timeout); }

// ---- trips: the frames of the page, from the press of a key to the arrival -------------------------------------------------------------

const RECORDER = `window.__trip = { key: 0, first: 0, last: 0, arrived: 0, long: 0, frames: 0, prev: 0, h0: "", dts: [], ys: [] };
  addEventListener("keydown", () => { const r = __trip; r.key = performance.now(); r.first = r.last = r.arrived = r.long = r.frames = r.prev = 0; r.h0 = location.hash; r.dts = []; r.ys = []; }, true);
  (function f(now) { const r = __trip, j = window.__app.engine.S.jump;
    if (r.key) { if (r.prev) { r.dts.push(now - r.prev); if (now - r.prev > 33.5) r.long++; } r.prev = now; r.frames++; r.ys.push([now, scrollY]);
      if (j && !r.first) r.first = now; if (r.first && !j && !r.last) r.last = now; if (!r.arrived && location.hash !== r.h0) r.arrived = now; }
    requestAnimationFrame(f); })(performance.now());`;
/** Arms a recorder in the page (needs ?debug=1). It restarts at every keydown. */
export const armTrips = (t) => ev(t, RECORDER);
/** What the recorder saw since the last keydown: `jump` ms of the tween (first frame with a jump to the frame it ended), `arrival` ms key to address, `long` frames over 33 ms. */
export const readTrip = (t) => ev(t, `(() => { const r = __trip; return { jump: r.last - r.first, arrival: r.arrived - r.key, long: r.long, frames: r.frames, dts: r.dts.slice(), ys: r.ys.slice(), key: r.key, first: r.first, last: r.last }; })()`);
export const jumpMs = (floors) => Math.min(800, 420 + 120 * (Math.max(1, floors) - 1));

/**
 * The frames of a trip read by `readTrip`, split as A7 needs them. `inside` are the lengths of the frames that END after the first frame of the tween and BEFORE the one
 * that closes it (what a person watching the tween sees: `long` of them, over 33 ms, are the stutters; the swap of the sheet halfway is one of them by design). The
 * closing frame is where the arrival happens (the address, the focus, the live region and the deferred work of the moment): `closing` is its length, and a long
 * one stretches the tween itself, which the duration test sees. `worst` is the longest wait of the whole trip from the press of the key: the lead (key to the first
 * frame, where a moment that mounts inside the key handler shows up), every frame and the closing one.
 */
export function tripFrames(r) {
  const inside = [];
  const all = r.ys.length ? [r.ys[0][0] - r.key] : [];
  let closing = 0;
  for (let i = 0; i < r.dts.length; i++) {
    const at = r.ys[i + 1][0];
    if (r.last && at > r.last) break;
    all.push(r.dts[i]);
    if (r.last && at === r.last) closing = r.dts[i];
    else if (r.first && at > r.first) inside.push(r.dts[i]);
  }
  return { inside, closing, long: inside.filter((d) => d > 33.5).length, worst: Math.max(0, ...all), lead: all.length ? all[0] : 0 };
}

export const pct = (list, p) => { const s = [...list].sort((a, b) => a - b); return s.length ? s[Math.min(s.length - 1, Math.floor(s.length * p))] : NaN; };

// ---- what a page shows, in words ----------------------------------------------------------------------------------------------------------

/** Every word a person can read at the stop on screen: the text of the page and what its elements say about themselves (labels, titles, alt). */
export const readable = (t) => run(t, () => {
  const attrs = [...document.querySelectorAll("[aria-label], [title], [alt], [placeholder], [aria-description]")].flatMap((el) => ["aria-label", "title", "alt", "placeholder", "aria-description"].map((a) => el.getAttribute(a)).filter(Boolean));
  const live = document.getElementById("live");
  return document.body.innerText + "\n" + attrs.join("\n") + "\n" + (live ? live.textContent : "");
});

/** { text, hit } for the first bad word of what is readable, or null. */
export async function badWordIn(t) {
  const text = await readable(t);
  const m = BAD_WORDS.exec(text);
  return m ? m[0].trim() + " | " + text.slice(Math.max(0, m.index - 40), m.index + 40).replace(/\s+/g, " ") : null;
}

/** Every address the page asked for that is not its own: the page and its fonts under /a/<launch id>/, /api/ and blob: icons. */
export const strangers = (t) => t.requests.filter((u) => !(new RegExp(`^http://127\\.0\\.0\\.1:${t.port}/(dist/|a/${t.launchId}/|api/|favicon\\.ico)`).test(u) || u.startsWith("blob:")));

// ---- contrast on pixels ---------------------------------------------------------------------------------------------------------------------

/** The text of the page (HUD, stops bar, sheet, bottom bar), each run with the parts of it that are painted, its colour, and a switch that makes the glyphs invisible. */
export const MEASURE_PAGE = `(() => {
  const roots = ["#hud", "#app", "#bar", "#detail.open"].map((s) => document.querySelector(s)).filter(Boolean);
  const clips = (el) => { const out = []; for (let e = el; e && e !== document.body; e = e.parentElement) { const cs = getComputedStyle(e); if (cs.overflowX !== "visible" || cs.overflowY !== "visible") out.push(e.getBoundingClientRect()); } return out; };
  const shown = (el) => { for (let e = el; e && e !== document.body; e = e.parentElement) { const cs = getComputedStyle(e); if (cs.visibility === "hidden" || cs.display === "none" || parseFloat(cs.opacity) < 0.5) return false; } return true; };
  const all = roots.flatMap((r) => [r, ...r.querySelectorAll("*")]);
  window.__ghost = (on) => { for (const el of all) { if (on) { el.style.setProperty("color", "transparent", "important"); el.style.setProperty("text-shadow", "none", "important"); el.style.setProperty("border-color", "transparent", "important"); el.style.setProperty("background-image", "none", "important"); el.style.setProperty("caret-color", "transparent", "important"); } else { for (const p of ["color", "text-shadow", "border-color", "background-image", "caret-color"]) el.style.removeProperty(p); } } };
  return all.flatMap((el) => [...el.childNodes].filter((n) => n.nodeType === 3 && n.textContent.trim() && shown(el)).map((n) => {
    const range = document.createRange(); range.selectNodeContents(n); const boxes = clips(el);
    const rects = [...range.getClientRects()].map((r) => boxes.reduce((a, b) => ({ left: Math.max(a.left, b.left), top: Math.max(a.top, b.top), right: Math.min(a.right, b.right), bottom: Math.min(a.bottom, b.bottom) }), { left: r.left, top: r.top, right: r.right, bottom: r.bottom })).filter((r) => r.right - r.left > 1 && r.bottom - r.top > 1 && r.right > 0 && r.bottom > 0);
    const fs = parseFloat(getComputedStyle(el).fontSize);
    let op = 1; for (let e = el; e && e !== document.body; e = e.parentElement) op *= parseFloat(getComputedStyle(e).opacity);
    return { op, part: el.closest("#hud") ? "hud" : el.closest("#bar") ? "bar" : el.closest("#detail") ? "drawer" : "sheet", cls: String(el.className.baseVal ?? el.className), text: n.textContent.trim().slice(0, 40), color: getComputedStyle(el).color, fs, rects };
  })).filter((b) => b.rects.length > 0 && !b.cls.includes("demo-badge"));
})()`;

/** The window as pixels, or the part of it at `clip` (window coordinates: CDP wants them in the document's, so the scroll is added here). */
export async function capture(t, clip) {
  let at = clip;
  if (clip) { const s = await ev(t, `({ x: scrollX, y: scrollY })`); at = { ...clip, x: clip.x + s.x, y: clip.y + s.y }; }
  const r = await t.page.send("Page.captureScreenshot", { format: "png", ...(at ? { clip: { ...at, scale: 1 } } : {}) });
  return decodePng(Buffer.from(r.data, "base64"));
}

const lumOf = (px) => luminance(px);
/** { rgb: [0..255 x 3], a } of a computed colour: "rgb(...)", "rgba(...)" or "color(srgb r g b / a)" (what color-mix() computes to). */
export function parseColor(css) {
  const n = (css.match(/[\d.]+/g) || []).map(Number);
  if (css.startsWith("color(")) return { rgb: n.slice(0, 3).map((v) => v * 255), a: n.length > 3 ? n[3] : 1 };
  return { rgb: n.slice(0, 3), a: n.length > 3 ? n[3] : 1 };
}
const rgbOf = (css) => parseColor(css).rgb;
/** `fg` painted over `bg` at `alpha`, as [r, g, b]. */
const mix = (fg, bg, alpha) => fg.map((v, i) => v * alpha + bg[i] * (1 - alpha));

/**
 * The contrast of every run of text of the mounted page against what it is painted on. The ground is the same window captured with every glyph transparent, so it
 * is exactly what the letters sit on (glass, sky, light, skyline, the badge behind a key). The verdict uses the worst 2 % tail of the ground: a star or one lit window
 * behind a letter does not decide it, a patch of sun does. Light text is judged against the lightest ground, dark text against the darkest.
 * Returns [{ part, cls, text, ratio, fs }].
 */
export async function pageContrast(t) {
  const boxes = await ev(t, MEASURE_PAGE);
  await ev(t, `window.__ghost(true)`);
  let ground;
  try { ground = await capture(t); } finally { await ev(t, `window.__ghost(false)`); }
  const out = [];
  for (const box of boxes) {
    const parsed = parseColor(box.color), alpha = parsed.a * (box.op ?? 1), color = parsed.rgb;
    const lights = [];
    for (const r of box.rects) {
      // the pixels wholly inside the box of the text, no more: the gold rule under the active stop and the bar of a new row touch the box and are not what the letters sit on
      const x0 = Math.max(0, Math.ceil(r.left)), x1 = Math.min(ground.w - 1, Math.floor(r.right) - 1);
      const y0 = Math.max(0, Math.ceil(r.top)), y1 = Math.min(ground.h - 1, Math.floor(r.bottom) - 1);
      for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) lights.push(ground.at(x, y));
    }
    if (lights.length <= 40) continue;
    lights.sort((a, b) => lumOf(a) - lumOf(b));
    const textLum = luminance(color);
    const median = lumOf(lights[Math.floor(lights.length / 2)]);
    const worst = textLum >= median ? lights[Math.floor(lights.length * 0.98)] : lights[Math.floor(lights.length * 0.02)];
    out.push({ part: box.part, cls: box.cls, text: box.text, fs: box.fs, ratio: contrast(mix(color, worst, alpha), worst) });
  }
  return out;
}

/**
 * The labels of the dial against what they are painted on, read from three captures of the dial's box: with the real fill (A), with the fill cleared but the halo kept
 * (D: the ground, halo included, which is part of the design) and with a white fill and a black one (W, K: where the ink is, whatever the real colours are). A pixel is
 * ink where white and black differ by more than half; a pixel is "touching" where it is not ink but one of its eight neighbours is. The verdict of a label is the contrast
 * of its fill against the touching pixels of D, leaving out the worst 3 % (the eye of an 8 is a hole no halo reaches).
 * Returns [{ text, cls, ink, touching, min, size }] for the labels that are drawn (display and visibility on).
 */
export async function dialContrast(t) {
  const labels = await run(t, () => {
    const out = [];
    for (const el of document.querySelectorAll("#dial text")) {
      const cs = getComputedStyle(el), r = el.getBoundingClientRect();
      if (cs.display === "none" || cs.visibility === "hidden" || parseFloat(cs.opacity) < 0.5 || r.width < 1 || r.height < 1) continue;
      out.push({ text: el.textContent.trim(), cls: el.getAttribute("class") || "", fill: cs.fill, rect: { left: r.left, top: r.top, right: r.right, bottom: r.bottom }, size: r.height });
    }
    return out;
  });
  if (!labels.length) return [];
  const box = { x: Math.max(0, Math.floor(Math.min(...labels.map((l) => l.rect.left)) - 6)), y: Math.max(0, Math.floor(Math.min(...labels.map((l) => l.rect.top)) - 6)) };
  box.w = Math.ceil(Math.max(...labels.map((l) => l.rect.right)) + 6 - box.x);
  box.h = Math.ceil(Math.max(...labels.map((l) => l.rect.bottom)) + 6 - box.y);
  const paint = (fill, stroke) => ev(t, `(() => { for (const el of document.querySelectorAll("#dial text")) { el.style.setProperty("fill", ${JSON.stringify(fill)}, "important"); ${stroke ? `el.style.setProperty("stroke", ${JSON.stringify(stroke)}, "important");` : ""} } })()`);
  const clear = () => ev(t, `(() => { for (const el of document.querySelectorAll("#dial text")) { el.style.removeProperty("fill"); el.style.removeProperty("stroke"); } for (const el of document.querySelectorAll("#dial svg *")) el.style.removeProperty("visibility"); })()`);
  // the hand, the ring and the hatch pass behind some labels on purpose and are not part of what a label is painted on
  const only = () => ev(t, `(() => { for (const el of document.querySelectorAll("#dial svg :is(path, line, circle, rect, ellipse, polygon, polyline, use, image)")) if (!el.closest("defs")) el.style.setProperty("visibility", "hidden", "important"); })()`);
  const clip = { x: box.x, y: box.y, width: box.w, height: box.h };
  let D, W, K;
  try {
    await only();
    await paint("transparent");            // the ground: the sky, the light, the pool and the halo of each label, with no ink
    D = await capture(t, clip);
    await paint("#ffffff", "transparent"); // the ink alone: white and black glyphs differ by 255 wherever a glyph covers a pixel, whatever is behind
    W = await capture(t, clip);
    await paint("#000000", "transparent");
    K = await capture(t, clip);
  } finally {
    await clear();
  }
  return labels.map((l) => {
    const fill = rgbOf(l.fill);
    const x0 = Math.max(1, Math.floor(l.rect.left - box.x) - 3), x1 = Math.min(D.w - 2, Math.ceil(l.rect.right - box.x) + 3);
    const y0 = Math.max(1, Math.floor(l.rect.top - box.y) - 3), y1 = Math.min(D.h - 2, Math.ceil(l.rect.bottom - box.y) + 3);
    const ink = (x, y) => (W.at(x, y)[0] - K.at(x, y)[0]) / 255;
    let n = 0;
    const ring = [];
    for (let y = y0 + 1; y < y1; y++) for (let x = x0 + 1; x < x1; x++) {
      if (ink(x, y) >= 0.5) { n++; continue; }
      if (ink(x, y) >= 0.08) continue;
      let near = false;
      for (let dy = -1; dy <= 1 && !near; dy++) for (let dx = -1; dx <= 1; dx++) if (ink(x + dx, y + dy) >= 0.5) { near = true; break; }
      if (near) ring.push(contrast(fill, D.at(x, y)));
    }
    ring.sort((a, b) => a - b);
    const min = ring.length ? ring[Math.min(ring.length - 1, Math.max(1, Math.floor(ring.length * 0.03)))] : Infinity;
    return { text: l.text, cls: l.cls, ink: n, touching: ring.length, min, size: l.size };
  });
}

// ---- the tour of A2 (used by acceptance.tour.* and acceptance.sizes.*) ----
export const TOUR = ["prologo", "hoy", "procesos", "cola", "aprender", "sistema", "prologo"];
export const OPENS = `!!document.querySelector("#app .sheet-host h1") && document.fonts.status === "loaded" && !!window.__app.model()`;                // the snapshot is in: the moment under the sheet is data, not a skeleton

/** One walk: the page at `query`, then 0 1 2 3 4 5 0 with the keys. Returns the sentences of what was wrong (empty: nothing). */
export async function walk(t, query, label) {
  const wrong = [];
  const before = t.page.problems().length;
  await t.page.goto(`${t.url.split("?")[0]}${query}`);
  await t.page.waitFor(OPENS, { timeout: 12000 });
  for (const id of TOUR) {
    await travel(t, id, 8000);
    const v = await run(t, () => ({ h1: document.querySelector("#app .sheet-host h1")?.textContent.trim() || "", hscroll: document.documentElement.scrollWidth - innerWidth, sheet: document.getElementById("app").getBoundingClientRect().toJSON(), vw: innerWidth, vh: innerHeight, inert: document.getElementById("app").inert, opacity: document.getElementById("app").style.opacity }));
    if (!v.h1) wrong.push(`${label} ${id}: the sheet has no heading`);
    if (v.hscroll > 0) wrong.push(`${label} ${id}: ${v.hscroll}px of sideways scroll`);
    if (v.sheet.right > v.vw + 0.5 || v.sheet.left < -0.5) wrong.push(`${label} ${id}: the sheet leaves the window (${Math.round(v.sheet.left)}..${Math.round(v.sheet.right)} of ${v.vw})`);
    if (v.inert || v.opacity !== "") wrong.push(`${label} ${id}: the sheet is not solid on arrival (inert ${v.inert}, opacity "${v.opacity}")`);
    const bad = await badWordIn(t);
    if (bad) wrong.push(`${label} ${id}: "${bad}"`);
  }
  const fresh = t.page.problems().slice(before).map((p) => `${label}: ${p.kind} ${p.text.slice(0, 160)}`);
  return [...wrong, ...fresh];
}


// ---- the host ----------------------------------------------------------------------------------------------------------------------------

/**
 * How long a fixed piece of work takes in the page, in ms (the best of five). A trip, a frame and a p95 are measured in wall time, so they depend on what else the
 * machine is doing: the timing tests print this number next to their verdict, and a verdict taken with a number far above the quiet one (about 40 ms on the
 * machine these tests were written on, with the CPU at 35 % from other programs) is a verdict on the machine as much as on the page.
 */
export async function hostSpeed(t) {
  let best = Infinity;
  for (let i = 0; i < 5; i++) best = Math.min(best, await ev(t, `(() => { const t0 = performance.now(); let x = 0; for (let i = 0; i < 2e7; i++) x += Math.sqrt(i); return performance.now() - t0 + x * 0; })()`));
  return Math.round(best);
}
