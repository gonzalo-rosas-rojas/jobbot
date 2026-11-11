import test from "node:test";
import assert from "node:assert/strict";
import { openBuilt } from "./helpers/browser.mjs";
import { decodePng } from "./helpers/png.mjs";
import { contrast } from "./helpers/color.mjs";

const open = (opts = {}) => openBuilt({ args: ["lab", "f0-moments"], file: "dist/lab-f0-moments.html", ...opts });
const ev = (t, expr) => t.page.eval(expr);
// What each moment opens with in Spanish, whatever the fixture: the title says what the stop says (an empty install says its own thing).
const NAMES = {
  prologo: /^anoche/i, hoy: /^(te tocan|nada pendiente)/i, procesos: /^procesos/i, cola: /^(por aprobar|nada por aprobar)/i,
  aprender: /^(no supo|nada que repasar)/i, sistema: /^(última corrida|sin corridas|todavía)/i,
};

test("each of the six moments mounts on each fixture in each language, says its name and leaves nothing behind when it goes", async () => {
  const t = await open();
  try {
    for (const lang of ["es", "en"]) {
      await ev(t, `window.__lab.lang(${JSON.stringify(lang)})`);
      for (const fixture of ["empty", "rich", "heavy"]) {
        for (const id of ["prologo", "hoy", "procesos", "cola", "aprender", "sistema"]) {
          await ev(t, `window.__lab.show(${JSON.stringify(id)}, ${JSON.stringify(fixture)}, 9)`);
          const v = await ev(t, `({ h1: document.querySelector("#host h1").textContent.trim(), moment: document.getElementById("app").dataset.moment, kids: document.getElementById("host").children.length, text: document.getElementById("host").textContent })`);
          assert.equal(v.moment, id);
          assert.equal(v.kids, 1, `${id}: one section in the host`);
          assert.ok(!v.text.includes("⟦") && !/NaN|undefined/.test(v.text), `${lang}/${fixture}/${id}: ${v.text.slice(0, 120)}`);
          if (lang === "es") assert.match(v.h1, NAMES[id], `${fixture}/${id}: ${v.h1}`);
        }
      }
    }
    await ev(t, `window.__lab.lang("es"); window.__lab.show("hoy", "rich", 9)`);
    await ev(t, `window.__lab.show("sistema", "rich", 9)`);
    assert.equal(await ev(t, `document.querySelectorAll("#host .hoy").length`), 0, "HOY left nothing behind");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the prologue: a veil from the HUD to the floor, light text in both themes, the figures, the voice, and a way to HOY from the first frame", async () => {
  const t = await open();
  try {
    await ev(t, `window.__lab.show("prologo", "rich", 9)`);
    const read = () => ev(t, `(() => { const q = (s) => document.querySelector(s); const app = getComputedStyle(q("#app")); const r = q("#app").getBoundingClientRect();
      return { top: r.top, bottom: r.bottom, bg: app.backgroundColor, filter: app.backdropFilter, h1: getComputedStyle(q(".prologo .mega")).color, label: getComputedStyle(q(".prologo .fig-l")).color, eyebrow: getComputedStyle(q(".prologo .eyebrow")).color,
        voiceColor: getComputedStyle(q(".prologo .voice")).color, voice: q(".prologo .voice").textContent, voiceFont: getComputedStyle(q(".prologo .voice")).fontFamily,
        figs: [...document.querySelectorAll(".prologo .fig-n")].map((n) => n.textContent), night: q(".prologo .night").textContent, base: q(".prologo .base").textContent,
        go: q(".prologo .go").textContent, goVisible: q(".prologo .go").getBoundingClientRect().bottom < innerHeight }; })()`);
    const tinta = await read();
    assert.deepEqual([tinta.top, tinta.bottom, tinta.bg, tinta.filter], [56, 1080, "rgba(6, 8, 14, 0.55)", "none"]);
    assert.deepEqual([tinta.h1, tinta.label, tinta.eyebrow, tinta.voiceColor], ["rgb(233, 239, 247)", "rgb(233, 239, 247)", "rgb(158, 172, 190)", "rgb(251, 192, 36)"], "the labels take the full ink (A18, see the contrast test below); the eyebrow is dim");
    assert.equal(tinta.voice, "Mientras dormías.");
    assert.match(tinta.voiceFont, /Instrument Serif/);
    assert.deepEqual(tinta.figs, ["120", "14", "10", "1"]);
    assert.match(tinta.night, /^\d+ avisos nuevos · \d+ movimientos$/);
    assert.equal(tinta.base, "base: 7 días", "the base of the figures is visible");
    assert.equal(tinta.goVisible, true, "Ir a HOY is on the first screen");
    await ev(t, `document.documentElement.setAttribute("data-theme", "light")`);
    const papel = await read();
    assert.deepEqual([papel.h1, papel.label, papel.eyebrow, papel.voiceColor, papel.bg], [tinta.h1, tinta.label, tinta.eyebrow, tinta.voiceColor, tinta.bg], "Papel changes the sheet, and the prologue has none: its text stays light on the dark veil");
    await t.page.click(".prologo .go");
    assert.deepEqual((await ev(t, `window.__lab.state().went`)), [["hoy", null]]);
    // the engine did not run: the sentence replaces the voice, from last_run_at only
    await ev(t, `window.__lab.show("prologo", "empty", 9)`);
    assert.equal(await ev(t, `document.querySelector(".prologo .voice").textContent`), "Anoche no hubo corrida: todavía no hay ninguna.");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

// ---- A18 over the veil and over the glass: the text measured on the pixels Chrome really paints ----

const lum = ([r, g, b]) => { const l = (c) => ((c /= 255) <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4); return 0.2126 * l(r) + 0.7152 * l(g) + 0.0722 * l(b); };

/**
 * For each text of the moment: the contrast of its colour against the ground behind its glyphs. The ground is the frame with the glyphs
 * made invisible (lab: ghost), so it is the veil or the sheet over the sky; the figure is taken at the brightest 2 % of its pixels
 * (the worst case, without letting one star decide).
 */
async function textContrast(t, id, fixture, hora) {
  await ev(t, `window.__lab.show(${JSON.stringify(id)}, ${JSON.stringify(fixture)}, ${hora})`);
  const boxes = await ev(t, `window.__lab.textBoxes()`);
  await ev(t, `window.__lab.ghost(true)`);
  const png = decodePng(Buffer.from((await t.page.send("Page.captureScreenshot", { format: "png" })).data, "base64"));
  await ev(t, `window.__lab.ghost(false)`);
  const rows = [];
  for (const box of boxes) {
    const color = box.color.match(/\d+(\.\d+)?/g).map(Number).slice(0, 3);
    const grounds = [];
    for (const r of box.rects) {
      const x0 = Math.max(0, Math.floor(r.left) - 2), x1 = Math.min(png.w - 1, Math.ceil(r.right) + 2);
      const y0 = Math.max(0, Math.floor(r.top) - 2), y1 = Math.min(png.h - 1, Math.ceil(r.bottom) + 2);
      for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) grounds.push(png.at(x, y));
    }
    if (grounds.length === 0) continue;                    // below the fold of a moment that scrolls: not painted, nothing to measure
    assert.ok(grounds.length > 40, `the measure saw "${box.text}" (${grounds.length} pixels)`);
    grounds.sort((a, b) => lum(a) - lum(b));
    rows.push({ text: box.text, cls: box.cls, ratio: contrast(color, grounds[Math.floor(grounds.length * 0.98)]) });
  }
  return rows;
}

for (const [width, height] of [[1920, 1080], [1280, 720], [3440, 1440]]) {
  test(`A18 on the prologue at ${width}x${height}: every text keeps 4.5:1 over the veil on the day sky at 09:00, 12:00 and 17:00`, async () => {
    const t = await open({ width, height });
    try {
      for (const hora of [9, 12, 17]) {
        const rows = await textContrast(t, "prologo", "rich", hora);
        assert.ok(rows.length >= 12, `the eyebrow, the title, the voice, eight figure texts, the base, the night and the button (${rows.length})`);
        for (const r of rows) assert.ok(r.ratio >= 4.5, `"${r.text}" (${r.cls}) is ${r.ratio.toFixed(2)}:1 at ${hora}:00, ${width}x${height}`);
      }
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}

// The same measure on the real moments, over the glass, one test each (the ones in OPEN are open findings of those moments, not of the frame)
// Closed 2026-11-05: COLA's retry got its scrim in a323667 and APRENDER's topic names stopped sitting over the bar in d2456d8; both measured
// at or above 4.5:1 in eight runs at 1920x1080 and 1280x720 on c3e9084 (headless Chrome, Tinta), so they are tests again.
const OPEN = {};   // what the measure finds and the file of that moment has still to answer, by moment id
const GLASS = [["hoy", 9], ["procesos", 17], ["cola", 12], ["aprender", 15], ["sistema", 9]];
for (const [id, hora] of GLASS) {
  test(`A18 on ${id.toUpperCase()} over the glass: every text keeps 4.5:1 over the sky at ${hora}:00, at 1920x1080 and at 1280x720`, { todo: OPEN[id] || false }, async () => {
    for (const [width, height] of [[1920, 1080], [1280, 720]]) {
      const t = await open({ width, height });
      try {
        const rows = await textContrast(t, id, "rich", hora);
        assert.ok(rows.length >= 5, `${id}: the hour, the title and its figures (${rows.length})`);
        for (const r of rows) assert.ok(r.ratio >= 4.5, `${id}: "${r.text}" (${r.cls}) is ${r.ratio.toFixed(2)}:1 at ${hora}:00, ${width}x${height}`);
        assert.deepEqual(t.page.problems(), []);
      } finally { await t.close(); }
    }
  });
}
