// tools/check.mjs: which tests need a browser, and the order of the steps of `npm run check` (cheapest first; the browser ones only in `check:full`).
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { isBrowserTest, testFiles, steps } from "../tools/check.mjs";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");

test("a test needs a browser by its name or by what it imports", () => {
  assert.equal(isBrowserTest("page.browser.test.mjs", "import test from 'node:test';"), true);
  assert.equal(isBrowserTest("dom.test.mjs", 'import { launch } from "../tools/cdp.mjs";'), true);
  assert.equal(isBrowserTest("x.test.mjs", 'import { openBuilt } from "./helpers/browser.mjs";'), true);
  assert.equal(isBrowserTest("x.test.mjs", "const { page } = await import('./helpers/page.mjs');"), true);
  assert.equal(isBrowserTest("x.test.mjs", 'import { rig } from "./helpers/acceptance.mjs";'), true);
  assert.equal(isBrowserTest("core.test.mjs", 'import { buildModel } from "../src/core/model.js"; import { startServer } from "../tools/serve.mjs";'), false);
});

test("every test file is in exactly one list, and none of the Node ones launches a browser", () => {
  const { node, browser } = testFiles(ROOT);
  const all = fs.readdirSync(path.join(ROOT, "test")).filter((f) => f.endsWith(".test.mjs"));
  assert.equal(node.length + browser.length, all.length, "test files in subfolders would be counted too; none exist today");
  assert.deepEqual(node.filter((f) => browser.includes(f)), []);
  assert.ok(node.length > 60 && browser.length > 25, `${node.length} without a browser, ${browser.length} with one`);
  assert.ok(node.includes("test/docs.refs.test.mjs") && node.includes("test/arch.layers.test.mjs") && node.includes("test/core.test.mjs"));
  assert.ok(browser.includes("test/page.browser.test.mjs") && browser.includes("test/dom.test.mjs"));
  for (const f of node) assert.doesNotMatch(fs.readFileSync(path.join(ROOT, f), "utf8"), /tools\/cdp\.mjs|helpers\/(page|browser|acceptance)\.mjs/, f);
});

test("the steps of check run cheapest first, the layers apart, and the browser only in the full one", () => {
  const files = { node: ["test/arch.layers.test.mjs", "test/a.test.mjs", "test/b.test.mjs"], browser: ["test/c.browser.test.mjs"] };
  const fast = steps({ files });
  assert.deepEqual(fast.map((s) => s.name), ["build", "layers", "bidi", "tests"]);
  assert.deepEqual(fast[0].args, ["build.mjs", "--out", "dist/check.html"]);
  assert.deepEqual(fast[1].args.slice(-1), ["test/arch.layers.test.mjs"]);
  assert.deepEqual(fast[3].args.slice(-2), ["test/a.test.mjs", "test/b.test.mjs"], "the layer test is not run twice");
  assert.ok(fast[3].args.includes("--test-concurrency=4"));
  const full = steps({ files, full: true, concurrency: 2, browserConcurrency: 1 });
  assert.deepEqual(full.map((s) => s.name), ["build", "layers", "bidi", "tests", "browser"]);
  assert.ok(full[3].args.includes("--test-concurrency=2"));
  assert.deepEqual(full[4].args.slice(-3), ["--test-concurrency=1", "--test-reporter=spec", "test/c.browser.test.mjs"]);
});

test("npm run check and check:full are defined", () => {
  const scripts = JSON.parse(fs.readFileSync(path.join(ROOT, "package.json"), "utf8")).scripts;
  assert.equal(scripts.check, "node tools/check.mjs");
  assert.equal(scripts["check:full"], "node tools/check.mjs --full");
});
