// The strips of the skyline on a real canvas: the city copied from its strips is the city painted directly (to the pixel when the pan is whole device pixels,
// up to the snap of a fractional pan to the pixel grid otherwise), and a frame from the strips costs a fraction of the direct one.
import test from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";
import { launch } from "../tools/cdp.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const bundle = async (file = "skyline.js", globalName = "SK") => (await build({ entryPoints: [path.join(here, "..", "src", "stage", file)], bundle: true, format: "iife", globalName, write: false })).outputFiles[0].text;

async function inPage(run, ...files) {
  const code = (await bundle()) + (files.includes("sky") ? await bundle("sky.js", "SKY") : "");
  const b = await launch({ width: 800, height: 600 });
  try {
    const page = await b.newPage();
    await page.goto("about:blank");
    await page.eval(code);
    return await page.eval(run);
  } finally { await b.close(); }
}

const COMPARE = `(() => {
  const direct = SK.createSkyline({ makeCanvas: null }), cached = SK.createSkyline();
  const mk = (w, h, ratio) => { const c = document.createElement("canvas"); c.width = Math.round(w * ratio); c.height = Math.round(h * ratio); const g = c.getContext("2d"); g.setTransform(ratio, 0, 0, ratio, 0, 0); g.fillStyle = "#101830"; g.fillRect(0, 0, w, h); return [c, g]; };
  const diff = (g1, g2, c) => { const a = g1.getImageData(0, 0, c.width, c.height).data, d = g2.getImageData(0, 0, c.width, c.height).data; let max = 0, over = 0; for (let i = 0; i < a.length; i += 4) { const m = Math.max(Math.abs(a[i] - d[i]), Math.abs(a[i + 1] - d[i + 1]), Math.abs(a[i + 2] - d[i + 2])); if (m > 8) over++; if (m > max) max = m; } return { max, over, pixels: a.length / 4 }; };
  const out = [];
  for (const [w, h, ratio] of [[1920, 1080, 1], [1440, 900, 1], [1920, 1080, 1.5], [3440, 1440, 1]]) {
    for (const [name, shift, lit, ghost, range] of [["HOY", [24, 60, 120], 0.6, false, {}], ["COLA", [72, 180, 360], 0.3, false, {}], ["day", [24, 60, 120], 0.04, false, {}], ["far", [24, 60, 120], 0.6, false, { to: 1 }], ["near", [24, 60, 120], 0.6, false, { from: 2 }], ["ghost", [24, 60, 120], 0.55, true, {}], ["fractional", [22.2, 55.5, 111], 0.55, false, {}]]) {
      const [c1, g1] = mk(w, h, ratio), [c2, g2] = mk(w, h, ratio);
      const args = { w, h, shift, lit, top: [16, 24, 48], hor: [88, 83, 127], ghost, ...range };
      direct.draw(g1, args);
      cached.draw(g2, { ...args, ratio });
      cached.draw(g2, { ...args, ratio });                       // the second time it is the strip that was kept
      out.push({ size: w + "x" + h + "@" + ratio, name, ...diff(g1, g2, c1) });
    }
  }
  return JSON.stringify(out);
})()`;

test("the city from its strips is the city painted directly: whole-pixel pans differ by a rounding of the alpha at most, a ghost by a few pixels, a fractional pan only at the edges", async () => {
  const rows = JSON.parse(await inPage(COMPARE));
  assert.equal(rows.length, 28);
  for (const r of rows) {
    const label = `${r.size} ${r.name}: ${r.over} of ${r.pixels} pixels differ by more than 8, at most ${r.max}`;
    if (r.name === "fractional") assert.ok(r.over / r.pixels < 0.006, label);          // the pan snapped to the device pixel: the edges of the buildings move by up to half of one
    else if (r.name === "ghost") assert.ok(r.over < 200 && r.over / r.pixels < 0.0001, label);   // the ghosts are all under the bodies of their layer, not each under its next building
    else assert.equal(r.over, 0, label);
    if (r.name !== "fractional" && r.name !== "ghost") assert.ok(r.max <= 8, label);
  }
});

test("a frame of the city from its strips costs a fraction of one painted directly (drawing calls of the frame, on a visible canvas, frame after frame)", async () => {
  const ms = JSON.parse(await inPage(`new Promise((resolve) => {
    const direct = SK.createSkyline({ makeCanvas: null }), cached = SK.createSkyline();
    const args = (i) => ({ w: 1920, h: 1080, shift: [24 + i * 0.5, 60 + i, 120 + i * 2], lit: 0.6, top: [16, 24, 48], hor: [88, 83, 127] });
    const run = (draw, n = 90) => new Promise((done) => {
      const c = document.createElement("canvas"); c.width = 1920; c.height = 1080; c.style.cssText = "position:absolute;left:0;top:0;width:1920px;height:1080px"; document.body.append(c);
      const g = c.getContext("2d", { alpha: false });
      let i = 0; const t = [];
      (function frame() {
        const t0 = performance.now(); g.fillStyle = "#101830"; g.fillRect(0, 0, 1920, 1080); draw(g, i); t.push(performance.now() - t0);
        if (++i < n) requestAnimationFrame(frame); else { c.remove(); t.sort((a, b) => a - b); done(t[n >> 1]); }
      })();
    });
    (async () => {
      const out = { direct: await run((g, i) => direct.draw(g, args(i))), cached: await run((g, i) => cached.draw(g, { ...args(i), ratio: 1 })) };
      out.direct = Math.min(out.direct, await run((g, i) => direct.draw(g, args(i))));
      out.cached = Math.min(out.cached, await run((g, i) => cached.draw(g, { ...args(i), ratio: 1 })));
      resolve(JSON.stringify(out));
    })();
  })`));
  assert.ok(ms.cached * 2 < ms.direct, `from the strips ${ms.cached.toFixed(2)} ms, direct ${ms.direct.toFixed(2)} ms (median of 90 frames)`);
});

test("the stars copied from their picture are the stars painted on the sky, at night, at dusk and with a pixel ratio of 1.5", async () => {
  const rows = JSON.parse(await inPage(`(() => {
    const out = [];
    for (const ratio of [1, 1.5]) for (const hora of [22, 19.8, 3, 5.6]) {
      const made = [];
      for (const makeCanvas of [null, undefined]) {
        const c = document.createElement("canvas"); c.style.cssText = "position:absolute;left:0;top:0;width:1920px;height:1080px"; document.body.append(c);
        let n = 12345; const rand = () => { n = (n * 16807) % 2147483647; return n / 2147483647; };
        const opts = { skyline: { draw() {} }, drawBeam() {}, rand, dpr: () => ratio };
        if (makeCanvas === null) opts.makeCanvas = null;
        const sky = SKY.createSky(c, opts);
        sky.resize(); sky.paint({ hora }); sky.paint({ hora });
        made.push(c.getContext("2d").getImageData(0, 0, c.width, c.height).data); c.remove();
      }
      let max = 0, over = 0; for (let i = 0; i < made[0].length; i += 4) { const m = Math.max(Math.abs(made[0][i] - made[1][i]), Math.abs(made[0][i + 1] - made[1][i + 1]), Math.abs(made[0][i + 2] - made[1][i + 2])); if (m > 3) over++; if (m > max) max = m; }
      out.push({ ratio, hora, max, over });
    }
    return JSON.stringify(out);
  })()`, "sky"));
  assert.equal(rows.length, 8);
  for (const r of rows) assert.ok(r.max <= 3 && r.over === 0, `ratio ${r.ratio}, ${r.hora} h: ${r.over} pixels differ by more than 3, at most ${r.max}`);
});
