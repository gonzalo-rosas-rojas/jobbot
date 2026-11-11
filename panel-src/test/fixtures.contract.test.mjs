import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { model } from "../src/core/model.js";
import { ACCOUNT_KEYS, TASK_KEYS, missing } from "./helpers/contract.mjs";

const load = (n) => JSON.parse(readFileSync(new URL(`./fixtures/${n}`, import.meta.url), "utf8"));
const LIVE = ["submitted", "replied", "technical_test", "interview", "offer", "ghosted"];

for (const name of ["empty", "rich", "heavy"]) {
  const s = load(`snapshot.${name}.json`);
  test(`snapshot.${name}.json has every key of the contract`, () => assert.deepEqual(missing(s, { withCards: name !== "empty" }), []));
  test(`snapshot.${name}.json builds a model without throwing`, () => {
    const m = model(s);
    assert.ok(Array.isArray(m.procs) && Number.isFinite(m.nowMs));
  });
}

test("rich and heavy carry an account and a scheduled task; empty says nothing about both", () => {
  for (const name of ["rich", "heavy"]) {
    const s = load(`snapshot.${name}.json`);
    for (const k of ACCOUNT_KEYS) assert.ok(Object.hasOwn(s.controls.claude_account, k), `${name}: account.${k}`);
    for (const k of TASK_KEYS) assert.ok(Object.hasOwn(s.controls.scheduled_task, k), `${name}: task.${k}`);
    assert.match(s.controls.claude_account.email, /\*\*\*@/, "the email is masked");
    assert.doesNotMatch(JSON.stringify(s), /Users[\\/]|[A-Za-z]:\\\\/, "no user path");
  }
  const e = load("snapshot.empty.json");
  assert.equal(e.controls.claude_account, null);
  assert.equal(e.controls.scheduled_task, null);
  assert.equal(e.controls.engine, "never");
  assert.equal(e.controls.last_run_at, null);
});

test("heavy is the ugly frame nobody tested: 74 live processes, 8 that need him, 45 sent, 3 to approve, 6 tasks", () => {
  const s = load("snapshot.heavy.json");
  const total = (st) => s.board.find((c) => c.state === st).total;
  assert.equal(LIVE.reduce((n, st) => n + total(st), 0), 74);
  assert.equal(total("submitted"), 45);
  assert.equal(s.board.find((c) => c.state === "submitted").cards.length, 45, "live columns are not cut");
  assert.equal(s.needs_gonza.total, 8);
  assert.equal(s.awaiting_approval.total, 3);
  assert.equal(s.today.tasks_total, 6);
  assert.equal(new Set(s.board.flatMap((c) => c.cards.map((x) => x.company))).size, s.board.flatMap((c) => c.cards).length, "company names are unique");
});
