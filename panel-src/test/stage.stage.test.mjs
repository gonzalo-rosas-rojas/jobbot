import test from "node:test";
import assert from "node:assert/strict";
import { recorder } from "./helpers/canvas.mjs";
import { createStage, clockText, POOL, FRONT_FROM } from "../src/stage/stage.js";
import { tokensOf, rgb } from "./helpers/color.mjs";
import { mulberry32 } from "../src/stage/skyline.js";
import { layoutOf } from "../src/stage/dial.js";
import { dialPoseOf, sheetGeometry, skylineShift, STOP_IDS } from "../src/scene/poses.js";

function build({ w = 1920, h = 1080, sheet = undefined, rec = recorder(), getHoles = undefined, ownerDocument = undefined } = {}) {
  const canvas = { clientWidth: w, clientHeight: h, width: 0, height: 0, getContext: () => rec.ctx, ownerDocument };
  const dial = {
    calls: [], baseSize: 0,
    base(px) { this.baseSize = px; this.calls.push(["base", px]); },
    set(hora, text) { this.calls.push(["set", hora, text]); },
    glow(g) { this.calls.push(["glow", g]); },
    layout(pose, hora) { this.calls.push(["layout", pose]); return layoutOf(pose, hora, this.baseSize); },
  };
  const skylineCalls = [], windowCalls = [];
  const skyline = {
    draw: (g, args) => skylineCalls.push(args),
    litWindows: (...args) => { windowCalls.push(args); return [{ x: w * 0.75, y: h * 0.9, a: 0.8, layer: 2 }]; },
    layers: [],
  };
  const stage = createStage({ canvas, dialHost: {}, t: (k) => k, getSheetRect: () => sheet, getHoles, rand: mulberry32(1), parts: { dial, skyline, dpr: () => 1 } });
  stage.resize();
  return { stage, dial, rec, skylineCalls, windowCalls, canvas };
}

test("clockText rounds to the nearest minute and wraps at midnight", () => {
  assert.deepEqual([9, 0.43, 18, 22, 23.999, 0, 12.5].map(clockText), ["09:00", "00:26", "18:00", "22:00", "00:00", "00:00", "12:30"]);
  assert.equal(clockText(-0.5), "23:30");
});

test("resize sizes the dial's box to the biggest pose, so layout only ever scales it down", () => {
  const { dial } = build();
  const g = { vw: 1920, vh: 1080, ...(({ gutter, sheetRight }) => ({ gutter, sheetRight }))(sheetGeometry({ vw: 1920 })) };
  assert.deepEqual(dial.calls[0], ["base", Math.max(...STOP_IDS.map((id) => dialPoseOf(id, g).size))]);
});

test("render at HOY at 09:00: the hand, the HOY pose, the sun born at the dial's hub, its beam clipped at the sheet, the skyline in front", () => {
  const sheet = { left: 63, right: 1177, top: 78, bottom: 1040 };
  const { stage, dial, rec, skylineCalls } = build({ sheet });
  const out = stage.render({ pos: 1, hora: 9 });
  assert.equal(out.beam.kind, "sun");
  assert.deepEqual([out.beam.beam.x, out.beam.beam.y], [out.layout.cx, out.layout.cy], "the sun is the hub of the dial, not the tip of the hand");
  assert.notDeepEqual([out.beam.beam.x, out.beam.beam.y], [out.layout.tip.x, out.layout.tip.y]);
  assert.deepEqual(dial.calls.find((c) => c[0] === "set"), ["set", 9, "09:00"]);
  assert.equal(dial.calls.find((c) => c[0] === "glow")[1].angle, 135, "the HOY mark is 09:00");
  assert.equal(out.layout.docked, false);
  assert.ok(Math.abs(out.layout.ring - 455) < 3, `ring ${out.layout.ring}`);
  const clip = rec.calls.find((c) => c[0] === "rect" && c[1] === 1177);
  assert.ok(clip, "the beam is clipped at the sheet's right edge");
  assert.equal(skylineCalls.length, 1);
  assert.deepEqual(skylineCalls[0].shift, skylineShift(1));
});

test("the beam follows the real hour, not the stop: the same hour gives the same light at HOY and in the dock", () => {
  const sheet = { left: 63, right: 1177, top: 78, bottom: 1040 };
  const { stage } = build({ sheet });
  const kinds = [];
  for (const pos of [1, 2, 3, 4, 5]) kinds.push(stage.render({ pos, hora: 0.43 }).beam.kind);
  assert.deepEqual(kinds, Array(5).fill("window"));
  assert.equal(stage.render({ pos: 2, hora: 18 }).beam.kind, "horizon");
});

test("the city's lit windows are looked up only when the light of the hour is a window's", () => {
  const sheet = { left: 63, right: 1177, top: 78, bottom: 1040 };
  const { stage, windowCalls } = build({ sheet });
  stage.render({ pos: 1, hora: 9 });
  stage.render({ pos: 1, hora: 18 });
  assert.equal(windowCalls.length, 0, "the sun and the horizon do not look at the city");
  stage.render({ pos: 1, hora: 0.43 });
  assert.equal(windowCalls.length, 1);
  assert.equal(windowCalls[0][4].minX, 1177 + 60);
});

test("the dial docks away from HOY and takes the dock pose", () => {
  const { stage } = build({ sheet: { left: 63, right: 1177, top: 78, bottom: 1040 } });
  const dock = stage.render({ pos: 3, hora: 18 }).layout;
  assert.equal(dock.docked, true);
  assert.ok(dock.size >= 92 && dock.size <= 120);
});

test("reduced motion: no pan, no ghost copies; a trip in between ghosts only when motion is on", () => {
  const sheet = { left: 63, right: 1177, top: 78, bottom: 1040 };
  const { stage, skylineCalls } = build({ sheet });
  stage.render({ pos: 1.5, hora: 9 });
  stage.render({ pos: 1.5, hora: 9, reduced: true });
  stage.render({ pos: 2, hora: 9 });
  assert.deepEqual(skylineCalls.map((c) => c.ghost), [true, false, false]);
  assert.deepEqual(skylineCalls[1].shift, [0, 0, 0]);
  assert.ok(skylineCalls[0].shift[2] > 0);
});

test("noBeam paints the same frame without the light and with the same warm band", () => {
  const sheet = { left: 63, right: 1177, top: 78, bottom: 1040 };
  const { stage, rec } = build({ sheet });
  stage.render({ pos: 1, hora: 9 });
  const withBeam = rec.calls.filter((c) => c[0] === "createRadialGradient");
  rec.calls.length = 0;
  const out = stage.render({ pos: 1, hora: 9, noBeam: true });
  const without = rec.calls.filter((c) => c[0] === "createRadialGradient");
  assert.ok(out.beam, "the beam is still worked out");
  assert.ok(withBeam.length > without.length, "the light's gradients are gone");
  assert.deepEqual(withBeam[0], without[0], "the warm band is the same");
});

test("with no sheet yet the stage uses the CSS geometry, and with a sheet of zero width as well", () => {
  for (const sheet of [undefined, null, { left: 0, right: 0, top: 0, bottom: 0 }]) {
    const { stage } = build({ sheet });
    const out = stage.render({ pos: 1, hora: 9 });
    assert.equal(Math.round(out.sheet.right), Math.round(sheetGeometry({ vw: 1920 }).sheetRight));
  }
});

test("a narrow window leaves no room for a beam and the stage still renders", () => {
  const { stage } = build({ w: 1100, h: 700 });
  const out = stage.render({ pos: 1, hora: 9 });
  assert.equal(out.beam, null);
});

test("destroy stops everything: render does nothing afterwards", () => {
  const { stage, rec } = build();
  stage.destroy();
  rec.calls.length = 0;
  assert.equal(stage.render({ pos: 1, hora: 9 }), null);
  assert.equal(rec.calls.length, 0);
});

const radials = (rec) => rec.calls.filter((c) => c[0] === "createRadialGradient");

test("a soft pool of --night-1 sits under the dial: after the beam's glow, before its rays and the sun's core", () => {
  const sheet = { left: 63, right: 1177, top: 78, bottom: 1040 };
  const { stage, rec } = build({ sheet });
  const out = stage.render({ pos: 1, hora: 9 });
  const all = radials(rec);
  const at = all.findIndex((c) => c[1] === out.layout.cx && c[2] === out.layout.cy);
  assert.equal(at, 4, "the sky's warm band, then three ellipses of glow, then the pool");
  assert.equal(all.length, 1 + 3 + 1 + 7, "and after it three rays and the four discs of the core");
  assert.deepEqual(all[at].slice(1), [out.layout.cx, out.layout.cy, 0, out.layout.cx, out.layout.cy, (out.layout.size / 2) * POOL.reach]);
  const fills = rec.calls.filter((c) => c[0] === "set:globalCompositeOperation").map((c) => c[1]);
  assert.ok(fills.includes("source-over") && fills.includes("lighter"));
});

test("the pool is part of the frame even when there is no light, and it follows the dial's pose", () => {
  const { stage, rec } = build({ sheet: { left: 63, right: 1177, top: 78, bottom: 1040 } });
  const hero = stage.render({ pos: 1, hora: 9, noBeam: true });
  assert.equal(radials(rec).length, 2, "the warm band and the pool");
  rec.calls.length = 0;
  const dock = stage.render({ pos: 3, hora: 13.6, noBeam: true }).layout;
  const pool = radials(rec).find((c) => c[1] === dock.cx);
  assert.ok(pool, "the pool moves with the dial");
  assert.ok(pool[6] < 100 && pool[6] > 40, `a docked dial gets a small pool (${pool[6]} px)`);
  assert.ok(hero.layout.size > dock.size);
});

test("the pool is cut at the sheet's edge like the light: the wide rim of the sunk dial of HOY does not tint the skyline under the sheet, and mid travel it is whole", () => {
  const sheet = { left: 63, right: 1177, top: 78, bottom: 1040 };
  const { stage, rec } = build({ sheet });
  const fillOf = (out) => {
    const at = rec.calls.findIndex((c) => c[0] === "createRadialGradient" && c[1] === out.layout.cx && c[2] === out.layout.cy);
    return rec.calls.slice(at).find((c) => c[0] === "fillRect");
  };
  const hero = stage.render({ pos: 1, hora: 22, noBeam: true });
  const r = (hero.layout.size / 2) * POOL.reach;
  assert.ok(hero.layout.cx - r < 1177, "the pool of the hero dial reaches left of the sheet's edge (the premise)");
  const [, x, , w] = fillOf(hero);
  assert.equal(x, 1177, "on a stop the pool starts at the sheet's right edge");
  assert.ok(Math.abs(x + w - (hero.layout.cx + r)) < 1e-6, "and ends where it did");
  rec.calls.length = 0;
  const mid = stage.render({ pos: 1.5, hora: 22, noBeam: true });
  const [, x2, , w2] = fillOf(mid);
  assert.ok(Math.abs(x2 - Math.max(mid.layout.cx - (mid.layout.size / 2) * POOL.reach, 63)) < 1e-6 && w2 > 0, "mid travel the sheet is gone and the cut is at its left side");
});

test("the pool's colour is the --night-1 token, written once in tokens.css", () => {
  const t = tokensOf(":root {\n  color-scheme: dark;");
  assert.deepEqual(POOL.rgb, rgb(t["--night-1"]), "POOL.rgb is --night-1, written once in tokens.css");
  assert.ok(POOL.alpha > 0.5 && POOL.alpha < 1 && POOL.solid > 0.4 && POOL.solid < 1 && POOL.reach > 1);
});

import { poolAlpha } from "../src/stage/stage.js";

test("B19: the pool is as dark as POOL.alpha at night, where it cannot be seen, and a lighter shadow under the pale sky of the day", () => {
  for (const hora of [0, 3, 6, 20, 22]) assert.ok(Math.abs(poolAlpha(hora) - POOL.alpha) < 1e-9, `night ${hora}: ${poolAlpha(hora)}`);
  for (const hora of [10, 12, 13.5, 16]) assert.ok(Math.abs(poolAlpha(hora) - POOL.alpha * POOL.day) < 1e-9, `day ${hora}: ${poolAlpha(hora)}`);
  assert.ok(POOL.day > 0 && POOL.day < 0.8, "the day pool is a fraction of the night one");
  let prev = poolAlpha(5);
  for (let hora = 5; hora <= 10; hora += 0.25) { assert.ok(poolAlpha(hora) <= prev + 1e-9, `no jump on the way into the day (${hora})`); prev = poolAlpha(hora); }
  // and the frame really paints the pool with that alpha: the stops of the pool's gradient (the one centred on the dial) say how dark it is
  for (const hora of [13, 22]) {
    const stops = [];
    const rec = recorder();
    const ctx = new Proxy({}, {
      get: (_, key) => (...args) => { rec.calls.push([key, ...args]); return String(key).startsWith("create") ? { addColorStop: (at, color) => stops.push([rec.calls.length, color]) } : undefined; },
      set: () => true,
    });
    const { stage } = build({ rec: { ...rec, ctx }, sheet: { left: 63, right: 1177, top: 78, bottom: 1040 } });
    const out = stage.render({ pos: 1, hora, noBeam: true });
    const at = rec.calls.findIndex((c) => c[0] === "createRadialGradient" && c[1] === out.layout.cx && c[2] === out.layout.cy);
    const mine = stops.filter(([n]) => n === at + 1).map(([, c]) => c);
    assert.ok(mine.length >= 2, `the pool's stops were seen (${mine.length})`);
    const alpha = Number(/rgba\(\d+,\d+,\d+,([\d.]+)\)/.exec(mine[0])[1]);
    assert.ok(Math.abs(alpha - poolAlpha(hora)) < 0.001, `hora ${hora}: the pool is painted at ${alpha}, poolAlpha says ${poolAlpha(hora)}`);
  }
});

const clipX = (rec) => rec.calls.find((c) => c[0] === "rect" && c[2] === -2)[1];

test("the light is cut at the sheet's right edge on a stop, and while the camera travels the cut follows the sheet's fade back to its left side", () => {
  const sheet = { left: 63, right: 1177, top: 78, bottom: 1040 };
  const { stage, rec } = build({ sheet });
  const at = (pos, extra = {}) => { rec.calls.length = 0; const out = stage.render({ pos, hora: 9, ...extra }); return { x: clipX(rec), out }; };
  assert.equal(at(1).x, 1177, "A17 on a stop");
  assert.equal(at(1.04).x, 1177, "the sheet is still solid within 0.04 of a stop");
  const mid = at(1.1).x;
  assert.ok(mid < 1177 && mid > 63, `the sheet is half gone and the cut is at ${mid}`);
  assert.equal(at(1.5).x, 63, "with no sheet the light has nothing to be cut by");
  assert.equal(at(1.5, { reduced: true }).x, 1177, "under reduced motion the sheet never fades, so the cut stays");
  assert.deepEqual([at(1.5).out.sheet.left, at(1.5).out.sheet.right], [63, 1177], "the sheet reported is the sheet's, not the light's edge");
  assert.ok(at(1.5).out.beam, "and the beam is still there, longer, all the way to the gutter");
});

test("the holes of the frame (the drawer) are painted around, not in: the light is clipped outside them", () => {
  const holes = [{ left: 1300, top: 78, right: 1700, bottom: 1040 }];
  const { stage, rec } = build({ sheet: { left: 63, right: 1177, top: 78, bottom: 1040 }, getHoles: () => holes });
  const out = stage.render({ pos: 1, hora: 9 });
  assert.deepEqual(out.holes, holes);
  assert.ok(rec.calls.some((c) => c[0] === "rect" && c[1] === 1300 && c[2] === 78 && c[3] === 400 && c[4] === 962));
  assert.ok(rec.calls.some((c) => c[0] === "clip" && c[1] === "evenodd"));
  const none = build({ sheet: { left: 63, right: 1177, top: 78, bottom: 1040 }, getHoles: () => [] });
  assert.ok(!none.rec.calls.some((c) => c[0] === "clip" && c[1] === "evenodd"));
});

test("by default the hole is the drawer: #detail while it is open, and it is painted again when the drawer opens or closes", () => {
  let open = false;
  const rect = { left: 1262, top: 78, right: 1682, bottom: 1054, width: 420, height: 976 };
  const drawer = { classList: { contains: (c) => c === "open" && open }, getBoundingClientRect: () => rect };
  const doc = { getElementById: (id) => (id === "detail" ? drawer : null) };
  const observers = [];
  globalThis.MutationObserver = class { constructor(fn) { this.fn = fn; this.disconnected = false; observers.push(this); } observe(el, opts) { this.el = el; this.opts = opts; } disconnect() { this.disconnected = true; } };
  try {
    const { stage, rec } = build({ sheet: { left: 63, right: 1177, top: 78, bottom: 1040 }, ownerDocument: doc });
    assert.deepEqual(stage.render({ pos: 1, hora: 9 }).holes, [], "closed: no hole");
    assert.equal(observers.length, 1);
    assert.deepEqual([observers[0].el === drawer, observers[0].opts], [true, { attributes: true, attributeFilter: ["class"] }]);
    open = true;
    rec.calls.length = 0;
    observers[0].fn();
    assert.ok(rec.calls.some((c) => c[0] === "rect" && c[1] === 1262 && c[3] === 420), "the open drawer is a hole, painted at once, with no scroll or tick to ask for it");
    open = false;
    rec.calls.length = 0;
    observers[0].fn();
    assert.ok(!rec.calls.some((c) => c[0] === "clip" && c[1] === "evenodd"), "and closed it is not");
    stage.destroy();
    assert.equal(observers[0].disconnected, true);
    rec.calls.length = 0;
    observers[0].fn();
    assert.equal(rec.calls.length, 0, "a destroyed stage paints nothing");
  } finally { delete globalThis.MutationObserver; }
});

test("the near layers of the city go on a canvas of their own over the dial, and the stage takes it away with itself", () => {
  const rec = recorder(), frontRec = recorder();
  const added = [], removed = [];
  const front = { className: "", attrs: {}, width: 0, height: 0, setAttribute(k, v) { this.attrs[k] = v; }, getContext: () => frontRec.ctx, remove() { removed.push(this); } };
  const doc = { createElement: (tag) => { assert.equal(tag, "canvas"); return front; } };
  const canvas = { clientWidth: 1920, clientHeight: 1080, width: 0, height: 0, getContext: () => rec.ctx, ownerDocument: doc };
  const dialHost = { after: (el) => added.push(el) };
  const calls = [];
  const dial = { base() {}, set() {}, glow() {}, layout: (pose, hora) => layoutOf(pose, hora, 600) };
  const skyline = { draw: (g, args) => calls.push([g === frontRec.ctx ? "front" : "sky", args.from, args.to]), litWindows: () => [], layers: [] };
  const stage = createStage({ canvas, dialHost, t: (k) => k, getSheetRect: () => ({ left: 63, right: 1177, top: 78, bottom: 1040 }), getHoles: () => [], rand: mulberry32(1), parts: { dial, skyline, dpr: () => 1 } });
  assert.deepEqual([added, front.className, front.attrs], [[front], "sky-front", { "aria-hidden": "true" }], "inserted right after the dial: over it, under the shell");
  stage.resize();
  assert.deepEqual([front.width, front.height], [1920, 1080]);
  stage.render({ pos: 1, hora: 9 });
  assert.deepEqual(calls, [["sky", undefined, FRONT_FROM], ["front", FRONT_FROM, undefined]]);
  assert.equal(FRONT_FROM, 1, "the mid and near layers stand in front of the dial");
  stage.destroy();
  assert.deepEqual(removed, [front]);
});
