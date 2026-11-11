import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { DEMO_SOURCE, snapshotOf } from "./helpers/demo-host.mjs";
import { ACCOUNT_KEYS, TASK_KEYS, missing } from "./helpers/contract.mjs";

const LIVE = ["submitted", "replied", "technical_test", "interview", "offer", "ghosted"];
const FULL = ["", "urgent", "stop", "stale", "heavy"];       // scenarios that have tasks, mails and cards
const THIN = ["calm", "empty"];

for (const scenario of [...FULL, ...THIN]) {
  for (const lang of ["es", "en"]) {
    test(`demo ?scenario=${scenario || "default"} (${lang}) answers every key of the contract and leaks no path`, async () => {
      const s = await snapshotOf({ search: scenario ? `?scenario=${scenario}` : "", lang });
      assert.deepEqual(missing(s, { withCards: FULL.includes(scenario) }), []);
      assert.doesNotMatch(JSON.stringify(s), /(^|[^\w])(profile|data|cv)\/|Users[\\/]|[A-Za-z]:\\\\/);
      if (scenario !== "empty") {
        for (const k of ACCOUNT_KEYS) assert.ok(Object.hasOwn(s.controls.claude_account, k), k);
        assert.match(s.controls.claude_account.email, /\*\*\*@/);
        for (const k of TASK_KEYS) assert.ok(Object.hasOwn(s.controls.scheduled_task, k), k);
      }
    });
  }
}

test("the three truths of the engine (A15) exist in the demo", async () => {
  const [fresh, stale, never] = await Promise.all(["", "?scenario=stale", "?scenario=empty"].map((search) => snapshotOf({ search })));
  assert.deepEqual([fresh.controls.engine, stale.controls.engine, never.controls.engine], ["recent", "stale", "never"]);
  assert.ok(Date.parse(fresh.generated_at) - Date.parse(fresh.controls.last_run_at) < 36 * 3600000);
  assert.ok(Date.parse(stale.generated_at) - Date.parse(stale.controls.last_run_at) > 36 * 3600000);
  assert.equal(never.controls.last_run_at, null);
  assert.equal(fresh.controls.scheduled_task.exists, true);
  assert.equal(stale.controls.scheduled_task.exists, false, "the stale demo has no task: SISTEMA says it");
  assert.equal(never.controls.scheduled_task, null, "no data is not 'does not exist'");
});

test("heavy has 74 live processes, 8 stuck forms, 3 captures to approve and 10 tasks", async () => {
  const s = await snapshotOf({ search: "?scenario=heavy" });
  const total = (st) => s.board.find((c) => c.state === st).total;
  assert.equal(LIVE.reduce((n, st) => n + total(st), 0), 74);
  assert.equal(total("submitted"), 45);
  assert.equal(s.needs_gonza.total, 8);
  assert.equal(s.awaiting_approval.total, 3);
  assert.equal(s.today.tasks_total, 10, "6 of the base set plus 4 of the heavy one");
  assert.equal(s.funnel.by_source.greenhouse.n + s.funnel.by_source.ashby.n + s.funnel.by_source.lever.n, s.board.flatMap((c) => c.cards).length);
});

// The contract (snapshot.rs, Activity): `moves` is cut at 60, `moves_total` says how many there are, and `moves_by_state` is NOT cut: its counts add up to `moves_total`.
test("moves_by_state adds up to moves_total, and the cut list never has more than moves_total", async () => {
  for (const search of ["", "?scenario=heavy", "?scenario=urgent"]) {
    const s = await snapshotOf({ search });
    assert.equal(Object.values(s.activity.moves_by_state).reduce((a, b) => a + b, 0), s.activity.moves_total, search || "default");
    assert.ok(s.activity.moves.length <= s.activity.moves_total, search || "default");
  }
});

test("A12: demo.js carries no path of the private folders", () => {
  assert.doesNotMatch(DEMO_SOURCE, /(^|[^\w/.-])(profile|data|cv)\/[\w.-]/, "a path of profile/, data/ or cv/");
  assert.doesNotMatch(DEMO_SOURCE, /\.sqlite|\.toml|answers\.toml|criteria\.toml/i);
});

// The list of private strings (names, mails, companies) is Gonza's own file and is never committed: git has to ignore it, or whoever creates it
// would see it in `git status` and could push it (review M7). The check against demo.js only means something where the file exists, so where it
// does not the test is reported as skipped, with the reason on screen, and not as a pass that checked nothing.
const LOCAL_LIST = new URL("../../scripts/demo/private-strings.local.txt", import.meta.url);

test("A12: git ignores the local list of private strings (scripts/demo/*.local.txt)", () => {
  let ignored = true;
  try { execFileSync("git", ["check-ignore", "-q", "--no-index", "scripts/demo/private-strings.local.txt"], { cwd: fileURLToPath(new URL("../..", import.meta.url)), stdio: "pipe" }); } catch { ignored = false; }
  assert.ok(ignored, "scripts/demo/private-strings.local.txt is not in .gitignore");
});

test("A12: demo.js contains none of the strings of the local list", { skip: existsSync(LOCAL_LIST) ? false : "A12 NOT CHECKED against private strings: scripts/demo/private-strings.local.txt does not exist on this machine (only the paths were checked)" }, () => {
  for (const line of readFileSync(LOCAL_LIST, "utf8").split(/\r?\n/).map((l) => l.trim()).filter(Boolean)) assert.ok(!DEMO_SOURCE.includes(line), `demo.js contains a private string: ${line.slice(0, 3)}...`);
});
