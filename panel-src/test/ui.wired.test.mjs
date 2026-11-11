import test from "node:test";
import assert from "node:assert/strict";
import { KEYS, keysFor } from "../src/ui/keys.js";
import { STOP_IDS } from "../src/scene/poses.js";
import { WIRED, wiredSet, liveSet } from "../src/ui/wired.js";
import { helpRows } from "../src/ui/help.js";
import { momentOf } from "../src/moments/registry.js";
import { MOMENT_VERBS } from "../src/ui/shell-keys.js";

const GLOBAL = KEYS.filter((k) => k.scope === "global").map((k) => k.id);

test("every stop has a list, and every id in it is a key of the table that the table gives to that stop", () => {
  assert.deepEqual(Object.keys(WIRED), STOP_IDS);
  for (const [stop, ids] of Object.entries(WIRED)) {
    assert.equal(new Set(ids).size, ids.length, `${stop}: an id twice`);
    for (const id of ids) {
      const entry = KEYS.find((k) => k.id === id);
      assert.ok(entry, `${stop}: ${id} is not in the table`);
      assert.ok(entry.scope === "row" || entry.scope === "stop", `${stop}: ${id} is global or local, the shell does not route it to a moment`);
      assert.ok(entry.stops.includes(stop), `${stop}: the table does not give ${id} to this stop`);
      assert.ok(MOMENT_VERBS.includes(id), `${stop}: ${id} has no handler in the shell`);
    }
  }
});

test("HOY answers every row and stop key the table gives it (a key of HOY missing here would be a shortcut the help hides)", () => {
  const given = KEYS.filter((k) => (k.scope === "row" || k.scope === "stop") && k.stops.includes("hoy")).map((k) => k.id).sort();
  assert.deepEqual([...WIRED.hoy].sort(), given);
});

test("a stop promises keys only when its moment can take them: a moment with no onKey (the prologue) wires nothing", () => {
  for (const stop of STOP_IDS) {
    const moment = momentOf(stop);
    assert.ok(moment, `${stop} has a moment`);
    if (typeof moment.onKey !== "function") assert.deepEqual(WIRED[stop], [], `${stop}: its moment has no onKey`);
    else assert.ok(WIRED[stop].length > 0, `${stop}: its moment has onKey and the help would announce nothing`);
  }
});

test("the help of a stop lists the global keys plus the wired ones, and nothing else", () => {
  assert.deepEqual(helpRows("hoy", wiredSet("hoy")).map((r) => r.id), keysFor("hoy").map((k) => k.id));
  for (const stop of STOP_IDS) {
    const wired = KEYS.filter((k) => k.scope === "global" || ((k.stops || []).includes(stop) && WIRED[stop].includes(k.id))).map((k) => k.id);
    assert.deepEqual(helpRows(stop, wiredSet(stop)).map((r) => r.id), wired, stop);
  }
  assert.deepEqual(helpRows("prologo", wiredSet("prologo")).map((r) => r.id), GLOBAL, "the prologue has the global keys only");
  assert.deepEqual(helpRows("procesos", wiredSet("nowhere")).map((r) => r.id), GLOBAL);
});

test("the keys a box reads for itself (the drill) are listed with the stop that has the box, and no other stop", () => {
  const local = KEYS.filter((k) => k.scope === "local");
  assert.ok(local.length > 0);
  for (const stop of STOP_IDS) {
    const listed = helpRows(stop, liveSet(stop)).filter((r) => r.scope === "local").map((r) => r.id);
    assert.deepEqual(listed, local.filter((k) => k.stops.includes(stop)).map((k) => k.id), stop);
  }
});

test("wiredSet is a fresh Set each time and an unknown stop answers nothing", () => {
  const a = wiredSet("hoy");
  a.add("x");
  assert.equal(wiredSet("hoy").has("x"), false);
  assert.equal(wiredSet("nowhere").size, 0);
  assert.equal(wiredSet(null).size, 0);
});
