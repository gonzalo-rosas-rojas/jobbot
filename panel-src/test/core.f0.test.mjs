import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { setLang, setTimeZone } from "../src/i18n/index.js";
import { model, candidates, turnCount, nightFigures, stopCounts, KIND_IDS, LIVE, countOf } from "../src/core/model.js";
import { snapshotOf } from "./helpers/demo-host.mjs";

const load = (name) => JSON.parse(readFileSync(new URL(`./fixtures/${name}`, import.meta.url), "utf8"));
const rich = () => load("snapshot.rich.json");
const empty = () => load("snapshot.empty.json");
setLang("es");
setTimeZone("UTC");

test("nightFigures: the four figures of the night, their base and the night's own totals, read from the snapshot", () => {
  const f = nightFigures(rich());
  assert.deepEqual(f, { read: 120, above: 14, threshold: 60, complete: 10, awaiting: 1, days: 7, newPostings: 99, moves: 8 });
  assert.deepEqual(nightFigures(empty()), { read: 0, above: 0, threshold: null, complete: 0, awaiting: 0, days: 7, newPostings: 0, moves: 0 });
});

test("nightFigures: newPostings is the sum of the 48 half-hour slots, never a literal", () => {
  const s = rich();
  s.activity.slots.forEach((slot, i) => { slot.seen = i % 3; });
  assert.equal(nightFigures(s).newPostings, s.activity.slots.reduce((n, slot) => n + slot.seen, 0));
  assert.equal(nightFigures({ pipeline: {}, activity: {}, awaiting_approval: {} }).awaiting, 0, "a thin snapshot does not throw");
});

test("stopCounts: live processes, proposals awaiting, drills due and the engine's state", () => {
  const m = model(rich());
  assert.deepEqual(stopCounts(m), { procesos: 10, cola: 1, aprender: 3, sistema: "recent" });
  assert.equal(stopCounts(m).procesos, LIVE.reduce((n, state) => n + countOf(m.snap, state), 0));
  assert.deepEqual(stopCounts(model(empty())), { procesos: 0, cola: 0, aprender: 0, sistema: "never" });
});

test("B17: before the first snapshot there is nothing to read, and the bar and the prologue draw zeros instead of throwing", () => {
  const zero = { read: 0, above: 0, threshold: null, complete: 0, awaiting: 0, days: 0, newPostings: 0, moves: 0 };
  for (const none of [null, undefined, {}]) assert.deepEqual(nightFigures(none), zero, String(none));
  for (const m of [null, undefined, {}, { snap: null }, { snap: {} }]) assert.deepEqual(stopCounts(m), { procesos: 0, cola: 0, aprender: 0, sistema: "never" }, JSON.stringify(m));
  assert.equal(countOf(null, "submitted"), 0);
  assert.equal(countOf({}, "submitted"), 0);
});

test("stopCounts on the demo's scenarios: heavy has 74 live processes, stale and empty say so", async () => {
  const heavy = model(await snapshotOf({ search: "?scenario=heavy" }));
  assert.equal(stopCounts(heavy).procesos, 74);
  assert.equal(stopCounts(heavy).cola, heavy.snap.awaiting_approval.total);
  assert.equal(stopCounts(model(await snapshotOf({ search: "?scenario=stale" }))).sistema, "stale");
  assert.equal(stopCounts(model(await snapshotOf({ search: "?scenario=empty" }))).sistema, "never");
});

test("candidates keep every field they had and add company, role and kindId", () => {
  const m = model(rich());
  const list = candidates(m);
  assert.equal(list.length, 13);
  for (const c of list) {
    assert.ok(KIND_IDS.includes(c.kindId), `${c.id}: ${c.kindId}`);
    assert.ok("company" in c && "role" in c);
    assert.ok(c.id && c.kind && c.title && "tone" in c && "why" in c, "the old fields are still there");
  }
  const byId = Object.fromEntries(list.map((c) => [c.id, c]));
  assert.deepEqual([byId["task:101"].kindId, byId["task:101"].company, byId["task:101"].role], ["task", "Beta Test Co", m.tasks.find((x) => x.task.id === 101).owner.title]);
  assert.deepEqual([byId["task:104"].company, byId["task:104"].role], [null, null], "a task nobody owns names no company");
  assert.deepEqual([byId["card:fx:7"].kindId, byId["card:fx:7"].company, byId["card:fx:7"].role], ["offer", "Eta Test Co", "Site Reliability Engineer"]);
  assert.equal(byId["card:fx:12"].kindId, "approve");
  assert.equal(byId["card:fx:9"].kindId, "handoff", "the forms he must send himself");
  assert.equal(byId["card:fx:10"].kindId, "missing_answers");
  assert.equal(byId["card:fx:11"].kindId, "blocked");
  assert.equal(byId["card:fx:6"].kindId, "followup");
});

test("a task that came from a technical-test or interview email says so in its kindId", () => {
  const s = rich();
  s.today.tasks[1].kind = "technical_test";
  s.today.tasks[2].kind = "interview_invite";
  const byId = Object.fromEntries(candidates(model(s)).map((c) => [c.id, c]));
  assert.equal(byId[`task:${s.today.tasks[1].id}`].kindId, "test");
  assert.equal(byId[`task:${s.today.tasks[2].id}`].kindId, "interview");
});

test("A16: the number HOY will show is turnCount, in every scenario", async () => {
  for (const scenario of ["calm", "urgent", "stop", "empty", "heavy"]) {
    const m = model(await snapshotOf({ search: `?scenario=${scenario}` }));
    assert.equal(candidates(m).length, turnCount(m), `${scenario}: one candidate per waiting thing`);
  }
});

test("Review Focus 1: a partial or hostile snapshot gives whole numbers, never NaN, never a string, never a throw", () => {
  const zero = { read: 0, above: 0, threshold: null, complete: 0, awaiting: 0, days: 0, newPostings: 0, moves: 0 };
  assert.deepEqual(nightFigures({}), zero, "nothing but an empty object");
  assert.deepEqual(nightFigures({ pipeline: null, activity: null, awaiting_approval: null }), zero);
  const odd = nightFigures({
    pipeline: { seen: "120", above: NaN, threshold: undefined, complete: -3, days: 7.9 },
    activity: { slots: [{ seen: "2" }, { seen: null }, {}, { seen: NaN }, { seen: 3 }], moves_total: "x" },
    awaiting_approval: { total: undefined },
  });
  assert.deepEqual(odd, { read: 120, above: 0, threshold: null, complete: 0, awaiting: 0, days: 7, newPostings: 5, moves: 0 });
  for (const value of Object.values(odd)) assert.ok(value === null || Number.isInteger(value), String(value));

  const s = rich();
  delete s.learn; delete s.controls; s.awaiting_approval = null;
  s.board.find((col) => col.state === "submitted").total = undefined;
  assert.deepEqual(stopCounts(model(s)), { procesos: 6, cola: 0, aprender: 0, sistema: "never" }, "a column with no total counts as none");
});
