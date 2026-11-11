// The sound lab lays itself out with its own small stylesheet, but the build also pours the whole page CSS into it (it needs the
// tokens and the fonts). A class of the lab that the page CSS defines too would be restyled by the page: that is how the frame's
// `.bar` once pushed the lab's "enable" button under another one and the sound tests could not click it. This test makes the
// next clash a one-line failure with the name of the class, instead of three timeouts in a browser.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(path.join(root, p), "utf8");

// The same set build.mjs pours into a lab: src/styles/*.css and src/styles/ui/*.css, minus what starts with "_" (archived sheets).
function pageCssFiles() {
  const list = (dir) => readdirSync(path.join(root, dir)).filter((f) => f.endsWith(".css") && !f.startsWith("_")).map((f) => `${dir}/${f}`);
  return [...list("src/styles"), ...list("src/styles/ui")];
}

function classSelectors(css) {
  const bare = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const found = new Set();
  // Only what stands before a `{` is a selector; declarations (`url(a.png)`, `1.5`) never match `.name` there.
  for (const rule of bare.matchAll(/([^{}]+)\{/g)) for (const m of rule[1].matchAll(/\.([a-zA-Z_][\w-]*)/g)) found.add(m[1]);
  return found;
}

function labClasses(name) {
  const found = new Set();
  for (const m of read(`lab/${name}.entry.js`).matchAll(/\bclass\s*:\s*"([^"]+)"/g)) for (const c of m[1].split(/\s+/)) if (c) found.add(c);
  return found;
}

test("the sound lab uses no class that the page CSS also defines", () => {
  const page = new Set();
  for (const file of pageCssFiles()) for (const c of classSelectors(read(file))) page.add(c);
  assert.ok(page.size > 20, "the page CSS was read (found " + page.size + " classes)");
  const lab = labClasses("sound");
  assert.ok(lab.size >= 6, "the lab classes were read (found " + lab.size + ")");
  assert.deepEqual([...lab].filter((c) => page.has(c)), [], "rename these in lab/sound.html and lab/sound.entry.js (prefix them with lab-)");
});
