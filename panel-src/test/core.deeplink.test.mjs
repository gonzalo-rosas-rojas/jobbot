import test from "node:test";
import assert from "node:assert/strict";
import { openRouteOnSnapshot, routeOfHash } from "../src/core/store.js";

// A session that only tells what the page needs: who listens, and the events it sends.
function fakeSession() {
  const subs = new Set();
  return {
    snapshot: null,
    subscribe: (fn) => { subs.add(fn); return () => subs.delete(fn); },
    emit(event) { if (event.type === "snapshot") this.snapshot = {}; for (const fn of [...subs]) fn(event); },
    listeners: () => subs.size,
  };
}

test("an address that names an application opens it with the first accepted snapshot, even after a failed first read", () => {
  const s = fakeSession();
  const opened = [];
  openRouteOnSnapshot(s, routeOfHash("#procesos/fx%3A12"), (job, room) => opened.push([job, room]));
  s.emit({ type: "error", error: { fatal: false } });
  assert.deepEqual(opened, [], "a failed read opens nothing");
  s.emit({ type: "snapshot" });
  assert.deepEqual(opened, [["fx:12", "procesos"]]);
  s.emit({ type: "snapshot" });
  assert.equal(opened.length, 1, "only once");
  assert.equal(s.listeners(), 0, "and it stops listening");
});

test("it opens at once when the first snapshot is already there, and does nothing for an address without an application", () => {
  const s = fakeSession();
  s.snapshot = {};
  const opened = [];
  openRouteOnSnapshot(s, routeOfHash("#cola/p%3A1"), (...a) => opened.push(a));
  assert.deepEqual(opened, [["p:1", "cola"]]);
  const t = fakeSession();
  openRouteOnSnapshot(t, routeOfHash("#cola"), () => opened.push("no"));
  openRouteOnSnapshot(t, null, () => opened.push("no"));
  t.emit({ type: "snapshot" });
  assert.equal(opened.length, 1);
  assert.equal(t.listeners(), 0);
});
