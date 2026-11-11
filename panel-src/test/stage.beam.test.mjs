import test from "node:test";
import assert from "node:assert/strict";
import { recorder } from "./helpers/canvas.mjs";
import { FREE_MIN, FADE_PX, BODY_HOURS, bodyKind, clipPolygon, polygonHitsRect, coneOf, beamFor, drawBeam, fadeWidth } from "../src/stage/beam.js";
import { createSkyline } from "../src/stage/skyline.js";
import { skyAt } from "../src/stage/sky.js";
import { layoutOf } from "../src/stage/dial.js";
import { STOP_IDS, dialPoseOf, sheetGeometry } from "../src/scene/poses.js";

// A scene as the stage builds it: the sheet of the window, the hub of the dial's pose at a stop, the lit windows of the near layer.
function scene(w, h, hora, stop = "hoy", { sheetTop = 78, sheetBottom = h - 40 } = {}) {
  const { gutter, sheetRight } = sheetGeometry({ vw: w });
  const g = { vw: w, vh: h, sheetRight, gutter };
  const base = Math.max(...STOP_IDS.map((id) => dialPoseOf(id, g).size));
  const pose = layoutOf(dialPoseOf(stop, g), hora, base);
  const hub = { x: pose.cx, y: pose.cy }, tip = pose.tip;
  const sheet = { left: gutter, right: sheetRight, top: sheetTop, bottom: sheetBottom };
  const windows = createSkyline().litWindows(w, h, [0, 0, 0], skyAt(hora).lit, { minX: sheetRight + 60, maxX: w - 40 });
  return { w, h, hora, sheet, hub, tip, windows };
}

test("the nearest body in time, round the 24 h ring: sun 09:00, window 00:26, horizon 18:00", () => {
  assert.deepEqual(BODY_HOURS, { sun: 9, window: 26 / 60, horizon: 18 });
  const kinds = (horas) => horas.map(bodyKind);
  assert.deepEqual(kinds([9, 8, 5, 12, 13.4, 7.5]), Array(6).fill("sun"));
  assert.deepEqual(kinds([0.43, 0, 23, 22, 21.3, 4.5, 2]), Array(7).fill("window"));
  assert.deepEqual(kinds([18, 16, 13.6, 15.5, 20, 20.5]), Array(6).fill("horizon"));
  assert.equal(bodyKind(24), bodyKind(0), "the ring closes");
  assert.equal(bodyKind(13.5), "sun", "a tie goes to the earlier body of the list");
});

test("clipPolygon keeps only x >= minX, and returns nothing when the polygon is all on the left", () => {
  const tri = [[0, 0], [200, 0], [100, 100]];
  const cut = clipPolygon(tri, 100);
  assert.ok(cut.length >= 3);
  assert.ok(cut.every(([x]) => x >= 100 - 1e-9));
  assert.deepEqual(clipPolygon(tri, 300), []);
  assert.deepEqual(clipPolygon(tri, -50), tri);
});

test("polygonHitsRect is a real separating-axis test: overlap is a hit, sharing an edge or a gap is not", () => {
  const rect = { left: 0, right: 100, top: 0, bottom: 100 };
  assert.equal(polygonHitsRect([[50, 50], [150, 50], [100, 150]], rect), true, "overlaps a corner");
  assert.equal(polygonHitsRect([[20, 20], [60, 20], [40, 60]], rect), true, "fully inside");
  assert.equal(polygonHitsRect([[100, 0], [200, 0], [200, 100]], rect), false, "shares an edge");
  assert.equal(polygonHitsRect([[120, 0], [220, 0], [170, 100]], rect), false, "a gap");
  assert.equal(polygonHitsRect([[-50, 40], [-1, 40], [-1, 60]], rect), false);
  assert.equal(polygonHitsRect([[150, -50], [250, -50], [100, 40]], { left: 0, right: 100, top: 0, bottom: 100 }), false, "a diagonal edge that never touches the rect");
  assert.equal(polygonHitsRect([], rect), false);
});

test("a cone is a convex quad from the source along the beam's angle", () => {
  const poly = coneOf({ x: 100, y: 100, ang: 0, len: 400, wid: 200, core: 10 });
  assert.equal(poly.length, 4);
  assert.deepEqual(poly[0].map(Math.round), [100, 110]);
  assert.ok(poly[1][0] > 450 && poly[2][0] > 450, "the far end is len away");
  assert.ok(poly[1][1] > 100 && poly[2][1] < 100);
});

test("A17: at 09:00 the sun is born at the hub of the dial, not at the tip of its hand, and the beam ends at the sheet", () => {
  for (const [w, h] of [[1920, 1080], [3440, 1440]]) {
    const s = scene(w, h, 9);
    const r = beamFor(s);
    assert.equal(r.kind, "sun");
    assert.deepEqual([r.beam.x, r.beam.y], [s.hub.x, s.hub.y]);
    assert.ok(Math.hypot(r.beam.x - s.tip.x, r.beam.y - s.tip.y) > 100, "the hand's tip is somewhere else on the ring");
    assert.ok(r.beam.core >= 12 && r.beam.a > 0.4);
    assert.ok(r.polygon.length >= 3 && r.polygon.every(([x]) => x >= s.sheet.right - 1e-6));
    assert.equal(polygonHitsRect(r.polygon, s.sheet), false);
  }
});

test("A17: at 00:26 the light comes from a lit window of the near layer, in the free part of the screen", () => {
  for (const [w, h] of [[1920, 1080], [3440, 1440]]) {
    const s = scene(w, h, 0.43);
    assert.ok(s.windows.length > 3, `${s.windows.length} lit windows at 00:26 (${w}x${h})`);
    const r = beamFor(s);
    assert.ok(r, "there is a window to light the beam");
    assert.equal(r.kind, "window");
    assert.ok(s.windows.some((win) => win.x === r.beam.x && win.y === r.beam.y), "the source is one of the visible windows");
    assert.ok(r.beam.x >= s.sheet.right + 60 && r.beam.x <= w - 40);
    assert.equal(polygonHitsRect(r.polygon, s.sheet), false);
  }
});

test("A17: at 18:00 the last light grazes the horizon just right of the sheet", () => {
  for (const [w, h] of [[1920, 1080], [3440, 1440]]) {
    const s = scene(w, h, 18);
    const r = beamFor(s);
    assert.equal(r.kind, "horizon");
    assert.ok(r.beam.x > s.sheet.right && r.beam.x < w);
    assert.ok(r.beam.y > h * 0.9 && r.beam.y < h);
    assert.equal(polygonHitsRect(r.polygon, s.sheet), false);
  }
});

test("A17: for every size, hour, stop and sheet height the beam is one, its source is on screen right of the sheet, and its polygon never meets the sheet", () => {
  const horas = [0, 0.43, 3, 6.5, 9, 11, 13, 15.5, 18, 20.5, 22];
  let beams = 0, nulls = 0;
  for (const [w, h] of [[1920, 1080], [3440, 1440], [2560, 1440], [1366, 768], [1280, 720]]) {
    for (const hora of horas) for (const stop of ["prologo", "hoy", "cola", "sistema"]) for (const tall of [false, true]) {
      const s = scene(w, h, hora, stop, tall ? { sheetTop: 56, sheetBottom: h } : {});
      const r = beamFor(s);
      if (!r) { nulls++; continue; }
      beams++;
      assert.ok(r.beam.x >= s.sheet.right && r.beam.x <= w && r.beam.y >= 0 && r.beam.y <= h, `source ${r.beam.x},${r.beam.y} at ${w}x${h} ${hora} ${stop}`);
      assert.ok(r.polygon.every(([x]) => x >= s.sheet.right - 1e-6), "clipped at the sheet's right edge");
      assert.equal(polygonHitsRect(r.polygon, s.sheet), false, `${w}x${h} ${hora} ${stop}`);
    }
  }
  assert.ok(beams > 150, `${beams} beams`);
  assert.ok(nulls < beams, `${nulls} nulls`);
});

test("no beam when the free part of the screen is narrower than 120 px, when the sun is behind the sheet, or when no window is lit there", () => {
  assert.equal(FREE_MIN, 120);
  const narrow = scene(1920, 1080, 9);
  assert.equal(beamFor({ ...narrow, w: narrow.sheet.right + 119 }), null);
  assert.equal(beamFor({ ...narrow, hub: { x: narrow.sheet.right - 10, y: 500 } }), null, "the sun behind the sheet is not visible");
  assert.equal(beamFor({ ...narrow, hub: { x: 1500, y: -20 } }), null, "above the window");
  const night = scene(1920, 1080, 0.43);
  assert.equal(beamFor({ ...night, windows: [] }), null, "no lit window, no light");
  assert.equal(beamFor({ ...night, windows: [{ x: night.sheet.right + 5, y: 1000, a: 0.8, layer: 2 }] }), null, "a window too close to the sheet's edge");
});

test("drawBeam clips the canvas at the sheet's edge before painting, lights additively, and balances save and restore", () => {
  const withCore = recorder(), noCore = recorder();
  const beam = { x: 1600, y: 600, ang: 3.0, len: 500, wid: 300, color: [255, 186, 120], a: 0.5, core: 14, coreColor: [255, 240, 205] };
  drawBeam(withCore.ctx, beam, 1177);
  drawBeam(noCore.ctx, { ...beam, core: 0 }, 0);
  const names = withCore.calls.map((c) => c[0]);
  const clip = names.indexOf("clip");
  assert.ok(clip > 0 && names.indexOf("rect") < clip && names.indexOf("rect") >= 0);
  assert.equal(withCore.calls[names.indexOf("rect")][1], 1177);
  assert.ok(withCore.calls.some((c) => c[0] === "set:globalCompositeOperation" && c[1] === "lighter"));
  assert.ok(names.indexOf("createRadialGradient") > clip, "everything is painted after the clip");
  assert.equal(withCore.count("save"), withCore.count("restore"));
  assert.equal(withCore.count("createRadialGradient"), 10);
  assert.equal(noCore.count("createRadialGradient"), 6);
  assert.equal(noCore.count("clip"), 0, "no clip when there is no sheet to respect");
  assert.equal(noCore.count("save"), noCore.count("restore"));
});

test("the light comes in over a fade of up to 72 px right of the sheet, and the fade never reaches the source", () => {
  assert.equal(FADE_PX, 72);
  assert.equal(fadeWidth({ x: 5000 }, 1177), 72, "far sources fade over the whole 72 px");
  assert.equal(fadeWidth({ x: 1277 }, 1177), 50, "half the distance to a source that is 100 px away");
  assert.equal(fadeWidth({ x: 1177 }, 1177), 0, "a source on the edge has nothing to fade");
  assert.equal(fadeWidth({ x: 3000 }, 0), 0, "no sheet, no fade");
  assert.equal(fadeWidth({ x: 900 }, 1177), 0, "a source behind the edge never gets a negative width");
});

test("drawBeam paints the first pixels past the sheet on a layer of its own, ramps it from 0 to 1 and adds it to the canvas", () => {
  const main = recorder(), layer = recorder();
  const asked = [];
  const beam = { x: 1600, y: 600, ang: 3.0, len: 500, wid: 300, color: [255, 186, 120], a: 0.5, core: 14, coreColor: [255, 240, 205] };
  drawBeam(main.ctx, beam, 1177, (w, h) => { asked.push([w, h]); return { g: layer.ctx, image: "the layer" }; });
  assert.equal(asked.length, 1);
  assert.equal(asked[0][0], 72, "the layer is the fade's width");
  const clip = main.calls.find((c) => c[0] === "rect");
  assert.equal(clip[1], 1177 + 72, "the canvas itself is clipped where the fade ends, so no pixel is painted twice");
  assert.equal(main.count("createRadialGradient"), 10);
  assert.equal(layer.count("createRadialGradient"), 10 + 0, "the same light is painted on the layer");
  assert.deepEqual(layer.calls.find((c) => c[0] === "setTransform"), ["setTransform", 1, 0, 0, 1, -1177, 0], "the layer starts at the sheet's edge");
  const names = layer.calls.map((c) => c[0] + (c[0] === "set:globalCompositeOperation" ? ":" + c[1] : ""));
  assert.ok(names.lastIndexOf("set:globalCompositeOperation:destination-in") > names.lastIndexOf("createRadialGradient"), "the ramp multiplies what was painted");
  assert.equal(layer.count("createLinearGradient"), 1);
  const draw = main.calls.find((c) => c[0] === "drawImage");
  assert.deepEqual(draw, ["drawImage", "the layer", 1177, 0]);
  const at = main.calls.indexOf(draw);
  assert.ok(main.calls.slice(0, at).some((c) => c[0] === "set:globalCompositeOperation" && c[1] === "lighter"), "added, not covered");
  assert.equal(main.count("save"), main.count("restore"));
});

test("a layer that cannot be made, or no sheet, cuts the beam at the edge as before; a source on the edge needs no layer", () => {
  const beam = { x: 1600, y: 600, ang: 3.0, len: 500, wid: 300, color: [255, 186, 120], a: 0.5 };
  const none = recorder();
  drawBeam(none.ctx, beam, 1177, () => null);
  assert.equal(none.calls.find((c) => c[0] === "rect")[1], 1177);
  assert.equal(none.count("drawImage"), 0);
  let asked = 0;
  const edge = recorder();
  drawBeam(edge.ctx, { ...beam, x: 1178 }, 1177, () => { asked++; return null; });
  drawBeam(recorder().ctx, beam, 0, () => { asked++; return null; });
  assert.equal(asked, 0);
});

test("the two parts of a light can be painted apart: the wide glow first, the rays and the sun's core on their own", () => {
  const beam = { x: 1600, y: 600, ang: 3.0, len: 500, wid: 300, color: [255, 186, 120], a: 0.5, core: 14, coreColor: [255, 240, 205] };
  const [all, bloom, rays] = [undefined, "bloom", "rays"].map((part) => { const r = recorder(); drawBeam(r.ctx, { ...beam, part }, 0); return r; });
  assert.equal(all.count("createRadialGradient"), 10);
  assert.equal(bloom.count("createRadialGradient"), 3, "three layered ellipses");
  assert.equal(rays.count("createRadialGradient"), 7, "three rays and the four discs of the core");
  const noCore = recorder();
  drawBeam(noCore.ctx, { ...beam, core: 0, part: "rays" }, 0);
  assert.equal(noCore.count("createRadialGradient"), 3);
  assert.equal(bloom.count("save"), bloom.count("restore"));
});

test("the sun's disc is at most 5.5 % of the dial's box, so on a small dial it stays off the digits of the time", () => {
  const s = scene(1920, 1080, 9);
  const big = beamFor(s).beam.core;
  assert.ok(Math.abs(big - 1080 * 0.03) < 1e-9, "a 625 px box keeps the disc of 3 % of the height");
  const small = beamFor({ ...s, hub: { ...s.hub, size: 291 } }).beam.core;
  assert.ok(Math.abs(small - 291 * 0.055) < 1e-9, `${small}`);
  assert.equal(beamFor({ ...s, hub: { ...s.hub, size: 60 } }).beam.core, 14, "never under 14 px");
  assert.equal(beamFor({ ...s, hub: { x: s.hub.x, y: s.hub.y } }).beam.core, 1080 * 0.03, "without a size the height rules");
});

test("the drawer is a hole in the light: the canvas is clipped outside its rectangle, and so is the fade strip that is added to it", () => {
  const beam = { x: 1600, y: 600, ang: 3.0, len: 500, wid: 300, color: [255, 186, 120], a: 0.5, core: 14, coreColor: [255, 240, 205] };
  const holes = [{ left: 1300, top: 78, right: 1700, bottom: 1040 }];
  const main = recorder(), layer = recorder();
  drawBeam(main.ctx, beam, 1177, (w, h) => ({ g: layer.ctx, image: "the layer" }), holes);
  const clips = main.calls.map((c, i) => [c, i]).filter(([c]) => c[0] === "clip");
  assert.equal(clips.length, 3, "the sheet's edge, the hole before the canvas is painted, and the hole again before the fade strip is added");
  assert.deepEqual(main.calls[clips[1][1] - 1], ["rect", 1300, 78, 400, 962], "the hole is a rectangle of the clip path");
  assert.deepEqual(clips[1][0], ["clip", "evenodd"], "even-odd: what is inside the hole is outside the clip");
  assert.ok(clips[1][1] < main.calls.findIndex((c) => c[0] === "createRadialGradient"), "and the light is painted after it");
  const draw = main.calls.findIndex((c) => c[0] === "drawImage");
  assert.deepEqual(main.calls[main.calls.findIndex((c, i) => i > clips[1][1] && c[0] === "clip") - 1], ["rect", 1300, 78, 400, 962], "the strip is added through the same hole (a ratio of 1 here)");
  assert.ok(clips[2][1] < draw, "before it is drawn");
  assert.equal(main.count("save"), main.count("restore"));
  const plain = recorder();
  drawBeam(plain.ctx, beam, 1177, () => null);
  assert.equal(plain.count("clip"), 1, "with no hole nothing changes");
});

test("a sun that sits low throws a light that lies nearly flat across the sky, and one at mid height keeps its old aim", () => {
  const s = scene(1920, 1080, 9);
  const low = beamFor({ ...s, hub: { x: 1517, y: 756, size: 625 } }).beam;
  const edgeY = (b) => b.y + Math.tan(b.ang) * (s.sheet.right - b.x);
  assert.ok(Math.abs(edgeY(low) - (756 + (1080 * 0.52 - 756) * 0.3)) < 1, `it reaches the sheet's edge at ${edgeY(low).toFixed(0)}`);
  assert.ok(756 - edgeY(low) < 80, "a rise of under 80 px over 340 px");
  const mid = beamFor({ ...s, hub: { x: 1517, y: 540, size: 625 } }).beam;
  assert.ok(Math.abs(edgeY(mid) - 540) < 10, "a sun at mid height lights the sheet's edge at about its own height");
});
