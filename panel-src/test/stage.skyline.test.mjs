import test from "node:test";
import assert from "node:assert/strict";
import { recorder } from "./helpers/canvas.mjs";
import { mulberry32, makeLayer, LAYER_SPEC, createSkyline } from "../src/stage/skyline.js";

test("the same seed makes the same city, another seed another one", () => {
  const a = createSkyline({ seed: 7 }), b = createSkyline({ seed: 7 }), c = createSkyline({ seed: 8 });
  assert.deepEqual(a.layers[2].slice(0, 5), b.layers[2].slice(0, 5));
  assert.notDeepEqual(a.layers[2].slice(0, 5), c.layers[2].slice(0, 5));
  assert.equal(createSkyline().layers.length, 3);
  assert.deepEqual(LAYER_SPEC.map((s) => s.name), ["far", "mid", "near"]);
});

test("every layer reaches past 4000 px so 3440 wide plus the pan never runs out of buildings", () => {
  const sky = createSkyline();
  for (const layer of sky.layers) {
    const last = layer.at(-1);
    assert.ok(last.x + last.w > 4000, `last building ends at ${last.x + last.w}`);
    for (const b of layer) assert.ok(b.w > 0 && b.h > 0 && Array.isArray(b.wins));
  }
  const rand = mulberry32(1), layer = makeLayer(rand, 40, 170, 30, 70, 2.2, 3);
  assert.ok(layer.length > 50);
});

test("geometry scales with the window height and moves with the shift", () => {
  const sky = createSkyline();
  const a = sky.geometry(1920, 1080, [0, 0, 0]), b = sky.geometry(1920, 1080, [24, 60, 120]);
  assert.equal(a.baseY, 1082);
  assert.ok(Math.abs(a.layers[2].scale - 1.2 * 1.05) < 1e-9);
  assert.equal(b.layers[2].offset - a.layers[2].offset, 120);
  assert.equal(sky.geometry(1920, 300, [0, 0, 0]).layers[1].scale, 0.7, "a short window is clamped at 0.7");
  assert.equal(sky.geometry(3440, 3000, [0, 0, 0]).layers[1].scale, 1.6, "a tall one at 1.6");
});

test("with no lit windows only the bodies are drawn; with some, windows too; buildings off the canvas are skipped", () => {
  const sky = createSkyline();
  const dark = recorder(), lit = recorder(), narrow = recorder();
  const g = { h: 1080, shift: [0, 0, 0], top: [4, 6, 11], hor: [10, 18, 36] };
  sky.draw(dark.ctx, { ...g, w: 1920, lit: 0 });
  sky.draw(lit.ctx, { ...g, w: 1920, lit: 0.6 });
  sky.draw(narrow.ctx, { ...g, w: 300, lit: 0.6 });
  const bodies = dark.count("fillRect");
  assert.ok(bodies > 40 && bodies < 400, `${bodies} bodies`);
  assert.ok(lit.count("fillRect") > bodies + 200, "lit windows are extra rects");
  assert.ok(narrow.count("fillRect") * 3 < lit.count("fillRect"), "a 300 px canvas draws a fraction of a 1920 one");
});

test("a ghost draws two extra copies of the mid and near buildings and nothing of the far ones", () => {
  const sky = createSkyline();
  const plain = recorder(), ghost = recorder();
  const g = { w: 1920, h: 1080, shift: [0, 0, 0], lit: 0, top: [4, 6, 11], hor: [10, 18, 36] };
  sky.draw(plain.ctx, g);
  sky.draw(ghost.ctx, { ...g, ghost: true });
  assert.ok(ghost.count("fillRect") > plain.count("fillRect") * 1.4);
});

test("litWindows lists the lit windows of one layer inside an x range, left to right, and more of them when more are lit", () => {
  const sky = createSkyline();
  const few = sky.litWindows(1920, 1080, [0, 0, 0], 0.2, { minX: 1300, maxX: 1900 });
  const many = sky.litWindows(1920, 1080, [0, 0, 0], 0.7, { minX: 1300, maxX: 1900 });
  assert.ok(many.length > few.length && many.length > 5);
  for (const w of many) assert.ok(w.x >= 1300 && w.x <= 1900 && w.layer === 2 && w.a >= 0.5 && w.y > 400);
  assert.deepEqual(many.map((w) => w.x), [...many.map((w) => w.x)].sort((x, y) => x - y));
  assert.deepEqual(sky.litWindows(1920, 1080, [0, 0, 0], 0, { minX: 0, maxX: 1920 }), []);
});

test("draw takes a range of layers: the far one and the others add up to the whole city, so the stage can paint them on two canvases", () => {
  const sky = createSkyline();
  const g = { w: 1920, h: 1080, shift: [0, 0, 0], lit: 0.6, top: [4, 6, 11], hor: [10, 18, 36] };
  const [all, far, rest, none] = [{}, { to: 1 }, { from: 1 }, { from: 3 }].map((range) => { const r = recorder(); sky.draw(r.ctx, { ...g, ...range }); return r.count("fillRect"); });
  assert.equal(far + rest, all);
  assert.ok(far > 0 && rest > far, "the mid and near layers are the bulk of the city");
  assert.equal(none, 0, "a range past the last layer draws nothing");
});

// ---- the strips: a layer is painted once and a frame of a trip only copies the slice that is on screen ---------------------------------------------------

/** A skyline whose strips are recorders, and the list of them. */
function stripped(options = {}) {
  const made = [];
  const makeCanvas = (width, height) => { const rec = recorder(); const c = { width, height, rec, getContext: () => rec.ctx }; made.push(c); return c; };
  return { sky: createSkyline({ makeCanvas, ...options }), made };
}
const FRAME = { w: 1920, h: 1080, shift: [24, 60, 120], lit: 0.6, top: [4, 6, 11], hor: [10, 18, 36], ratio: 1 };

test("with a pixel ratio the first frame paints a strip per layer and draws them on the canvas; the next frames only copy slices of them", () => {
  const { sky, made } = stripped();
  const g = recorder();
  sky.draw(g.ctx, FRAME);
  assert.equal(made.length, 3, "one strip for each layer");
  assert.equal(g.count("fillRect"), 0, "nothing is painted on the canvas but the copies of the strips");
  assert.equal(g.count("drawImage"), 3);
  assert.ok(made.every((c) => c.rec.count("fillRect") > 40), "each strip holds its buildings and windows");
  for (const c of made) assert.ok(c.width > 1920 + 800 && c.height < 400, `a strip is a band: ${c.width}x${c.height}`);
  const painted = made.map((c) => c.rec.calls.length);
  for (const pos of [1, 1.4, 2, 3.7, 5]) {
    const shift = [pos * 24, pos * 60, pos * 120];
    sky.draw(g.ctx, { ...FRAME, shift });
  }
  assert.equal(made.length, 3, "no strip is made again while the pan stays in it");
  assert.deepEqual(made.map((c) => c.rec.calls.length), painted, "and none is painted again");
  assert.equal(g.count("drawImage"), 3 * 6);
});

test("the slice is cut in whole device pixels and lands on the bottom of the window", () => {
  const { sky, made } = stripped();
  const g = recorder();
  sky.draw(g.ctx, { ...FRAME, ratio: 1.5, shift: [24, 60, 120.5] });
  const near = g.calls.filter((c) => c[0] === "drawImage")[2];
  const [, picture, sx, sy, sw, sh, dx, dy, dw, dh] = near;
  assert.equal(picture, made[2]);
  for (const v of [sx, sy, sw, sh]) assert.ok(Number.isInteger(v), `source ${v} is whole device pixels`);
  assert.ok(Number.isInteger(dw * 1.5) && Number.isInteger(dy * 1.5), "and so is where it lands");
  assert.equal(Math.round((dy + dh) * 1.5), Math.round(1080 * 1.5), "the bottom of the strip is the bottom of the window");
  assert.equal(dx, 0);
  assert.equal(sw, Math.ceil(1920 * 1.5));
});

test("a strip is painted again when what it shows changes: the lit fraction, the colours, the height, the pixel ratio, or a pan that leaves it", () => {
  const { sky, made } = stripped();
  const g = recorder();
  sky.draw(g.ctx, FRAME);
  const base = made.length;
  sky.draw(g.ctx, { ...FRAME, lit: 0.6 + 1 / 1000 });
  assert.equal(made.length, base, "a lit fraction within 1/400 is the same picture");
  sky.draw(g.ctx, { ...FRAME, lit: 0.6 + 1 / 100 });
  assert.equal(made.length, base + 3, "a hundredth more lit is another one");
  sky.draw(g.ctx, { ...FRAME, lit: 0.61, top: [20, 30, 60] });
  assert.equal(made.length, base + 6, "other colours");
  sky.draw(g.ctx, { ...FRAME, lit: 0.61, top: [20, 30, 60], h: 900 });
  assert.equal(made.length, base + 9, "another height");
  sky.draw(g.ctx, { ...FRAME, lit: 0.61, top: [20, 30, 60], h: 900, ratio: 2 });
  assert.equal(made.length, base + 12, "another pixel ratio");
  const n = made.length;
  sky.draw(g.ctx, { ...FRAME, lit: 0.61, top: [20, 30, 60], h: 900, ratio: 2, shift: [24, 60, 4000] });
  assert.equal(made.length, n + 1, "a pan far beyond the strip repaints that layer's strip only");
});

test("a ghost copies the silhouette of the mid and near layers twice at 0.45, made once", () => {
  const { sky, made } = stripped();
  const g = recorder();
  sky.draw(g.ctx, { ...FRAME, ghost: true });
  assert.equal(made.length, 3 + 2, "a silhouette for each of the two layers that ghost");
  assert.equal(g.count("drawImage"), 3 + 4);
  assert.equal(g.calls.filter((c) => c[0] === "set:globalAlpha").map((c) => c[1]).join(","), "0.45,1,0.45,1");
  const silhouette = [made[2], made[4]];                              // made right after the strip of their layer: far, mid, its silhouette, near, its silhouette
  assert.ok(silhouette.every((c) => c.rec.count("fillRect") > 15 && c.rec.calls.every((k) => k[0] !== "set:fillStyle" || /^rgb\(/.test(k[1]))), "silhouettes are bodies only, in the body colour");
  sky.draw(g.ctx, { ...FRAME, ghost: true, shift: [30, 70, 130] });
  assert.equal(made.length, 3 + 2, "and not made again");
});

test("without a pixel ratio, or without somewhere to paint a strip, every frame paints the whole city on the canvas as before", () => {
  const { sky, made } = stripped();
  const direct = recorder();
  const { ratio, ...noRatio } = FRAME;
  sky.draw(direct.ctx, noRatio);
  assert.equal(made.length, 0);
  assert.ok(direct.count("fillRect") > 300 && direct.count("drawImage") === 0);
  const none = recorder();
  createSkyline({ makeCanvas: null }).draw(none.ctx, FRAME);
  assert.equal(none.count("fillRect"), direct.count("fillRect"), "makeCanvas: null is the same direct painting");
});

test("litWindows by search gives exactly what a walk over the buildings gives, for any pan and range", () => {
  const sky = createSkyline();
  const walk = (w, h, shift, lit, { minX, maxX, layer = 2, minA = 0.5 }) => {
    const geo = sky.geometry(w, h, shift), L = geo.layers[layer], out = [], winPx = L.spec.size * L.scale;
    for (const b of L.buildings) {
      const bx = b.x * L.scale - L.offset, bh = b.h * L.scale;
      for (const wn of b.wins) {
        const a = Math.min(1, Math.max(0, (lit * L.spec.litK - wn.thr) / 0.1)) * L.spec.winAlpha;
        if (a < minA) continue;
        const x = bx + wn.x * L.scale + winPx / 2, y = geo.baseY - bh + wn.y * L.scale + (winPx * 1.15) / 2;
        if (x >= minX && x <= maxX) out.push({ x, y, a, layer });
      }
    }
    return out.sort((p, q) => p.x - q.x);
  };
  for (const [w, h, shift, lit, range, minA] of [[1920, 1080, [0, 0, 0], 0.55, { minX: 1237, maxX: 1880 }, 0.5], [1920, 1080, [24, 60, 120], 0.2, { minX: 1237, maxX: 1880 }, 0.2], [3440, 1440, [72, 180, 360], 0.4, { minX: 1524, maxX: 3400 }, 0.5], [1440, 900, [120, 300, 600], 0.55, { minX: 1068, maxX: 1400 }, 0.5]]) {
    const got = sky.litWindows(w, h, shift, lit, { ...range, minA }), want = walk(w, h, shift, lit, { ...range, minA });
    assert.equal(got.length, want.length);
    assert.ok(got.length > 5);
    got.forEach((p, i) => { assert.ok(Math.abs(p.x - want[i].x) < 1e-6 && Math.abs(p.y - want[i].y) < 1e-6 && Math.abs(p.a - want[i].a) < 1e-9 && p.layer === 2, `window ${i}`); });
  }
});
