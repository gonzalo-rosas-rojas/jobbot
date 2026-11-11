// Acceptance test of the encargo docs/encargos/posponer-un-nombre.md (B3: one verb, one name). Putting an application aside
// until a chosen day is called the same everywhere: Posponer in Spanish, Postpone in English. It is read where Gonza sees it
// (the verb of HOY and of COLA, its menu of days, the shortcut sheet and the drawer of PROCESOS) in both languages, and the old
// words (Después, Saltar, Later, Skip, Snooze) are gone from every string of that verb.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { setLang, setTimeZone, t } from "../src/i18n/index.js";
import { model } from "../src/core/model.js";
import { hoyView } from "../src/moments/hoy/view.js";
import { colaView } from "../src/moments/cola/view.js";
import { helpRows } from "../src/ui/help.js";

const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/snapshot.${name}.json`, import.meta.url), "utf8"));
const nowOf = (snap) => ({ nowMs: Date.parse(snap.generated_at) });
const WORD = { es: "Posponer", en: "Postpone" };

/** Every verb with id `snooze` anywhere in a view (HOY's next and rows, COLA's focus and queue rows). */
const snoozeVerbs = (value, out = []) => {
  if (Array.isArray(value)) value.forEach((v) => snoozeVerbs(v, out));
  else if (value && typeof value === "object") {
    if (value.id === "snooze" && typeof value.label === "string") out.push(value);
    Object.values(value).forEach((v) => snoozeVerbs(v, out));
  }
  return out;
};

for (const lang of ["es", "en"]) {
  test(`posponer (${lang}): the snooze verb of HOY and of COLA is labelled ${WORD[lang]}`, () => {
    setLang(lang);
    setTimeZone("America/Santiago");
    const rich = fixture("rich");
    const heavy = fixture("heavy");
    const hoy = snoozeVerbs(hoyView(model(rich), nowOf(rich)));
    const cola = [...snoozeVerbs(colaView(model(rich), nowOf(rich))), ...snoozeVerbs(colaView(model(heavy), nowOf(heavy)))];
    assert.ok(hoy.length > 0 && cola.length > 0, "the fixtures have rows that can be postponed in both stops");
    for (const v of [...hoy, ...cola]) assert.equal(v.label, WORD[lang]);
  });

  test(`posponer (${lang}): its menu of days, the shortcut sheet and the drawer say ${WORD[lang]}`, () => {
    setLang(lang);
    const until = { es: "Posponer hasta cuándo", en: "Postpone until when" }[lang];
    assert.equal(t("hoy.snooze.title"), until);
    assert.equal(t("cola.snooze.title"), until);
    for (const stop of ["hoy", "cola", "procesos"]) {
      const row = helpRows(stop).find((r) => r.id === "snooze");
      assert.ok(row, `the shortcut sheet of ${stop} lists s`);
      assert.ok(t(row.label).startsWith(WORD[lang] + ":"), `${stop}: ${t(row.label)}`);
    }
    assert.equal(t("procesos.drawer.act.snooze"), WORD[lang]);
  });
}

// The strings of this verb: the keys named skip or snooze (its label, hint, menu, toast, legend, the state of what was put
// aside) and the sentence of COLA that tells what to do with a proposal that is not right. Two other skips are other verbs:
// `shell.skip` is the skip link ("go to the content") and `sistema.step.skip.*` a step of a run the engine did not do. The
// palette's search aliases (palette.does.*) keep the old words on purpose, so that whoever learnt them finds the same.
const OTHER_SKIPS = /^(shell\.skip$|sistema\.step\.skip\.)/;
const OF_THE_VERB = (key) => !OTHER_SKIPS.test(key) && (/(^|\.)(skip|snooze|snoozed)(\.|$)/.test(key) || key.startsWith("cola.why.decide"));
const OLD = { es: /s[aá]lt|despu[eé]s/i, en: /\bskip|\blater\b|\bsnooz/i };

/** Every dictionary file of a language, merged: a string moved to another file is still read. */
async function dictionary(lang) {
  const dir = new URL(`../src/i18n/${lang}/`, import.meta.url);
  const files = readdirSync(dir).filter((name) => name.endsWith(".js"));
  return Object.assign({}, ...(await Promise.all(files.map(async (name) => (await import(new URL(name, dir))).default))));
}

test("posponer: no string of the verb says Saltar, Después, Skip, Later or Snooze in either language", async () => {
  for (const lang of ["es", "en"]) {
    const entries = Object.entries(await dictionary(lang)).filter(([key]) => OF_THE_VERB(key));
    assert.ok(entries.length >= 8, `${lang}: the verb's strings are found`);
    assert.deepEqual(entries.filter(([, text]) => OLD[lang].test(text)), [], `${lang}: these still use an old word`);
  }
});
