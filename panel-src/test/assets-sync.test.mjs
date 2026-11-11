import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, cpSync, readFileSync, writeFileSync, readdirSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { readFontHashes, sha256, syncFonts, FONT_NAMES } from "../tools/assets-sync.mjs";

const fonts = fileURLToPath(new URL("../assets/fonts/", import.meta.url));

test("VENDORED.md names the five fonts and each subset hash is the real file's", () => {
  const hashes = readFontHashes(readFileSync(path.join(fonts, "VENDORED.md"), "utf8"));
  assert.deepEqual([...hashes.keys()].sort(), [...FONT_NAMES].sort());
  for (const [name, hash] of hashes) assert.equal(sha256(readFileSync(path.join(fonts, name))), hash, name);
});

test("syncFonts copies the five verified files and refuses a tampered source", () => {
  const to = mkdtempSync(path.join(os.tmpdir(), "assets-sync-"));
  const ok = syncFonts({ from: fonts, to });
  assert.deepEqual(ok.problems, []);
  assert.deepEqual(readdirSync(to).filter((n) => n.endsWith(".woff2")).sort(), [...FONT_NAMES].sort());
  const from = mkdtempSync(path.join(os.tmpdir(), "assets-src-"));
  cpSync(fonts, from, { recursive: true });
  const victim = path.join(from, "inter-400.woff2");
  const bytes = readFileSync(victim); bytes[bytes.length - 1] ^= 1; writeFileSync(victim, bytes);
  const bad = syncFonts({ from, to: mkdtempSync(path.join(os.tmpdir(), "assets-dst-")) });
  assert.ok(bad.problems.some((p) => p.includes("inter-400.woff2")), bad.problems.join("; "));
  assert.equal(bad.copied.includes("inter-400.woff2"), false, "the altered file is not copied");
  assert.equal(bad.copied.length, 4);
});
