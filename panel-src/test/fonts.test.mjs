// The vendored fonts must be what tools/fonts.mjs says they are: real WOFF2 files, each under its ceiling and all
// under the budget, every one listed in VENDORED.md and nothing listed that is not there, every licence cited present
// and the OFL 1.1, and every download in the manifest pinned by both fingerprints.
import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { FACES, FILES, FONTS_DIR, LICENSES_DIR, TOOLS, TOTAL_BUDGET, VENDORED, verifyLocal } from "../tools/fonts.mjs";

const vendored = readFileSync(VENDORED, "utf8");
const listed = [...vendored.matchAll(/^### `([^`]+)`$/gm)].map((m) => m[1]).sort();
const present = readdirSync(FONTS_DIR).filter((n) => n.endsWith(".woff2")).sort();

test("every face is a WOFF2 file under its ceiling", () => {
  for (const face of FACES) {
    const file = path.join(FONTS_DIR, face.file);
    assert.ok(existsSync(file), `${face.file} missing`);
    const buf = readFileSync(file);
    assert.equal(buf.subarray(0, 4).toString("latin1"), "wOF2", `${face.file} has no wOF2 magic`);
    assert.ok(buf.length <= face.max, `${face.file} is ${buf.length} bytes, over ${face.max}`);
  }
});

test("the faces together stay under the budget", () => {
  const total = FACES.reduce((n, f) => n + statSync(path.join(FONTS_DIR, f.file)).size, 0);
  assert.ok(total <= TOTAL_BUDGET, `${total} bytes, over ${TOTAL_BUDGET}`);
});

test("VENDORED.md lists exactly the woff2 files present, which are exactly the faces", () => {
  assert.deepEqual(listed, present);
  assert.deepEqual(FACES.map((f) => f.file).sort(), present);
});

test("VENDORED.md records both hashes and the subset size of every face", () => {
  for (const face of FACES) {
    const section = vendored.split(`### \`${face.file}\``)[1].split("\n### ")[0];
    const size = statSync(path.join(FONTS_DIR, face.file)).size;
    assert.match(section, /- original: \d+ bytes .*sha256 `[0-9a-f]{64}`, git blob `[0-9a-f]{40}`/);
    assert.match(section, new RegExp(`- subset: ${size} bytes .*sha256 \`[0-9a-f]{64}\``));
    assert.match(section, /- ots-sanitize: ok/);
    assert.match(section, /- date: \d{4}-\d{2}-\d{2}/);
  }
});

test("every licence cited exists, is the OFL 1.1, and nothing else sits in licenses/", () => {
  const cited = [...vendored.matchAll(/`licenses\/([^`]+)`/g)].map((m) => m[1]);
  const unique = [...new Set(cited)].sort();
  assert.ok(unique.length > 0);
  for (const name of unique) {
    const text = readFileSync(path.join(LICENSES_DIR, name), "utf8");
    assert.ok(text.includes("SIL OPEN FONT LICENSE Version 1.1"), `${name} is not the OFL 1.1`);
  }
  assert.deepEqual(readdirSync(LICENSES_DIR).sort(), unique);
  for (const face of FACES) assert.ok(unique.includes(face.licenseFile), `${face.file} cites no licence`);
});

test("every file on disk is what VENDORED.md and the manifest say (hashes of the subsets and of the licence texts)", () => {
  assert.deepEqual(verifyLocal(), []);
});

test("the build tools are pinned to exact versions", () => {
  for (const [name, version] of Object.entries(TOOLS)) assert.match(version, /^\d+\.\d+\.\d+$/, name);
});

test("the manifest pins every download by sha256 and git blob", () => {
  for (const [file, want] of Object.entries(FILES)) {
    assert.match(want.sha256, /^[0-9a-f]{64}$/, `${file} sha256`);
    assert.match(want.blob, /^[0-9a-f]{40}$/, `${file} blob`);
    assert.ok(Number.isInteger(want.size) && want.size > 0, `${file} size`);
  }
  for (const face of FACES) {
    assert.ok(FILES[face.from], `${face.file} comes from an unlisted file`);
    assert.ok(FILES[face.license], `${face.file} cites an unlisted licence file`);
  }
});
