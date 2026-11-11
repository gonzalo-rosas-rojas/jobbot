// Keeps the two copies of each font honest: panel-src/assets/fonts is the verified origin (sha256 of every subset in its
// VENDORED.md), crates/jobbot-panel/src/assets is what the binary embeds. `build.mjs --install` calls syncFonts; test/assets-sync.test.mjs (R4)
// checks the Rust side, the notices and the hashes together.
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
export const FONTS_DIR = path.join(here, "..", "assets", "fonts");
export const RUST_ASSETS = path.join(here, "..", "..", "crates", "jobbot-panel", "src", "assets");
export const FONT_NAMES = ["barlow-condensed-700.woff2", "geist-mono-400.woff2", "instrument-serif-400italic.woff2", "inter-400.woff2", "inter-600.woff2"];
export const sha256 = (buf) => createHash("sha256").update(buf).digest("hex");

/** name -> sha256 of the SUBSET file, read from the "### `name`" sections of assets/fonts/VENDORED.md. */
export function readFontHashes(text) {
  const out = new Map();
  let current = null;
  for (const line of text.replace(/\r\n/g, "\n").split("\n")) {
    const head = /^### `([^`]+\.woff2)`/.exec(line);
    if (head) { current = head[1]; continue; }
    const subset = /^- subset: .*sha256 `([0-9a-f]{64})`/.exec(line);
    if (current && subset) { out.set(current, subset[1]); current = null; }
  }
  return out;
}

/** file name -> sha256 of the text (line ends as \n) of each licence in the "Licences" table of VENDORED.md, the way tools/fonts.mjs checks them. */
export function readLicenceHashes(text) {
  const out = new Map();
  for (const line of text.replace(/\r\n/g, "\n").split("\n")) {
    const row = /^\| `licenses\/([^`/]+\.txt)` \|.*\| `([0-9a-f]{64})` \|$/.exec(line);
    if (row) out.set(row[1], row[2]);
  }
  return out;
}

/**
 * Copies the five fonts from `from` to `to` after checking each against VENDORED.md; a file that fails is not copied.
 * With `licenses`, the licences of that table go to `to`/licenses, checked the same way (the published demo carries them beside its fonts).
 */
export function syncFonts({ from = FONTS_DIR, to = RUST_ASSETS, licenses = false } = {}) {
  const problems = [];
  const copied = [];
  const vendored = readFileSync(path.join(from, "VENDORED.md"), "utf8");
  const hashes = readFontHashes(vendored);
  mkdirSync(to, { recursive: true });
  if (licenses) {
    const wanted = readLicenceHashes(vendored);
    if (!wanted.size) problems.push("VENDORED.md lists no licences");
    mkdirSync(path.join(to, "licenses"), { recursive: true });
    for (const [name, hash] of wanted) {
      const src = path.join(from, "licenses", name);
      if (!existsSync(src)) { problems.push(`licenses/${name}: missing in ${from}`); continue; }
      const bytes = readFileSync(src);
      if (sha256(Buffer.from(bytes.toString("utf8").replace(/\r\n/g, "\n"))) !== hash) { problems.push(`licenses/${name}: sha256 differs from VENDORED.md`); continue; }
      writeFileSync(path.join(to, "licenses", name), bytes);
      copied.push(`licenses/${name}`);
    }
  }
  for (const name of FONT_NAMES) {
    const src = path.join(from, name);
    if (!existsSync(src) || !hashes.has(name)) { problems.push(`${name}: missing in ${from} or in VENDORED.md`); continue; }
    const bytes = readFileSync(src);
    if (sha256(bytes) !== hashes.get(name)) { problems.push(`${name}: sha256 differs from VENDORED.md`); continue; }
    writeFileSync(path.join(to, name), bytes);
    copied.push(name);
  }
  return { problems, copied, to };
}
