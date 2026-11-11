import test from "node:test";
import assert from "node:assert/strict";
import { createPlace, initialOrder } from "../src/moments/procesos/place.js";

const e = (key, lane, days, extra = {}) => ({ key, lane, days, you: false, state: lane, ...extra });
const keys = (slots) => slots.map((s) => (s.tomb ? "~" + s.tomb : s.key));

test("the first order of a lane: what is yours first, then the one that has waited longest in the stage, then the key (so a tie is always the same)", () => {
  const order = initialOrder([e("b", "submitted", 3), e("a", "submitted", 3), e("old", "submitted", 14), e("mine", "submitted", 1, { you: true }), e("mine2", "submitted", 9, { you: true })]);
  assert.deepEqual(order.map((x) => x.key), ["mine2", "mine", "old", "a", "b"]);
});

test("a lane keeps its order for the whole session: a card that turns yours, ages or is rescored does not move", () => {
  const place = createPlace();
  const first = place.sync([e("a", "submitted", 14), e("b", "submitted", 13), e("c", "submitted", 12)], () => null);
  assert.deepEqual(keys(first.lanes.submitted), ["a", "b", "c"]);
  assert.deepEqual(first.moves, []);
  const second = place.sync([e("a", "submitted", 15), e("b", "submitted", 14, { you: true }), e("c", "submitted", 40, { you: true })], () => null);
  assert.deepEqual(keys(second.lanes.submitted), ["a", "b", "c"], "b and c are yours now and c is the oldest: they stay where they were");
  assert.deepEqual(second.moves, []);
});

test("a new application enters at the end of its lane, however old or urgent", () => {
  const place = createPlace();
  place.sync([e("a", "submitted", 5), e("b", "submitted", 4)], () => null);
  const next = place.sync([e("a", "submitted", 5), e("b", "submitted", 4), e("late", "submitted", 30, { you: true }), e("x", "replied", 1)], () => null);
  assert.deepEqual(keys(next.lanes.submitted), ["a", "b", "late"]);
  assert.deepEqual(keys(next.lanes.replied), ["x"]);
});

test("one that leaves the stage leaves a tombstone of its own size where it was, and arrives at the end of the new lane; nobody else moves", () => {
  const place = createPlace();
  place.sync([e("a", "submitted", 14), e("b", "submitted", 13), e("c", "submitted", 12), e("r", "replied", 6)], () => null);
  const next = place.sync([e("a", "submitted", 14), e("b", "replied", 0, { state: "replied" }), e("c", "submitted", 12), e("r", "replied", 6)], () => null);
  assert.deepEqual(keys(next.lanes.submitted), ["a", "~b", "c"], "b's place is a hole, c did not slide up");
  assert.deepEqual(next.lanes.submitted[1], { tomb: "b", to: "replied" }, "the hole says where it went");
  assert.deepEqual(keys(next.lanes.replied), ["r", "b"]);
  assert.deepEqual(next.moves, [{ key: "b", from: "submitted", to: "replied" }], "so the page can make the pile travel");
  const again = place.sync([e("a", "submitted", 14), e("b", "replied", 0), e("c", "submitted", 12), e("r", "replied", 6)], () => null);
  assert.deepEqual(keys(again.lanes.submitted), ["a", "~b", "c"], "the hole stays until the next load of the page");
  assert.deepEqual(again.moves, [], "the trip is made once");
});

test("one that leaves the field (closed, or gone from the board) leaves a tombstone that names the state it went to", () => {
  const place = createPlace();
  place.sync([e("a", "submitted", 9), e("b", "submitted", 8)], () => null);
  const next = place.sync([e("b", "submitted", 8)], (key) => (key === "a" ? "rejected" : null));
  assert.deepEqual(next.lanes.submitted, [{ tomb: "a", to: "rejected" }, { key: "b" }]);
  assert.deepEqual(next.moves, [{ key: "a", from: "submitted", to: null }]);
  const gone = place.sync([], () => null);
  assert.deepEqual(gone.lanes.submitted, [{ tomb: "a", to: "rejected" }, { tomb: "b", to: null }], "a card the snapshot no longer knows is a hole with no destination");
});

test("a card that comes back to the lane it left fills its own hole", () => {
  const place = createPlace();
  place.sync([e("a", "submitted", 9), e("b", "submitted", 8)], () => null);
  place.sync([e("a", "replied", 0), e("b", "submitted", 8)], () => null);
  const back = place.sync([e("a", "submitted", 0), e("b", "submitted", 8)], () => null);
  assert.deepEqual(keys(back.lanes.submitted), ["a", "b"]);
  assert.deepEqual(keys(back.lanes.replied), ["~a"]);
});

test("reset forgets every place (a new page load), and a key listed twice counts once", () => {
  const place = createPlace();
  place.sync([e("a", "submitted", 1), e("a", "replied", 1), e("b", "submitted", 9)], () => null);
  const first = place.sync([e("a", "submitted", 1), e("a", "replied", 1), e("b", "submitted", 9)], () => null);
  assert.deepEqual(keys(first.lanes.submitted), ["b", "a"]);
  assert.deepEqual(first.lanes.replied || [], [], "the first listing wins");
  place.reset();
  const fresh = place.sync([e("b", "submitted", 1), e("a", "submitted", 9)], () => null);
  assert.deepEqual(keys(fresh.lanes.submitted), ["a", "b"], "ordered again from scratch");
});

test("an empty field and a missing list are fine", () => {
  const place = createPlace();
  assert.deepEqual(place.sync([], () => null), { lanes: {}, moves: [] });
  assert.deepEqual(place.sync(undefined, () => null), { lanes: {}, moves: [] });
});
