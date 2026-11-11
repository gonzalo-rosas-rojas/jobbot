import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { typeProblems } from "../guard.mjs";

// The style files that go into the page, listed the way build.mjs lists them: its explicit OWN list, then every file of styles/ui
// whose name does not start with "_". A page build runs the same rules over the whole style at the end; this test is how a new sheet
// learns it broke them the day it is written, long before there is a page to build.
const ROOT = new URL("../", import.meta.url);
const OWN = ["tokens", "fonts", "base", "stage", "dial", "sheet", "actions", "moments", "menu"];
const files = () => [
  ...OWN.map((n) => `src/styles/${n}.css`).filter((p) => existsSync(new URL(p, ROOT))),
  ...readdirSync(new URL("src/styles/ui/", ROOT)).filter((n) => n.endsWith(".css") && !n.startsWith("_")).map((n) => `src/styles/ui/${n}`),
];
const read = (p) => readFileSync(new URL(p, ROOT), "utf8").replace(/\r\n/g, "\n");
const code = (p) => read(p).replace(/\/\*[\s\S]*?\*\//g, ""); // a comment may name a token it does not use

test("every style of the page keeps the closed type scale, writes no font shorthand and no pure black or white", () => {
  const list = files();
  assert.ok(list.length >= 9, list.join(", "));
  for (const file of list) {
    const css = read(file);
    const problems = typeProblems(css);
    for (const hex of ["#000", "#fff"]) if (new RegExp(`${hex}(?:${hex.slice(1)})?(?![0-9a-fA-F])`, "i").test(css)) problems.push(`has ${hex}`);
    assert.deepEqual(problems, [], file);
  }
});

test("no style asks for a custom property that nothing defines (a token of the old page that no longer exists)", () => {
  const all = files().map(code).join("\n");
  const defined = new Set([...all.matchAll(/(--[\w-]+)\s*:/g)].map((m) => m[1]));
  // the ones the scripts write on an element (style.setProperty, a `style: { "--w": 1 }` object, a helper that takes the name), which no
  // stylesheet declares: a name in quotes in a script is a name the script sets
  const walk = (dir) => readdirSync(new URL(dir, ROOT), { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(`${dir}${e.name}/`) : e.name.endsWith(".js") ? [`${dir}${e.name}`] : []));
  for (const file of walk("src/")) for (const m of read(file).matchAll(/["'](--[\w-]+)["']/g)) defined.add(m[1]);
  const missing = new Set();
  for (const file of files()) {
    for (const m of code(file).matchAll(/var\(\s*(--[\w-]+)\s*(?:,[^)]*)?\)/g)) {
      const fallback = /var\(\s*--[\w-]+\s*,/.test(m[0]);
      if (!defined.has(m[1]) && !fallback) missing.add(`${file}: ${m[1]}`);
    }
  }
  assert.deepEqual([...missing], []);
});

// Spec §4.8: "sin columnas con borde, píldoras ni chips con borde". The drawer is the one sheet that has buttons, tags and a text field of
// its own, so it is where the old page's boxed controls would come back: a button here is the panel's one action button (styles/actions.css),
// a tag is words, and the only rounded things are the round markers of a line (the Recorrido's nodes, its dots, the bullets).
test("the drawer's skin draws no boxed button, no bordered tag and no pill (spec 4.8)", () => {
  for (const file of ["src/styles/ui/drawer.css", "src/styles/ui/detail.css"]) {
    const css = code(file);
    for (const rule of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
      const selector = rule[1].trim();
      const body = rule[2];
      if (/(^|[\s,.])(btn|chip)\b/.test(selector) && /(^|[\s;])border(-top|-right|-left)?\s*:\s*(?!0\b|none\b|1px solid transparent)/.test(body)) {
        assert.fail(`${file}: "${selector}" draws a border around a button or a tag`);
      }
      const radius = /border-radius\s*:\s*([^;]+)/.exec(body);
      if (radius && radius[1].trim() !== "50%") assert.fail(`${file}: "${selector}" rounds a corner (${radius[1].trim()})`);
      if (radius && !/(node|dot|::before|::after|marker)/.test(selector)) assert.fail(`${file}: "${selector}" is round and is not a marker`);
    }
  }
});
