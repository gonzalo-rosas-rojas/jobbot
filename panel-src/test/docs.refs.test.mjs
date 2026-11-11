// The documents an agent reads first must not lie about the tree. Every repo path that AGENT-RULES.md, README.md and the records of docs/adr/ put in code
// (or in a markdown link) has to exist, and the names of things that are gone (the replaced spec, the old worktree, a function that never existed) must not
// come back into the live documents.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");   // panel-src
const REPO = path.join(ROOT, "..");
const BASES = [ROOT, REPO, path.join(ROOT, "src")];                          // the documents also write paths relative to src/ ("ui/hud.js")
// not paths: a git branch and the name of a library
const NOT_PATHS = new Set(["design/turno-continuado", "anime.js"]);
const EXT = new Set(["js", "mjs", "md", "json", "css", "html", "rs", "toml", "svg", "woff2", "ps1", "cmd"]);
const SKIP_DIRS = new Set(["node_modules", "dist", ".vendor-src", ".git"]);

/** The inline `code` spans and fenced blocks of a markdown text. */
function codeOf(md) {
  const out = [];
  const rest = md.replace(/```[^\n]*\n([\s\S]*?)```/g, (_, body) => { out.push(body); return ""; });
  for (const m of rest.matchAll(/`([^`\n]+)`/g)) out.push(m[1]);
  return out;
}

/** `a/{x,y}/b` -> [`a/x/b`, `a/y/b`] (one group, which is all the documents use). */
function braces(token) {
  const m = /^(.*)\{([^{}]*)\}(.*)$/.exec(token);
  return m ? m[2].split(",").map((alt) => m[1] + alt + m[3]) : [token];
}

/** The tokens of the code that look like a path of the repo, with their placeholders and globs intact. */
function pathsIn(md) {
  const found = new Set();
  for (const block of codeOf(md)) {
    for (const raw of block.replace(/<[^>]*>/g, "<x>").split(/[\s,;()]+/)) {
      for (const token of braces(raw.replace(/^[\[\]"'`]+|[\[\]"'`.:]+$/g, ""))) {
        if (!token || NOT_PATHS.has(token) || /^(https?:|--|__|\/|[A-Za-z]:|dist\/|node_modules\/)/.test(token) || token.includes("\\")) continue;
        if (!/^(?:\.\.\/)*[\w.@<>*\-]+(?:\/[\w.@<>*\-]+)*\/?$/.test(token)) continue;
        const last = token.split("/").pop();
        const isFile = last.includes(".") && EXT.has(last.split(".").pop());
        const isDir = token.endsWith("/") || (token.includes("/") && !last.includes("."));
        if (isDir || isFile) found.add(token);
      }
    }
  }
  return [...found];
}

function listAll(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    if (e.isDirectory()) { if (!SKIP_DIRS.has(e.name)) listAll(path.join(dir, e.name), out); } else out.push(e.name);
  }
  return out;
}
const NAMES = new Set(listAll(ROOT));                                         // every file name under panel-src (a bare `registry.js` means one of them)
for (const e of fs.readdirSync(REPO, { withFileTypes: true })) if (e.isFile()) NAMES.add(e.name);

const globRe = (name) => new RegExp("^" + name.replace(/[.+^$|\\]/g, "\\$&").replace(/\*/g, "[^/]*") + "$");

/** Does `token` name something that exists? A `<placeholder>` segment is checked up to the segment before it; a `*` must match at least one entry. */
function exists(token, bases = BASES, names = NAMES) {
  if (!token.includes("/") && !token.includes("<")) {
    return token.includes("*") ? [...names].some((n) => globRe(token).test(n)) : names.has(token);
  }
  const parts = token.replace(/\/$/, "").split("/");
  const cut = parts.findIndex((p) => p.includes("<"));
  const known = cut < 0 ? parts : parts.slice(0, cut);
  return bases.some((base) => {
    let here = base;
    for (let i = 0; i < known.length; i++) {
      const seg = known[i];
      if (seg === "..") { here = path.resolve(here, ".."); continue; }
      if (!seg.includes("*")) { here = path.join(here, seg); if (!fs.existsSync(here)) return false; continue; }
      if (!fs.existsSync(here) || !fs.statSync(here).isDirectory()) return false;
      const match = fs.readdirSync(here).filter((n) => globRe(seg).test(n));
      if (i < known.length - 1) return match.some((n) => exists(known.slice(i + 1).join("/"), [path.join(here, n)], names));
      return match.length > 0;
    }
    return true;
  });
}

/** Relative links `[text](target)` of a markdown file whose target is missing (a http link or an in-page anchor is not checked). */
function brokenLinks(md, dir) {
  const bad = [];
  for (const m of md.matchAll(/\]\(([^)\s]+)\)/g)) {
    const target = m[1].split("#")[0];
    if (!target || /^[a-z]+:/i.test(target)) continue;
    if (!fs.existsSync(path.resolve(dir, target))) bad.push(m[1]);
  }
  return bad;
}

const DOCS = ["AGENT-RULES.md", "README.md", ...fs.readdirSync(path.join(ROOT, "docs", "adr")).filter((f) => f.endsWith(".md")).map((f) => path.join("docs", "adr", f))];
const read = (rel) => fs.readFileSync(path.join(ROOT, rel), "utf8");

test("every repo path cited in AGENT-RULES.md, README.md and the ADRs exists", () => {
  const missing = [];
  for (const rel of DOCS) {
    const md = read(rel);
    for (const token of pathsIn(md)) if (!exists(token)) missing.push(`${rel}: ${token}`);
    for (const link of brokenLinks(md, path.dirname(path.join(ROOT, rel)))) missing.push(`${rel}: link ${link}`);
  }
  assert.deepEqual(missing, [], "the document names a path that is not in the tree: fix the document (or restore the file)");
});

test("the checker sees the paths of the documents and refuses a path that is not there", () => {
  const rules = pathsIn(read("AGENT-RULES.md"));
  for (const must of ["src/ui/shell.js", "src/moments/registry.js", "guard.mjs", "docs/adr/README.md"]) assert.ok(rules.includes(must), `${must} should be found in AGENT-RULES.md`);
  assert.ok(pathsIn(read("README.md")).length > 30, "README.md should cite dozens of paths");
  assert.deepEqual(pathsIn("see `src/core/nothing-here.js`, `src/<x>.js` and `https://a.b/c.js` and `dist/page.html`"), ["src/core/nothing-here.js", "src/<x>.js"]);
  assert.equal(exists("src/core/nothing-here.js"), false);
  assert.equal(exists("src/core/dom.js"), true);
  assert.equal(exists("src/moments/<id>.js"), true);
  assert.equal(exists("src/nothing/<id>.js"), false);
  assert.equal(exists("test/*.browser.test.mjs"), true);
  assert.equal(exists("test/*.nothing.mjs"), false);
  assert.equal(exists("registry.js"), true);
  assert.equal(exists("src/i18n/es/"), true);
  assert.deepEqual(brokenLinks("[a](0001-esbuild-to-one-inline-page.md) [b](nope.md) [c](https://x.y) [d](#top)", path.join(ROOT, "docs", "adr")), ["nope.md"]);
});

test("the live documents do not name what is gone", () => {
  const live = [...DOCS, path.join("src", "moments", "registry.js")];
  const stale = /turno-de-dia|estacion-viva|setFocusStop/;
  const hits = live.filter((rel) => stale.test(read(rel)));
  assert.deepEqual(hits, [], "the replaced spec, the old worktree and setFocusStop (which does not exist) belong only to archive/ and to historical notes");
});
