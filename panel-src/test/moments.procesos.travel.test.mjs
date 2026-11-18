// The trip of a pile that changed lane (moments/procesos/travel.js) over plain stand-ins: who is measured, who slides, who just arrives.
import test from "node:test";
import assert from "node:assert/strict";
import { positionsBefore, travelTo, TRAVEL_MS } from "../src/moments/procesos/travel.js";

const box = (left, top) => ({ left, top });

function pile(id, at, { connected = true, animate = true } = {}) {
  const trips = [];
  const listeners = new Map();
  const el = {
    isConnected: connected,
    dataset: {},
    at,
    getBoundingClientRect() { return this.at; },
    animate: animate ? (frames, options) => { trips.push({ frames, options }); return { addEventListener: (type, fn) => listeners.set(type, fn) }; } : undefined,
  };
  return { id, el, trips, listeners };
}

test("positionsBefore measures only the moved piles that are on screen", () => {
  const piles = new Map([["card:a", pile("card:a", box(1, 2))], ["card:b", pile("card:b", box(3, 4), { connected: false })]]);
  const before = positionsBefore([{ key: "a" }, { key: "b" }, { key: "c" }], piles);
  assert.deepEqual([...before.keys()], ["card:a"]);
  assert.deepEqual(before.get("card:a"), box(1, 2));
});

test("a pile slides from its old box to its new one and drops the mark when it is done", () => {
  globalThis.window = { innerHeight: 1000 };
  const a = pile("card:a", box(10, 100));
  travelTo(new Map([["card:a", box(110, 300)]]), new Map([["card:a", a]]), () => false);
  assert.equal(a.trips.length, 1);
  assert.deepEqual(a.trips[0].frames, [{ transform: "translate(100px, 200px)" }, { transform: "none" }]);
  assert.equal(a.trips[0].options.duration, TRAVEL_MS);
  assert.equal(a.el.dataset.travel, "true");
  a.listeners.get("finish")();
  assert.equal(a.el.dataset.travel, undefined);
});

test("reduced motion, a pile that did not move and one too far away just arrive", () => {
  globalThis.window = { innerHeight: 1000 };
  const still = pile("card:s", box(5, 5));
  const far = pile("card:f", box(0, 0));
  const near = pile("card:n", box(0, 100));
  const piles = new Map([["card:s", still], ["card:f", far], ["card:n", near]]);
  const before = new Map([["card:s", box(5, 5)], ["card:f", box(0, 5000)], ["card:n", box(0, 160)]]);
  travelTo(before, piles, () => true);
  assert.equal(near.trips.length, 0, "reduced motion plays nothing");
  travelTo(before, piles, () => false);
  assert.equal(still.trips.length, 0);
  assert.equal(far.trips.length, 0, "more than one and a half screens is too far to follow");
  assert.equal(near.trips.length, 1);
});
