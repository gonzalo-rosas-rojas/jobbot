// T35 (demo-publicado-sin-marcadores): the demo that is published (build.mjs demo --publish) is a static file that a host serves as it is, with no
// server after it to fill a placeholder. Until T35 it kept url("__ASSETS__/<font>"), so a static host looked for its fonts in a folder called
// __ASSETS__ (jobbot-showcase had to copy the five faces there by hand), and its policy names every inline script by its SHA-256, so an edit of
// the file after the build silently stops the demo's data script ("No se pudo hablar con el panel"). These tests build the published demo
// into a folder of their own and read what the build wrote: the fonts beside the page, nothing left for a server, and a check of the file
// alone (guard.mjs, publishedProblems) that the build runs on what it writes and that catches any edit after the hashes. No browser:
// test/demo.publicado.browser.test.mjs opens the same folder in Chrome.
import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import * as G from "../guard.mjs";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const FONTS_DIR = path.join(root, "assets", "fonts");
const FONTS = ["barlow-condensed-700.woff2", "geist-mono-400.woff2", "instrument-serif-400italic.woff2", "inter-400.woff2", "inter-600.woff2"];
const LICENCES = readdirSync(path.join(FONTS_DIR, "licenses")).sort();
// The placeholders of the build (page.template.html and the labs, the define of __DEFAULT_LANG__) and of the server (__NONCE__, __ASSETS__).
const PLACEHOLDERS = ["__ASSETS__", "__NONCE__", "__STYLE__", "__ICONS__", "__SCRIPT__", "__DEFAULT_LANG__"];

const tmp = mkdtempSync(path.join(os.tmpdir(), "panel-t35-"));
mkdirSync(path.join(root, "dist"), { recursive: true });
const entries = mkdtempSync(path.join(root, "dist", "t35-"));   // an --entry is read relative to panel-src, so the tiny ones live under dist/
process.on("exit", () => { for (const d of [tmp, entries]) rmSync(d, { recursive: true, force: true }); });
const build = (...args) => spawnSync(process.execPath, ["build.mjs", ...args], { cwd: root, encoding: "utf8", timeout: 60000 });
const entry = (name, code) => { const file = path.join(entries, name); writeFileSync(file, code); return path.relative(root, file); };

const PUB = path.join(tmp, "pub", "index.html");
const built = build("demo", "--publish", "--out", PUB);
const html = existsSync(PUB) ? readFileSync(PUB, "utf8") : "";
const DEV = path.join(tmp, "dev", "demo.html");
const dev = build("demo", "--out", DEV);
const devHtml = existsSync(DEV) ? readFileSync(DEV, "utf8") : "";

/** Every file under `dir`, as paths relative to it with "/". */
const tree = (dir, at = "") => existsSync(dir) ? readdirSync(dir).sort().flatMap((n) => statSync(path.join(dir, n)).isDirectory() ? tree(path.join(dir, n), `${at}${n}/`) : [`${at}${n}`]) : [];
const urls = (page) => [...page.matchAll(/url\(\s*(["']?)([^"')]*)\1\s*\)/g)].map((m) => m[2]);
const metaOf = (page) => [...page.matchAll(/<meta http-equiv="Content-Security-Policy" content="([^"]*)">/g)];
const policyOf = (page) => Object.fromEntries((metaOf(page)[0] || [, ""])[1].split(";").map((d) => d.trim().split(/\s+/)).filter((d) => d[0]).map(([k, ...v]) => [k, v]));
const sha = (text) => `'sha256-${createHash("sha256").update(text, "utf8").digest("base64")}'`;

/** The inline scripts and styles as a browser splits them (text inside a script is not a tag: the demo's own comments say "<style>"). */
function elements(page) {
  const out = [];
  const open = /<(script|style)(?=[\s>])[^>]*>/gi;
  for (let at = 0; ;) {
    open.lastIndex = at;
    const m = open.exec(page);
    if (!m) return out;
    const tag = m[1].toLowerCase();
    const end = page.toLowerCase().indexOf(`</${tag}`, m.index + m[0].length);
    out.push({ tag, start: m.index + m[0].length, end, body: page.slice(m.index + m[0].length, end) });
    at = end;
  }
}
/** The page with its policy named again from its bodies, as the build would: style-src keeps the hashes no inline style has (the demo's runtime one). */
function rehash(page) {
  const els = elements(page);
  const old = policyOf(page);
  const inlineStyles = els.filter((e) => e.tag === "style").map((e) => sha(e.body));
  const runtime = (old["style-src"] || []).filter((h) => !elements(html).some((e) => e.tag === "style" && sha(e.body) === h));
  const meta = metaOf(page)[0];
  const content = meta[1]
    .replace(/script-src [^;]*/, `script-src ${els.filter((e) => e.tag === "script").map((e) => sha(e.body)).join(" ")}`)
    .replace(/style-src [^;]*/, `style-src ${[...inlineStyles, ...runtime].join(" ")}`);
  return page.slice(0, meta.index) + meta[0].replace(meta[1], () => content) + page.slice(meta.index + meta[0].length);
}
/** Inserts `text` at `offset` characters into the body of the n-th inline <tag>. */
function insertInto(page, tag, n, text, offset = 0) {
  const e = elements(page).filter((x) => x.tag === tag)[n];
  return page.slice(0, e.start + offset) + text + page.slice(e.start + offset);
}

test("the published demo names its fonts as fonts/<name>, and the build writes the five faces and their licences there, beside the page", () => {
  assert.equal(built.status, 0, `the published build ran (${built.stderr})`);
  assert.deepEqual(urls(html).filter((u) => !u.startsWith("#")).sort(), FONTS.map((f) => `fonts/${f}`).sort(), "every url( of the page that is not a #fragment is one of the five fonts, by a path relative to the page");
  assert.deepEqual(tree(path.dirname(PUB)).sort(), [...FONTS.map((f) => "fonts/" + f), ...LICENCES.map((l) => "fonts/licenses/" + l), "index.html"].sort(),"the folder holds the page, the five fonts and their licences, and nothing else");
  for (const f of FONTS) assert.ok(readFileSync(path.join(path.dirname(PUB), "fonts", f)).equals(readFileSync(path.join(FONTS_DIR, f))), `${f} is the verified file of assets/fonts`);
  for (const l of LICENCES) assert.ok(readFileSync(path.join(path.dirname(PUB), "fonts", "licenses", l)).equals(readFileSync(path.join(FONTS_DIR, "licenses", l))), `${l} is the licence of assets/fonts/licenses`);
  const faces = readFileSync(path.join(root, "src", "styles", "fonts.css"), "utf8").replace(/\r\n/g, "\n").split("\n").filter(Boolean);
  assert.equal(faces.length, 5);
  for (const face of faces) assert.ok(html.includes(face.replace("__ASSETS__/", "fonts/")), `the face is written as in fonts.css but for its folder: ${face}`);
  const policy = policyOf(html);
  assert.deepEqual(policy["font-src"], ["'self'"], "the fonts come from the page's own host: the policy does not change");
  assert.ok(!/data:/.test(metaOf(html)[0][1]) && !/url\(\s*["']?data:/.test(html), "no font is inlined as data: (guard.mjs forbids url(data: in the page)");
});

test("nothing is left for a server to fill in the published demo, and the demo of the dev server keeps what tools/serve.mjs fills", () => {
  assert.equal(built.status, 0, `the published build ran (${built.stderr})`);
  for (const p of PLACEHOLDERS) assert.ok(!html.includes(p), `${p} is not in the published demo`);
  assert.ok(!/<(?:script|style)[^>]*\snonce=/.test(html), "no nonce on a tag");
  assert.deepEqual([...new Set(html.match(/__[A-Z][A-Z0-9_]*__/g) || [])], ["__DEMO__"], "the only __NAME__ in the file is window.__DEMO__, the demo's own server");
  assert.equal(dev.status, 0, `the dev demo built (${dev.stderr})`);
  for (const f of FONTS) assert.ok(devHtml.includes(`url("__ASSETS__/${f}")`), `the dev demo keeps __ASSETS__ for ${f}`);
  assert.ok(devHtml.includes('<script nonce="__NONCE__">') && !devHtml.includes("Content-Security-Policy"), "and its nonces, with no meta");
  assert.deepEqual(tree(path.dirname(DEV)), ["demo.html"], "and it writes no fonts folder");
});

test("the build refuses a published demo that still holds a placeholder, names it, and writes nothing; the same entry without it builds", () => {
  const ok = build("demo", "--publish", "--entry", entry("ok.js", 'document.body.dataset.t35 = "ok";'), "--out", path.join(tmp, "ok", "index.html"));
  assert.equal(ok.status, 0, `a tiny entry with no placeholder builds (${ok.stderr})`);
  for (const p of PLACEHOLDERS.filter((x) => x !== "__ASSETS__")) {   // __ASSETS__ is what --publish fills, like the server: its leftover is caught by publishedProblems below
    const out = path.join(tmp, `bad-${p.replace(/_/g, "")}`, "index.html");
    const r = build("demo", "--publish", "--entry", entry(`${p.replace(/_/g, "")}.js`, `document.body.dataset.t35 = "${p}";`), "--out", out);
    assert.notEqual(r.status, 0, `${p} left in the script: the build is refused`);
    assert.ok(r.stderr.includes(p), `and it says which placeholder (${r.stderr.trim()})`);
    assert.deepEqual(tree(path.dirname(out)), [], `and nothing was written for ${p}: no page, no fonts`);
  }
});

test("publishedProblems reads the file alone: the fresh build passes, an edit named again passes, and every edit after the hashes is caught", () => {
  assert.equal(typeof G.publishedProblems, "function", "guard.mjs exports publishedProblems(page)");
  assert.equal(built.status, 0, `the published build ran (${built.stderr})`);
  const check = G.publishedProblems;
  assert.deepEqual(check(html), [], "the page as the build wrote it");
  const scripts = elements(html).filter((e) => e.tag === "script");
  assert.equal(scripts.length, 3, "the head script, the demo and the page");
  const edited = insertInto(html, "script", 1, "/* edited by hand */", 1);
  const meta = metaOf(html)[0];
  const policyText = meta[1];
  const good = {
    "an edit, named again by its hashes": rehash(edited),
    "the hashes in another order": html.replace(/script-src ([^;]*)/, (_, list) => `script-src ${list.split(" ").reverse().join(" ")}`),
    "one more style hash, as the demo's runtime style has": html.replace("style-src ", `style-src 'sha256-${Buffer.alloc(32, 7).toString("base64")}' `),
    "a script that says <script> and <style> in its text, named again": rehash(insertInto(html, "script", 1, "/* <script> and <style> */\n")),
  };
  for (const [what, page] of Object.entries(good)) assert.deepEqual(check(page), [], what);
  const bad = {
    "the demo's data script edited by hand (as jobbot-showcase 82cc16b had to fix)": edited,
    "the head script edited": insertInto(html, "script", 0, "\n"),
    "the page script edited": insertInto(html, "script", 2, ";"),
    "the style edited": insertInto(html, "style", 0, "p{}"),
    "a script added": html.replace("</body>", "<script>document.title = 1</script>\n</body>"),
    "a script with an attribute added": html.replace("</body>", "<script defer>document.title = 1</script>\n</body>"),
    "a stale hash left in script-src": html.replace("script-src ", `script-src 'sha256-${Buffer.alloc(32).toString("base64")}' `),
    "a hash taken out of script-src": html.replace(policyText, () => policyText.replace(/ 'sha256-[^']*'(?=;\s*style-src)/, "")),
    "'unsafe-inline' put in script-src": html.replace("script-src ", "script-src 'unsafe-inline' "),
    "no meta policy": html.replace(meta[0], ""),
    "two meta policies": html.replace(meta[0], meta[0] + "\n" + meta[0]),
    "a nonce put back on a tag": html.replace("<script>", '<script nonce="abc">'),
    "a nonce on the style": html.replace("<style>", '<style nonce="abc">'),
    "a NONCE in capitals, in single quotes": html.replace("<script>", "<script NONCE='abc'>"),
    "a style added in the head": html.replace("</head>", "<style>p{color:red}</style>\n</head>"),
    "a style with an attribute added": html.replace("</head>", '<style media="all">p{}</style>\n</head>'),
    "a <SCRIPT> in capitals added": html.replace("</body>", "<SCRIPT>document.title = 1</SCRIPT>\n</body>"),
    "'self' put in script-src": html.replace("script-src ", "script-src 'self' "),
    "* put in script-src": html.replace("script-src ", "script-src * "),
  };
  for (const p of PLACEHOLDERS) bad[`${p} in the demo script, named again by its hashes`] = rehash(insertInto(html, "script", 1, `/* ${p} */\n`));
  bad["url(__ASSETS__/ left in the style, named again"] = rehash(html.replace('url("fonts/inter-400.woff2")', 'url("__ASSETS__/inter-400.woff2")'));
  for (const [what, page] of Object.entries(bad)) {
    assert.notEqual(page, html, `the edit changed the page: ${what}`);
    const found = check(page);
    assert.ok(Array.isArray(found) && found.length > 0, `caught: ${what}`);
    assert.ok(found.every((x) => typeof x === "string" && x.length > 0), `as words: ${what}`);
  }
  // Which one: a problem names the edited script or style by the SHA-256 of its body as it is now, and none names one that was not touched.
  const named = [
    ["the demo's data script edited", edited, "script", 1],
    ["the head script edited", insertInto(html, "script", 0, "\n"), "script", 0],
    ["the page script edited", insertInto(html, "script", 2, ";"), "script", 2],
    ["the style edited", insertInto(html, "style", 0, "p{}"), "style", 0],
  ];
  for (const [what, page, tag, n] of named) {
    const found = check(page).join("\n");
    const els = elements(page).filter((e) => e.tag === "script" || e.tag === "style");
    const target = elements(page).filter((e) => e.tag === tag)[n];
    assert.ok(found.includes(sha(target.body).slice(1, -1)), `${what}: a problem names it by its hash now (${found})`);
    for (const other of els) if (other !== target && other.start !== target.start) assert.ok(!found.includes(sha(other.body).slice(1, -1)), `${what}: no problem names an untouched <${other.tag}>`);
  }
});

test("the build checks the page it writes: buildDemo refuses on publishedProblems(html) after the last change to html and before writing anything", () => {
  const src = readFileSync(path.join(root, "build.mjs"), "utf8").replace(/\r\n/g, "\n");
  const at = src.indexOf("async function buildDemo(");
  assert.ok(at >= 0, "build.mjs has buildDemo");
  const next = src.slice(at + 1).search(/\n(?:async )?function |\nif \(labName\)/);
  const body = src.slice(at, next < 0 ? src.length : at + 1 + next);
  const call = body.search(/refuse\(\s*publishedProblems\(\s*html\s*\)/);
  assert.ok(call >= 0, "buildDemo calls refuse(publishedProblems(html), ...)");
  const lastChange = Math.max(...[...body.matchAll(/\bhtml\s*(?:=|\+=)(?!=)/g)].map((m) => m.index));
  assert.ok(call > lastChange, "after the last change to html (the meta policy is the last)");
  for (const write of [/writeFileSync\(/, /syncFonts\(/, /copyFileSync\(/, /cpSync\(/]) {
    const w = body.search(write);
    if (w >= 0) assert.ok(call < w, `before ${write.source.replace("\\(", "")}`);
  }
  assert.ok(/writeFileSync\(/.test(body), "buildDemo writes the page");
});

test("the fonts go through syncFonts to the folder of the page the build writes, the page is written once as it was checked, and the header of build.mjs says so", () => {
  const src = readFileSync(path.join(root, "build.mjs"), "utf8").replace(/\r\n/g, "\n");
  const at = src.indexOf("async function buildDemo(");
  assert.ok(at >= 0, "build.mjs has buildDemo");
  const next = src.slice(at + 1).search(/\n(?:async )?function |\nif \(labName\)/);
  const body = src.slice(at, next < 0 ? src.length : at + 1 + next);
  const sync = /refuse\(\s*syncFonts\(\s*\{([^}]*)\}\s*\)\.problems/.exec(body);
  assert.ok(sync, "buildDemo copies the fonts with refuse(syncFonts({ ... }).problems, ...): the faces verified against VENDORED.md, as --install does");
  const checked = body.search(/refuse\(\s*publishedProblems\(\s*html\s*\)/);
  assert.ok(checked >= 0 && checked < sync.index, "after the page was checked");
  const targetAt = body.search(/\bconst\s+target\s*=/);
  assert.ok(targetAt >= 0 && targetAt < sync.index, "and after const target = ...");
  // The `to` of syncFonts comes from path.dirname(target): written in place, or through a variable made from it.
  const fromTarget = (expr) => /path\.dirname\(\s*target\s*\)/.test(expr) || [...expr.matchAll(/\b([A-Za-z_$][\w$]*)\b/g)].some((m) => new RegExp(String.raw`\b(?:const|let)\s+${m[1].replace(/\$/g, "\\$")}\s*=[^;\n]*path\.dirname\(\s*target\s*\)`).test(body));
  const to = /\bto\s*:\s*([^,}]+)/.exec(sync[1]) || (/\bto\b/.test(sync[1]) ? [null, "to"] : null);
  assert.ok(to && fromTarget(to[1]), `the fonts go beside the written page, from path.dirname(target) (syncFonts({${sync[1]}}))`);
  for (const copy of body.matchAll(/\b(?:copyFileSync|cpSync)\(([^;\n]*)\)/g)) assert.ok(fromTarget(copy[1]), `every other copy goes beside the page too: ${copy[0]}`);
  const writes = [...body.matchAll(/writeFileSync\(([^;\n]*)\)/g)].map((m) => m[0].replace(/\s+/g, ""));
  assert.deepEqual(writes, ["writeFileSync(target,html)"], "the page is written once, as html was checked, and buildDemo writes nothing else with writeFileSync");
  const head = src.split("\n").slice(0, 20);
  const line = head.findIndex((l) => l.includes("demo --publish"));
  assert.ok(line >= 0, "the header of build.mjs names demo --publish");
  const entry = [head[line], ...head.slice(line + 1).filter((l, i, a) => a.slice(0, i + 1).every((x) => /^\/\/\s{8,}/.test(x)))];
  assert.ok(entry.some((l) => l.includes("fonts/")), `and its entry names the fonts/ folder beside the page: ${JSON.stringify(entry)}`);
});
