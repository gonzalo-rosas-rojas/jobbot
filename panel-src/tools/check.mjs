// The one command that says whether panel-src is fit to hand in: `npm run check` (fast, no browser) and `npm run check:full` (adds the browser tests).
// Steps run in this order and the first failure stops everything, cheapest first:
//   1. build   node build.mjs --out dist/check.html   the page and the guard: forbidden strings, the two global caps, the budget of each area (own file, so agents building in parallel do not collide)
//   2. layers  test/arch.layers.test.mjs              the imports go one way (AGENT-RULES.md, "The layers")
//   3. bidi    tools/check-bidi.mjs                   no bidirectional control character in src/ and test/ (Trojan Source)
//   4. tests   every test that needs no browser, node --test with a few files at a time
//   5. browser (check:full only) the tests that start a Chrome, one file at a time: they hang when the machine is busy
// A test needs a browser when its name ends in .browser.test.mjs or when it imports tools/cdp.mjs or a helper that does.
//
//   node tools/check.mjs [--full] [--concurrency N] [--browser-concurrency N] [--verbose] [--list]
//   --list prints which test files each step would run and exits; --verbose streams what the test runner prints instead of summarising it.
// Nothing here installs anything or touches git: wiring it to a hook is a separate decision (AGENT-RULES.md, "Before you say done").

import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const NEEDS_BROWSER = /(?:tools\/cdp\.mjs|helpers\/(?:page|browser|acceptance)\.mjs)["']/;
const LAYERS_TEST = "test/arch.layers.test.mjs";

/** Does this test file start a browser? By its name, or because it imports the launcher or a helper that does. */
export function isBrowserTest(name, text) {
  return /\.browser\.test\.mjs$/.test(name) || NEEDS_BROWSER.test(text);
}

function listTests(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, e.name);
    if (e.isDirectory()) listTests(full, out); else if (e.name.endsWith(".test.mjs")) out.push(full);
  }
  return out;
}

/** { node, browser }: the test files of test/ (paths relative to panel-src, with /), split by whether they need a browser. */
export function testFiles(root = ROOT) {
  const node = [];
  const browser = [];
  for (const file of listTests(path.join(root, "test")).sort()) {
    const rel = path.relative(root, file).split(path.sep).join("/");
    (isBrowserTest(path.basename(file), fs.readFileSync(file, "utf8")) ? browser : node).push(rel);
  }
  return { node, browser };
}

/** The steps, in order. A step is { name, what, args, tests? } where `args` go to node and `tests` marks a test-runner step. */
export function steps({ full = false, concurrency = 4, browserConcurrency = 1, files = testFiles() } = {}) {
  const runner = (list, n) => ["--test", `--test-concurrency=${n}`, "--test-reporter=spec", ...list];
  const rest = files.node.filter((f) => f !== LAYERS_TEST);
  const list = [
    { name: "build", what: "the page and its guard", args: ["build.mjs", "--out", "dist/check.html"] },
    { name: "layers", what: "imports go one way", args: runner([LAYERS_TEST], 1), tests: true },
    { name: "bidi", what: "no bidirectional control characters", args: ["tools/check-bidi.mjs"] },
    { name: "tests", what: `${rest.length} test files without a browser`, args: runner(rest, concurrency), tests: true },
  ];
  if (full) list.push({ name: "browser", what: `${files.browser.length} test files with a browser`, args: runner(files.browser, browserConcurrency), tests: true });
  return list;
}

const COUNTS = ["tests", "pass", "fail", "cancelled", "skipped", "todo"];

/** The counts of the runner's final summary block ("tests 8", "pass 3", ...), in node's order. A test can print lines that look like counts earlier, so only the block that starts at the last "ℹ tests N" line counts, each count once. */
export function summaryLines(text) {
  const lines = text.split(/\r?\n/);
  const start = lines.findLastIndex((l) => /^ℹ tests \d+\s*$/.test(l));
  if (start < 0) return [];
  const seen = new Map();
  for (const l of lines.slice(start)) {
    const m = /^ℹ (\w+) (\d+)\s*$/.exec(l);
    if (!m) break;
    if (COUNTS.includes(m[1]) && !seen.has(m[1])) seen.set(m[1], `${m[1]} ${m[2]}`);
  }
  return COUNTS.filter((name) => seen.has(name)).map((name) => seen.get(name));
}

/** Runs one step and returns { ok, seconds, out, stdout, error }: `out` is stdout then stderr (what a failed step shows), `stdout` alone is what the runner wrote (null when --verbose streamed it). */
export function runStep(step, { verbose }) {
  const started = Date.now();
  const r = spawnSync(process.execPath, step.args, { cwd: ROOT, encoding: "utf8", maxBuffer: 1 << 28, stdio: verbose ? ["ignore", "inherit", "inherit"] : "pipe", env: { ...process.env, FORCE_COLOR: "0", NO_COLOR: "1" } });
  const seconds = ((Date.now() - started) / 1000).toFixed(1);
  const out = `${r.stdout || ""}${r.stderr || ""}`;
  return { ok: r.status === 0 && !r.error, seconds, out, stdout: r.stdout ?? null, error: r.error };
}

/** The detail of a test step's line: the counts of the runner's summary, read from stdout only (a child of a test writes count-like lines to stderr, after it). */
function testDetail(result) {
  if (result.stdout == null) return "";
  const counts = summaryLines(result.stdout).join(", ");
  return counts || "no test summary";
}

/** Prints the line of a step; a failed one also prints what it wrote. */
export function report(step, result, index, total) {
  const head = `[${index}/${total}] ${step.name.padEnd(8)}`;
  if (result.ok) {
    const detail = step.tests ? testDetail(result) : result.out.trim().split(/\r?\n/)[0];   // the build prints its table after the "page ..." line
    console.log(`${head} ok    ${result.seconds}s  ${step.what}${detail ? ` (${detail})` : ""}`);
    return;
  }
  console.error(`${head} FAILED ${result.seconds}s  ${step.what}`);
  if (result.error) console.error(String(result.error));
  const lines = result.out.split(/\r?\n/);
  const from = lines.findIndex((l) => /^✖ failing tests:/.test(l));
  console.error((step.tests && from >= 0 ? lines.slice(from) : lines).slice(-150).join("\n"));
}

function main(argv) {
  const flag = (name) => argv.includes(name);
  const value = (name, fallback) => { const i = argv.indexOf(name); return i >= 0 ? Math.max(1, Number(argv[i + 1]) || fallback) : fallback; };
  const files = testFiles();
  const list = steps({ full: flag("--full"), concurrency: value("--concurrency", 4), browserConcurrency: value("--browser-concurrency", 1), files });
  if (flag("--list")) {
    console.log(`without a browser (${files.node.length}):\n  ${files.node.join("\n  ")}\nwith a browser (${files.browser.length}):\n  ${files.browser.join("\n  ")}`);
    return 0;
  }
  const started = Date.now();
  for (const [i, step] of list.entries()) {
    const result = runStep(step, { verbose: flag("--verbose") });
    report(step, result, i + 1, list.length);
    if (!result.ok) { console.error(`check stopped at "${step.name}": it is not ready to hand in`); return 1; }
  }
  console.log(`check ${flag("--full") ? "full " : ""}passed in ${((Date.now() - started) / 1000).toFixed(1)}s`);
  return 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) process.exit(main(process.argv.slice(2)));
