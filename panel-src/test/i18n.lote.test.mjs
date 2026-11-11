// Acceptance test of the encargo docs/encargos/lote-claves-y-textos.md: five small text fixes of the panel in one run, one test each.
// (1) Posponer's hint is one key, the shared act.skip.hint, and COLA shows it on its verb; (2) COLA's copies of Posponer's label and
// hint (cola.act.skip, cola.act.skip.hint, the same words as act.skip and act.skip.hint) are gone; (3) the button that starts the review
// in APRENDER has its own name for a screen reader (its text starts with the ↵ of its key), and the three words no screen says
// are gone; (4) the demo's server says Postponed, the word of the glossary, when a card is postponed; (5) the capture of a practice run is
// said one way in Spanish, "Captura de la prueba", the way drawer.shot.alt says it. No browser: views, dictionaries and the demo in a vm.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { setLang, setTimeZone, t } from "../src/i18n/index.js";
import { model } from "../src/core/model.js";
import { colaView } from "../src/moments/cola/view.js";
import { hoyView } from "../src/moments/hoy/view.js";
import { aprenderView } from "../src/moments/aprender/view.js";
import { richAprender } from "./helpers/aprender-fixture.mjs";
import { loadDemo } from "./helpers/demo-host.mjs";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/snapshot.${name}.json`, import.meta.url), "utf8"));
const nowOf = (snap) => ({ nowMs: Date.parse(snap.generated_at) });
const fresh = (lang) => { setLang(lang); setTimeZone("America/Santiago"); };

/** Every dictionary file of a language, merged: a string moved to another file is still read. */
async function dictionary(lang) {
  const dir = new URL(`../src/i18n/${lang}/`, import.meta.url);
  const files = readdirSync(dir).filter((name) => name.endsWith(".js"));
  return Object.assign({}, ...(await Promise.all(files.map(async (name) => (await import(new URL(name, dir))).default))));
}

/** The page's source (src/ without the dictionaries), comments out: is `key` written there whole, in quotes? */
function namedInSource(key) {
  const out = [];
  const walk = (dir) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, e.name);
      if (e.isDirectory()) { if (!/[\\/]src[\\/]i18n[\\/](es|en)$/.test(full)) walk(full); } else if (e.name.endsWith(".js")) out.push(full);
    }
  };
  walk(path.join(ROOT, "src"));
  const quoted = new RegExp(`["'\`]${key.replace(/\./g, "\\.")}["'\`]`);
  return out.filter((f) => quoted.test(readFileSync(f, "utf8").replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "")))
    .map((f) => path.relative(ROOT, f).split(path.sep).join("/"));
}

/** Every verb with id `snooze` anywhere in a view. */
const snoozeVerbs = (value, out = []) => {
  if (Array.isArray(value)) value.forEach((v) => snoozeVerbs(v, out));
  else if (value && typeof value === "object") {
    if (value.id === "snooze" && typeof value.label === "string") out.push(value);
    Object.values(value).forEach((v) => snoozeVerbs(v, out));
  }
  return out;
};

test("act.skip.hint: Posponer's hint is asked for by name, and it is the title of COLA's Posponer", () => {
  assert.ok(namedInSource("act.skip.hint").length > 0, "no module of src/ writes act.skip.hint: today nothing asks for it (actionHint builds act.<id>.hint, and the id of the verb is snooze)");
  for (const lang of ["es", "en"]) {
    fresh(lang);
    const rich = fixture("rich");
    const shown = snoozeVerbs(colaView(model(rich), nowOf(rich))).filter((v) => v.title);
    assert.ok(shown.length > 0, `${lang}: COLA's focus has a Posponer with a title`);
    for (const v of shown) assert.equal(v.title, t("act.skip.hint"), lang);
  }
});

test("cola.act.skip: Posponer has one label key and one hint key in both languages, the shared act.skip and act.skip.hint", async () => {
  for (const lang of ["es", "en"]) {
    const dict = await dictionary(lang);
    assert.deepEqual(["cola.act.skip", "cola.act.skip.hint"].filter((key) => key in dict), [], `${lang}: COLA keeps its own copy of the words of act.skip`);
    assert.ok(typeof dict["act.skip"] === "string" && typeof dict["act.skip.hint"] === "string", `${lang}: the shared keys stay`);
    fresh(lang);
    const rich = fixture("rich");
    const heavy = fixture("heavy");
    const verbs = [...snoozeVerbs(hoyView(model(rich), nowOf(rich))), ...snoozeVerbs(colaView(model(rich), nowOf(rich))), ...snoozeVerbs(colaView(model(heavy), nowOf(heavy)))];
    assert.ok(verbs.length > 0);
    for (const v of verbs) assert.equal(v.label, dict["act.skip"], lang);
  }
});

test("aprender.drill.start.aria: the button that starts the review has its own name for a screen reader, the review of all topics", () => {
  const said = { es: "Empezar el repaso de todos los temas", en: "Start the review of all topics" };
  for (const lang of ["es", "en"]) {
    fresh(lang);
    const snap = richAprender();
    const start = aprenderView(model(snap), nowOf(snap)).drill.start;
    assert.equal(start.aria, said[lang], `${lang}: the view of the start button carries its aria-label`);
    assert.equal(start.aria, t("aprender.drill.start.aria", { topic: t("aprender.drill.topic.all") }));
    assert.ok(start.aria.startsWith(start.label), `${lang}: the name heard starts with the words seen ("${start.label}")`);
  }
  // that render.js puts it on the button is checked on the button itself, in a real browser: moments.aprender.browser.test.mjs reads its aria-label
});

test("aprender.loading, sistema.row.result, sistema.row.window: the three words no screen says are gone from both languages", async () => {
  for (const lang of ["es", "en"]) {
    const dict = await dictionary(lang);
    assert.deepEqual(["aprender.loading", "sistema.row.result", "sistema.row.window"].filter((key) => key in dict), [], lang);
  }
});

test("demo /api/snooze: the demo's server says Postponed, the glossary's word, never Put aside", async () => {
  const said = {
    en: { 1: "Postponed until tomorrow.", 3: "Postponed for 3 days." },
    es: { 1: "Pospuesta hasta mañana.", 3: "Pospuesta 3 días." },
  };
  for (const lang of ["en", "es"]) {
    for (const until of [1, 3]) {
      const demo = loadDemo({ search: "?scenario=heavy", lang });
      const key = (await demo.api("/api/snapshot")).data.board.flatMap((col) => col.cards)[0].key;
      const res = await demo.api("/api/snooze", { key, until });
      assert.equal(res.ok, true);
      assert.equal(res.data.message, said[lang][until], `${lang}, ${until} day(s)`);
    }
  }
  const demoSource = readFileSync(path.join(ROOT, "..", "scripts", "demo", "demo.js"), "utf8");
  assert.doesNotMatch(demoSource, /"[^"\n]*\bput aside\b[^"\n]*"/i, "no string of the demo says put aside");
});

test("captura de la prueba: the capture of a practice run is said one way in Spanish, as drawer.shot.alt says it", async () => {
  const dict = await dictionary("es");
  assert.equal(dict["drawer.shot.alt"], "Captura de la prueba", "the glossary's form (test/i18n.glosario.test.mjs reads this key)");
  assert.equal(dict["procesos.last.prepared.shot"], "Captura de la prueba lista");
  assert.deepEqual(Object.entries(dict).filter(([, text]) => /\bcaptura de prueba\b/i.test(text)), [], "no string says captura de prueba");
});
