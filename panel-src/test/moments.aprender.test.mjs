import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { setLang, setTimeZone } from "../src/i18n/index.js";
import esAprender from "../src/i18n/es/aprender.js";
import enAprender from "../src/i18n/en/aprender.js";
import { model, stopCounts } from "../src/core/model.js";
import { aprenderView, numWord, keyShown, MISSING_ROWS, TOPIC_ROWS, SPELLED_UP_TO } from "../src/moments/aprender/view.js";
import { missingOf, parseDossier } from "../src/moments/aprender/data.js";
import { KEYS } from "../src/ui/keys.js";
import { richAprender, bareAprender, hostileAprender, DOSSIER_MD } from "./helpers/aprender-fixture.mjs";

const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/snapshot.${name}.json`, import.meta.url), "utf8"));
const nowOf = (snap) => Date.parse(snap.generated_at);
const view = (snap, opts = {}) => aprenderView(model(snap), { nowMs: nowOf(snap), ...opts });
const fresh = (lang = "es") => { setLang(lang); setTimeZone("America/Santiago"); };
// every string a person would read; `raw` is what a request sends back as the server wrote it, and is never shown
const strings = (value, out = []) => {
  if (typeof value === "string") out.push(value);
  else if (Array.isArray(value)) value.forEach((v) => strings(v, out));
  else if (value && typeof value === "object") Object.entries(value).filter(([k]) => k !== "raw").forEach(([, v]) => strings(v, out));
  return out;
};
const ok = new Map([["fx:4", { state: "ok", parsed: parseDossier(DOSSIER_MD) }]]);

test("A16: every number of APRENDER is the model's: the headline counts the questions, the bar and the page agree on what is due", () => {
  fresh();
  for (const [name, snap] of [["rich", richAprender()], ["bare", bareAprender()], ["empty", fixture("empty")], ["heavy", fixture("heavy")]]) {
    const m = model(snap);
    const v = aprenderView(m, { nowMs: nowOf(snap) });
    const miss = missingOf(snap);
    assert.equal(v.missing.count, miss.groups.length + (miss.unnamed.length ? 1 : 0), name);
    assert.equal(v.missing.forms, miss.forms, name);
    assert.equal(v.drill.due ?? 0, stopCounts(m).aprender, `${name}: the due count is the one of the bar of stops`);
    assert.equal(v.bank.figures.find((f) => f.id === "blocked").n, miss.forms, name);
  }
  const rich = richAprender();
  const before = view(rich);
  rich.needs_gonza.cards.push({ ...rich.needs_gonza.cards[1], key: "fx:77", company: "Upsilon Test Co", reason: "faltan respuestas: A brand new question" });
  rich.needs_gonza.total += 1;
  const after = view(rich);
  assert.equal(after.missing.count, before.missing.count + 1, "one more question: the headline follows the data");
  assert.equal(after.missing.forms, before.missing.forms + 1);
  rich.learn.due += 3;
  assert.equal(view(rich).drill.due, before.drill.due + 3);
  assert.equal(numWord(7), "siete");
  assert.equal(numWord(1), "una", "it counts questions");
  assert.equal(numWord(SPELLED_UP_TO + 1), String(SPELLED_UP_TO + 1), "above twenty it is the figure");
});

test("the headline says what blocks the bot, then what is due, then that there is nothing", () => {
  fresh();
  const rich = view(richAprender());
  assert.equal(rich.state, "work");
  assert.equal(rich.lead, "No supo");
  assert.equal(rich.turn, "seis.", "six questions block six forms");
  assert.match(rich.sub[0], /^6 preguntas de formularios que el banco no sabe contestar\.$/);
  assert.match(rich.sub[1], /^Con tu respuesta en el banco, u devuelve 6 postulaciones a la cola/);
  assert.equal(rich.missing.head, "Respuestas que el bot no supo (6): bloquean 6 formularios");
  const bare = view(bareAprender());
  assert.equal(bare.turn, "una.");
  assert.equal(bare.sub[0], "Una pregunta de formulario que el banco no sabe contestar.");
  assert.match(bare.sub[1], /^Con tu respuesta en el banco, u devuelve 1 postulación a la cola; el motor la prepara/);

  const review = richAprender();
  review.needs_gonza = { total: 0, cards: [] };
  const r = view(review);
  assert.deepEqual([r.state, r.lead, r.turn], ["work", "Repaso:", "doce."]);
  assert.match(r.sub[0], /^Ninguna pregunta frena una postulación\. Hay 12 del repaso esperando/);

  const calm = view(fixture("empty"));
  assert.deepEqual([calm.state, calm.calm, calm.lead, calm.turn], ["calm", true, "Nada que repasar.", ""]);
  assert.equal(calm.sub[0], "Vuelve cuando el bot pregunte.");
  assert.equal(calm.missing.head, "Respuestas que el bot no supo", "nothing is missing: the head carries no \"0 · bloquean 0\"");
});

test("A15 in the sentence: an engine that never ran or that is stale says nothing will be prepared; one that runs says nothing", () => {
  fresh();
  const never = view(fixture("empty"));
  assert.equal(never.sub.at(-1), "El motor todavía no ha corrido; hasta que corra, no prepara nada.");
  assert.deepEqual(never.engine, ["sin corridas todavía"]);
  const stale = richAprender();
  stale.controls.engine = "stale";
  stale.controls.last_run_at = new Date(nowOf(stale) - 40 * 3600000).toISOString();
  assert.equal(view(stale).sub.at(-1), "El motor corrió por última vez hace 40 h; hasta que corra, no prepara nada.");
  const running = view(richAprender());
  assert.ok(!running.sub.some((s) => /motor corrió|todavía no ha corrido/.test(s)));
  assert.match(running.engine.join(" · "), /^Ventana cerrada, abre .* · última corrida hace 7 h$/);
  // a clock that is wrong never prints a negative time
  const skew = richAprender();
  skew.controls.last_run_at = new Date(nowOf(skew) + 5 * 3600000).toISOString();
  for (const s of strings(view(skew))) assert.doesNotMatch(s, /NaN|undefined|null|−/);
});

test("the questions: one row per question, the most blocking first, the first five shown and the rest behind 'y N más'", () => {
  fresh();
  const v = view(richAprender());
  assert.equal(v.missing.rows.length, MISSING_ROWS);
  assert.equal(v.missing.more, 1);
  assert.equal(v.missing.moreLabel, "y 1 más");
  const first = v.missing.rows[0];
  assert.equal(first.formsLabel, "2 formularios");
  assert.match(first.who, /^Frena a /);
  assert.deepEqual(first.actions.map((a) => a.id), ["resolve", "open", "link"]);
  assert.deepEqual(first.actions.filter((a) => a.visible).map((a) => [a.id, a.key, a.label, a.permanent]), [["resolve", "u", "Ya la contesté", true]]);
  assert.equal(first.keys.length, 2, "what u gives back are all the forms that question blocks");
  assert.equal(first.data.card, first.keys[0], "the cursor walks the rows through the first form of each");
  const all = view(richAprender(), { expanded: true });
  assert.equal(all.missing.rows.length, 6);
  assert.equal(all.missing.moreLabel, "ver menos");
  // three companies: two named and "y 1 más"
  const snap = richAprender();
  const stuck = snap.needs_gonza.cards.filter((c) => c.reason_kind === "missing_answers");
  for (const c of stuck) c.reason = "faltan respuestas: One question for all";
  const row = view(snap).missing.rows[0];
  assert.equal(row.forms, 6);
  assert.match(row.who, /^Frena a .+, .+ y 4 más$/);
  assert.equal(row.formsLabel, "6 formularios");
});

test("a queue cut at its cap says 'al menos'; a form whose reason lists no question is one row apart", () => {
  fresh();
  const cut = richAprender();
  cut.needs_gonza.total = 90;
  assert.match(view(cut).missing.head, /bloquean al menos 6 formularios$/);
  const odd = richAprender();
  odd.needs_gonza.cards[1].reason = "faltan respuestas:";
  const v = view(odd, { expanded: true });
  const unnamed = v.missing.rows.find((r) => r.id === "miss:unnamed");
  assert.equal(unnamed.label, "Preguntas sin nombre");
  assert.equal(unnamed.who, "El motor no dijo cuáles faltan");
  assert.equal(v.missing.forms, 6);
});

test("what the bank knows: two numbers from the snapshot and two that say 'sin dato', because the bank is not in it", () => {
  fresh();
  const v = view(richAprender());
  const byId = Object.fromEntries(v.bank.figures.map((f) => [f.id, f]));
  assert.deepEqual([byId.complete.n, byId.blocked.n, byId.seen.n], [10, 6, 36]);
  assert.equal(byId.complete.label, "formularios completos en 7 días");
  assert.deepEqual([byId.bank.n, byId.bank.nodata], [null, true], "the answers in the bank are not in the snapshot: sin dato, never zero");
  assert.match(v.bank.note, /no viaja en el snapshot/);
  const noLearn = richAprender();
  delete noLearn.learn;
  const n = view(noLearn);
  assert.equal(n.bank.figures.find((f) => f.id === "seen").n, null, "no learn at all is sin dato, not zero");
  assert.equal(n.drill.state, "nodata");
  assert.equal(n.drill.head, "Repaso de hoy: sin dato");
});

test("the rejections are counted, never rated; with few there is no pattern; with none the block is not drawn", () => {
  fresh();
  const v = view(richAprender());
  assert.equal(v.rejects.head, "Patrones de rechazo: 9 cerradas por rechazo");
  assert.equal(v.rejects.rows.length, 2);
  assert.match(v.rejects.rows[0].text, /^Puntaje: mediana \d+ en las rechazadas y \d+ en las \d+ que siguen vivas\.$/);
  assert.equal(v.rejects.rows[1].text, "5 de 9 vinieron de ashby.");
  assert.equal(v.rejects.note, "n = 9: una hipótesis para ajustar el umbral, no una predicción.");
  assert.match(v.rejects.nodata, /sin dato/, "how long each took is not in the snapshot");
  for (const s of strings(v.rejects)) assert.doesNotMatch(s, /%/, "no rate");
  const few = richAprender();
  const col = few.board.find((c) => c.state === "rejected");
  col.cards = col.cards.slice(0, 2);
  col.total = 2;
  const f = view(few).rejects;
  assert.equal(f.few, "Con 2 rechazadas todavía no hay patrón que mostrar; aparecen desde 5.");
  assert.equal(f.rows.length, 0);
  assert.equal(f.note, null);
  assert.equal(view(bareAprender()).rejects, null);
  const cut = richAprender();
  cut.board.find((c) => c.state === "rejected").total = 40;
  assert.equal(view(cut).rejects.head, "Patrones de rechazo: las 9 listadas de 40");
});

test("the eve: the interview, its date, and its dossier in the state it is in; and where there is none, a sentence", () => {
  fresh();
  const snap = richAprender();
  const loading = view(snap).eve;
  assert.equal(loading.state, "loading");
  assert.match(loading.head, /^La víspera: Delta Test Co, \d/);
  assert.equal(loading.text, "Leyendo el dossier…");
  const done = view(snap, { dossiers: ok }).eve;
  assert.equal(done.state, "ok");
  assert.equal(done.questions.length, 3, "three of the four likely questions");
  assert.equal(done.stories.length, 3);
  assert.equal(done.stories[0].evidence, "Rewrote the queue service in Rust, halving p99 latency");
  assert.equal(done.gaps, "2 tarjetas nuevas de repaso");
  assert.deepEqual(done.row.actions.filter((a) => a.visible).map((a) => [a.id, a.key]), [["open", "↵"]]);
  assert.equal(done.row.data.card, "fx:4");
  assert.equal(view(snap, { dossiers: new Map([["fx:4", { state: "error", why: "No existe esa postulación" }]]) }).eve.text, "No pude leer el dossier: No existe esa postulación");
  assert.equal(view(snap, { dossiers: new Map([["fx:4", { state: "ok", parsed: null }]]) }).eve.state, "unreadable");
  assert.equal(view(snap, { dossiers: new Map([["fx:4", { state: "none" }]]) }).eve.state, "nodossier", "the server has no dossier for it: said as such");
  snap.board.find((c) => c.state === "interview").cards[0].has_dossier = false;
  assert.equal(view(snap).eve.state, "nodossier");
  const none = view(bareAprender()).eve;
  assert.deepEqual([none.state, none.row, none.text], ["none", null, "Ninguna entrevista en curso. La víspera de una, su dossier aparece aquí."]);
  const no = richAprender();
  no.today.tasks = no.today.tasks.filter((t) => t.kind !== "interview_invite");
  no.board.find((c) => c.state === "interview").cards[0].note = null;
  assert.match(view(no).eve.head, /sin fecha$/, "an unknown date is said, never invented");
});

test("the review: the figure of the bar, the topics most due first with their average, the gaps of the dossiers", () => {
  fresh();
  const v = view(richAprender());
  assert.equal(v.drill.head, "Repaso de hoy: 12 pendientes, vistas 36 de 56");
  assert.equal(v.drill.state, "due");
  assert.equal(v.drill.topics.length, 7);
  assert.ok(v.drill.topics.length <= TOPIC_ROWS);
  assert.deepEqual(v.drill.topics.slice(0, 2).map((x) => [x.topic, x.dueLabel, x.avgLabel]), [["rust", "4 hoy", null], ["system-design", "2 hoy", "prom. 2,8"]]);
  assert.ok(v.drill.topics.every((x) => x.wSeen >= 0 && x.wSeen <= 1 && x.wDue >= 0 && x.wDue <= 1));
  assert.equal(v.drill.gaps.head, "Huecos que dejaron los dossiers (2)");
  assert.equal(v.drill.gaps.rows[0].data.card, "fx:4");
  assert.equal(v.drill.start.key, "↵");
  const calm = view(fixture("empty")).drill;
  assert.deepEqual([calm.state, calm.text], ["empty", "No hay preguntas todavía. Cuando se agreguen, las verás aquí."]);
  const done = richAprender();
  done.learn.due = 0;
  assert.deepEqual([view(done).drill.state, view(done).drill.text], ["calm", "Nada que repasar. Vuelve cuando el bot pregunte."]);
});

test("the heavy snapshot: the headline is the real number and the list is cut, not the number", () => {
  fresh();
  const snap = fixture("heavy");
  const stuck = snap.needs_gonza.cards.filter((c) => c.reason_kind === "missing_answers");
  assert.ok(stuck.length >= 2);
  const many = richAprender();
  for (let i = 0; i < 20; i++) many.needs_gonza.cards.push({ ...many.needs_gonza.cards[1], key: "fx:" + (300 + i), company: "Test Co " + i, reason: "faltan respuestas: Question number " + i });
  many.needs_gonza.total += 20;
  const v = view(many);
  assert.equal(v.missing.count, 26);
  assert.equal(v.turn, "26.", "above twenty the headline is the figure");
  assert.equal(v.missing.rows.length, MISSING_ROWS);
  assert.equal(v.missing.more, 21);
});

test("hostile text is text: no control or bidi character, no address that is not http(s), nothing that says null, undefined or NaN", () => {
  fresh();
  const snap = hostileAprender();
  const v = view(snap, { dossiers: new Map([["fx:4", { state: "ok", parsed: parseDossier(DOSSIER_MD) }]]), expanded: true });
  for (const s of strings(v)) {
    assert.doesNotMatch(s, /[\u202e\u2066\u2069\u0000-\u001f]/, "control or bidi character in: " + s.slice(0, 40));
    assert.doesNotMatch(s, /NaN|undefined|\bnull\b|⟦/, s.slice(0, 60));
    assert.ok(s.length < 700, "a text of 300 characters is cut before it reaches the page");
  }
  const row = v.missing.rows.find((r) => r.keys.includes("fx:10"));
  assert.equal(row.link, null, "javascript: is not a link");
  assert.ok(!row.actions.some((a) => a.id === "link"));
});

test("both languages: every string exists, none is a missing key, and es and en have the same keys and variables", () => {
  const vars = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(",");
  assert.deepEqual(Object.keys(esAprender).sort(), Object.keys(enAprender).sort());
  for (const key of Object.keys(esAprender)) assert.equal(vars(esAprender[key]), vars(enAprender[key]), key);
  for (const lang of ["es", "en"]) {
    fresh(lang);
    for (const snap of [richAprender(), bareAprender(), fixture("empty"), fixture("heavy"), hostileAprender()]) {
      for (const s of strings(aprenderView(model(snap), { nowMs: nowOf(snap), dossiers: ok, expanded: true }))) assert.ok(!s.includes("⟦"), `${lang}: ${s}`);
    }
  }
  fresh("en");
  const en = view(richAprender());
  assert.deepEqual([en.lead, en.turn], ["Stuck on", "six."]);
  assert.match(en.sub[0], /^6 form questions\b/, "the count leads the sentence");
  fresh();
});

test("the keys the page shows are the ones of the key table", () => {
  assert.equal(keyShown("resolve"), "u");
  assert.equal(keyShown("open"), "↵");
  assert.equal(keyShown("link"), "o");
  assert.throws(() => keyShown("nope"));
  const aprender = KEYS.filter((k) => (k.stops || []).includes("aprender")).map((k) => k.id);
  for (const id of ["move", "open", "resolve", "link", "grade", "reveal"]) assert.ok(aprender.includes(id), `${id} works in APRENDER`);
});
