// Builds the panel page from panel-src/ into ONE html file with exactly two nonce'd <script> tags and one nonce'd <style>
// tag (what the Rust server and its page guard expect), after checking the result against the same rules the Rust tests
// enforce (guard.mjs). Node only; the Rust build never needs it because the generated page is committed.
//
//   node build.mjs                    -> dist/page.html            (checked, minified)
//   node build.mjs --dev              -> dist/page.html            (checked, readable)
//   node build.mjs --install          -> ../crates/jobbot-panel/src/page.html    (the page the Rust binary embeds) and the fonts into crates/jobbot-panel/src/assets
//   node build.mjs lab <name>         -> dist/lab-<name>.html      (lab/<name>.html + lab/<name>.entry.js; markers /*__STYLE__*/
//                                                                   and <!--__ICONS__--> are optional)
//   node build.mjs demo               -> dist/demo.html            (the page + scripts/demo/demo.js, fictional data, still nonce'd so
//                                                                   tools/serve.mjs can serve it under the real CSP)
//   node build.mjs demo --publish     -> ../docs/demo/index.html   (same, nonces removed, English by default; opens from disk)
//                                                                   and, beside the page, fonts/ with the five faces and fonts/licenses/
//                                                                   with their licences: a static host serves the folder as it is
//   add --out dist/<name>.html to a page or demo build to write to a file of your own (parallel agents)
//   add --entry <file> to a page or demo build to bundle another entry than src/main.js (tests use a tiny one)
//   The real page also prints its script by area (tools/budgets.mjs) and is refused when an area, or the whole script, grew past its margin over
//   budgets.json; --update-budgets accepts the sizes as they are (commit the file with the reason), --budgets <file> reads another baseline.

import { build, transform } from "esbuild";
import { readFileSync, writeFileSync, mkdirSync, readdirSync, existsSync, statSync } from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { guard, publishedProblems } from "./guard.mjs";
import { syncFonts } from "./tools/assets-sync.mjs";
import { measure, compare, table, readBaseline, writeBaseline } from "./tools/budgets.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const dev = args.includes("--dev");
const install = args.includes("--install");
const labName = args[0] === "lab" ? args[1] : null;
const demoMode = args[0] === "demo";
const publish = args.includes("--publish");
const val = (name) => (args.includes(name) ? args[args.indexOf(name) + 1] : null);
const entryArg = val("--entry") || "src/main.js";
// --out dist/<name>.html builds to a file of your own, so agents building in parallel never overwrite each other.
const outArg = val("--out");
const outPath = (fallback) => (outArg ? path.resolve(here, outArg) : fallback);

const kb = (n) => (n / 1024).toFixed(1) + " KB";
const read = (p) => readFileSync(path.join(here, p), "utf8").replace(/\r\n/g, "\n");

function listCss(dir) {
  const full = path.join(here, dir);
  if (!existsSync(full)) return [];
  const out = [];
  for (const name of readdirSync(full).sort()) {
    const p = path.join(dir, name);
    if (statSync(path.join(here, p)).isDirectory()) out.push(...listCss(p));
    else if (name.endsWith(".css")) out.push(p);
  }
  return out;
}

// The styles of the page, in order. tokens.css and fonts.css are written as they are (the guard and the colour tests read
// their blocks); the Lenis stylesheet comes from the pinned package right after base.css, so there is no copy to drift.
// The list is explicit (the sheets of the v2 tower are in archive/2026-10-07-torre/), and a file under styles/ui whose name starts
// with "_" is left out. Every other sheet of styles/ui is in the page (the palette's, the last one kept out by name, came in with its new skin).
const OWN = ["tokens", "fonts", "base", "stage", "dial", "sheet", "actions", "moments", "menu"].map((n) => `src/styles/${n}.css`);
const cssFiles = () => [
  ...OWN.filter((p) => existsSync(path.join(here, p))),
  ...listCss("src/styles/ui").filter((p) => !path.basename(p).startsWith("_")),
];
const RAW = ["tokens.css", "fonts.css"];
const VENDOR_CSS = ["node_modules/lenis/dist/lenis.css"];

async function vendorCss() {
  const parts = [];
  for (const file of VENDOR_CSS) parts.push((await transform(read(file), { loader: "css", minify: !dev, legalComments: "none" })).code.trim());
  return parts.join("\n");
}

async function styles() {
  const parts = [];
  for (const file of cssFiles()) {
    const text = read(file);
    parts.push(dev || RAW.some((n) => file.endsWith(n)) ? text : (await transform(text, { loader: "css", minify: true, legalComments: "none" })).code.trim());
    if (file.endsWith("base.css")) parts.push(await vendorCss());
  }
  return parts.join("\n");
}

/** The bundle and the list of files esbuild put in it (the guard looks for three among them). */
async function bundle(entry, defaultLang = "es") {
  const result = await build({
    entryPoints: [path.join(here, entry)],
    absWorkingDir: here,
    bundle: true,
    minify: !dev,
    format: "iife",
    target: "es2022",
    write: false,
    legalComments: "none",
    logLevel: "warning",
    metafile: true,
    define: { "process.env.NODE_ENV": '"production"', __DEFAULT_LANG__: JSON.stringify(defaultLang) },
  });
  return { js: result.outputFiles[0].text, inputs: Object.keys(result.metafile.inputs), metafile: result.metafile };
}

// A function replacer, never a string: minified code is full of `$&` and `$'`, which String.replace would expand.
function put(html, marker, text) {
  if (!html.includes(marker)) throw new Error(`template has no ${marker}`);
  return html.replace(marker, () => text);
}

function refuse(problems, what) {
  if (!problems.length) return;
  console.error(`${what} refused:\n - ` + problems.join("\n - "));
  process.exit(1);
}

// The per-area budgets (tools/budgets.mjs) judge the real page only: a build with another --entry (tests use a tiny one) has other areas and no baseline.
const BASELINE = val("--budgets") ? path.resolve(here, val("--budgets")) : path.join(here, "budgets.json");   // --budgets <file>: another baseline (the tests use one of their own)
const updateBudgets = args.includes("--update-budgets");

async function buildPage() {
  const { js, inputs, metafile } = await bundle(entryArg);
  const css = await styles();
  let html = read("page.template.html");
  html = put(html, "/*__STYLE__*/", css);
  html = put(html, "<!--__ICONS__-->", read("assets/icons.svg").trim());
  html = put(html, "/*__SCRIPT__*/", js);
  refuse(guard(html, js, { strictPage: true, inputs }), "build");
  const judged = entryArg === "src/main.js" && !dev;
  const sizes = judged ? measure(metafile, Buffer.byteLength(js)) : null;
  let verdict = null;
  if (judged) {
    let baseline = null;
    try { baseline = readBaseline(BASELINE); } catch (e) {
      if (!updateBudgets) refuse([e.message], "build");   // --update-budgets is how a broken file is replaced
    }
    verdict = compare(sizes, baseline);
    if (!baseline && !updateBudgets) console.error("budgets.json is missing: the per-area budgets are not being checked (node build.mjs --update-budgets creates it)");
    if (!updateBudgets && verdict.problems.length) {
      console.log(table(sizes, verdict));
      refuse([...verdict.problems, "if the growth is meant, run `node build.mjs --update-budgets` and commit budgets.json with the reason; if not, the table above says where it is"], "build");
    }
  }
  if (install) refuse(syncFonts().problems, "font copy");
  const target = outPath(install ? path.join(here, "..", "crates", "jobbot-panel", "src", "page.html") : path.join(here, "dist", "page.html"));
  mkdirSync(path.dirname(target), { recursive: true });
  writeFileSync(target, html);
  console.log(`page ${kb(html.length)} (script ${kb(js.length)}, style ${kb(css.length)}) -> ${path.relative(process.cwd(), target)}`);
  if (judged && updateBudgets) {
    writeBaseline(BASELINE, sizes);
    verdict = compare(sizes, readBaseline(BASELINE));
    console.log(`budgets.json updated: ${Object.keys(sizes.areas).length} areas, script ${kb(sizes.total)} (commit it with the reason for the growth)`);
  }
  if (judged) console.log(table(sizes, verdict));
}

async function buildLab(name) {
  const entry = `lab/${name}.entry.js`;
  if (!existsSync(path.join(here, entry))) throw new Error(`no ${entry}`);
  const { js } = await bundle(entry);
  let html = read(`lab/${name}.html`);
  html = put(html, "/*__SCRIPT__*/", js);
  if (html.includes("<!--__ICONS__-->")) html = put(html, "<!--__ICONS__-->", read("assets/icons.svg").trim());
  if (html.includes("/*__STYLE__*/")) html = put(html, "/*__STYLE__*/", await styles());
  refuse(guard(html, js, { strictPage: false }), "lab build");
  const target = path.join(here, "dist", `lab-${name}.html`);
  mkdirSync(path.dirname(target), { recursive: true });
  writeFileSync(target, html);
  console.log(`lab ${name} ${kb(html.length)} (script ${kb(js.length)}) -> dist/lab-${name}.html`);
}

/**
 * The policy of the published demo, for a <meta http-equiv>. The real page gets its policy in a header, with a nonce; the demo is a static
 * file on GitHub Pages, so the nonces are removed and each inline script and style is named by its SHA-256 instead. The demo draws its own
 * data and never asks the network, hence connect-src 'none' (the real page has 'self'). frame-ancestors is not allowed in a meta, so it is
 * left out rather than written where it would do nothing.
 */
function demoPolicy(html, runtimeStyles = []) {
  const sha = (text) => `'sha256-${createHash("sha256").update(text, "utf8").digest("base64")}'`;
  const hashes = (tag) => [...html.matchAll(new RegExp(`<${tag}>([\\s\\S]*?)</${tag}>`, "g"))].map((m) => sha(m[1]));
  return [
    "default-src 'none'", `script-src ${hashes("script").join(" ")}`, `style-src ${[...hashes("style"), ...runtimeStyles.map(sha)].join(" ")}`, "connect-src 'none'", "img-src 'self' blob:",
    "font-src 'self'", "base-uri 'none'", "form-action 'none'", "require-trusted-types-for 'script'",
  ].join("; ");
}

/**
 * The <style> elements that the demo script makes while it runs (today: the note that says "demo"). A policy by hash has to name their text,
 * so it is read from the source: `const x = document.createElement("style")` followed by `x.textContent = "..." + "..."`, plain literals only.
 * test/demo.csp.test.mjs opens the demo in Chrome and fails on the violation if the script ever builds its style some other way.
 */
function runtimeStyles(demoSource) {
  const out = [];
  for (const made of demoSource.matchAll(/(?:const|let|var)\s+(\w+)\s*=\s*document\.createElement\(\s*"style"\s*\)/g)) {
    const rest = demoSource.slice(made.index + made[0].length);
    const at = rest.indexOf(made[1] + ".textContent");
    if (at < 0) continue;
    const literals = /^\s*=\s*((?:\s*\+?\s*"(?:[^"\\\n]|\\.)*")+)\s*;/.exec(rest.slice(at + made[1].length + ".textContent".length));
    if (literals) out.push([...literals[1].matchAll(/"((?:[^"\\\n]|\\.)*)"/g)].map((m) => JSON.parse(`"${m[1]}"`)).join(""));
  }
  return out;
}

async function buildDemo() {
  const { js } = await bundle(entryArg, publish ? "en" : "es");
  let html = read("page.template.html");
  html = put(html, "/*__STYLE__*/", await styles());
  html = put(html, "<!--__ICONS__-->", read("assets/icons.svg").trim());
  html = put(html, "/*__SCRIPT__*/", js);
  const demo = readFileSync(path.join(here, "..", "scripts", "demo", "demo.js"), "utf8").replace(/\r\n/g, "\n");
  const marker = '<script nonce="__NONCE__">';
  const at = html.lastIndexOf(marker); // the page's own script is the last one; the one in the head only picks theme and language
  html = html.slice(0, at) + marker + "\n" + demo + "\n</script>\n" + html.slice(at);
  refuse(guard(html, js, { strictPage: false }), "demo build");
  const target = outPath(publish ? path.join(here, "..", "docs", "demo", "index.html") : path.join(here, "dist", "demo.html"));
  if (publish) {
    html = html.replaceAll(' nonce="__NONCE__"', "").replaceAll('url("__ASSETS__/', 'url("fonts/');
    html = put(html, '<meta charset="utf-8">', `<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="${demoPolicy(html, runtimeStyles(demo))}">`);
    refuse(publishedProblems(html), "published demo");
    refuse(syncFonts({ to: path.join(path.dirname(target), "fonts"), licenses: true }).problems, "font copy");
  }
  mkdirSync(path.dirname(target), { recursive: true });
  writeFileSync(target, html);
  console.log(`demo ${kb(html.length)} -> ${path.relative(process.cwd(), target)}`);
}

if (labName) await buildLab(labName);
else if (demoMode) await buildDemo();
else await buildPage();
