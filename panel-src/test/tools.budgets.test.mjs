// tools/budgets.mjs and the way build.mjs uses it: the script of the page by area, a baseline in budgets.json, and a build that refuses when an area (or the whole script)
// grows past its margin without the baseline being updated on purpose. The global caps of guard.mjs are not touched by any of it.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { areaOf, measure, compare, allowance, table, readBaseline, writeBaseline, MARGIN } from "../tools/budgets.mjs";
import { JS_BUDGET, HTML_GZ_BUDGET } from "../guard.mjs";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const KB = 1024;

test("a bundled file belongs to a layer, a dictionary, a moment or a library", () => {
  const cases = {
    "src/main.js": "main",
    "src/core/model.js": "core",
    "src/ui/detail/act.js": "ui",
    "src/scene/engine.js": "scene",
    "src/stage/sky.js": "stage",
    "src/sound/cues.js": "sound",
    "src/i18n/index.js": "i18n",
    "src/i18n/es/hoy.js": "i18n/es",
    "src/i18n/en/common.js": "i18n/en",
    "src/moments/cola.js": "moments/cola",
    "src/moments/cola/render.js": "moments/cola",
    "src/moments/registry.js": "moments/registry",
    "src/moments/shared/armer.js": "moments/shared",
    "node_modules/lenis/dist/lenis.mjs": "lib/lenis",
    "node_modules/@leeoniya/ufuzzy/dist/uFuzzy.mjs": "lib/ufuzzy",
    "scripts/demo/demo.js": "other",
    "src/stray.js": "other",
    "src\\ui\\keys.js": "ui",
  };
  for (const [file, area] of Object.entries(cases)) assert.equal(areaOf(file), area, file);
});

test("measure sums the bytes each input put in the minified output, by area", () => {
  const metafile = { outputs: { "out.js": { inputs: { "src/ui/a.js": { bytesInOutput: 100 }, "src/ui/b.js": { bytesInOutput: 50 }, "src/core/c.js": { bytesInOutput: 7 }, "src/core/empty.js": { bytesInOutput: 0 } } } } };
  assert.deepEqual(measure(metafile, 170), { areas: { ui: 150, core: 7 }, total: 170 });
});

test("an area may grow by its margin (the larger of 3 KB and 5 %) and no more", () => {
  assert.equal(allowance(10 * KB, MARGIN.kb, MARGIN.pct), 3 * KB, "a small area: the absolute margin");
  assert.equal(allowance(100 * KB, MARGIN.kb, MARGIN.pct), 5 * KB, "a large one: the share");
  const baseline = { total: 400 * KB, areas: { ui: 60 * KB, core: 20 * KB } };
  const ok = compare({ areas: { ui: 60 * KB + 3 * KB, core: 20 * KB + 3 * KB }, total: 400 * KB + 5 * KB }, baseline);
  assert.deepEqual(ok.problems, []);
  const bad = compare({ areas: { ui: 60 * KB + 5 * KB, core: 20 * KB }, total: 400 * KB + 5 * KB }, baseline);
  assert.equal(bad.problems.length, 1);
  assert.match(bad.problems[0], /area "ui" is 65\.0 KB, 5\.0 KB over its baseline of 60\.0 KB \(the margin is 3\.0 KB\)/);
  assert.equal(bad.rows[0].over, true);
});

test("a new area over 3 KB is refused until it is accepted; a small one passes; the whole script has its own margin", () => {
  const baseline = { total: 400 * KB, areas: { ui: 60 * KB } };
  const fresh = compare({ areas: { ui: 60 * KB, "moments/story": 20 * KB }, total: 400 * KB + 20 * KB }, baseline);
  assert.match(fresh.problems.join("|"), /new area "moments\/story" is 20\.0 KB, 20\.0 KB over an empty baseline/);
  assert.deepEqual(compare({ areas: { ui: 60 * KB, tiny: 2 * KB }, total: 400 * KB + 2 * KB }, baseline).problems, []);
  const creep = compare({ areas: { ui: 62 * KB, core: 2 * KB, scene: 2 * KB, sound: 2 * KB }, total: 400 * KB + 8 * KB }, { total: 400 * KB, areas: { ui: 60 * KB, core: 0, scene: 0, sound: 0 } });
  assert.equal(creep.totalOver, true, "every area inside its margin, the whole 8 KB over a 6 KB margin");
  assert.match(creep.problems.join("|"), /the script is 408\.0 KB, 8\.0 KB over its baseline of 400\.0 KB \(the margin is 6\.0 KB\)/);
});

test("shrinking never fails, and the table says when the baseline has slack worth tightening", () => {
  const baseline = { total: 400 * KB, areas: { ui: 60 * KB } };
  const now = { areas: { ui: 50 * KB }, total: 390 * KB };
  const verdict = compare(now, baseline);
  assert.deepEqual(verdict.problems, []);
  assert.deepEqual(verdict.slack, ["ui"]);
  assert.match(table(now, verdict), /slack in ui/);
  assert.match(table(now, verdict), /ui\s+50\.0 KB\s+12\.8%\s+-10\.0 KB/);
});

test("the table marks what is over its margin; without a baseline nothing is judged", () => {
  const baseline = { total: 100 * KB, areas: { ui: 10 * KB } };
  const now = { areas: { ui: 20 * KB }, total: 100 * KB };
  assert.match(table(now, compare(now, baseline)), /ui\s+20\.0 KB\s+20\.0%\s+\+10\.0 KB\s+OVER THE MARGIN/);
  const none = compare(now, null);
  assert.deepEqual(none.problems, []);
  assert.match(table(now, none), /new/);
});

test("budgets.json is there, well formed, and does not move the global caps", () => {
  const baseline = readBaseline(path.join(ROOT, "budgets.json"));
  assert.ok(baseline, "budgets.json exists and is JSON");
  assert.ok(Number.isInteger(baseline.total) && baseline.total > 100 * KB);
  const names = Object.keys(baseline.areas);
  assert.ok(names.length >= 15);
  for (const name of names) assert.ok(Number.isInteger(baseline.areas[name]) && baseline.areas[name] > 0, name);
  for (const must of ["core", "ui", "scene", "stage", "sound", "i18n/es", "i18n/en", "moments/hoy", "moments/cola", "moments/procesos", "moments/aprender", "moments/sistema", "lib/lenis"]) assert.ok(names.includes(must), must);
  assert.ok(Object.values(baseline.areas).reduce((a, b) => a + b, 0) <= baseline.total);
  assert.deepEqual(names, [...names].sort((a, b) => a.localeCompare(b)), "keys in order, so a diff shows what moved");
  assert.equal(JS_BUDGET, 480 * KB, "the global script cap is still 480 KB");
  assert.equal(HTML_GZ_BUDGET, 200 * KB, "the global page cap is still 200 KB gzipped");
});

// ---- the real build, against baselines of its own (the committed one is judged by the build in test/build.page.test.mjs and by `npm run check`)
function build(extra) {
  return spawnSync(process.execPath, ["build.mjs", ...extra], { cwd: ROOT, encoding: "utf8" });
}

test("the build refuses a page whose area grew past its margin, names the area, and writes nothing", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "budgets-"));
  try {
    const own = path.join(dir, "budgets.json");
    const out = path.join(dir, "page.html");
    assert.equal(build(["--update-budgets", "--budgets", own, "--out", out]).status, 0);
    const accepted = readBaseline(own);
    assert.ok(accepted.areas.ui > 10 * KB);
    fs.rmSync(out);
    const run = build(["--budgets", own, "--out", out]);
    assert.equal(run.status, 0, run.stderr);
    assert.match(run.stdout, /^page /, "the first line is still the page line");
    assert.match(run.stdout, /\bui\s+\d+\.\d KB\s+\d+\.\d%\s+\+0\.0 KB/);
    fs.rmSync(out);
    // pretend the baseline of ui was 10 KB smaller: the same tree is now 10 KB of growth
    fs.writeFileSync(own, JSON.stringify({ ...accepted, areas: { ...accepted.areas, ui: accepted.areas.ui - 10 * KB } }));
    const refused = build(["--budgets", own, "--out", out]);
    assert.equal(refused.status, 1);
    assert.match(refused.stderr, /area "ui" is \d+\.\d KB, 10\.\d KB over its baseline of \d+\.\d KB \(the margin is 3\.\d KB\)/);
    assert.match(refused.stderr, /--update-budgets/);
    assert.match(refused.stdout, /OVER THE MARGIN/, "the table says where");
    assert.equal(fs.existsSync(out), false, "no page is written");
    assert.equal(build(["--update-budgets", "--budgets", own, "--out", out]).status, 0, "accepting it on purpose works");
    assert.deepEqual(readBaseline(own).areas, accepted.areas);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("a build of another entry is not judged by the baseline of the real page", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "budgets-entry-"));
  try {
    const entry = path.join(ROOT, "dist", `budgets-entry-${process.pid}.js`);
    fs.mkdirSync(path.dirname(entry), { recursive: true });
    fs.writeFileSync(entry, 'document.body.dataset.ok = "1";');
    const run = build(["--entry", path.relative(ROOT, entry), "--budgets", path.join(dir, "missing.json"), "--out", path.join(dir, "page.html")]);
    fs.rmSync(entry, { force: true });
    assert.equal(run.status, 0, run.stderr);
    assert.doesNotMatch(run.stdout + run.stderr, /vs baseline|budgets\.json/);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("writeBaseline writes sorted integer bytes that readBaseline gives back", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "budgets-io-"));
  try {
    const file = path.join(dir, "b.json");
    writeBaseline(file, { areas: { zeta: 3, alpha: 9 }, total: 12 });
    assert.deepEqual(Object.keys(readBaseline(file).areas), ["alpha", "zeta"]);
    assert.equal(readBaseline(file).total, 12);
    assert.equal(readBaseline(path.join(dir, "nope.json")), null);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("readBaseline: a missing file is null, a file that exists but is not JSON throws an error that names it", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "budgets-bad-"));
  try {
    const broken = path.join(dir, "budgets.json");
    fs.writeFileSync(broken, "{ not json");
    assert.throws(() => readBaseline(broken), (e) => e.message.includes(broken) && /JSON/.test(e.message));
    assert.throws(() => readBaseline(dir), (e) => e.message.includes(dir), "a path that cannot be read as a file is not 'missing' either");
    assert.equal(readBaseline(path.join(dir, "nope.json")), null);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("the build fails, naming the file, on a corrupt budgets file; with no file it only warns; --update-budgets replaces the corrupt one", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "budgets-corrupt-"));
  try {
    const own = path.join(dir, "budgets.json");
    const out = path.join(dir, "page.html");
    fs.writeFileSync(own, "{ not json");
    const refused = build(["--budgets", own, "--out", out]);
    assert.notEqual(refused.status, 0);
    assert.ok(refused.stderr.includes(own), refused.stderr);
    assert.equal(fs.existsSync(out), false, "no page is written");
    const missing = build(["--budgets", path.join(dir, "nope.json"), "--out", out]);
    assert.equal(missing.status, 0, missing.stderr);
    assert.match(missing.stderr, /budgets\.json is missing/);
    fs.rmSync(out);
    const fixed = build(["--update-budgets", "--budgets", own, "--out", out]);
    assert.equal(fixed.status, 0, fixed.stderr);
    assert.ok(readBaseline(own).total > 100 * KB);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
