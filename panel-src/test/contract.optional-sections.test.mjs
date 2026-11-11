// The server sends `metrics`, `funnel`, `activity`, `claude` and `learn` as null when it could not build one of them (a damaged
// prep.db must not take the queues and their buttons away). Every stop still builds its view, in both languages, with any one of
// them missing and with all five missing, and nothing a person reads says "null", "undefined" or "NaN".
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { setLang, setTimeZone } from "../src/i18n/index.js";
import { model, nightFigures, stopCounts } from "../src/core/model.js";
import { createPlace } from "../src/moments/procesos/place.js";
import { prologoView } from "../src/moments/prologo.js";
import { hoyView } from "../src/moments/hoy/view.js";
import { procesosView } from "../src/moments/procesos/view.js";
import { colaView } from "../src/moments/cola/view.js";
import { aprenderView } from "../src/moments/aprender/view.js";
import { sistemaView } from "../src/moments/sistema/view.js";

const OPTIONAL = ["metrics", "funnel", "activity", "claude", "learn"];
const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/snapshot.${name}.json`, import.meta.url), "utf8"));
const BAD = /(?:^|[^\p{L}\p{N}_])(null|undefined|NaN)(?![\p{L}\p{N}_])|\[object/u;
const strings = (value, out = []) => {
  if (typeof value === "string") out.push(value);
  else if (Array.isArray(value)) value.forEach((v) => strings(v, out));
  else if (value && typeof value === "object") Object.values(value).forEach((v) => strings(v, out));
  return out;
};
const VIEWS = {
  prologo: (m, nowMs) => prologoView(m, { nowMs }),
  hoy: (m, nowMs) => hoyView(m, { nowMs }),
  procesos: (m, nowMs) => procesosView(m, { nowMs, place: createPlace() }),
  cola: (m, nowMs) => colaView(m, { nowMs }),
  aprender: (m, nowMs) => aprenderView(m, { nowMs }),
  sistema: (m, nowMs) => sistemaView(m, { nowMs }),
};

function without(snap, sections) {
  const copy = structuredClone(snap);
  for (const section of sections) copy[section] = null;
  return copy;
}

for (const lang of ["es", "en"]) {
  for (const name of ["rich", "empty"]) {
    for (const gone of [...OPTIONAL.map((s) => [s]), OPTIONAL]) {
      test(`optional sections (${lang}, ${name}): every stop builds without ${gone.join(", ")}`, () => {
        setLang(lang);
        setTimeZone("America/Santiago");
        const snap = without(fixture(name), gone);
        const nowMs = Date.parse(snap.generated_at);
        const m = model(snap);
        assert.doesNotThrow(() => nightFigures(snap));
        assert.doesNotThrow(() => stopCounts(m));
        for (const [id, view] of Object.entries(VIEWS)) {
          let built;
          assert.doesNotThrow(() => { built = view(m, nowMs); }, `${id} builds`);
          const bad = strings(built).filter((s) => BAD.test(s));
          assert.deepEqual(bad, [], `${id} says nothing broken`);
        }
      });
    }
  }
}

test("optional sections: the queues are the same with the five sections missing", () => {
  setLang("es");
  setTimeZone("America/Santiago");
  const whole = fixture("rich");
  const thin = without(whole, OPTIONAL);
  const nowMs = Date.parse(whole.generated_at);
  const cola = (snap) => colaView(model(snap), { nowMs });
  const hoy = (snap) => hoyView(model(snap), { nowMs });
  assert.ok(cola(whole).proposals.length > 0 && hoy(whole).rows.length > 0, "the fixture has work to show");
  assert.deepEqual(cola(thin).proposals, cola(whole).proposals);
  // (the rest of COLA's list includes "sent tonight", which is read from `activity`: that part goes, the proposals stay)
  assert.deepEqual(model(thin).tasks, model(whole).tasks);
  assert.deepEqual(hoy(thin).rows, hoy(whole).rows);
  assert.equal(hoy(thin).headline, hoy(whole).headline);
});

test("optional sections: the contract accepts a null section and still asks for the key itself", async () => {
  const { missing, KEYS } = await import("./helpers/contract.mjs");
  assert.deepEqual(KEYS.optional, OPTIONAL, "the list the server's test reads is this one");
  const whole = fixture("rich");
  assert.deepEqual(missing(without(whole, OPTIONAL)), [], "null sections lack nothing");
  const dropped = structuredClone(whole);
  delete dropped.learn;
  assert.deepEqual(missing(dropped).filter((p) => !p.includes(".")), ["learn"], "a section that is not there at all is reported");
  const hollow = structuredClone(whole);
  hollow.metrics = {};
  assert.ok(missing(hollow).includes("metrics.sent_week"), "an empty object is not a null: its keys are asked for");
});
