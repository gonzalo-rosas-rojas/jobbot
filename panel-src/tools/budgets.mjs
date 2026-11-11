// Per-area budgets of the page's script. guard.mjs holds the two global caps (480 KB of script, 200 KB gzipped for the page); they say THAT the page is too big, not
// WHERE it grew. This module groups the bytes that esbuild put in the minified bundle (the metafile) by area: a layer (core, ui, scene, stage, sound, main), the two
// dictionaries (i18n/es, i18n/en), each moment (moments/hoy, moments/cola, ...) and each library (lib/lenis, ...). build.mjs prints the table at every build of the page
// and refuses to write it when an area grew past its margin over the baseline in budgets.json, so growth is a decision that shows up in a diff and not an accident.
//
//   node build.mjs                    prints the table, refuses when an area is over its margin
//   node build.mjs --update-budgets   accepts the sizes as they are: rewrites budgets.json (commit it, with the reason in the message)
//
// The global caps are not touched here. Shrinking never fails (the table says when the baseline has slack worth tightening).

import fs from "node:fs";

// An area may grow this much over its baseline before the build refuses: the larger of an absolute number and a share of the baseline; the whole script has its own.
export const MARGIN = { kb: 3, pct: 5, totalKb: 6, totalPct: 1.5 };

/** The area a bundled input belongs to: "ui", "core", "i18n/es", "moments/cola", "lib/lenis", "main"; anything else is "other". */
export function areaOf(file) {
  const p = String(file).replace(/\\/g, "/");
  const lib = /(?:^|\/)node_modules\/((?:@[^/]+\/)?[^/]+)\//.exec(p);
  if (lib) return "lib/" + lib[1].replace(/^@[^/]+\//, "");
  const m = /^src\/(.+)$/.exec(p);
  if (!m) return "other";
  const parts = m[1].split("/");
  if (parts.length === 1) return parts[0] === "main.js" ? "main" : "other";
  const [layer, second] = parts;
  if (layer === "i18n") return parts.length > 2 && (second === "es" || second === "en") ? "i18n/" + second : "i18n";
  if (layer === "moments") return "moments/" + second.replace(/\.js$/, "");
  return layer;
}

/** { areas: { name: bytes }, total } from the esbuild metafile of the minified bundle and the size of the script that was written (the few bytes of esbuild's own wrapper are in the total only). */
export function measure(metafile, jsBytes) {
  const out = Object.values(metafile.outputs)[0];
  const areas = {};
  for (const [file, info] of Object.entries(out.inputs)) {
    const bytes = info.bytesInOutput || 0;
    if (!bytes) continue;
    const area = areaOf(file);
    areas[area] = (areas[area] || 0) + bytes;
  }
  return { areas, total: jsBytes };
}

export const allowance = (base, kb, pct) => Math.max(kb * 1024, Math.round((base * pct) / 100));
const kb = (n) => (n / 1024).toFixed(1) + " KB";

/**
 * Judges the sizes against the baseline ({ total, areas }). Returns { rows, problems, slack }: `rows` for the table, `problems` (one sentence each) when an
 * area, a NEW area or the whole script is over its margin, `slack` the areas that shrank by more than their margin (the baseline could be tightened).
 */
export function compare(current, baseline, margin = MARGIN) {
  const base = baseline && baseline.areas ? baseline.areas : null;
  const rows = [];
  const problems = [];
  const slack = [];
  for (const [area, bytes] of Object.entries(current.areas).sort((a, b) => b[1] - a[1])) {
    const was = base && area in base ? base[area] : null;
    const limit = base ? allowance(was || 0, margin.kb, margin.pct) : null;
    const over = base !== null && bytes > (was || 0) + limit;
    rows.push({ area, bytes, was, delta: was === null ? null : bytes - was, over });
    if (over) problems.push(`${was === null ? "new area" : "area"} "${area}" is ${kb(bytes)}, ${kb(bytes - (was || 0))} over ${was === null ? "an empty baseline" : `its baseline of ${kb(was)}`} (the margin is ${kb(limit)})`);
    else if (was !== null && was - bytes > limit) slack.push(area);
  }
  let totalOver = false;
  if (base && baseline.total != null) {
    const limit = allowance(baseline.total, margin.totalKb, margin.totalPct);
    totalOver = current.total > baseline.total + limit;
    if (totalOver) problems.push(`the script is ${kb(current.total)}, ${kb(current.total - baseline.total)} over its baseline of ${kb(baseline.total)} (the margin is ${kb(limit)})`);
  }
  return { rows, problems, slack, totalOver };
}

/** The table build.mjs prints: biggest area first, with its share and its distance from the baseline. */
export function table(current, result) {
  const w = Math.max(...result.rows.map((r) => r.area.length), 5);
  const line = (a, b, c, d) => `  ${a.padEnd(w)}  ${b.padStart(9)}  ${c.padStart(6)}  ${d}`;
  const out = [line("area", "script", "share", "vs baseline")];
  for (const r of result.rows) {
    const vs = r.was === null ? "new" : `${r.delta >= 0 ? "+" : "-"}${kb(Math.abs(r.delta))}`;
    out.push(line(r.area, kb(r.bytes), ((100 * r.bytes) / current.total).toFixed(1) + "%", vs + (r.over ? "  OVER THE MARGIN" : "")));
  }
  out.push(line("total", kb(current.total), "100.0%", result.totalOver ? "OVER THE MARGIN" : ""));
  if (result.slack.length) out.push(`  the baseline has slack in ${result.slack.join(", ")}: node build.mjs --update-budgets tightens it`);
  return out.join("\n");
}

/** The baseline, or null when the file does not exist. A file that exists but cannot be read or is not JSON throws an Error that names it: a broken baseline must not switch the budgets off. */
export function readBaseline(file) {
  let text;
  try { text = fs.readFileSync(file, "utf8"); } catch (e) {
    if (e && e.code === "ENOENT") return null;
    throw new Error(`${file} exists but cannot be read (${e.message})`);
  }
  try { return JSON.parse(text); } catch (e) { throw new Error(`${file} is not valid JSON (${e.message}); fix it or rebuild it with node build.mjs --update-budgets`); }
}

/** budgets.json: the sizes accepted, keys in order so a diff shows exactly what moved. */
export function writeBaseline(file, current) {
  const areas = Object.fromEntries(Object.entries(current.areas).sort(([a], [b]) => a.localeCompare(b)));
  const note = "Per-area baseline of the minified script (bytes). node build.mjs refuses to write the page when an area, or the total, grows past its margin over this file "
    + "(tools/budgets.mjs, MARGIN). To accept growth on purpose: node build.mjs --update-budgets, and commit this file with the reason. The global caps live in guard.mjs.";
  fs.writeFileSync(file, JSON.stringify({ note, total: current.total, areas }, null, 2) + "\n");
}
