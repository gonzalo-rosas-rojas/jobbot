import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { setLang, setTimeZone, t, has } from "../src/i18n/index.js";
import esMoments from "../src/i18n/es/moments.js";
import enMoments from "../src/i18n/en/moments.js";
import { model, nightFigures } from "../src/core/model.js";
import { STOP_IDS } from "../src/scene/poses.js";
import { KEYS } from "../src/ui/keys.js";
import { MOMENTS, momentOf, STOPS_META } from "../src/moments/registry.js";
import { prologoView } from "../src/moments/prologo.js";

const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/snapshot.${name}.json`, import.meta.url), "utf8"));
const fresh = (lang = "es") => { setLang(lang); setTimeZone("America/Santiago"); };
const nowOf = (snap) => Date.parse(snap.generated_at);
const HOUR = 3600000;
const strings = (value, out = []) => {
  if (typeof value === "string") out.push(value);
  else if (Array.isArray(value)) value.forEach((v) => strings(v, out));
  else if (value && typeof value === "object") Object.values(value).forEach((v) => strings(v, out));
  return out;
};

test("the registry has one moment for each of the six stops, in stop order, with the contract of spec §5", () => {
  assert.deepEqual(Object.keys(MOMENTS), STOP_IDS);
  for (const id of STOP_IDS) {
    const moment = momentOf(id);
    assert.equal(moment.id, id);
    for (const fn of ["mount", "update", "unmount"]) assert.equal(typeof moment[fn], "function", `${id}.${fn}`);
  }
  assert.equal(momentOf("nope"), null);
  assert.equal(momentOf(undefined), null);
  assert.equal(typeof momentOf("hoy").onKey, "function", "HOY takes the row keys");
});

test("STOPS_META is the one table the bar, the marks, the palette and the help read: ids in stop order, the number of each stop, names and questions in both languages", () => {
  assert.deepEqual(STOPS_META.map((s) => s.id), STOP_IDS);
  const go = KEYS.find((k) => k.id === "go").keys;
  const prologue = KEYS.find((k) => k.id === "prologue").keys;
  assert.deepEqual(STOPS_META.map((s) => s.key), [...prologue, ...go], "0 is the prologue and 1-5 the other stops, as the key table says");
  for (const lang of ["es", "en"]) {
    fresh(lang);
    for (const s of STOPS_META) {
      assert.ok(has(s.name) && has(s.ask), `${lang}: ${s.id}`);
      assert.ok(!t(s.name).includes("⟦") && !t(s.ask).includes("⟦"));
    }
  }
  fresh();
  assert.equal(t("stop.hoy.ask"), "¿Qué me toca hoy?");
});

test("the words of the moments exist in both languages with the same variables", () => {
  assert.deepEqual(Object.keys(esMoments).sort(), Object.keys(enMoments).sort());
  const vars = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(",");
  for (const key of Object.keys(esMoments)) assert.equal(vars(esMoments[key]), vars(enMoments[key]), key);
});

test("the prologue: the four figures are nightFigures (one function, no number twice), 'Anoche' in numbers, and the voice of a person only when the engine ran", () => {
  fresh();
  for (const name of ["empty", "rich", "heavy"]) {
    const snap = fixture(name);
    const m = model(snap);
    const v = prologoView(m, { nowMs: nowOf(snap) });
    const night = nightFigures(snap);
    assert.deepEqual(v.figures.map((f) => f.n), [night.read, night.above, night.complete, night.awaiting], name);
    assert.equal(v.figures[3].gold, true);
    assert.equal(v.title, "Anoche");
    assert.match(v.night, /aviso|ningún/, name);
    assert.equal(v.base, night.days > 0 ? `base: ${night.days} días` : null, "the base of the figures is visible");
  }
  const rich = fixture("rich");
  const ran = prologoView(model(rich), { nowMs: nowOf(rich) });
  assert.equal(ran.ran, true);
  assert.equal(ran.voice, "Mientras dormías.");
  assert.match(ran.eyebrow, /^La noche en una pantalla, de \d{1,2} \S+,? \d\d:\d\d a \d{1,2} \S+,? \d\d:\d\d/);

  // A15: the run is told from last_run_at and nothing else
  const old = fixture("rich");
  old.controls.last_run_at = new Date(nowOf(old) - 31 * HOUR).toISOString();
  const late = prologoView(model(old), { nowMs: nowOf(old) });
  assert.equal(late.ran, false);
  assert.equal(late.voice, "Anoche no hubo corrida. Última: hace 31 h.", "the sentence of the spec (§2.6), from last_run_at");
  const none = fixture("empty");
  assert.equal(none.controls.last_run_at, null);
  assert.equal(prologoView(model(none), { nowMs: nowOf(none) }).voice, "Anoche no hubo corrida: todavía no hay ninguna.");
  // a clock that is wrong never prints a negative time or NaN
  const skew = fixture("rich");
  skew.controls.last_run_at = new Date(nowOf(skew) + 5 * HOUR).toISOString();
  for (const s of strings(prologoView(model(skew), { nowMs: nowOf(skew) }))) assert.doesNotMatch(s, /NaN|undefined|null|−|⟦/);
});

test("the prologue speaks both languages and never shows a missing key", () => {
  for (const lang of ["es", "en"]) {
    fresh(lang);
    for (const name of ["empty", "rich", "heavy"]) {
      const snap = fixture(name);
      for (const s of strings(prologoView(model(snap), { nowMs: nowOf(snap) }))) assert.ok(!s.includes("⟦"), `${lang}/${name}: ${s}`);
    }
  }
  fresh("en");
  const snap = fixture("rich");
  assert.equal(prologoView(model(snap), { nowMs: nowOf(snap) }).voice, "While you slept.");
  fresh();
});
