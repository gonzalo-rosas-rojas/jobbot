// The words and the code that asks for them stay in step: every key the code asks for by name exists, and no key of the dictionaries is left that
// nothing asks for. Both ways are found by reading the source (the page is never run here); a key that the code builds at run time, as
// "state." + name or `tab.${id}`, is found by its static start and end, so it counts as asked for by anything that starts and ends like that.
//
// The dictionaries had kept the words of the v2 tower (the five rooms, the spine, the world's toasts) after the tower was archived; this test is
// what keeps the next page from doing the same. The list at the top is the only keys allowed to wait, with the reason, and it is
// checked: a waiting key must still exist and must not be in use (when it is, it leaves the list).
import test from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync, existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const SCOPES = ["common", "shell", "hoy", "procesos", "cola", "aprender", "sistema", "detail", "stage", "hud", "keys", "moments", "sound"];

// Words of moments that were still being written when this test came in; the one who finishes the moment uses the key or deletes it and its line.
const STILL_WRITING = [];
const WAITING = new Set(STILL_WRITING);

const dict = {};
for (const lang of ["es", "en"]) {
  dict[lang] = {};
  for (const scope of SCOPES) Object.assign(dict[lang], (await import(`../src/i18n/${lang}/${scope}.js`)).default);
}
const KEYS = Object.keys(dict.es);
const KEYSET = new Set(KEYS);

// ---- the source that asks for words: the page, its labs and the demo's script (the dictionaries themselves are not source)
const EXCLUDED = ["src/i18n/es", "src/i18n/en"];
function walk(dir, out = []) {
  if (!existsSync(dir)) return out;
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, e.name);
    const rel = path.relative(ROOT, full).split(path.sep).join("/");
    if (EXCLUDED.includes(rel)) continue;
    if (e.isDirectory()) walk(full, out); else if (e.name.endsWith(".js")) out.push(full);
  }
  return out;
}
const files = [...walk(path.join(ROOT, "src")), ...walk(path.join(ROOT, "lab")), path.join(ROOT, "..", "scripts", "demo", "demo.js")].filter((f) => existsSync(f));
// a comment may name a key it does not use: block comments and whole-line comments go
const code = (file) => readFileSync(file, "utf8").replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "");
const sources = files.map((f) => ({ file: path.relative(ROOT, f).split(path.sep).join("/"), text: code(f) }));

const QUOTED = /"((?:[^"\\\n]|\\.)*)"|'((?:[^'\\\n]|\\.)*)'/g;
const TEMPLATE = /`((?:[^`\\]|\\.)*)`/g;
const CONCAT = /["']([a-z]\w*(?:\.\w+)*\.)["']\s*\+\s*[^"'`;]*?\+\s*["'](\.[a-zA-Z][\w.]*)["']/g; // "act." + name + ".hint"
const literals = new Set(); // every string of the source, whole
const built = []; // [start, end] of a key put together at run time: `state.${x}` is ["state.", ""], `act.${a}.hint` is ["act.", ".hint"]
for (const { text } of sources) {
  for (const m of text.matchAll(QUOTED)) literals.add(m[1] ?? m[2]);
  for (const m of text.matchAll(TEMPLATE)) {
    const parts = m[1].split(/\$\{[^}]*\}/);
    if (parts.length === 1) literals.add(parts[0]);
    else built.push([parts[0], parts[parts.length - 1]]);
  }
  for (const m of text.matchAll(CONCAT)) built.push([m[1], m[2]]);
}
// label("state.", x) and "time." + unit: a string that is a run of words with dots and ends with a dot is the start of a key
for (const s of literals) if (/^[a-z]\w*(\.\w+)*\.$/.test(s)) built.push([s, ""]);

/** Why the code asks for `key`, or null when it does not. */
function reason(key) {
  if (literals.has(key)) return "named";
  const base = key.replace(/\.(one|other|zero)$/, "");
  if (base !== key && literals.has(base)) return "plural of " + base;
  for (const [start, end] of built) {
    if (start.length + end.length > 2 && key.startsWith(start) && key.endsWith(end)) return `built as "${start}…${end}"`;
  }
  return null;
}

test("every key the code asks for by name exists in both languages", () => {
  const missing = [];
  // t("x"), tn("x", n) and has("x") with the key written out (a key followed by "+" is the start of one built at run time; `.has(` is a Set's or a URL's)
  const arg = String.raw`(?:"((?:[^"\\\n]|\\.)*)"|'((?:[^'\\\n]|\\.)*)'|` + "`((?:[^`\\\\]|\\\\.)*)`)" + String.raw`\s*[,)]`;
  const call = new RegExp(String.raw`(?:^|[^\w$.])(has)\(\s*` + arg + "|" + String.raw`(?:^|[^\w$])(tn|t)\(\s*` + arg, "g");
  for (const { file, text } of sources) {
    for (const m of text.matchAll(call)) {
      const fn = m[1] || m[5];
      const double = m[2] ?? m[6];
      const single = m[3] ?? m[7];
      const template = m[4] ?? m[8];
      const key = double ?? single ?? template;
      if (template !== undefined && template.includes("${")) {
        const start = template.split("${")[0];
        if (start && !KEYS.some((k) => k.startsWith(start))) missing.push(`${file}: no key starts with "${start}"`);
        continue;
      }
      const wanted = fn === "tn" ? [key + ".other"] : [key];
      for (const k of wanted) for (const lang of ["es", "en"]) if (typeof dict[lang][k] !== "string") missing.push(`${file}: ${fn}("${key}") has no ${lang} text`);
    }
  }
  assert.deepEqual([...new Set(missing)], []);
});

test("no key is left in the dictionaries that nothing asks for", () => {
  const orphans = KEYS.filter((k) => !WAITING.has(k) && !reason(k));
  assert.deepEqual(orphans, [], "these keys are not asked for by any module (a key of a tower or a room that is gone? delete it from both languages):\n" + orphans.join("\n"));
});

test("a key that waits is real, and leaves the list as soon as the code asks for it", () => {
  for (const key of WAITING) assert.ok(KEYSET.has(key), `${key} is on a waiting list but is not in the dictionaries any more`);
  for (const key of WAITING) assert.equal(reason(key), null, `${key} is in use now (${reason(key)}): take it off the waiting list`);
});
