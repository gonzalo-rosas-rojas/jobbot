import test from "node:test";
import assert from "node:assert/strict";
import { clamp, wrapHour } from "../src/core/math.js";
import { mulberry32 } from "../src/core/random.js";

// The four ways the panel used to write the same thing, kept here as the table the single definition must agree with.
const clampOfScene = (x, a, b) => Math.min(b, Math.max(a, x));          // scene/engine.js and scene/poses.js
const clampOfStage = (x, a = 0, b = 1) => Math.min(b, Math.max(a, x)); // stage/shared.js
const wrapOfClock = (x) => ((x % 24) + 24) % 24;                        // scene/clock.js, scene/poses.js, stage/stage.js, stage/sky.js, stage/dial.js

test("clamp agrees with both of the old clamps, and defaults to the unit interval", () => {
  const xs = [-1e9, -2, -0.5, 0, 0.25, 1, 1.5, 37.2, 1e9, NaN];
  const bounds = [[0, 1], [0, 5], [-3, 3], [24, 64], [960, 1400], [0, 0]];
  for (const x of xs) {
    for (const [a, b] of bounds) {
      assert.ok(Object.is(clamp(x, a, b), clampOfScene(x, a, b)), `clamp(${x}, ${a}, ${b})`);
      assert.ok(Object.is(clamp(x, a, b), clampOfStage(x, a, b)));
    }
    assert.ok(Object.is(clamp(x), clampOfStage(x)), `clamp(${x}) defaults to 0..1`);
  }
});

test("clamp(v, 0, 0.6) is the volume rule sound/index.js used to write by itself, also for a value out of range on each side and for NaN", () => {
  const MAX_VOLUME = 0.6;
  const oldRule = (v) => Math.min(MAX_VOLUME, Math.max(0, v));
  for (const v of [-3, -0.001, 0, 0.25, 0.6, 0.61, 7, NaN]) assert.ok(Object.is(clamp(v, 0, MAX_VOLUME), oldRule(v)), `volume ${v}`);
});

test("wrapHour puts any hour on the 24 h ring the way the old copies did", () => {
  for (const x of [-49, -24, -12.5, -0.001, 0, 0.43, 3.25, 9, 17.999, 18, 23.999, 24, 24.5, 48, 100.7]) {
    assert.ok(Object.is(wrapHour(x), wrapOfClock(x)), `wrapHour(${x})`);
  }
  assert.equal(wrapHour(-6), 18);
  assert.equal(wrapHour(25), 1);
  assert.ok(wrapHour(-0.001) < 24 && wrapHour(-0.001) >= 0);
});

test("mulberry32 gives the same known sequence for the same seed", () => {
  const a = mulberry32(1), b = mulberry32(1);
  const first = [a(), a(), a(), a()];
  assert.deepEqual([b(), b(), b(), b()], first);
  assert.ok(first.every((x) => x >= 0 && x < 1));
  assert.notEqual(mulberry32(2)(), first[0], "another seed, another sequence");
  // pinned from the generator as it stood in stage/skyline.js before it moved to core/random.js
  assert.deepEqual(first, [0.6270739405881613, 0.002735721180215478, 0.5274470399599522, 0.9810509674716741]);
  assert.equal(mulberry32(20261007)(), 0.9620683940593153);
});

test("the sound jitter draws from that same generator", async () => {
  const { jitter } = await import("../src/sound/envelopes.js");
  const j = jitter({ seed: 1 });
  const b = mulberry32(1);
  b();                                   // jitter() spends its first draw on the pitch factor
  assert.equal(j.r(), b());
  assert.equal(j.r(), b());
});
