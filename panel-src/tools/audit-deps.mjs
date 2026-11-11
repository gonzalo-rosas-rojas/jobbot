// Looks inside the files of every dependency that reaches the page for what the page must not do: reach the network,
// run code built from strings, or turn strings into markup (R8 §2 rules 7 and 8; R8b §7 step 5). Comments are removed
// first, so a URL in a licence header does not count. The result is compared with KNOWN below: every hit there was read
// by hand and has a reason; a hit that is not there, or one more of the same, fails, and so does a known hit that vanished
// (the table must not rot). CSP `connect-src 'self'` and guard.mjs on the final bundle are the second and third lines.
//
//   node tools/audit-deps.mjs            prints every hit with its line, then the verdict
//   node tools/audit-deps.mjs --quiet    verdict only

import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { transform } from "esbuild";

const here = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.join(here, "..");

// The files esbuild reads for each package (its "module" entry) plus the stylesheet, when there is one; a path ending in /
// means every .js under it (for a package that is a barrel over many modules). three.js is not listed: it is not a dependency any more
// (its tower is archived) and guard.mjs refuses it in the final bundle.
export const TARGETS = {
  lenis: ["dist/lenis.mjs", "dist/lenis.css"],
  tinykeys: ["dist/tinykeys.mjs"],
  "@leeoniya/ufuzzy": ["dist/uFuzzy.mjs"],
};

// What each pattern looks for. `net`: reaches out. `code`: builds code from a string. `html`: turns text into markup.
// `env`: reads or writes something about the page or the visitor. Case-sensitive, on code without comments.
export const PATTERNS = [
  ["fetch(", "net", /\bfetch\s*\(/g],
  ["XMLHttpRequest", "net", /XMLHttpRequest/g],
  ["WebSocket", "net", /WebSocket/g],
  ["EventSource", "net", /EventSource/g],
  ["sendBeacon", "net", /sendBeacon/g],
  ["Worker(", "net", /\b(?:Shared)?Worker\s*\(|importScripts/g],
  ["import(", "code", /\bimport\s*\(/g],
  ["eval", "code", /\beval\b/g],
  ["new Function", "code", /new\s+Function\b/g],
  ["document.write", "html", /document\s*\.\s*write/g],
  ["innerHTML", "html", /innerHTML/g],
  ["outerHTML", "html", /outerHTML/g],
  ["insertAdjacentHTML", "html", /insertAdjacentHTML/g],
  ["srcdoc", "html", /srcdoc/g],
  ["location", "env", /\blocation\b/g],
  ["document.cookie", "env", /document\s*\.\s*cookie/g],
  ["localStorage", "env", /localStorage|sessionStorage|indexedDB/g],
  ["navigator", "env", /\bnavigator\b/g],
  ["url", "net", /https?:\/\/[^\s"'`)]+/g],
];

// package -> pattern -> { count, why }. Counts are of the code without comments, in the files listed in TARGETS.
export const KNOWN = {
  lenis: {
    location: { count: 1, why: "onClick parses window.location.href with new URL() to tell an in-page anchor from another page; it only runs when the options anchors or stopInertiaOnNavigate are on, and nothing is written" },
    navigator: { count: 1, why: "navigator.userAgent, tested against iPad|iPhone|iPod to set isIos; read-only" },
  },
  tinykeys: {
    navigator: { count: 2, why: "one line: typeof navigator guards navigator.platform, which picks Meta or Control for the $mod alias; read-only" },
  },
};

const around = (text, at, length) => text.slice(Math.max(0, at - 45), at + length + 55).replace(/\s+/g, " ");

/** The code of a file without its comments: minifying is the one way esbuild drops every comment (unminified it keeps some). */
async function code(file) {
  const loader = file.endsWith(".css") ? "css" : "js";
  const out = await transform(readFileSync(file, "utf8"), { loader, legalComments: "none", minify: true });
  return out.code;
}

function expand(dir, rel) {
  if (!rel.endsWith("/")) return [rel];
  return readdirSync(path.join(dir, rel), { recursive: true }).filter((f) => f.endsWith(".js")).sort().map((f) => rel + f.replaceAll("\\", "/"));
}

/** Every hit: { pkg, pattern, kind, file, text } where text is the code around it. */
export async function scan(targets = TARGETS, root = path.join(ROOT, "node_modules")) {
  const hits = [];
  for (const [pkg, files] of Object.entries(targets)) {
    for (const rel of files.flatMap((f) => expand(path.join(root, pkg), f))) {
      const text = await code(path.join(root, pkg, rel));
      for (const [pattern, kind, re] of PATTERNS) {
        for (const m of text.matchAll(re)) hits.push({ pkg, pattern, kind, file: rel, text: around(text, m.index, m[0].length) });
      }
    }
  }
  return hits;
}

/** Problems: hits beyond KNOWN, and KNOWN entries that no longer match. Empty = as audited. */
export function judge(hits, known = KNOWN) {
  const problems = [];
  const counts = new Map();
  for (const h of hits) counts.set(`${h.pkg}|${h.pattern}`, (counts.get(`${h.pkg}|${h.pattern}`) || 0) + 1);
  for (const [key, n] of counts) {
    const [pkg, pattern] = key.split("|");
    const allowed = known[pkg]?.[pattern]?.count ?? 0;
    if (n > allowed) problems.push(`${pkg}: ${n} x ${pattern} (audited: ${allowed})`);
  }
  for (const [pkg, byPattern] of Object.entries(known)) {
    for (const [pattern, { count }] of Object.entries(byPattern)) {
      const n = counts.get(`${pkg}|${pattern}`) || 0;
      if (n < count) problems.push(`${pkg}: audited ${count} x ${pattern} but found ${n}; update KNOWN`);
    }
  }
  return problems;
}

async function main() {
  const hits = await scan();
  if (!process.argv.includes("--quiet")) {
    for (const h of hits) console.log(`${h.pkg.padEnd(18)} ${h.kind.padEnd(4)} ${h.pattern.padEnd(15)} ${h.file}  ...${h.text}...`);
  }
  const problems = judge(hits);
  for (const p of problems) console.error("BAD " + p);
  console.log(problems.length ? "FAILED: dependencies do something that was not audited" : `ok: ${hits.length} hits in ${Object.keys(TARGETS).length} packages, all audited`);
  process.exit(problems.length ? 1 : 0);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
