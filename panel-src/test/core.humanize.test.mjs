import test from "node:test";
import assert from "node:assert/strict";
import { humanize, cardIndex } from "../src/core/humanize.js";

// What the server says in its own words ("Aprobada: greenhouse:7345211", "Pospuesta hasta 2026-10-12 18:03: ...") is turned into what Gonza reads: the
// company and the role instead of the internal key, and the dates the way he writes them. Pure: a text and the cards of the model in, a text out.
const cards = new Map([
  ["test:2", { company: "Lumen Pay", title: "Rust Engineer" }],
  ["greenhouse:7345211", { company: "Brisa Mobility", title: "Full-stack Engineer" }],
  ["ashby:abc-123", { company: "Ancla Security", title: "Principal Staff Backend Platform Engineer (Remote, EMEA)" }],
  ["lever:1", { company: "Nimbus Health", title: null }],
  ["lever:2", { company: null, title: "Site Reliability Engineer" }],
  ["lever:3", { company: "<b>Evil</b>", title: "<i>x</i>" }],
  ["lever:4", { company: "", title: "" }],
]);
const SCL = { tz: "America/Santiago" };
const es = (text, extra = {}) => humanize(text, cards, { lang: "es", ...SCL, ...extra });
const en = (text, extra = {}) => humanize(text, cards, { lang: "en", ...SCL, ...extra });

test("a key of the model becomes the company and the role, wherever the server put it", () => {
  assert.equal(es("Aprobada: test:2"), "Aprobada: Lumen Pay — Rust Engineer");
  assert.equal(es("De vuelta en tu lista: greenhouse:7345211"), "De vuelta en tu lista: Brisa Mobility — Full-stack Engineer");
  assert.equal(en("Approved: test:2"), "Approved: Lumen Pay — Rust Engineer");
  assert.equal(es("test:2"), "Lumen Pay — Rust Engineer");
});

test("the punctuation around a key stays where it was", () => {
  assert.equal(es("Listo (test:2)."), "Listo (Lumen Pay — Rust Engineer).");
  assert.equal(es("Se fue test:2, y volvió."), "Se fue Lumen Pay — Rust Engineer, y volvió.");
  assert.equal(es("test:2: sin respuesta"), "Lumen Pay — Rust Engineer: sin respuesta");
  assert.equal(es("«test:2»"), "«Lumen Pay — Rust Engineer»");
  assert.equal(es("Dos   espacios   test:2"), "Dos   espacios   Lumen Pay — Rust Engineer");
});

test("a key the model does not know stays as it came", () => {
  assert.equal(es("Aprobada: ghost:99"), "Aprobada: ghost:99");
  assert.equal(es("test:22 y test:"), "test:22 y test:", "a key is the whole word, not a piece of one");
  assert.equal(es("xtest:2"), "xtest:2");
  assert.equal(es("test:2x"), "test:2x");
  assert.equal(es("mira http://example.com:8080/x"), "mira http://example.com:8080/x");
});

test("two keys in one message, and the same key twice, are each replaced", () => {
  assert.equal(es("Unidas test:2 y greenhouse:7345211"), "Unidas Lumen Pay — Rust Engineer y Brisa Mobility — Full-stack Engineer");
  assert.equal(es("test:2 → test:2"), "Lumen Pay — Rust Engineer → Lumen Pay — Rust Engineer");
});

test("a long role is left out, and a missing company or role does not leave a dash behind", () => {
  assert.equal(es("Aprobada: ashby:abc-123"), "Aprobada: Ancla Security");
  assert.equal(es("Aprobada: lever:1"), "Aprobada: Nimbus Health");
  assert.equal(es("Aprobada: lever:2"), "Aprobada: Site Reliability Engineer");
  assert.equal(es("Aprobada: lever:4"), "Aprobada: lever:4", "a card with nothing to say keeps its key");
});

test("a date of the server is written the way he writes it, in the language of the page", () => {
  assert.equal(es("Pospuesta hasta 2026-10-12 18:03: test:2"), "Pospuesta hasta lun 12 oct, 18:03: Lumen Pay — Rust Engineer");
  assert.equal(en("Snoozed until 2026-10-12 18:03: test:2"), "Snoozed until Mon 12 Oct, 6:03 PM: Lumen Pay — Rust Engineer");
  assert.equal(es("hasta 2026-10-12 09:05."), "hasta lun 12 oct, 09:05.");
  assert.equal(en("until 2026-10-12 00:30"), "until Mon 12 Oct, 12:30 AM");
  assert.equal(en("until 2026-10-12 12:00"), "until Mon 12 Oct, 12:00 PM");
});

test("an ISO date with a zone is moved to the clock of the page; one without a zone is the server's own wall clock (Santiago)", () => {
  assert.equal(es("hasta 2026-10-12T21:03:00Z"), "hasta lun 12 oct, 18:03", "Santiago is UTC-3 in October");
  assert.equal(es("hasta 2026-10-12T21:03:00.250Z"), "hasta lun 12 oct, 18:03");
  assert.equal(es("hasta 2026-10-12T18:03:00-03:00"), "hasta lun 12 oct, 18:03");
  assert.equal(es("hasta 2026-10-12T20:03:00-01:00"), "hasta lun 12 oct, 18:03");
  assert.equal(es("hasta 2026-10-12T18:03:00-0300"), "hasta lun 12 oct, 18:03");
  assert.equal(es("hasta 2026-10-12T18:03"), "hasta lun 12 oct, 18:03", "no zone: the server's wall clock");
  assert.equal(es("hasta 2026-10-12 18:03", { tz: "UTC" }), "hasta lun 12 oct, 21:03", "the same instant on a clock in UTC");
  assert.equal(es("hasta 2026-10-12T21:03:00Z", { tz: "UTC" }), "hasta lun 12 oct, 21:03");
  assert.equal(es("hasta 2026-10-13T01:30:00Z"), "hasta lun 12 oct, 22:30", "the day follows the zone");
  assert.equal(en("until 2026-10-13T01:30:00Z"), "until Mon 12 Oct, 10:30 PM");
});

test("a date with no hour is only a day; things that look like dates and are not stay as they came", () => {
  assert.equal(es("hasta 2026-10-12"), "hasta lun 12 oct");
  assert.equal(en("until 2026-10-12."), "until Mon 12 Oct.");
  assert.equal(es("el 18:03 de hoy"), "el 18:03 de hoy", "an hour alone is not a date");
  assert.equal(es("2026-13-45 99:99"), "2026-13-45 99:99");
  assert.equal(es("2026-02-30 10:00"), "2026-02-30 10:00", "February has no 30th");
  assert.equal(es("v2026-10-12"), "v2026-10-12");
  assert.equal(es("full-stack 12-10-2026"), "full-stack 12-10-2026");
});

test("a text that is not about a key or a date comes back exactly as it was", () => {
  for (const text of ["", "Listo: jobbot se detuvo.", "Marcada como enviada.", "  espacios  ", "línea uno\nlínea dos", "Aprobada: la próxima corrida la envía."]) assert.equal(es(text), text, JSON.stringify(text));
  assert.equal(es(null), "");
  assert.equal(es(undefined), "");
  assert.equal(es(42), "42");
});

test("hostile text is only text: no markup is made, none is removed, and a company with markup shows it as written", () => {
  assert.equal(es("<b>hola</b> test:2"), "<b>hola</b> Lumen Pay — Rust Engineer");
  assert.equal(es("<b>test:2</b>"), "<b>test:2</b>", "a key inside a tag is not a word of its own");
  assert.equal(es("<img src=x onerror=alert(1)> test:2"), "<img src=x onerror=alert(1)> Lumen Pay — Rust Engineer");
  assert.equal(es("Aprobada: lever:3"), "Aprobada: <b>Evil</b> — <i>x</i>");
  assert.equal(es("&lt;b&gt; test:2"), "&lt;b&gt; Lumen Pay — Rust Engineer");
});

test("it is idempotent: said twice, it says the same", () => {
  for (const text of ["Pospuesta hasta 2026-10-12 18:03: test:2", "Aprobada: ashby:abc-123", "hasta 2026-10-12T21:03:00Z y lever:1"]) {
    assert.equal(es(es(text)), es(text), text);
    assert.equal(en(en(text)), en(text), text);
  }
});

test("without cards it still writes the dates, and a plain object of cards is accepted", () => {
  assert.equal(humanize("hasta 2026-10-12 18:03: test:2", new Map(), { lang: "es", ...SCL }), "hasta lun 12 oct, 18:03: test:2");
  assert.equal(humanize("Aprobada: test:2", { "test:2": { company: "Lumen Pay", title: "Rust Engineer" } }, { lang: "es" }), "Aprobada: Lumen Pay — Rust Engineer");
  assert.equal(humanize("Aprobada: test:2", null, { lang: "es" }), "Aprobada: test:2");
});

test("cardIndex gathers every card of a snapshot by key: the board, what needs him, what awaits approval and what is put aside", () => {
  const snap = {
    board: [{ state: "submitted", cards: [{ key: "a:1", company: "A", title: "T1" }] }, { state: "scored", cards: [{ key: "a:2", company: "B", title: "T2" }] }],
    needs_gonza: { cards: [{ key: "a:3", company: "C", title: "T3" }] },
    awaiting_approval: { cards: [{ key: "a:1", company: "A", title: "T1" }] },
    snoozed: { cards: [{ key: "a:4", company: "D", title: "T4" }] },
  };
  const index = cardIndex(snap);
  assert.deepEqual([...index.keys()].sort(), ["a:1", "a:2", "a:3", "a:4"]);
  assert.deepEqual(index.get("a:4"), { company: "D", title: "T4" });
  assert.equal(cardIndex(snap), index, "one index per snapshot");
  assert.equal(cardIndex(null).size, 0);
  assert.equal(cardIndex({}).size, 0);
});
