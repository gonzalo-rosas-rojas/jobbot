// The text system: both languages say the same things, the API behaves, the formatters give the expected strings
// for a fixed instant, and no moment, stage or UI module writes user-visible words as literals.
import test from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync, existsSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { useStorage, local } from "../src/core/store.js";
import * as i18n from "../src/i18n/index.js";

const { t, tn, has, lang, setLang, onLang, detectLang, initLang, setTimeZone, fmtNumber, fmtPercent, fmtMoney, fmtDate, fmtRelative } = i18n;

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const SCOPES = ["common", "shell", "hoy", "procesos", "cola", "aprender", "sistema", "detail", "stage", "hud", "keys", "moments", "sound"];
const dicts = { es: {}, en: {} };
for (const l of ["es", "en"]) {
  for (const scope of SCOPES) dicts[l][scope] = (await import(`../src/i18n/${l}/${scope}.js`)).default;
}
const flat = (l) => Object.assign({}, ...SCOPES.map((scope) => dicts[l][scope]));
const vars = (text) => Array.from(new Set(Array.from(text.matchAll(/\{([A-Za-z_]\w*)\}/g), (m) => m[1]))).sort();

function fakeStorage() {
  const m = new Map();
  return { getItem: (k) => (m.has(k) ? m.get(k) : null), setItem: (k, v) => m.set(k, String(v)), removeItem: (k) => m.delete(k) };
}

function fresh() {
  useStorage({ local: fakeStorage(), session: fakeStorage() });
  setTimeZone("UTC");
  setLang("es");
}

// ---- the dictionaries

test("both languages have exactly the same keys, scope by scope", () => {
  for (const scope of SCOPES) {
    const es = Object.keys(dicts.es[scope]).sort();
    const en = Object.keys(dicts.en[scope]).sort();
    assert.deepEqual(es.filter((k) => !en.includes(k)), [], `${scope}: in es but not in en`);
    assert.deepEqual(en.filter((k) => !es.includes(k)), [], `${scope}: in en but not in es`);
  }
});

test("a {variable} set is the same in es and en for every key", () => {
  const es = flat("es");
  const en = flat("en");
  for (const key of Object.keys(es)) assert.deepEqual(vars(es[key]), vars(en[key]), `variables of ${key}`);
});

test("no value is empty and every value is a string", () => {
  for (const l of ["es", "en"]) {
    for (const [key, value] of Object.entries(flat(l))) {
      assert.equal(typeof value, "string", `${l} ${key} is not a string`);
      assert.ok(value.trim().length > 0, `${l} ${key} is empty`);
    }
  }
});

test("no key is defined in two scope files, and keys are dotted lowercase words", () => {
  for (const l of ["es", "en"]) {
    const seen = new Map();
    for (const scope of SCOPES) {
      for (const key of Object.keys(dicts[l][scope])) {
        assert.ok(!seen.has(key), `${l}: ${key} is in ${seen.get(key)} and ${scope}`);
        seen.set(key, scope);
        assert.match(key, /^[a-z][a-zA-Z0-9_]*(\.[a-zA-Z0-9_]+)+$/, `key shape: ${key}`);
      }
    }
  }
});

test("a plural key has its forms together: .one and .zero both need an .other", () => {
  for (const l of ["es", "en"]) {
    const d = flat(l);
    for (const key of Object.keys(d)) {
      // (a lone "x.other" is fine: it is a plain key that happens to be called that, like kind.other)
      if (key.endsWith(".one")) assert.ok(d[key.slice(0, -4) + ".other"], `${l}: ${key} has no .other`);
      if (key.endsWith(".zero")) assert.ok(d[key.slice(0, -5) + ".other"], `${l}: ${key} has no .other`);
    }
  }
});

test("common holds the five stages, every state, mark, kind, action, turn and tab", () => {
  const es = dicts.es.common;
  for (const s of ["submitted", "replied", "technical_test", "interview", "offer"]) assert.ok(es["stage." + s], s);
  for (const s of ["scored", "prepared", "needs_gonza", "submitted", "replied", "technical_test", "interview", "offer", "accepted", "rejected", "withdrawn", "ghosted"]) {
    assert.ok(es["state." + s] && es["state.plural." + s], s);
  }
  for (const m of ["replied", "technical_test", "interview", "offer", "accepted", "rejected", "withdrawn", "ghosted"]) assert.ok(es["mark." + m], m);
  for (const k of ["ack", "rejection", "recruiter", "interview_invite", "technical_test", "action_required", "other"]) assert.ok(es["kind." + k], k);
  for (const a of ["approve", "resolve", "confirm"]) assert.ok(es["act." + a] && es["act." + a + ".hint"], a);
  for (const r of ["hoy", "procesos", "cola", "aprender", "sistema"]) assert.ok(es["tab." + r], r);
  for (const x of ["mine", "check", "wait"]) assert.ok(es["turn." + x], x);
});

// ---- t, tn, has

test("t fills {vars}, leaves a missing one visible, and shows a missing key without throwing", () => {
  fresh();
  const warnings = [];
  const warn = console.warn;
  console.warn = (...args) => warnings.push(args.join(" "));
  try {
    assert.equal(t("act.approve"), "Aprobar");
    assert.equal(t("time.ago.min", { n: 3 }), "hace 3 min");
    assert.equal(t("time.ago.min", { n: 0 }), "hace 0 min");
    assert.equal(t("time.ago.min"), "hace {n} min");
    assert.equal(t("time.ago.min", { other: 1 }), "hace {n} min");
    assert.equal(t("time.ago.min", { n: null }), "hace {n} min");
    assert.equal(t("no.such.key"), "⟦no.such.key⟧");
    assert.equal(t("no.such.key", { n: 1 }), "⟦no.such.key⟧");
    assert.equal(t("no.such.key2"), "⟦no.such.key2⟧");
    assert.equal(warnings.filter((w) => w.includes("no.such.key ")).length, 1, "warned once per key");
    assert.equal(has("act.approve"), true);
    assert.equal(has("no.such.key"), false);
  } finally {
    console.warn = warn;
  }
});

test("tn picks one, other and zero, in Spanish and in English", () => {
  fresh();
  assert.deepEqual([0, 1, 2].map((n) => tn("time.ago.days", n)), ["hace 0 días", "hace 1 día", "hace 2 días"]);
  assert.equal(tn("time.ago.days", 1), "hace 1 día");
  assert.equal(tn("time.ago.days", 5), "hace 5 días");
  // a number the Spanish rules call "many" (one million) still uses .other
  assert.equal(tn("time.ago.days", 1000000), "hace 1000000 días");
  assert.equal(tn("news.sinceVisit", 0), "Nada nuevo desde tu última visita");
  assert.equal(tn("news.sinceVisit", 1), "Desde tu última visita: 1 novedad");
  setLang("en");
  assert.deepEqual([0, 1, 2].map((n) => tn("time.ago.days", n)), ["0 days ago", "1 day ago", "2 days ago"]);
  assert.equal(tn("news.sinceVisit", 0), "Nothing new since your last visit");
  assert.equal(tn("news.sinceVisit", 3), "Since your last visit: 3 updates");
  const warn = console.warn;
  console.warn = () => {};
  try { assert.equal(tn("no.such.plural", 2), "⟦no.such.plural.other⟧"); } finally { console.warn = warn; }
  setLang("es");
});

// ---- language: choice, persistence, detection

test("setLang saves the choice, rejects bad values and tells the listeners once", () => {
  fresh();
  const doc = { documentElement: { lang: "" } };
  globalThis.document = doc;
  try {
    const heard = [];
    const off = onLang((l) => heard.push(l));
    assert.equal(setLang("en"), true);
    assert.equal(lang(), "en");
    assert.equal(local.get("lang"), "en");
    assert.equal(doc.documentElement.lang, "en");
    assert.equal(t("act.approve"), "Approve");
    assert.equal(setLang("fr"), false);
    assert.equal(setLang(undefined), false);
    assert.equal(setLang("EN"), false);
    assert.equal(lang(), "en");
    assert.equal(local.get("lang"), "en");
    setLang("en");
    assert.deepEqual(heard, ["en"], "setting the same language again is not a change");
    setLang("es");
    assert.deepEqual(heard, ["en", "es"]);
    assert.equal(doc.documentElement.lang, "es");
    off();
    setLang("en");
    assert.deepEqual(heard, ["en", "es"], "a listener that unsubscribed hears nothing");
    const offBad = onLang(() => { throw new Error("a bad listener"); });
    const error = console.error;
    console.error = () => {};
    try { assert.equal(setLang("es"), true, "a listener that throws does not break setLang"); } finally { console.error = error; offBad(); }
  } finally {
    delete globalThis.document;
    setLang("es");
  }
});

test("detectLang: the address first, then the saved choice, then the build's default, and the browser only on auto", () => {
  fresh();
  assert.equal(detectLang({ search: "?lang=en", saved: "es", def: "es" }), "en");
  assert.equal(detectLang({ search: "?t=abc&lang=es", saved: "en", def: "en" }), "es");
  assert.equal(detectLang({ search: "?lang=fr", saved: "en", def: "es" }), "en");
  assert.equal(detectLang({ search: "", saved: "en", def: "es" }), "en");
  assert.equal(detectLang({ search: "", saved: "xx", def: "es" }), "es");
  assert.equal(detectLang({ search: "", saved: null, def: "en", navLang: "es-CL" }), "en", "the demo build opens in English");
  assert.equal(detectLang({ search: "", saved: null, def: "es", navLang: "en-US" }), "es", "the panel for Gonza opens in Spanish");
  assert.equal(detectLang({ search: "", saved: null, def: "auto", navLang: "es-AR" }), "es");
  assert.equal(detectLang({ search: "", saved: null, def: "auto", navLang: "fr-FR" }), "en");
  assert.equal(detectLang({ search: "", saved: null, def: "auto", navLang: "" }), "en");
  // with no define at all (tests, labs) the default is Spanish
  assert.equal(detectLang({ search: "", saved: null }), "es");
});

test("initLang picks the language without saving it", () => {
  fresh();
  local.remove("lang"); // fresh() chose Spanish on purpose, which saves it
  assert.equal(initLang({ search: "?lang=en", saved: null, def: "es" }), "en");
  assert.equal(lang(), "en");
  assert.equal(local.get("lang"), null);
  setLang("es");
});

// ---- formatters for a fixed instant (Wednesday 7 October 2026, 14:05 UTC)

const AT = "2026-10-07T14:05:00Z";
const AT_MS = Date.parse(AT);

test("dates follow the language and the pinned time zone", () => {
  fresh();
  assert.deepEqual(["short", "long", "time", "datetime", "day"].map((s) => fmtDate(AT, s)),
    ["mié, 7 oct", "miércoles, 7 de octubre de 2026", "14:05", "7 oct, 14:05", "7 oct"]);
  setLang("en");
  assert.deepEqual(["short", "long", "time", "datetime", "day"].map((s) => fmtDate(AT, s)),
    ["Wed 7 Oct", "Wednesday, 7 October 2026", "14:05", "7 Oct, 14:05", "7 Oct"]);
  setTimeZone("America/Santiago");
  assert.equal(fmtDate(AT, "time"), "11:05", "Santiago is UTC-3 in October");
  assert.equal(fmtDate(AT, "time", "UTC"), "14:05", "a zone can be given per call");
  setTimeZone("UTC");
  assert.equal(fmtDate("not a date"), "not a date");
  assert.equal(fmtDate(undefined), "");
  setLang("es");
});

test("numbers, percentages and dollars follow the language", () => {
  fresh();
  assert.equal(fmtNumber(1234.5), "1.234,5");
  assert.equal(fmtPercent(0.456), "46%");
  assert.equal(fmtPercent(0), "0%");
  assert.equal(fmtPercent(null), "n/a");
  assert.equal(fmtMoney(1.84), "US$1,84", "in Chile a bare $ means pesos");
  assert.equal(fmtMoney(5), "US$5,00");
  assert.equal(fmtMoney(4800, { digits: 0 }), "US$4.800");
  setLang("en");
  assert.equal(fmtNumber(1234.5), "1,234.5");
  assert.equal(fmtPercent(1 / 3), "33%");
  assert.equal(fmtMoney(1.84), "$1.84");
  assert.equal(fmtMoney(4800, { digits: 0 }), "$4,800");
  setLang("es");
});

test("fmtRelative says how long ago, and 'now' for the future or for nonsense", () => {
  fresh();
  const ago = (ms) => new Date(AT_MS - ms).toISOString();
  const MIN = 60000, HOUR = 3600000, DAY = 86400000;
  assert.deepEqual([30000, 5 * MIN, 59 * MIN, 3 * HOUR, 47 * HOUR, 50 * HOUR, 5 * DAY].map((ms) => fmtRelative(ago(ms), AT_MS)),
    ["hace 1 min", "hace 5 min", "hace 59 min", "hace 3 h", "hace 47 h", "hace 2 días", "hace 5 días"]);
  assert.equal(fmtRelative(new Date(AT_MS + HOUR).toISOString(), AT_MS), "ahora");
  assert.equal(fmtRelative("garbage", AT_MS), "ahora");
  setLang("en");
  assert.deepEqual([30000, 3 * HOUR, 50 * HOUR].map((ms) => fmtRelative(ago(ms), AT_MS)), ["1 min ago", "3 h ago", "2 days ago"]);
  assert.equal(fmtRelative("garbage", AT_MS), "just now");
  setLang("es");
});

// ---- the lint: user-visible words belong to the dictionaries, not to room or UI code

const TEXT_CALLS = new Set(["h", "sv", "append", "prepend", "fill", "replaceChildren", "createTextNode", "toast"]);
const TEXT_KEYS = new Set(["title", "aria-label", "placeholder", "alt", "label", "aria-description", "aria-placeholder", "aria-valuetext"]);
const TEXT_PROPS = new Set(["textContent", "innerText", "title", "placeholder", "alt", "ariaLabel", "ariaDescription"]);

// Two or more words of letters with a space between: prose, not a tag name, a class list of one word, a key or a unit.
const looksLikeProse = (text) => /\s/.test(text.trim()) && (text.match(/\p{L}{2,}/gu) || []).length >= 2;

/** A small tokenizer (comments, strings, templates and regexes skipped or captured) and a walk over its brackets. */
export function lintSource(source, file = "source") {
  const flagged = [];
  const tokens = [];
  let i = 0;
  let line = 1;
  const n = source.length;
  const push = (type, value, at = line) => tokens.push({ type, value, line: at });
  const prevSignificant = () => tokens[tokens.length - 1];
  const regexAllowed = () => {
    const p = prevSignificant();
    if (!p) return true;
    if (p.type === "punct") return !")]}".includes(p.value);
    return p.type === "id" && /^(return|typeof|case|of|in|new|delete|void|throw|else|do)$/.test(p.value);
  };
  while (i < n) {
    const c = source[i];
    if (c === "\n") { line += 1; i += 1; continue; }
    if (/\s/.test(c)) { i += 1; continue; }
    if (c === "/" && source[i + 1] === "/") { while (i < n && source[i] !== "\n") i += 1; continue; }
    if (c === "/" && source[i + 1] === "*") {
      i += 2;
      while (i < n && !(source[i] === "*" && source[i + 1] === "/")) { if (source[i] === "\n") line += 1; i += 1; }
      i += 2;
      continue;
    }
    if (c === '"' || c === "'") {
      const start = line;
      let text = "";
      i += 1;
      while (i < n && source[i] !== c && source[i] !== "\n") {
        if (source[i] === "\\") { text += source[i + 1] || ""; i += 2; continue; }
        text += source[i];
        i += 1;
      }
      i += 1;
      push("str", text, start);
      continue;
    }
    if (c === "`") {
      const start = line;
      let text = "";
      i += 1;
      let depth = 0;
      while (i < n && !(source[i] === "`" && depth === 0)) {
        if (source[i] === "\n") line += 1;
        if (depth === 0 && source[i] === "$" && source[i + 1] === "{") { depth = 1; i += 2; text += "\u0001"; continue; }
        if (depth > 0) {
          if (source[i] === "{") depth += 1;
          else if (source[i] === "}") depth -= 1;
          i += 1;
          continue;
        }
        if (source[i] === "\\") { text += source[i + 1] || ""; i += 2; continue; }
        text += source[i];
        i += 1;
      }
      i += 1;
      push("str", text.replace(/\u0001/g, " "), start);
      continue;
    }
    if (c === "/" && regexAllowed()) {
      i += 1;
      let inClass = false;
      while (i < n && source[i] !== "\n" && (source[i] !== "/" || inClass)) {
        if (source[i] === "\\") i += 1;
        else if (source[i] === "[") inClass = true;
        else if (source[i] === "]") inClass = false;
        i += 1;
      }
      i += 1;
      while (i < n && /[a-z]/.test(source[i])) i += 1;
      push("regex", "");
      continue;
    }
    if (/[A-Za-z_$]/.test(c)) {
      let word = "";
      while (i < n && /[\w$]/.test(source[i])) { word += source[i]; i += 1; }
      push("id", word);
      continue;
    }
    if (/\d/.test(c)) {
      while (i < n && /[\w.]/.test(source[i])) i += 1;
      push("num", "");
      continue;
    }
    push("punct", c);
    i += 1;
  }

  // Walk: a stack of open brackets; a "(" remembers the name before it and which argument it is on.
  const stack = [];
  let assign = null; // depth at which `.textContent =` was seen; ends at the next ";" or when that bracket closes
  tokens.forEach((tok, at) => {
    if (tok.type === "punct") {
      if ("([{".includes(tok.value)) {
        const before = tokens[at - 1];
        stack.push({ open: tok.value, callee: tok.value === "(" && before && before.type === "id" ? before.value : null, arg: 0 });
      } else if (")]}".includes(tok.value)) {
        stack.pop();
        if (assign !== null && stack.length < assign) assign = null;
      } else if (tok.value === "," && stack.length) {
        stack[stack.length - 1].arg += 1;
      } else if (tok.value === ";" && assign !== null && stack.length <= assign) {
        assign = null;
      } else if (tok.value === "=" && tokens[at - 1] && tokens[at - 1].type === "id" && TEXT_PROPS.has(tokens[at - 1].value) && tokens[at - 2] && tokens[at - 2].value === ".") {
        assign = stack.length;
      } else if (tok.value === "=" && tokens[at - 1] && tokens[at - 1].value === "+" && tokens[at - 2] && tokens[at - 2].type === "id" && TEXT_PROPS.has(tokens[at - 2].value)) {
        assign = stack.length;
      }
      return;
    }
    if (tok.type !== "str" || !looksLikeProse(tok.value)) return;
    const top = stack[stack.length - 1];
    let why = null;
    if (assign !== null) why = "assigned as text";
    else if (top && top.open === "(" && TEXT_CALLS.has(top.callee) && !((top.callee === "h" || top.callee === "sv") && top.arg === 0)) why = "argument of " + top.callee + "()";
    else if (top && top.open === "[" && stack.length > 1 && stack[stack.length - 2].open === "(" && TEXT_CALLS.has(stack[stack.length - 2].callee)) why = "child in a list for " + stack[stack.length - 2].callee + "()";
    else if (top && top.open === "{" && tokens[at - 1] && tokens[at - 1].value === ":" && tokens[at - 2] && TEXT_KEYS.has(tokens[at - 2].value)) why = "the " + tokens[at - 2].value + " attribute";
    if (why) flagged.push({ file, line: tok.line, text: tok.value, why });
  });
  return flagged;
}

test("the lint flags prose in h() children, textContent and text attributes, and leaves the rest", () => {
  const bad = [
    'h("p", {}, "Hola mundo cruel")',
    'h("p", { class: "x" }, a ? "Cargando datos" : null)',
    'el.textContent = "Cargando datos ahora";',
    'el.textContent = cond ? "Uno dos" : "Tres cuatro";',
    'h("button", { title: "Abrir esta cosa" }, t("x"))',
    'h("input", { "aria-label": "Buscar algo", type: "text" })',
    'h("ul", {}, ["Primer elemento", "Segundo elemento"])',
    "h(\"p\", {}, `Hola ${name} amigos`)",
    "toast('Algo salió bien')",
  ];
  const good = [
    'h("div", { class: "item plain stack", "data-card": key }, t("room.hoy.title"))',
    'h("p", {}, "Hola")',
    'h("p", {}, count, " · ", other)',
    'h("p", {}, t("room.hoy.title"), " ", t("x"))',
    '// h("p", {}, "comentario con varias palabras")',
    '/* el.textContent = "dos palabras" */',
    'console.log("two words here"); throw new Error("another bad thing");',
    'document.querySelector("#app .row"); el.classList.add("one two");',
    'const re = /["\']two words["\']/; h("p", {}, t("a"));',
    'h("svg", { viewBox: "0 0 10 10", d: "M 0 0 L 10 10" }, h("path", { transform: "rotate(10 5 5)" }))',
    'el.dataset.x = "two words"; el.className = "a b";',
    'sv("g", { class: "car mine fresh", role: "button" })',
  ];
  for (const source of bad) assert.ok(lintSource(source).length > 0, "should flag: " + source);
  for (const source of good) assert.deepEqual(lintSource(source), [], "should not flag: " + source);
  const [one] = lintSource('x();\n\nh("p", {}, "Hola mundo cruel");', "a.js");
  assert.equal(one.line, 3);
  assert.equal(one.file, "a.js");
});

function jsFiles(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir).flatMap((name) => {
    const full = path.join(dir, name);
    if (statSync(full).isDirectory()) return jsFiles(full);
    return name.endsWith(".js") ? [full] : [];
  });
}

test("no module under src/ui, src/moments or src/stage writes words of its own as literals", () => {
  const problems = [];
  for (const dir of ["src/ui", "src/moments", "src/stage"]) {
    for (const file of jsFiles(path.join(root, dir))) {
      for (const f of lintSource(readFileSync(file, "utf8"), file)) {
        problems.push(`${path.relative(root, f.file)}:${f.line}  ${JSON.stringify(f.text)}  (${f.why})`);
      }
    }
  }
  assert.deepEqual(problems, [], "user-visible text must come from t()/tn():\n" + problems.join("\n"));
});
