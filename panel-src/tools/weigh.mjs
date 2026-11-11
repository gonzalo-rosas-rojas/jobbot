// What each foundation library costs the page: it is bundled alone, the way build.mjs bundles (minified IIFE, es2022, no
// legal comments, named imports so tree shaking can work) and measured raw and gzipped at level 9 (R8 §2 rule 9: "peso
// medido con el esbuild del proyecto"). BUDGETS are R8 §1's figures with a little room; test/weights.test.mjs enforces them.
//
//   node tools/weigh.mjs            prints the table
//   node tools/weigh.mjs --json     the same as JSON

import { build } from "esbuild";
import { gzipSync } from "node:zlib";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.join(here, "..");

// Each entry is the import the page makes, and one use of it so the bundler keeps it.
export const ENTRIES = {
  lenis: 'import Lenis from "lenis"; globalThis.__w = Lenis;',
  tinykeys: 'import { tinykeys } from "tinykeys"; globalThis.__w = tinykeys;',
  uFuzzy: 'import uFuzzy from "@leeoniya/ufuzzy"; globalThis.__w = uFuzzy;',
};

// gzip bytes. R8 §1 measured Lenis at 5.5 KB, tinykeys at 1.1 and uFuzzy at 4.0; each budget is that with a little room.
export const BUDGETS = {
  lenis: 7 * 1024,
  tinykeys: 2 * 1024,
  uFuzzy: 6 * 1024,
};
export const TOTAL_BUDGET = 14 * 1024;

export async function weigh(entries = ENTRIES) {
  const rows = [];
  for (const [name, code] of Object.entries(entries)) {
    const out = await build({
      stdin: { contents: code, resolveDir: ROOT, loader: "js" },
      bundle: true, minify: true, format: "iife", target: "es2022", write: false, legalComments: "none", logLevel: "silent",
      define: { "process.env.NODE_ENV": '"production"' },
    });
    const text = out.outputFiles[0].contents;
    rows.push({ name, raw: text.length, gzip: gzipSync(text, { level: 9 }).length });
  }
  return rows;
}

async function main() {
  const rows = await weigh();
  const total = rows.reduce((n, r) => n + r.gzip, 0);
  if (process.argv.includes("--json")) { console.log(JSON.stringify({ rows, total }, null, 2)); return; }
  const kb = (n) => (n / 1024).toFixed(1).padStart(6) + " KB";
  for (const r of rows) console.log(`${r.name.padEnd(46)} raw ${kb(r.raw)}  gzip ${kb(r.gzip)}  budget ${kb(BUDGETS[r.name] || 0)}`);
  console.log(`${"together".padEnd(46)} raw ${kb(rows.reduce((n, r) => n + r.raw, 0))}  gzip ${kb(total)}  budget ${kb(TOTAL_BUDGET)}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
