import test from "node:test";
import assert from "node:assert/strict";
import { CENTER, R_RING, angleOf, polar, arcPath, sectorPath, dialSpec, railAt, tipOf, layoutOf } from "../src/stage/dial.js";
import { dialPoseOf, dialPoseAt, sheetGeometry, MARKS, STOP_IDS } from "../src/scene/poses.js";

const near = (a, b, eps = 0.01) => assert.ok(Math.abs(a - b) <= eps, `${a} is not within ${eps} of ${b}`);
const geoOf = (vw, vh) => { const { gutter, sheetRight } = sheetGeometry({ vw }); return { vw, vh, sheetRight, gutter }; };

test("midnight is at the top and the hours run clockwise", () => {
  assert.deepEqual([0, 6, 12, 18, 24, -3].map(angleOf), [0, 90, 180, 270, 0, 315]);
  const [x0, y0] = polar(R_RING, 0), [x1, y1] = polar(R_RING, 90), [x2, y2] = polar(R_RING, 180);
  near(x0, CENTER); near(y0, CENTER - R_RING);
  near(x1, CENTER + R_RING); near(y1, CENTER);
  near(x2, CENTER); near(y2, CENTER + R_RING);
});

test("arcs and sectors are SVG paths, with the large-arc flag only past half a turn", () => {
  assert.match(arcPath(R_RING, 270, 495), /^M38\.00 300\.00 A262 262 0 1 1 /);
  assert.match(arcPath(R_RING, 0, 90), /^M300\.00 38\.00 A262 262 0 0 1 562\.00 300\.00$/);
  const s = sectorPath(210, 262, 135, 270);
  assert.match(s, /^M[\d. ]+A262 262 0 0 1 [\d. ]+L[\d. ]+A210 210 0 0 0 [\d. ]+Z$/);
});

test("the spec of the dial: 24 hour ticks, 45 quarter ticks inside the window, 8 numerals, 16 runs, 6 marks", () => {
  const s = dialSpec();
  const hours = s.ticks.filter((t) => !t.quarter), quarters = s.ticks.filter((t) => t.quarter);
  assert.equal(hours.length, 24);
  assert.equal(quarters.length, 45);
  assert.equal(hours.filter((t) => t.win).length, 15, "18:00 to 08:00 inclusive");
  assert.ok(quarters.every((t) => t.win));
  assert.deepEqual(hours.filter((t) => t.major).map((t) => t.hh), [0, 3, 6, 9, 12, 15, 18, 21]);
  assert.deepEqual(s.numerals.map((n) => n.text), ["00", "03", "06", "09", "12", "15", "18", "21"]);
  assert.equal(s.runs.length, 16);
  assert.deepEqual(s.runs.filter((r) => r.big).map((r) => r.k), [0, 6, 12]);
  assert.deepEqual(s.marks.map((m) => m.id), STOP_IDS);
  for (const m of s.marks) near(m.angle, angleOf(MARKS[m.id]), 1e-9);
  assert.equal(s.rail, arcPath(R_RING, 270, 495));
  assert.match(s.busy, /^M/);
  assert.match(s.busyLabel, /^M/);
});

test("the tick marks never leave the viewBox (-60..660) so nothing is cut at any size", () => {
  const s = dialSpec();
  for (const t of s.ticks) for (const v of [t.x1, t.y1, t.x2, t.y2]) assert.ok(v > -60 && v < 660, `tick at ${v}`);
  for (const n of s.numerals) assert.ok(n.x > -60 && n.x < 660 && n.y > -60 && n.y < 660);
});

test("railAt: empty at 18:00, lit as the night goes, full from 09:00 until 18:00 starts a new one", () => {
  const start = railAt(18);
  assert.equal(start.lit, "");
  assert.equal(start.sweep, "");
  assert.deepEqual([start.nightT, start.passed, start.now, start.hand], [0, 0, 0, 270]);
  const late = railAt(22);
  assert.equal(late.nightT, 4);
  assert.equal(late.lit, arcPath(R_RING, 270, 330));
  assert.deepEqual([late.passed, late.now, late.hand], [4, 4, 330]);
  assert.match(late.sweep, /^M/);
  const small = railAt(0.43);
  near(small.nightT, 6.43, 1e-9);
  assert.deepEqual([small.passed, small.now], [6, 6]);
  for (const hora of [9, 12, 17.9]) {
    const day = railAt(hora);
    assert.equal(day.nightT, 15, `at ${hora} the night is complete`);
    assert.equal(day.lit, arcPath(R_RING, 270, 495));
    assert.equal(day.sweep, "", "no sweep outside the window");
    assert.equal(day.now, -1, "no run is 'now' while Gonza is busy");
    assert.equal(day.passed, 15);
  }
  assert.equal(railAt(8.99).now, 14);
  assert.equal(railAt(9).now, -1);
});

test("the tip of the hand in window pixels: centred on the box, mapped from the viewBox", () => {
  // 09:00 is 135 degrees; the tip sits 6 units inside the ring
  const t9 = tipOf(1000, 200, 600, 9);
  const [tx, ty] = polar(R_RING - 6, 135);
  near(t9.x, 1000 + ((tx + 60) / 720) * 600);
  near(t9.y, 200 + ((ty + 60) / 720) * 600);
  const t6 = tipOf(1000, 200, 720, 6);                       // 90 degrees: level with the centre, 556 units from the left
  near(t6.y, 200 + 360);
  near(t6.x, 1000 + 556 + 60);
});

test("layoutOf scales the box DOWN from the base, keeps the centre, and flags the dock", () => {
  const g = geoOf(1920, 1080);
  const base = Math.max(...STOP_IDS.map((id) => dialPoseOf(id, g).size));
  const hoy = layoutOf(dialPoseOf("hoy", g), 9, base);
  near(hoy.scale, hoy.size / base, 1e-12);
  assert.ok(hoy.scale < 1 && hoy.scale > 0);
  near(hoy.x + hoy.size / 2, hoy.cx, 1e-9);
  near(hoy.y + hoy.size / 2, hoy.cy, 1e-9);
  assert.equal(hoy.docked, false);
  assert.deepEqual(layoutOf(dialPoseOf("hoy", g), 9, base), hoy, "a pure function");
  const dock = layoutOf(dialPoseOf("cola", g), 18, base);
  assert.equal(dock.docked, true);
  assert.ok(dock.scale < hoy.scale);
  for (const pos of [0, 0.5, 1, 1.5, 3, 4.5, 5]) assert.ok(layoutOf(dialPoseAt(pos, g), 9, base).scale <= 1 + 1e-9, `never upscaled at ${pos}`);
});

test("A13: the ring of the HOY dial is about 455 px at 1920x1080 and about 629 px at 3440x1440; docked it is the dock's 73 %", () => {
  for (const [vw, vh, ring, tol] of [[1920, 1080, 455, 3], [3440, 1440, 629, 3]]) {
    const g = geoOf(vw, vh);
    const base = Math.max(...STOP_IDS.map((id) => dialPoseOf(id, g).size));
    const hoy = layoutOf(dialPoseOf("hoy", g), 9, base);
    near(hoy.ring, ring, tol);
    near(hoy.ring / hoy.size, (2 * R_RING) / 720, 1e-9);
    const dock = layoutOf(dialPoseOf("sistema", g), 5.5, base);
    near(dock.ring / dock.size, (2 * R_RING) / 720, 1e-9);
  }
});

import { detailOf, LOD_MIN_PX, VIEW } from "../src/stage/dial.js";

test("detailOf: the level of detail follows the pixels a unit of the SVG gets, and no tier draws a text under 11 px", () => {
  // The texts of the dial in SVG units: the micro labels are 13, the numerals 15 and, in the sparse tiers, 22 and 34 (--fs-4 and --fs-5).
  const sizeOf = { full: { micro: 13, num: 15 }, mid: { num: 15 }, compact: { num: 22 }, tiny: { num: 34 }, bare: {} };
  assert.equal(LOD_MIN_PX, 11);
  assert.deepEqual([0.9, 0.847, 0.8, 0.74, 0.6, 0.5, 0.404, 0.33, 0.3, 0.13].map(detailOf), ["full", "full", "mid", "mid", "compact", "compact", "tiny", "tiny", "bare", "bare"]);
  for (let k = 0.1; k <= 1.3; k += 0.005) {
    const tier = sizeOf[detailOf(k)];
    for (const units of Object.values(tier)) assert.ok(units * k >= LOD_MIN_PX - 1e-9, `k=${k.toFixed(3)} (${detailOf(k)}): ${units} units are ${(units * k).toFixed(2)} px`);
  }
  assert.equal(VIEW.size, 720);
});
