// Acceptance test of the encargo docs/encargos/check-cuenta-todo-y-cancelados.md. `npm run check` sums up each test step with the
// counts node's runner prints at the end. Today it keeps tests, pass, fail and skipped (tools/check.mjs, summaryLines), so a todo or
// a cancelled test is counted in `tests` and then shown nowhere: the browser step of check:full says 5 fewer than it ran. The summary
// has to keep cancelled and todo too, so the counts it shows add up to the total. Only the runner's final summary block counts: a
// test can print a line that looks like a count at the start of a line before it (weights.test.mjs already prints `ℹ lenis: ...`
// there). Pure text in, lines out: no test runner is started here (a child `node --test` would inherit the reporter forja passes
// through NODE_OPTIONS).
import test from "node:test";
import assert from "node:assert/strict";

const check = await import("../tools/check.mjs");

// The end of a real run of node 24.22 with --test-reporter=spec over a file with one test of each kind: a pass, a failure, a skip,
// two todo (one passing, one failing), a subtest left running when its parent ended (cancelled) and a timeout (counted in fail).
const MIXED = [
  "✔ passes (0.4ms)",
  "✖ fails (1.2ms)",
  "﹣ skipped (0.1ms) # why",
  "✔ todo (0.1ms) # later",
  "✖ todo that fails (0.3ms) # later",
  "✖ timeout (30.9ms)",
  "ℹ tests 8",
  "ℹ suites 0",
  "ℹ pass 3",
  "ℹ fail 1",
  "ℹ cancelled 1",
  "ℹ skipped 1",
  "ℹ todo 2",
  "ℹ duration_ms 766.4827",
  "",
  "✖ failing tests:",
].join("\n");

// A green browser step of check:full as Windows hands it over (CRLF): five todo tests, nothing else left out.
const GREEN_WITH_TODO = ["▶ A18", "ℹ tests 412", "ℹ suites 3", "ℹ pass 407", "ℹ fail 0", "ℹ cancelled 0", "ℹ skipped 0", "ℹ todo 5", "ℹ duration_ms 98123.4"].join("\r\n");

// Real runs of node 24.22 with --test-reporter=spec, in the order node prints them. A top-level test diagnostic goes out at column 0,
// right after its test line, like the weights of weights.test.mjs. node drops a top-level diagnostic that starts with a count's name
// only when the test runs in its own process; with NODE_OPTIONS=--test-isolation=none it reaches the output as is:
//   test("each foundation library ...", (t) => { t.diagnostic("lenis: ..."); t.diagnostic("todo 3"); t.diagnostic("pass 12"); });
const DIAGNOSTIC_LOOKS_LIKE_A_COUNT = [
  "✔ each foundation library stays inside its gzip budget (1.4612ms)",
  "ℹ lenis: 18.2 KB raw, 6.1 KB gzip",
  "ℹ todo 3",
  "ℹ pass 12",
  "✔ passes (0.127ms)",
  "✔ later (0.0814ms) # not yet",
  "ℹ tests 3",
  "ℹ suites 0",
  "ℹ pass 2",
  "ℹ fail 0",
  "ℹ cancelled 0",
  "ℹ skipped 0",
  "ℹ todo 1",
  "ℹ duration_ms 13.4425",
].join("\n");

// And what a test writes to stdout goes out as is, at column 0 and before its test line, with the default isolation too:
//   test("prints its own counts", () => { console.log("ℹ todo 3"); console.log("ℹ pass 12"); });
const STDOUT_LOOKS_LIKE_A_COUNT = [
  "ℹ todo 3",
  "ℹ pass 12",
  "✔ prints its own counts (1.0312ms)",
  "✔ passes (0.1056ms)",
  "✔ later (0.0944ms) # not yet",
  "ℹ tests 3",
  "ℹ suites 0",
  "ℹ pass 2",
  "ℹ fail 0",
  "ℹ cancelled 0",
  "ℹ skipped 0",
  "ℹ todo 1",
  "ℹ duration_ms 81.4405",
].join("\n");

const summaryOf = (text) => {
  assert.equal(typeof check.summaryLines, "function", "tools/check.mjs exports summaryLines(text), the lines its report shows for a test step");
  return check.summaryLines(text);
};
const counts = (lines) => Object.fromEntries(lines.map((line) => { const [name, n] = line.split(" "); return [name, Number(n)]; }));

test("check summary: a run with todo and cancelled tests shows them, so its counts add up to the total", () => {
  const lines = summaryOf(MIXED);
  assert.deepEqual(lines, ["tests 8", "pass 3", "fail 1", "cancelled 1", "skipped 1", "todo 2"], "the four counts of today, plus cancelled and todo, in node's order");
  const c = counts(lines);
  assert.equal(c.pass + c.fail + c.cancelled + c.skipped + c.todo, c.tests, "every test counted in the total is shown in one of the counts");
});

test("check summary: a green browser step says how many tests are still todo", () => {
  const lines = summaryOf(GREEN_WITH_TODO);
  assert.ok(lines.includes("todo 5"), `the five todo tests are shown (${lines.join(", ")})`);
  assert.ok(lines.includes("cancelled 0"), "a zero is shown like the other zeros (fail 0, skipped 0)");
  const c = counts(lines);
  assert.equal(c.pass + c.fail + c.cancelled + c.skipped + c.todo, c.tests, `${lines.join(", ")} adds up to tests ${c.tests}`);
});

test("check summary: only the runner's own summary lines count, and suites and duration stay out", () => {
  const text = ["✔ todo 9 (1ms)", "  ℹ todo 3", "# todo 4", "ℹ todo list is empty", "ℹ tests 2", "ℹ suites 1", "ℹ pass 2", "ℹ fail 0", "ℹ cancelled 0", "ℹ skipped 0", "ℹ todo 0", "ℹ duration_ms 3.1"].join("\n");
  assert.deepEqual(summaryOf(text), ["tests 2", "pass 2", "fail 0", "cancelled 0", "skipped 0", "todo 0"]);
});

test("check summary: a top-level diagnostic `ℹ todo 3` or `ℹ pass 12` before the summary block is not a count", () => {
  const lines = summaryOf(DIAGNOSTIC_LOOKS_LIKE_A_COUNT);
  assert.deepEqual(lines, ["tests 3", "pass 2", "fail 0", "cancelled 0", "skipped 0", "todo 1"], "only the runner's final summary block, each count once");
  const c = counts(lines);
  assert.equal(c.pass + c.fail + c.cancelled + c.skipped + c.todo, c.tests, `${lines.join(", ")} adds up to tests ${c.tests}`);
});

test("check summary: a test that prints `ℹ todo 3` and `ℹ pass 12` to stdout does not change the counts", () => {
  const lines = summaryOf(STDOUT_LOOKS_LIKE_A_COUNT);
  assert.deepEqual(lines, ["tests 3", "pass 2", "fail 0", "cancelled 0", "skipped 0", "todo 1"], "only the runner's final summary block, each count once");
  const c = counts(lines);
  assert.equal(c.pass + c.fail + c.cancelled + c.skipped + c.todo, c.tests, `${lines.join(", ")} adds up to tests ${c.tests}`);
});

test("check summary: the steps of check stay as they are (the order, the layer test apart, the browser only in the full one)", () => {
  const files = { node: ["test/arch.layers.test.mjs", "test/a.test.mjs"], browser: ["test/b.browser.test.mjs"] };
  assert.deepEqual(check.steps({ files }).map((s) => [s.name, !!s.tests]), [["build", false], ["layers", true], ["bidi", false], ["tests", true]]);
  const full = check.steps({ files, full: true });
  assert.deepEqual(full.map((s) => s.name), ["build", "layers", "bidi", "tests", "browser"]);
  assert.deepEqual(full[4].args.slice(-2), ["--test-reporter=spec", "test/b.browser.test.mjs"]);
});
