import test from "node:test";
import assert from "node:assert/strict";
import { openBuilt } from "./helpers/browser.mjs";
import { decodePng } from "./helpers/png.mjs";
import { contrast } from "./helpers/color.mjs";

const open = (opts = {}) => openBuilt({ args: ["lab", "f0-stage"], file: "dist/lab-f0-stage.html", ...opts });
const ev = (t, expr) => t.page.eval(expr);

for (const [width, height, ring] of [[1920, 1080, 455], [3440, 1440, 629]]) {
  test(`A13 + A17 at ${width}x${height}: the dial rings ${ring} px at HOY and the light never paints a pixel of the sheet, at 09:00, 00:26 and 18:00`, async () => {
    const t = await open({ width, height });
    try {
      for (const [hora, kind] of [[9, "sun"], [0.43, "window"], [18, "horizon"]]) {
        const out = await ev(t, `window.__lab.render(1, ${hora})`);
        assert.equal(out.kind, kind, `hora ${hora}`);
        assert.ok(Math.abs(out.ring - ring) <= 3, `ring ${out.ring}`);
        assert.ok(Math.abs(out.sheet.right - (width === 1920 ? 1177 : 1464)) <= 2, `the sheet's right edge is ${out.sheet.right}`);
        const d = await ev(t, `window.__lab.diff(1, ${hora})`);
        assert.equal(d.insideSheet, 0, `${d.insideSheet} pixels of the sheet changed with the light on (${hora})`);
        assert.ok(d.rightOfSheet > 2000, `the light is really painted right of the sheet (${d.rightOfSheet} pixels at ${hora})`);
      }
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}

test("the stage keeps the same light when the dial docks, and a window of 1100 px has no beam but still draws", async () => {
  const t = await open();
  try {
    const hoy = await ev(t, `window.__lab.render(1, 0.43)`);
    const cola = await ev(t, `window.__lab.render(3, 0.43)`);
    const sistema = await ev(t, `window.__lab.render(5, 0.43)`);
    assert.equal(hoy.kind, "window");
    assert.equal(cola.kind, "window");
    assert.equal(cola.docked, true);
    assert.equal(sistema.kind, "window");
    assert.equal(sistema.docked, false, "SISTEMA keeps the hero dial (relay2): the same window light, from the same city");
  } finally { await t.close(); }
  const narrow = await open({ width: 1100, height: 700 });
  try {
    const out = await ev(narrow, `window.__lab.render(1, 9)`);
    assert.equal(out.kind, null);
    assert.deepEqual(narrow.page.problems(), []);
  } finally { await narrow.close(); }
});

test("the sun is born at the hub of the dial, and the light comes in over the sheet's edge instead of being cut at full strength", async () => {
  const t = await open();
  try {
    const out = await ev(t, `window.__lab.render(1, 9)`);
    const hub = await ev(t, `(() => { const r = document.querySelector(".dial .hub").getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`);
    assert.ok(Math.abs(out.hub.x - hub.x) < 1.5 && Math.abs(out.hub.y - hub.y) < 1.5, `the sun at ${out.hub.x},${out.hub.y}, the dial's hub at ${hub.x},${hub.y}`);
    // The light on the row where the beam reaches the sheet (the sun's frame minus the same frame without the light): just past the
    // sheet's edge it is a trace, a little further in it is the beam, so it comes in over the edge and is not cut at full strength.
    // the sun is low (the dial of HOY is sunk) and its light lies nearly flat: it meets the sheet's edge 30 % of the way from the sun's height to 52 % of the window's
    const row = Math.round(out.hub.y + (1080 * 0.52 - out.hub.y) * 0.3);
    const withLight = await shot(t);
    await ev(t, `window.__lab.render(1, 9, false, true)`);
    const without = await shot(t);
    const light = (x) => { const [a, b] = [withLight.at(x, row), without.at(x, row)]; return a[0] + a[1] + a[2] - b[0] - b[1] - b[2]; };
    const edge = Math.round(out.sheet.right) + 3, inner = Math.round(out.sheet.right) + 70;
    assert.ok(light(inner) > 60, `the beam is lit 70 px past the sheet (${light(inner)})`);
    assert.ok(light(edge) < 0.35 * light(inner), `the light is ${light(edge)} 3 px past the sheet and ${light(inner)} 70 px past it`);
  } finally { await t.close(); }
});

// ---- A18 on the dial: the labels measured on the pixels Chrome really paints ----

async function shot(t) {
  const r = await t.page.send("Page.captureScreenshot", { format: "png" });
  return decodePng(Buffer.from(r.data, "base64"));
}

/**
 * For each text of the dial: the contrast of its fill against the pixels that touch its glyphs. M is the glyphs alone, white on black
 * (lab: mask), so it says exactly where the ink is, whatever the sky behind is; D is the same frame with the glyphs' fill cleared and
 * the dial's other drawing hidden (lab: ground), so D is what the glyphs are painted on: the sky, the pool, the light and the labels'
 * own halo. A pixel is ink when M is lit by 50 % or more; a pixel is "touching" when M leaves it dark (under 8 %) and one of its
 * eight neighbours is ink. The hand and the hatch of the dial pass behind some labels on purpose and are not part of the ground.
 */
async function labelContrast(t, hora) {
  await ev(t, `window.__lab.render(1, ${hora})`);
  const labels = await ev(t, `window.__lab.labels()`);
  await ev(t, `window.__lab.ground(true)`);
  const D = await shot(t);
  await ev(t, `window.__lab.ground(false); window.__lab.mask(true)`);
  const M = await shot(t);
  await ev(t, `window.__lab.mask(false)`);
  return labels.map((l) => {
    const fill = l.fill.match(/\d+/g).slice(0, 3).map(Number);
    const x0 = Math.max(1, Math.floor(l.rect.left) - 3), x1 = Math.min(M.w - 2, Math.ceil(l.rect.right) + 3);
    const y0 = Math.max(1, Math.floor(l.rect.top) - 3), y1 = Math.min(M.h - 2, Math.ceil(l.rect.bottom) + 3);
    const w = x1 - x0 + 1;
    const lit = (x, y) => M.at(x, y)[0] / 255;
    let ink = 0;
    const ring = [];
    for (let y = y0 + 1; y < y1; y++) for (let x = x0 + 1; x < x1; x++) {
      if (lit(x, y) >= 0.5) { ink++; continue; }
      if (lit(x, y) >= 0.08) continue;
      let near = false;
      for (let dy = -1; dy <= 1 && !near; dy++) for (let dx = -1; dx <= 1; dx++) if (lit(x + dx, y + dy) >= 0.5) { near = true; break; }
      if (near) ring.push(contrast(fill, D.at(x, y)));
    }
    // The worst pixel of the ring, leaving out the worst 3 % (at least one): the eye of an 8 at 13 px is a hole one pixel wide that no halo
    // reaches, and one pixel of it is not a label that cannot be read; a patch of the sun on the ink would be many pixels and still counts.
    ring.sort((a, b) => a - b);
    const min = ring.length ? ring[Math.min(ring.length - 1, Math.max(1, Math.floor(ring.length * 0.03)))] : Infinity;
    return { text: l.text, cls: l.cls, shown: l.shown, ink, touching: ring.length, min };
  });
}

// The windows a browser really gives (a 1080p monitor has about 950 px of window, a laptop 768 or less) and the hours when the sun sits
// behind the digits (review A2 and M4: at 1366x768 the aureole of the sun touched the time at 13:30, 1.1:1).
for (const [width, height] of [[1366, 657], [1366, 768], [1440, 900], [1536, 730], [1920, 950], [1920, 1080], [3440, 1440]]) {
  test(`A18 on the dial at ${width}x${height}: every label it draws keeps 4.5:1 over the day sky at 09:00, 11:00, 12:00, 13:30 and 17:00`, async () => {
    const t = await open({ width, height });
    try {
      for (const hora of [9, 11, 12, 13.5, 17]) {
        const rows = (await labelContrast(t, hora)).filter((r) => r.shown);
        assert.ok(rows.length >= 5, `at least the four cardinal numerals and the time are drawn (${rows.length})`);
        if (width >= 1920 && height >= 1000) assert.equal(rows.length, 11, "eight numerals, the zone, the busy label and the time");
        for (const r of rows) {
          assert.ok(r.ink > 15 && r.touching > 15, `the measure saw the glyphs of "${r.text}" (${r.ink} glyph pixels, ${r.touching} around them) at ${hora}`);
          assert.ok(r.min >= 4.5, `"${r.text}" (${r.cls}) is ${r.min.toFixed(2)}:1 at ${hora}:00, ${width}x${height}`);
        }
      }
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}

test("the measure is not vacuous: without the pool and without the labels' halo the time and the labels fall under 4.5:1 on the sky of 09:00", async () => {
  const t = await open();
  try {
    await ev(t, `window.__lab.POOL.alpha = 0; window.__lab.halo(false)`);
    const rows = (await labelContrast(t, 9)).filter((r) => r.shown);
    const low = rows.filter((r) => r.min < 4.5).map((r) => `${r.text} ${r.min.toFixed(1)}`);
    assert.ok(low.length >= 3 && low.some((l) => l.startsWith("09:00")), `only ${low.length} labels fall under 4.5:1 without the pool: ${low}`);
  } finally { await t.close(); }
});
