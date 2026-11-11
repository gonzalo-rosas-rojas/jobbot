import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

// The tower of v2 (the three.js world, the five rooms, the tower's motion and shell, their styles and tests) lives in archive/2026-10-07-torre/ and
// nothing that builds or runs may reach it again. This is the guard of that: where it was, and who could still import it or three.
const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const rel = (p) => path.relative(ROOT, p).split(path.sep).join("/");

// What moved out (a folder, or one file). A layer of the page is src/stage/ and src/scene/, a stop is src/moments/: none of these names comes back.
const TOWER = [
  "src/world", "src/rooms", "src/motion", "src/ui/stage.js", "src/ui/nav.js", "src/core/geometry.js",
  "src/styles/shell.css", "src/styles/components.css", "src/styles/rooms", "src/styles/ui/nav.css",
  "lab/world.entry.js", "lab/world.html", "lab/kit.entry.js", "lab/kit.html", "ROOM-AGENT-BRIEF.md", "spike-size.mjs", "spike-size2.mjs",
  "test/world.test.mjs", "test/shell.test.mjs", "test/room-aprender.test.mjs", "test/room-cola.test.mjs", "test/room-hoy.test.mjs", "test/room-procesos.test.mjs", "test/room-sistema.test.mjs",
];
const inTower = (file) => TOWER.some((p) => file === p || file.startsWith(p + "/"));

const SKIP = new Set(["node_modules", "dist", "tmp", "archive", "legacy", ".vendor-src"]);
function sources(dir = ROOT, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) { if (!SKIP.has(entry.name)) sources(path.join(dir, entry.name), out); } else if (/\.m?js$/.test(entry.name)) out.push(path.join(dir, entry.name));
  }
  return out;
}

/** The specifiers a file imports: `from "x"`, `import "x"` and `import("x")`. */
const specifiers = (text) => [...text.matchAll(/(?:\bfrom\s*|\bimport\s*\(\s*|\bimport\s+)["']([^"']+)["']/g)].map((m) => m[1]);

test("the tower is where the archive says and the archive explains itself in one line", () => {
  for (const p of TOWER) assert.equal(existsSync(path.join(ROOT, p)), false, `${p} is back in the tree`);
  // The archive is kept only in the private repository: a public copy has no archive/, and has nothing to explain.
  if (!existsSync(path.join(ROOT, "archive"))) return;
  const readme = readFileSync(path.join(ROOT, "archive/2026-10-07-torre/README.md"), "utf8").trim().split("\n");
  assert.equal(readme.length, 1, "a README of one line");
  for (const p of ["src/world", "src/rooms", "src/motion", "src/ui/stage.js", "src/ui/nav.js", "src/core/geometry.js", "src/styles/shell.css", "test/room-hoy.test.mjs", "test/world.test.mjs"]) {
    assert.ok(existsSync(path.join(ROOT, "archive/2026-10-07-torre", p)), `${p} was archived, not deleted`);
  }
});

test("nothing that builds the page, runs a lab or runs a test imports the tower or three", () => {
  const files = sources();
  assert.ok(files.length > 150, `scanned ${files.length} files`);
  const bad = [];
  for (const file of files) {
    for (const spec of specifiers(readFileSync(file, "utf8"))) {
      if (spec === "three" || spec.startsWith("three/")) bad.push(`${rel(file)} imports ${spec}`);
      else if (spec.startsWith(".")) {
        const target = rel(path.resolve(path.dirname(file), spec));
        if (inTower(target)) bad.push(`${rel(file)} imports ${spec}`);
      }
    }
  }
  assert.deepEqual(bad, []);
});
