import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { setLang, setTimeZone } from "../src/i18n/index.js";
import { model, candidates, SENT_STATES, STAGES } from "../src/core/model.js";

const fixture = (name) => JSON.parse(readFileSync(new URL("./fixtures/" + name, import.meta.url), "utf8"));
const queue = (name) => candidates(model(fixture(name)));

// The order of the queue is the point of the ticket: what is late or due soon, an offer, the approvals, what stopped the robot by kind,
// the other tasks, the undated tasks, and last the follow-ups nobody put a task on.
test("the queue of a rich snapshot comes in the order the ticket promises", () => {
  setTimeZone("UTC");
  setLang("es");
  const got = queue("snapshot.rich.json").map((c) => [c.id, c.kindId, c.extra ? 1 : 0]);
  assert.deepEqual(got, [
    ["task:101", "task", 0], ["task:103", "task", 0], ["task:102", "task", 0],
    ["card:fx:7", "offer", 1],
    ["card:fx:12", "approve", 0],
    ["card:fx:9", "handoff", 0], ["card:fx:10", "missing_answers", 0], ["card:fx:11", "blocked", 0],
    ["task:104", "task", 0], ["task:105", "task", 0], ["task:106", "task", 0],
    ["card:fx:6", "followup", 1], ["card:fx:1", "followup", 1],
  ]);
});

test("the queue of a heavy snapshot keeps its groups in the same order", () => {
  setTimeZone("UTC");
  setLang("es");
  const runs = [];
  for (const c of queue("snapshot.heavy.json")) {
    if (runs.length === 0 || runs[runs.length - 1][0] !== c.kindId) runs.push([c.kindId, 0]);
    runs[runs.length - 1][1] += 1;
  }
  assert.deepEqual(runs, [["task", 6], ["offer", 1], ["approve", 3], ["handoff", 2], ["missing_answers", 2], ["blocked", 2], ["manual_apply", 2], ["followup", 50]]);
});

test("an empty snapshot has an empty queue", () => {
  assert.deepEqual(queue("snapshot.empty.json"), []);
});

test("the states that count as sent are the five stages plus the three that close or quiet them", () => {
  assert.deepEqual(SENT_STATES, [...STAGES, "accepted", "rejected", "ghosted"]);
  assert.deepEqual(SENT_STATES, ["submitted", "replied", "technical_test", "interview", "offer", "accepted", "rejected", "ghosted"]);
});
