// Trojan Source guard (CVE-2021-42574): a source file must not hold a bidirectional control character, because it makes the code a reviewer sees differ from
// the code that runs, and it is invisible in an editor, a diff and a terminal. `clean()` already strips these characters from the text that comes from outside;
// nothing watched the sources that people and agents paste. The 12 characters are the Unicode property Bidi_Control: U+061C, U+200E, U+200F, U+202A to U+202E and
// U+2066 to U+2069. A test that needs one writes it as an escape ("\u202e"), which is the same string and visible to every reader.
//
//   node tools/check-bidi.mjs               scans DEFAULT_PATHS (src/, test/, tools/, docs/, lab/, ../scripts/demo/, the build files, the page template and two documents)
//   node tools/check-bidi.mjs src lab       scans the folders or files you name (relative to panel-src)

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.join(here, "..");

export const BIDI_NAMES = new Map([
  [0x061c, "ARABIC LETTER MARK"], [0x200e, "LEFT-TO-RIGHT MARK"], [0x200f, "RIGHT-TO-LEFT MARK"],
  [0x202a, "LEFT-TO-RIGHT EMBEDDING"], [0x202b, "RIGHT-TO-LEFT EMBEDDING"], [0x202c, "POP DIRECTIONAL FORMATTING"],
  [0x202d, "LEFT-TO-RIGHT OVERRIDE"], [0x202e, "RIGHT-TO-LEFT OVERRIDE"],
  [0x2066, "LEFT-TO-RIGHT ISOLATE"], [0x2067, "RIGHT-TO-LEFT ISOLATE"], [0x2068, "FIRST STRONG ISOLATE"], [0x2069, "POP DIRECTIONAL ISOLATE"],
]);
const TEXT = new Set([".js", ".mjs", ".cjs", ".css", ".json", ".html", ".md", ".svg", ".txt"]);
const SKIP = new Set(["node_modules", "dist", ".vendor-src", ".git"]);

/** Every bidi control character of a text: [{ line, col, code, name }], 1-based line and column (in UTF-16 units, as an editor counts). */
export function findBidi(text) {
  const out = [];
  let line = 1;
  let col = 0;
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i);
    col++;
    if (code === 10) { line++; col = 0; continue; }
    if (BIDI_NAMES.has(code)) out.push({ line, col, code, name: BIDI_NAMES.get(code) });
  }
  return out;
}

/** What `npm run check` scans: the page's sources and tests, the tools, the records, what runs or is published, and the two documents every agent reads first. */
// Also what runs or is published: the build files, the page template, the lab pages and the demo's fake server (its demo.js goes into the public demo).
// archive/ and legacy/ are history that nothing loads, so they stay out.
export const DEFAULT_PATHS = ["src", "test", "tools", "docs", "AGENT-RULES.md", "README.md", "build.mjs", "guard.mjs", "check-lock.mjs", "page.template.html", "lab", "../scripts/demo"];

/** The text files under `dir`, recursively (or `dir` itself when it is a text file). */
export function textFiles(dir) {
  const out = [];
  if (!fs.existsSync(dir)) return out;
  if (fs.statSync(dir).isFile()) return TEXT.has(path.extname(dir).toLowerCase()) ? [dir] : out;
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, e.name);
    if (e.isDirectory()) { if (!SKIP.has(e.name)) out.push(...textFiles(full)); } else if (TEXT.has(path.extname(e.name).toLowerCase())) out.push(full);
  }
  return out;
}

/** The findings of every text file under the folders: [{ file, line, col, code, name }] with `file` relative to `base`. */
export function scan(dirs, base = ROOT) {
  const out = [];
  for (const dir of dirs) {
    for (const file of textFiles(path.resolve(base, dir))) {
      for (const hit of findBidi(fs.readFileSync(file, "utf8"))) out.push({ file: path.relative(base, file).split(path.sep).join("/"), ...hit });
    }
  }
  return out;
}

export const hex = (code) => "U+" + code.toString(16).toUpperCase().padStart(4, "0");

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const dirs = process.argv.length > 2 ? process.argv.slice(2) : DEFAULT_PATHS;
  const hits = scan(dirs);
  if (hits.length) {
    console.error(`bidirectional control characters in the sources (Trojan Source; write them as \\u escapes):`);
    for (const h of hits) console.error(`  ${h.file}:${h.line}:${h.col}  ${hex(h.code)} ${h.name}`);
    process.exit(1);
  }
  console.log(`no bidirectional control characters in ${dirs.join(", ")}`);
}
