// Lab "f0-stage": the whole stage (sky, skyline, dial, beam) behind a fake sheet that has the real measures, driven through
// window.__lab by test/stage.lab.browser.test.mjs and by hand for screenshots. ?clock=<hora>&pos=<stops> sets the first frame.
//   __lab.render(pos, hora, reduced?, noBeam?)   draws one frame and returns { kind, ring, docked, sheet, hub }
//   __lab.diff(pos, hora)               renders with and without the light and compares the pixels: how many differ inside the
//                                       sheet's rectangle (A17: none) and right of it
//   __lab.labels()                      the text of the dial as { cls, text, fill, shown, rect } in window px (A18 on the dial)
//   __lab.POOL                          the stage's dark pool (stage.js), so a test can switch it off to prove the measure bites
//   __lab.mask(on)                      the glyphs alone, white on black: the exact place of the ink (A18 on the dial)
//   __lab.halo(on)                      switches the labels' own halo off (false) or on, so a test can prove the measure bites
//   __lab.ground(on)                    clears the fill of that text and hides the rest of the dial's drawing, so a screenshot shows
//                                       only what the glyphs are painted on (sky, pool, light, the labels' halo)
import { createStage, POOL } from "../src/stage/stage.js";
import { mulberry32 } from "../src/stage/skyline.js";
import { t } from "../src/i18n/index.js";

const canvas = document.getElementById("sky");
const sheetEl = document.getElementById("app");
// The pixel test reads the canvas back. A GPU-backed canvas that is read once is moved to the CPU by the browser, and the two
// rasterisers differ by a level in the gradients: the first comparison would then "see" a change across the whole sheet.
// Asking for a CPU-backed context first (createSky gets the same context back) makes every read equal to every other.
canvas.getContext("2d", { alpha: false, willReadFrequently: true });
const dialHost = document.getElementById("dial");
const stage = createStage({ canvas, dialHost, t, getSheetRect: () => sheetEl.getBoundingClientRect(), rand: mulberry32(20261007) });
stage.resize();
window.addEventListener("resize", () => stage.resize());

const summary = (out) => out && ({
  kind: out.beam ? out.beam.kind : null, ring: out.layout.ring, docked: out.layout.docked,
  hub: { x: out.layout.cx, y: out.layout.cy },
  sheet: { left: out.sheet.left, right: out.sheet.right, top: out.sheet.top, bottom: out.sheet.bottom },
});

function diff(pos, hora) {
  const ctx = canvas.getContext("2d");
  const ratio = canvas.width / canvas.clientWidth;
  const take = (r) => ctx.getImageData(Math.round(r.left * ratio), Math.round(r.top * ratio), Math.max(1, Math.round((r.right - r.left) * ratio)), Math.max(1, Math.round((r.bottom - r.top) * ratio))).data;
  const out = stage.render({ pos, hora });
  const { left, top, bottom, right } = out.sheet;                  // a DOMRect: its fields are getters, so it cannot be spread
  const s = { left, top, bottom, right: right - 2 };            // the clip's own edge is antialiased: compare the sheet minus that column
  const rightOf = { left: out.sheet.right + 2, right: canvas.clientWidth, top: 0, bottom: canvas.clientHeight };
  const [a, b] = [take(s), take(rightOf)];
  stage.render({ pos, hora, noBeam: true });
  const [c, d] = [take(s), take(rightOf)];
  const count = (x, y) => { let n = 0; for (let i = 0; i < x.length; i += 4) if (x[i] !== y[i] || x[i + 1] !== y[i + 1] || x[i + 2] !== y[i + 2]) n++; return n; };
  stage.render({ pos, hora });
  return { kind: out.beam ? out.beam.kind : null, insideSheet: count(a, c), rightOfSheet: count(b, d) };
}

const LABELS = ".dial-num, .dial-micro, .dial-time";
const labels = () => [...dialHost.querySelectorAll(LABELS)].map((el) => {
  const r = el.getBoundingClientRect();
  const cs = getComputedStyle(el);
  // `shown`: the dial draws it at this size (it hides the micro labels and the numerals that do not fit, stage/dial.js detailOf).
  return { cls: el.getAttribute("class"), text: el.textContent, fill: cs.fill, shown: cs.display !== "none" && cs.opacity !== "0", rect: { left: r.left, top: r.top, right: r.right, bottom: r.bottom } };
});
// "Ground" = everything the glyphs are painted on: the sky, the pool, the light and the labels' own halo, with the glyphs' fill
// cleared and the dial's other drawing (hand, hatch, ticks) hidden; the hand and the hatch pass behind some labels on purpose.
const ground = (on) => {
  dialHost.classList.toggle("lab-ground", on);
  for (const el of dialHost.querySelectorAll(LABELS)) { if (on) el.style.setProperty("fill", "transparent"); else el.style.removeProperty("fill"); }
};

// The glyphs alone: white text on black, no sky, no halo, no other drawing. Where this is white is where the ink of a label is, whatever
// colour the sky behind it has (a pale sun makes the ink and its ground one colour, so they cannot be told apart in the real frame).
const mask = (on) => document.body.classList.toggle("lab-mask", on);

// The halo of the labels, switched off: the measure of A18 has to be able to fail (test/stage.lab.browser.test.mjs).
const halo = (on) => dialHost.classList.toggle("lab-nohalo", !on);

window.__lab = {
  stage,
  render: (pos, hora, reduced = false, noBeam = false) => summary(stage.render({ pos, hora, reduced, noBeam })),
  diff, labels, ground, mask, halo, POOL,
};
const query = new URLSearchParams(location.search);
const num = (key, fallback) => (query.has(key) && Number.isFinite(Number(query.get(key))) ? Number(query.get(key)) : fallback);
stage.render({ pos: num("pos", 1), hora: num("clock", 9) });
