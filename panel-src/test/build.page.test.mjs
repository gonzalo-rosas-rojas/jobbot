import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
mkdirSync(path.join(root, "dist"), { recursive: true });
writeFileSync(path.join(root, "dist", "t3-entry.js"), 'document.body.dataset.ok = "1";');

test("the real build, with a tiny entry, writes a page the strict guard accepts", () => {
  const out = execFileSync(process.execPath, ["build.mjs", "--entry", "dist/t3-entry.js", "--out", "dist/t3-page.html"], { cwd: root, encoding: "utf8" });
  assert.match(out, /^page /);
  const html = readFileSync(path.join(root, "dist", "t3-page.html"), "utf8");
  assert.equal(html.split("<script").length - 1, 2);
  assert.equal(html.split("<style").length - 1, 1);
  assert.ok(html.includes('id="stage"') && html.includes("<symbol"), "the stage skeleton and the icon sprite are in");
  for (const f of ["barlow-condensed-700", "geist-mono-400", "instrument-serif-400italic", "inter-400", "inter-600"]) assert.ok(html.includes(`__ASSETS__/${f}.woff2`), f);
  assert.ok(html.includes(".lenis"), "the Lenis stylesheet is included");
  assert.ok(!html.includes("innerHTML") && !html.includes("WebGLRenderer"));
});

test("a page whose bundle includes three is refused by the build, whatever else it is", () => {
  // three is not installed any more, so a stand-in under dist/node_modules/three/ is what the guard sees in the metafile: it judges the path of the bundled file.
  const fake = path.join(root, "dist", "node_modules", "three");
  mkdirSync(fake, { recursive: true });
  writeFileSync(path.join(fake, "index.js"), "export const Scene = class {};");
  writeFileSync(path.join(root, "dist", "t22-three-entry.js"), 'import { Scene } from "./node_modules/three/index.js"; document.body.dataset.scene = typeof Scene;');
  rmSync(path.join(root, "dist", "t22-three-page.html"), { force: true });
  const run = spawnSync(process.execPath, ["build.mjs", "--entry", "dist/t22-three-entry.js", "--out", "dist/t22-three-page.html"], { cwd: root, encoding: "utf8" });
  assert.notEqual(run.status, 0, "the build must fail");
  assert.match(run.stderr, /the bundle includes "three"/);
  assert.equal(existsSync(path.join(root, "dist", "t22-three-page.html")), false, "and write nothing");
  rmSync(path.join(root, "dist", "t22-three-entry.js"), { force: true });
  rmSync(path.join(root, "dist", "node_modules"), { recursive: true, force: true });
});

test("the whole page (src/main.js, every moment, the drawer) builds under the strict guard with no three in it", () => {
  rmSync(path.join(root, "dist", "t22-real-page.html"), { force: true });
  const out = execFileSync(process.execPath, ["build.mjs", "--out", "dist/t22-real-page.html"], { cwd: root, encoding: "utf8" });
  assert.match(out, /^page /);
  const html = readFileSync(path.join(root, "dist", "t22-real-page.html"), "utf8");
  assert.equal(html.split("<script").length - 1, 2);
  assert.ok(!html.includes("WebGLRenderer"), "three's renderer is not in the page");
});
