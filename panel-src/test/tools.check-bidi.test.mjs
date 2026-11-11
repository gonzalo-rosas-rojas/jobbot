// tools/check-bidi.mjs: the 12 bidirectional control characters are found with their line and column, ordinary text (accents, dashes, arrows, an emoji) is not, and the
// sources of the page and of the tests are clean today.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { BIDI_NAMES, DEFAULT_PATHS, findBidi, scan, hex } from "../tools/check-bidi.mjs";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const U = (code) => String.fromCharCode(code);   // built at run time: this file holds no bidi character itself

test("each of the 12 bidi controls is found, with line and column", () => {
  assert.equal(BIDI_NAMES.size, 12);
  for (const code of BIDI_NAMES.keys()) {
    const [hit, ...more] = findBidi("a\nbc" + U(code) + "d");
    assert.deepEqual({ ...hit }, { line: 2, col: 3, code, name: BIDI_NAMES.get(code) }, hex(code));
    assert.equal(more.length, 0);
  }
  assert.equal(findBidi(U(0x202e) + "x" + U(0x2066)).length, 2);
});

test("the attack of the advisory is caught: an override that hides code inside a string", () => {
  const source = `const access = "user ${U(0x202e)} ${U(0x2066)}// check admin${U(0x2069)} ${U(0x2066)}";`;
  assert.ok(findBidi(source).length >= 3);
});

test("ordinary text is not flagged", () => {
  const fine = "Ñandú, canción, ¿qué tal? — “comillas” → ✓ 日本語 العربية   ​ ﻿   é 😀\n\r\n\t";
  assert.deepEqual(findBidi(fine), []);
  assert.deepEqual(findBidi(""), []);
});

test("scan reads the text files of a folder, recursively, and names the file", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "bidi-"));
  try {
    fs.mkdirSync(path.join(dir, "a", "b"), { recursive: true });
    fs.mkdirSync(path.join(dir, "node_modules"));
    fs.writeFileSync(path.join(dir, "a", "ok.js"), "export const x = 1;\n");
    fs.writeFileSync(path.join(dir, "a", "b", "bad.mjs"), "// one\nexport const y = \"" + U(0x202e) + "\";\n");
    fs.writeFileSync(path.join(dir, "a", "bad.png"), U(0x202e));        // not a text file: not read
    fs.writeFileSync(path.join(dir, "node_modules", "dep.js"), U(0x202e)); // not ours
    const hits = scan(["a"], dir);
    assert.deepEqual(hits.map((h) => `${h.file}:${h.line}:${h.col}:${hex(h.code)}`), ["a/b/bad.mjs:2:19:U+202E"]);
    assert.deepEqual(scan(["missing"], dir), []);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("the sources, the tests, the tools and the documents hold no bidi character", () => {
  assert.deepEqual(scan(DEFAULT_PATHS, ROOT), [], "write the character as an escape in a test, or name it (U+202E) in a document: it has no place in the sources");
});

test("scan also reads a single file, which is how the two top documents are covered", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "bidi-file-"));
  try {
    fs.writeFileSync(path.join(dir, "NOTES.md"), "a " + U(0x202e) + " b\n");
    fs.writeFileSync(path.join(dir, "clean.md"), "nothing\n");
    assert.deepEqual(scan(["NOTES.md", "clean.md", "missing.md"], dir).map((h) => `${h.file}:${h.line}:${h.col}`), ["NOTES.md:1:3"]);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("the command exits 0 on a clean folder and 1, naming the file, on a dirty one", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "bidi-cli-"));
  try {
    const tool = path.join(ROOT, "tools", "check-bidi.mjs");
    fs.mkdirSync(path.join(dir, "ok"));
    fs.mkdirSync(path.join(dir, "bad"));
    fs.writeFileSync(path.join(dir, "ok", "a.js"), "export {};\n");
    fs.writeFileSync(path.join(dir, "bad", "b.js"), "export const s = \"" + U(0x2067) + "\";\n");
    const clean = execFileSync(process.execPath, [tool, path.join(dir, "ok")], { encoding: "utf8" });
    assert.match(clean, /no bidirectional control characters/);
    const dirty = spawnSync(process.execPath, [tool, path.join(dir, "bad")], { encoding: "utf8" });
    assert.equal(dirty.status, 1);
    assert.match(dirty.stderr, /b\.js:1:\d+\s+U\+2067 RIGHT-TO-LEFT ISOLATE/);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("the default paths also cover what runs or is published, and leave out history", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "bidi-defaults-"));
  const bad = "x " + String.fromCodePoint(0x202e) + " y\n";
  try {
    const mustCover = ["build.mjs", "guard.mjs", "check-lock.mjs", "page.template.html", "lab/f0.entry.js", "../scripts/demo/demo.js"];
    // The demo folder sits beside panel-src, so the tree is panel-src/ and scripts/ under one temp folder.
    const base = path.join(dir, "panel-src");
    for (const rel of mustCover) {
      const file = path.resolve(base, rel);
      fs.mkdirSync(path.dirname(file), { recursive: true });
      fs.writeFileSync(file, bad);
    }
    for (const rel of ["archive/old.js", "legacy/old.js", "node_modules/dep/index.js"]) {
      fs.mkdirSync(path.dirname(path.join(base, rel)), { recursive: true });
      fs.writeFileSync(path.join(base, rel), bad);
    }
    const found = scan(DEFAULT_PATHS, base).map((h) => h.file).sort();
    assert.deepEqual(found, [...mustCover].map((r) => path.posix.normalize(r)).sort());
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
