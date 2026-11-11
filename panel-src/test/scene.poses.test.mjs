import test from "node:test";
import assert from "node:assert/strict";
import { createSkyline } from "../src/stage/skyline.js";
import { FRONT_FROM } from "../src/stage/stage.js";
import { STOP_IDS, MARKS, PAN_PX, PARALLAX, HERO_SUNK, HERO_MID, WIDE_MIN, smoothstep, sheetOpacity, sheetIndex, skylineShift, shortestDelta, sheetGeometry, dialPoseOf, dialPoseAt, markGlow } from "../src/scene/poses.js";

const near = (a, b, eps = 0.5) => assert.ok(Math.abs(a - b) <= eps, `${a} vs ${b}`);
const geo = (vw, vh) => { const s = sheetGeometry({ vw }); return { vw, vh, ...s }; };

test("six stops and the marks of §2.4, at least 30 degrees apart on the ring", () => {
  assert.deepEqual(STOP_IDS, ["prologo", "hoy", "procesos", "cola", "aprender", "sistema"]);
  assert.deepEqual(MARKS, { prologo: 3.25, hoy: 9, procesos: 26 / 60, cola: 18, aprender: 21, sistema: 5.5 });
  const hours = STOP_IDS.map((id) => MARKS[id]);
  let min = 360;
  for (let i = 0; i < hours.length; i++) for (let j = i + 1; j < hours.length; j++) min = Math.min(min, Math.abs(shortestDelta(hours[i], hours[j])) * 15);
  assert.ok(min >= 30 && min < 34, `closest pair is ${min} degrees`);
});

test("sheetGeometry is the CSS clamp(): 1113 px at 1920, 1400 at 3440, 960 at the floor", () => {
  near(sheetGeometry({ vw: 1920 }).sheet, 1113.6); near(sheetGeometry({ vw: 1920 }).sheetRight, 63.36 + 1113.6);
  assert.equal(sheetGeometry({ vw: 3440 }).sheet, 1400); assert.equal(sheetGeometry({ vw: 3440 }).gutter, 64);
  near(sheetGeometry({ vw: 3440 }).sheetRight, 1464);
  assert.equal(sheetGeometry({ vw: 1024 }).sheet, 960);
});

test("sheetOpacity is 1 on a stop, 0 between two, symmetric and monotonic", () => {
  assert.equal(sheetOpacity(0), 1); assert.equal(sheetOpacity(3), 1); assert.equal(sheetOpacity(2.04), 1);
  assert.equal(sheetOpacity(0.5), 0); assert.equal(sheetOpacity(2.3), 0); assert.equal(sheetOpacity(2.7), 0);
  let prev = 2;
  for (let u = 0; u <= 0.5; u += 0.02) { const v = sheetOpacity(1 + u); assert.ok(v <= prev + 1e-12); prev = v; }
  near(sheetOpacity(1.1), sheetOpacity(0.9), 1e-9);
});

test("sheetIndex rounds to the nearest stop and clamps", () => {
  assert.deepEqual([-0.4, 0, 0.49, 0.5, 1.5, 4.51, 5, 7].map(sheetIndex), [0, 0, 0, 1, 2, 5, 5, 5]);
});

test("the skyline pans 600 px per stop at 0.04 / 0.10 / 0.20, and not at all under reduced motion", () => {
  assert.equal(PAN_PX, 600); assert.deepEqual(PARALLAX, [0.04, 0.1, 0.2]);
  assert.deepEqual(skylineShift(1).map((v) => Math.round(v * 100) / 100), [24, 60, 120]);
  assert.deepEqual(skylineShift(0), [0, 0, 0]);
  assert.deepEqual(skylineShift(3.5, true), [0, 0, 0]);
});

test("shortestDelta takes the short way round the ring and 12 h is +12", () => {
  assert.equal(shortestDelta(9, 26 / 60), 26 / 60 - 9);
  near(shortestDelta(9, 18), 9, 1e-9);
  assert.equal(shortestDelta(0, 12), 12);
  near(shortestDelta(23, 1), 2, 1e-9);
  near(shortestDelta(1, 23), -2, 1e-9);
});

test("the HOY dial is the Relay hero: about 460 px of ring at 1920 and 630 at 3440, right of the sheet, sunk into the city", () => {
  const a = dialPoseOf("hoy", geo(1920, 1080));
  near(a.size, 625.2, 1); near(a.size * 0.728, 455, 6); assert.equal(a.docked, false);
  near(a.cy, HERO_SUNK * 1080, 0.01); near(a.cx, 1176.96 + 679.68 / 2, 1);
  const b = dialPoseOf("hoy", geo(3440, 1440));
  near(b.size, 864, 1); near(b.size * 0.728, 629, 6); near(b.cy, HERO_SUNK * 1440, 0.01);
  // the sunk dial keeps its time off the roofs: the digits end 0.21 of the box under the centre and the highest roof of the city is at 0.838 of the height
  for (const [w, h] of [[1366, 768], [1440, 900], [1920, 1080], [3440, 1440]]) {
    const p = dialPoseOf("hoy", geo(w, h));
    assert.ok(p.cy + 0.21 * p.size < 0.838 * h - 12, `${w}x${h}: the digits end at ${Math.round(p.cy + 0.21 * p.size)}, the roofs begin at ${Math.round(0.838 * h)}`);
  }
});

test("the sunk dial of HOY keeps its numerals 09 and 15 above the buildings in front of it: a label behind a roof is a label nobody reads (A18 sees no ink there)", () => {
  const sky = createSkyline();
  for (const [w, h] of [[1366, 768], [1440, 900], [1920, 1080], [2560, 1440], [3440, 1440]]) {
    const p = dialPoseOf("hoy", geo(w, h));
    const g = sky.geometry(w, h, skylineShift(1));                  // HOY is the second stop: the skyline is panned by one stop
    const roofIn = (x0, x1) => {
      let top = Infinity;
      for (const L of g.layers.slice(FRONT_FROM)) for (const b of L.buildings) {
        const bx = b.x * L.scale - L.offset;
        if (bx + b.w * L.scale >= x0 && bx <= x1) top = Math.min(top, g.baseY - b.h * L.scale);
      }
      return top;
    };
    // the numerals sit 304 units from the hub (R_RING + 42) and their baseline 5 units lower, in a box of 720 units
    for (const [name, dx] of [["15", -215], ["09", 215]]) {
      const x = p.cx + (dx / 720) * p.size, baseline = p.cy + (220 / 720) * p.size;
      assert.ok(baseline + 4 <= roofIn(x - 14, x + 14), `${w}x${h}: the ${name} ends at ${Math.round(baseline)}, a roof begins at ${Math.round(roofIn(x - 14, x + 14))}`);
    }
  }
});

test("the prologue's big dial keeps its hour and 'hora local' above the buildings in front of it (2026-10-24: at 0.62 the roofs cut the 22:00 and hid the caption)", () => {
  const sky = createSkyline();
  for (const [w, h] of [[1366, 768], [1440, 900], [1920, 1080], [2560, 1440], [3440, 1440]]) {
    const p = dialPoseOf("prologo", geo(w, h));
    const g = sky.geometry(w, h, skylineShift(0));                  // the prologue is the first stop: the skyline is not panned
    const roofIn = (x0, x1) => {
      let top = Infinity;
      for (const L of g.layers.slice(FRONT_FROM)) for (const b of L.buildings) {
        const bx = b.x * L.scale - L.offset;
        if (bx + b.w * L.scale >= x0 && bx <= x1) top = Math.min(top, g.baseY - b.h * L.scale);
      }
      return top;
    };
    // in the box of 720 units the hour's baseline is 146 under the hub and about 160 wide each way; the caption's is 180 under it, 70 each way
    for (const [name, down, half] of [["the hour", 146, 160], ["the caption", 180, 70]]) {
      const baseline = p.cy + (down / 720) * p.size, half_px = (half / 720) * p.size;
      const roof = roofIn(p.cx - half_px, p.cx + half_px);
      assert.ok(baseline + 4 <= roof, `${w}x${h}: ${name} ends at ${Math.round(baseline)}, a roof begins at ${Math.round(roof)}`);
    }
  }
});

test("prologue hero and the dock of the other stops", () => {
  const p = dialPoseOf("prologo", geo(1920, 1080));
  near(p.size, Math.min(0.94 * 1080, 0.54 * 1920)); near(p.cx, 0.72 * 1920); near(p.cy, 0.55 * 1080); assert.equal(p.docked, false);
  const d = dialPoseOf("cola", geo(1920, 1080));
  near(d.size, 115.2); assert.equal(d.docked, true);
  near(d.cx, 1920 - 63.36 - d.size / 2); near(d.cy, 56 + 14 + d.size / 2);
  assert.equal(dialPoseOf("procesos", geo(1000, 700)).size, 92);
  assert.equal(dialPoseOf("cola", geo(3440, 1440)).docked, true, "COLA keeps the dock at any width: the capture is in that sky");
});

test("SISTEMA keeps the hero dial (relay2), PROCESOS and APRENDER keep it from a very wide window on, and where there is no room it docks", () => {
  for (const [w, h] of [[1366, 768], [1920, 1080], [3440, 1440]]) {
    const hoy = dialPoseOf("hoy", geo(w, h)), s = dialPoseOf("sistema", geo(w, h));
    assert.equal(s.docked, false, `SISTEMA at ${w}`);
    near(s.size, hoy.size, 1e-9); near(s.cx, hoy.cx, 1e-9); near(s.cy, HERO_MID * h, 1e-9);
  }
  for (const id of ["procesos", "aprender"]) {
    assert.equal(dialPoseOf(id, geo(1920, 1080)).docked, true, `${id} at 1920 docks: the drawer opens in that sky`);
    assert.equal(dialPoseOf(id, geo(WIDE_MIN - 1, 1440)).docked, true, `${id} just under ${WIDE_MIN}`);
    const wide = dialPoseOf(id, geo(3440, 1440));
    assert.equal(wide.docked, false, `${id} at 3440`);
    near(wide.cy, HERO_MID * 1440, 1e-9); near(wide.size, 864, 1);
    assert.ok(wide.cx - wide.size / 2 > 1464 + 420 + 22, "right of the sheet and of the drawer, which opens right of the sheet");
  }
  for (const vw of [760, 1024, 1280]) assert.equal(dialPoseOf("sistema", geo(vw, 768)).docked, true, `no room right of the sheet at ${vw}`);
});

test("the dial travels between any two stops without a jump: its centre, its size and the sheet's edge keep to a small step", () => {
  for (const [w, h] of [[1366, 768], [1920, 1080], [3440, 1440]]) {
    const g = geo(w, h);
    let prev = dialPoseAt(0, g);
    for (let pos = 0.01; pos <= 5.0001; pos += 0.01) {
      const p = dialPoseAt(pos, g);
      const step = Math.hypot(p.cx - prev.cx, p.cy - prev.cy), grow = Math.abs(p.size - prev.size);
      assert.ok(step < 0.05 * w && grow < 0.05 * w, `${w}x${h} at ${pos.toFixed(2)}: the dial moved ${step.toFixed(1)} px and grew ${grow.toFixed(1)} px in one hundredth of a stop`);
      prev = p;
    }
  }
});

test("on a narrow desktop (760-1300 px) HOY docks the dial because there is no scene right of the sheet", () => {
  for (const vw of [760, 1024, 1280]) {
    const pose = dialPoseOf("hoy", geo(vw, 768));
    assert.equal(pose.docked, true, `vw ${vw}`);
    assert.ok(pose.cx + pose.size / 2 <= vw, "the dial stays inside the window");
  }
  assert.equal(dialPoseOf("hoy", geo(1920, 1080)).docked, false);
});

test("dialPoseAt blends neighbouring stops and is exact on a stop", () => {
  const g = geo(1920, 1080);
  const on = dialPoseAt(1, g), hoy = dialPoseOf("hoy", g);
  near(on.size, hoy.size, 1e-6); near(on.cx, hoy.cx, 1e-6);
  const mid = dialPoseAt(1.5, g), dock = dialPoseOf("procesos", g);
  assert.ok(mid.size < hoy.size && mid.size > dock.size);
  near(mid.size, (hoy.size + dock.size) / 2, 1e-6);
  assert.equal(dialPoseAt(-2, g).size, dialPoseOf("prologo", g).size);
  assert.equal(dialPoseAt(9, g).size, dialPoseOf("sistema", g).size);
});

test("markGlow sits on the active mark and travels the short way between two", () => {
  near(markGlow(1).angle, 9 * 15, 1e-9); assert.equal(markGlow(1).strength, 1);
  near(markGlow(0).angle, 3.25 * 15, 1e-9);
  const mid = markGlow(1.5);             // 09:00 -> 00:26: 8.57 h backwards, so the glow passes 04:43, not 16:43
  near(mid.angle, (((9 + shortestDelta(9, 26 / 60) * 0.5) % 24) + 24) % 24 * 15, 1e-9);
  assert.ok(mid.strength < 1 && mid.strength > 0.4);
  const across = markGlow(3.5);          // 18:00 -> 21:00
  near(across.angle, 19.5 * 15, 1e-9);
  assert.ok(markGlow(2.9999).strength > 0.99);
});

test("under reduced motion the sheet is never faded: a page that rests between two stops (the browser does not snap it) stays legible", () => {
  for (const pos of [0, 0.3, 0.5, 1.5, 2.28, 4.72, 5]) assert.equal(sheetOpacity(pos, true), 1, `pos ${pos}`);
  assert.equal(sheetOpacity(1.5, false), 0, "without reduced motion the fade is as before");
  assert.equal(sheetOpacity(1.5), 0);
});
