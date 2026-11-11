// What the sheet shows while the camera travels (spec §3.3: "Salto largo: directo, sin poses intermedias"). The camera used to read the sheet from the
// position alone, so a trip from HOY to SISTEMA mounted the moment of PROCESOS, of COLA and of APRENDER on the way, each one built and thrown away
// inside a frame: at CPU x4 that was 175-320 ms a stop, and the reason a trip of 420 ms took 600-1300 (ACEPTACION A3 and A7).
import test from "node:test";
import assert from "node:assert/strict";
import { sheetIndex, sheetOpacity } from "../src/scene/poses.js";
import { sheetView } from "../src/scene/sheetview.js";

const FLOOR = 1000;
const trip = (fromStop, toStop) => ({ from: fromStop * FLOOR, to: toStop * FLOOR, t0: 0, ms: 800, id: "x", push: true });
const walk = (job, steps = 400) => Array.from({ length: steps + 1 }, (_, k) => {
  const y = job.from + (job.to - job.from) * (k / steps);
  return { y, pos: y / FLOOR, ...sheetView({ pos: y / FLOOR, job, floorHeight: FLOOR }) };
});

test("with no trip, or a glide, or a trip of one floor, the sheet is what the position says (poses.js), exactly", () => {
  for (const job of [null, { ...trip(1, 2), snap: true }, trip(1, 2), trip(3, 2), trip(2.3, 3)]) {
    for (let pos = 0; pos <= 5; pos += 0.037) {
      const got = sheetView({ pos, job, floorHeight: FLOOR });
      assert.deepEqual(got, { index: sheetIndex(pos), opacity: sheetOpacity(pos) }, `pos ${pos.toFixed(3)}`);
    }
  }
  assert.deepEqual(sheetView({ pos: 1.4, job: trip(1, 4), floorHeight: FLOOR, reduced: true }), { index: 1, opacity: 1 }, "calm motion: solid wherever it rests");
});

test("a long trip shows the stop it leaves, then the stop it goes to, and never the ones in between", () => {
  for (const [a, b] of [[1, 5], [1, 4], [2, 5], [5, 1], [4, 1], [0, 3], [3, 0]]) {
    const frames = walk(trip(a, b));
    const shown = [...new Set(frames.map((f) => f.index))];
    assert.deepEqual(shown.sort(), [a, b].sort(), `${a} to ${b} showed ${shown}`);
    const switched = frames.findIndex((f) => f.index === b);
    assert.ok(frames.slice(0, switched).every((f) => f.index === a) && frames.slice(switched).every((f) => f.index === b), "one swap, in the middle");
    assert.equal(frames[0].opacity, 1);
    assert.equal(frames.at(-1).opacity, 1);
    for (const f of frames) if (Math.abs(f.pos - a) > 0.5 && Math.abs(f.pos - b) > 0.5) assert.equal(f.opacity, 0, `invisible in the middle (pos ${f.pos.toFixed(2)})`);
  }
});

test("a long trip fades out and in like a trip of one floor does at each end (the same curve of poses.js, in floors from the stop)", () => {
  for (const f of walk(trip(1, 5), 800)) {
    const d = Math.min(Math.abs(f.pos - 1), Math.abs(f.pos - 5));
    if (d < 0.5) assert.equal(f.opacity, sheetOpacity(d), `pos ${f.pos.toFixed(3)}`);
  }
});

test("a trip that starts between two stops starts as dark as the page was, and does not light the sheet it leaves", () => {
  const job = { from: 1300, to: 5000, t0: 0, ms: 800, id: "sistema", push: true };
  const first = sheetView({ pos: 1.3, job, floorHeight: FLOOR });
  assert.equal(first.opacity, sheetOpacity(1.3));
  assert.equal(first.index, 1);
  const frames = Array.from({ length: 200 }, (_, k) => sheetView({ pos: 1.3 + 3.7 * k / 199, job, floorHeight: FLOOR }));
  assert.deepEqual([...new Set(frames.map((f) => f.index))].sort(), [1, 5]);
});
