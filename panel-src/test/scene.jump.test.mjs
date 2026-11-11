import test from "node:test";
import assert from "node:assert/strict";
import { ADJACENT_MS, STEP_MS, MAX_MS, jumpDuration, easeInOutQuart, planJump, stepJump } from "../src/scene/jump.js";

test("durations: 420 ms one floor, +120 per floor, never over 800", () => {
  assert.deepEqual([ADJACENT_MS, STEP_MS, MAX_MS], [420, 120, 800]);
  assert.deepEqual([0.2, 1, 2, 3, 4, 5, 9].map(jumpDuration), [420, 420, 540, 660, 780, 800, 800]);
});
test("the curve is 0 at 0, 1 at 1, symmetric, monotonic", () => {
  assert.equal(easeInOutQuart(0), 0); assert.equal(easeInOutQuart(1), 1); assert.equal(easeInOutQuart(0.5), 0.5);
  let prev = -1;
  for (let u = 0; u <= 1.0001; u += 0.05) { const v = easeInOutQuart(u); assert.ok(v >= prev); prev = v; }
  assert.ok(Math.abs(easeInOutQuart(0.3) + easeInOutQuart(0.7) - 1) < 1e-12);
});
test("planJump: null when there, a cut under reduced motion, a timed job otherwise", () => {
  assert.equal(planJump({ from: 1000, to: 1000.2, floorHeight: 1000 }), null);
  assert.deepEqual(planJump({ from: 0, to: 3000, floorHeight: 1000, reduced: true }), { cut: true, to: 3000 });
  const job = planJump({ from: 0, to: 3000, floorHeight: 1000, now: 50 });
  assert.deepEqual(job, { cut: false, from: 0, to: 3000, t0: 50, ms: 660 });
  assert.equal(stepJump(job, 50).y, 0);
  assert.equal(stepJump(job, 50 + 660).done, true);
  assert.equal(stepJump(job, 50 + 660).y, 3000);
  assert.equal(stepJump(job, 50 + 5000).y, 3000);
});

import { SNAP_MAX_MS, planSnap } from "../src/scene/jump.js";

test("planSnap: null when already there, a job of at most 400 ms otherwise, shorter for a shorter way, never a cut", () => {
  assert.equal(planSnap({ from: 2000.3, to: 2000, floorHeight: 1000 }), null);
  assert.equal(SNAP_MAX_MS, 400);
  const far = planSnap({ from: 2500, to: 2000, floorHeight: 1000, now: 80 });
  const near = planSnap({ from: 2080, to: 2000, floorHeight: 1000, now: 80 });
  assert.deepEqual({ cut: far.cut, from: far.from, to: far.to, t0: far.t0 }, { cut: false, from: 2500, to: 2000, t0: 80 });
  assert.ok(far.ms <= SNAP_MAX_MS && far.ms >= 300, `half a floor takes ${far.ms}`);
  assert.ok(near.ms < far.ms && near.ms >= 160, `a tenth of a floor takes ${near.ms}`);
  assert.ok(planSnap({ from: 0, to: 1000, floorHeight: 1000 }).ms <= SNAP_MAX_MS, "even a whole floor stays under the cap");
});

// A glide that starts while the page is still moving (the end of a wheel gesture) carries the speed it had: no stop, no kick.
const slopeAt0 = (job) => (stepJump(job, job.t0 + 0.01).y - stepJump(job, job.t0).y) / 0.01;       // px per ms

test("planSnap with a speed towards the mark: the glide starts at that speed (up to 2.5 times the way over its time), stays monotone and ends exactly on the mark", () => {
  assert.equal(planSnap({ from: 1300, to: 1000, floorHeight: 1000 }).carry, undefined, "from rest the glide keeps its ease-in-out");
  for (const v0 of [-0.05, -0.5, -3, -20]) {                                   // px per ms; the mark is 300 px below the page
    const job = planSnap({ from: 1300, to: 1000, floorHeight: 1000, speed: v0 });
    const cap = -2.5 * 300 / job.ms;
    assert.ok(Math.abs(slopeAt0(job) - Math.max(v0, cap)) < 0.02, `a speed of ${v0} starts at ${slopeAt0(job).toFixed(3)} px/ms (cap ${cap.toFixed(2)})`);
    assert.equal(stepJump(job, job.t0).y, 1300);
    let prev = 1300;
    for (let k = 1; k <= 60; k++) { const y = stepJump(job, job.t0 + job.ms * k / 60).y; assert.ok(y <= prev + 1e-9 && y >= 1000 - 1e-9, `monotone and inside (${y} after ${prev})`); prev = y; }
    assert.deepEqual([stepJump(job, job.t0 + job.ms).y, stepJump(job, job.t0 + job.ms).done], [1000, true]);
  }
});

test("planSnap with a speed away from the mark: the page gives way a little, comes back, and ends exactly on the mark", () => {
  const job = planSnap({ from: 1300, to: 1000, floorHeight: 1000, speed: 6 });
  let max = 1300;
  for (let k = 0; k <= 80; k++) max = Math.max(max, stepJump(job, job.t0 + job.ms * k / 80).y);
  assert.ok(max > 1300 && max <= 1300 + 0.1 * 300, `the page went on to ${max}`);
  assert.equal(stepJump(job, job.t0 + job.ms).y, 1000);
  assert.ok(slopeAt0(job) > 0, "it starts the way it was going");
});
