// A18 on the HUD and the bottom bar, measured on what Chrome paints (review M3). The older test of ui.frame.test.mjs measures the tokens
// against the nominal colours of the sky at its top edge; the real ground is the sky 30 to 50 px lower, the beam of the docked sun and,
// at night, the windows of the skyline behind the bar. For every run of text the lab (f0-frame) gives its colour and its box; the ground is a
// capture with the text made transparent, so it is exactly what the text is painted on, veil included.
//
// The ground of a run is not one colour. The verdict is the contrast of the text against the 90th percentile of the ground's luminance: nine
// pixels in ten behind the letters are at least that dark, so a star or a lit window (one or two pixels) does not decide it, but a patch of sun
// behind a chip does.
import test from "node:test";
import assert from "node:assert/strict";
import { openBuilt } from "./helpers/browser.mjs";
import { decodePng } from "./helpers/png.mjs";
import { contrast, luminance } from "./helpers/color.mjs";

const open = (opts = {}) => openBuilt({ args: ["lab", "f0-frame"], file: "dist/lab-f0-frame.html", ...opts });
const ev = (t, expr) => t.page.eval(expr);
const SNAP = `{ controls: { stopped: false, window_open: false, next_window: "2026-10-07 18:00", last_run_at: "2026-10-07T05:00:00Z", engine: "recent" } }`;
const COUNTS = `{ procesos: 74, cola: 2, aprender: 12, sistema: "recent" }`;
const NEEDS = Number(process.env.CONTRAST_NEEDS) || 4.5;

async function grab(t, x, y, width, height) {
  const r = await t.page.send("Page.captureScreenshot", { format: "png", clip: { x, y, width, height, scale: 1 } });
  return decodePng(Buffer.from(r.data, "base64"));
}

/** For each run of text on the screen: { text, color, ratio }, the ratio against the 90th percentile of the luminance of the pixels behind it. */
async function measure(t, vw, vh) {
  const runs = await ev(t, `window.__lab.texts()`);
  await ev(t, `window.__lab.notext(true)`);
  const strips = { top: await grab(t, 0, 0, vw, 96), bottom: await grab(t, 0, vh - 64, vw, 64) };
  await ev(t, `window.__lab.notext(false)`);
  return runs.map((run) => {
    const strip = run.rect.top > vh / 2 ? strips.bottom : strips.top;
    const oy = run.rect.top > vh / 2 ? vh - 64 : 0;
    const x0 = Math.max(0, Math.floor(run.rect.left) - 1), x1 = Math.min(vw - 1, Math.ceil(run.rect.right) + 1);
    const y0 = Math.max(0, Math.floor(run.rect.top - oy) - 1), y1 = Math.min(strip.h - 1, Math.ceil(run.rect.bottom - oy) + 1);
    const lum = [];
    for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) lum.push(luminance(strip.at(x, y)));
    lum.sort((a, b) => a - b);
    const ground = lum[Math.min(lum.length - 1, Math.floor(lum.length * 0.9))];
    const color = run.color.match(/[\d.]+/g).slice(0, 3).map(Number);
    const lt = luminance(color);
    return { text: run.text, color: run.color, ratio: (Math.max(lt, ground) + 0.05) / (Math.min(lt, ground) + 0.05) };
  });
}

const HOURS = [0, 3, 6, 8, 9, 10, 12, 13, 15, 17, 18, 19, 21, 23];
const STOP_POSES = ["prologo", "hoy", "procesos"];       // the dial is big in the first two and docked under the HUD in the rest (all four docked stops look alike)

for (const [width, height] of [[1920, 950], [1366, 768]]) {
  test(`A18 on the HUD and the bar at ${width}x${height}: every run of text keeps ${NEEDS}:1 over what is painted behind it, at ${HOURS.length} hours and three poses`, async () => {
    const t = await open({ width, height });
    try {
      await ev(t, `window.__lab.counts(${COUNTS}); window.__lab.status(${SNAP})`);
      const worst = [];
      for (const id of STOP_POSES) {
        for (const hora of HOURS) {
          await ev(t, `window.__lab.go(${JSON.stringify(id)}, ${hora})`);
          const rows = await measure(t, width, height);
          assert.ok(rows.length >= 8, `the lab shows the runs of text (${rows.length})`);
          if (width >= 1440) assert.ok(rows.some((r) => r.text === "apagado"), "the sound's indicator (off, the dimmest ink) is among the runs; from 1280 to 1439 px it is the icon alone (ui/sound.css)");
          for (const r of rows) if (r.ratio < NEEDS) worst.push(`${id} ${hora}:00 "${r.text}" ${r.color} ${r.ratio.toFixed(2)}:1`);
        }
      }
      assert.deepEqual(worst, [], `${worst.length} runs of text under ${NEEDS}:1`);
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}
