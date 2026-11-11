// Acceptance test of the encargo docs/encargos/lote-cara-publica.md: what whoever opens the public demo sees in the first minute.
// (1) Every scenario of the demo whose engine ran passes its happy path: SISTEMA has the result of the last run and every step ran, saying
//     what it did with the numbers of the night; the failures stay in ?scenario=stale and ?scenario=empty.
// (2) APRENDER reads the dossier of every interview in Spanish and in English, each in its own words and about its own application: the one
//     of the default demo, the ones of ?scenario=heavy and one marked by hand, with or without a gap on file; and the parser reads the
//     engine's dossier when a section comes empty.
// (3) Every question the form-filler could not answer has a name, in every scenario.
// (4) APRENDER says how to answer today and promises no panel route that does not exist.
// (5) The English dictionary has none of the calques of the Spanish that the review found, one name for the quiet state across PROCESOS,
//     and the signature as it is.
// (6) The demo calls itself jobbot in every scenario; the real panel still says "Panel", in its tab too (ui/title.js with no brand). main.js
//     hands the demo's brand to the shell and the shell to the HUD and the tab (read in the source). The note that says what jobbot is, with its
//     links, is a later encargo (t34).
// No browser: the demo runs in a vm (test/helpers/demo-host.mjs), the views and the parser are pure, the dictionaries and the wiring of
// the brand are read; the tab is ui/title.js with a one-property document.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { loadDemo, loadDemoPage } from "./helpers/demo-host.mjs";
import { setLang, setTimeZone, t } from "../src/i18n/index.js";
import { model } from "../src/core/model.js";
import { sistemaView, STEP_IDS } from "../src/moments/sistema/view.js";
import { aprenderView } from "../src/moments/aprender/view.js";
import { missingOf, parseDossier, eveOf } from "../src/moments/aprender/data.js";

const LANGS = ["es", "en"];
const I18N = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "src", "i18n");
const plain = (x) => JSON.parse(JSON.stringify(x));          // what the vm hands back, as plain objects of this realm
const fresh = (lang) => { setLang(lang); setTimeZone("America/Santiago"); };
const snapshot = async (demo) => plain((await demo.api("/api/snapshot")).data);
const sistemaOf = (snap) => sistemaView(model(snap), { nowMs: Date.parse(snap.generated_at) });
const cardsOf = (snap) => snap.board.flatMap((col) => col.cards);
/** What the demo's server answers, or the error it threw, as an answer the test can assert on. */
const ask = async (demo, p, body) => plain(await demo.api(p, body).catch((e) => ({ ok: false, status: 0, data: String(e && e.stack || e) })));

/** Every text of one language's dictionaries: [file, key, text]. */
async function dictionary(lang) {
  const out = [];
  for (const file of fs.readdirSync(path.join(I18N, lang)).filter((f) => f.endsWith(".js")).sort()) {
    const words = (await import(pathToFileURL(path.join(I18N, lang, file)).href)).default;
    for (const [key, text] of Object.entries(words)) out.push([file, key, String(text)]);
  }
  return out;
}

// ---- (1) SISTEMA: the happy path in every scenario that ran, and the failures where they belong

// The scenarios of the demo whose engine ran (demo.js, ?scenario=): all but stale and empty.
const RAN = ["", "?scenario=calm", "?scenario=urgent", "?scenario=stop", "?scenario=window", "?scenario=closed", "?scenario=heavy"];

test("every demo scenario whose engine ran shows the result of its last run, and every step ran saying what it did with the numbers of the night, in both languages; stale and empty have no record", async () => {
  for (const lang of LANGS) {
    for (const search of RAN) {
      fresh(lang);
      const at = `${lang} ${search || "default"}`;
      const snap = await snapshot(loadDemo({ search, lang }));
      const v = sistemaOf(snap);
      assert.equal(v.run.result.tone, "ok", `${at}: the result is "${v.run.result.text}"`);
      assert.deepEqual(v.run.steps.map((s) => [s.id, s.tone]), STEP_IDS.map((id) => [id, "ran"]), `${at}: every step ran`);
      for (const step of v.run.steps) assert.notEqual(step.text, t("sistema.step.ran"), `${at}: ${step.id} says what it did, with its numbers, not a bare "${step.text}"`);
      assert.ok(!v.errors.rows.some((r) => r.id === "run"), `${at}: no "the last run ended with an error" among the errors`);
      // the numbers are the night's, so SISTEMA agrees with the prologue and the board: what moved overnight, and one dossier per interview
      const moved = (state) => (snap.activity.moves_by_state || {})[state] || 0;
      const s = snap.controls.last_run_steps;
      assert.deepEqual(
        [s.discover.new, s.prepare.prepared, s.submit.submitted, s.submit.needs_gonza, s.dossiers.written],
        [moved("scored"), moved("prepared"), moved("submitted"), moved("needs_gonza"), cardsOf(snap).filter((c) => c.state === "interview").length],
        `${at}: discovered, prepared, sent, left for you and dossiers written`,
      );
    }
  }
  // after the morning's work (Sendero's form resolved by hand and back in the queue) the board has one more prepared and one fewer for him,
  // and the record still counts the night, as the prologue does: the numbers do not come from the board
  {
    const demo = loadDemo({});
    assert.ok((await ask(demo, "/api/resolve", { key: "demo:13" })).ok, "Sendero's form can be resolved by hand");
    const snap = await snapshot(demo);
    const moved = (state) => (snap.activity.moves_by_state || {})[state] || 0;
    const s = snap.controls.last_run_steps;
    assert.deepEqual([s.prepare.prepared, s.submit.needs_gonza], [moved("prepared"), moved("needs_gonza")], "after a resolve: prepared and left for you, as the night counts them");
    assert.notEqual(cardsOf(snap).filter((c) => c.state === "prepared").length, moved("prepared"), "the board and the night tell apart here");
  }
  for (const lang of LANGS) {
    fresh(lang);
    const stale = sistemaOf(await snapshot(loadDemo({ search: "?scenario=stale", lang })));
    assert.equal(stale.chip.tone, "stale", `${lang}: ?scenario=stale is still the engine that missed a night`);
    assert.equal(stale.run.result.tone, "none", `${lang}: and its last run has no record, neither "finished fine" nor a made-up error: "${stale.run.result.text}"`);
    assert.ok(stale.run.steps.every((s) => s.tone === "none"), `${lang}: nor any step (${stale.run.steps.map((s) => s.tone)})`);
    const empty = sistemaOf(await snapshot(loadDemo({ search: "?scenario=empty", lang })));
    assert.equal(empty.engine, "never", `${lang}: ?scenario=empty is still a fresh install`);
    assert.equal(empty.run.result.tone, "none", `${lang}: with no result`);
    assert.ok(empty.run.steps.every((s) => s.tone === "none"), `${lang}: and no step made up`);
  }
});

// ---- (2) APRENDER: the dossiers in both languages

// What the engine writes in Spanish and must not show up in an English dossier: its headings and its labels; and the other way round.
const SPANISH_DOSSIER = ["Generado el", "## Qué dice", "## Preguntas", "## Historias", "## Huecos", "Por qué", "Evidencia", "repasa con", "Ninguno detectado", "«"];
const ENGLISH_DOSSIER = ["Generated on", "## What the", "## Likely", "## Stories", "## Gaps", "*Why:*", "Evidence in", "review with", "None found", "“"];
const foreign = (lang, text) => (lang === "en" ? SPANISH_DOSSIER : ENGLISH_DOSSIER).filter((word) => text.includes(word));
/** The engine's first line: "# Dossier — <company> · <title>". */
const titleLine = (card) => "# Dossier — " + card.company + " · " + card.title + "\n";
const lf = (text) => String(text).replace(/\r\n?/g, "\n");
/** The engine's second line (crates/jobbot-app/src/learn/dossier.rs): the day it was written, Santiago's date only, and the posting. */
const GENERATED = { es: /^Generado el (\d{4}-\d{2}-\d{2}) \(hora de Santiago\) · aviso: https:\/\/\S+$/, en: /^Generated on (\d{4}-\d{2}-\d{2}) \(Santiago time\) · posting: https:\/\/\S+$/ };
const santiagoDay = (iso) => new Intl.DateTimeFormat("sv-SE", { timeZone: "America/Santiago", year: "numeric", month: "2-digit", day: "2-digit" }).format(new Date(iso));
/** How each language labels a story's literal evidence, and its quotes. */
const EVIDENCE = { es: /^Evidencia en el CV: «[^«»]+»$/m, en: /^Evidence in the CV: “[^“”]+”$/m };
/** Nimbus Health (demo:4) keeps its own words: the language-model team for support, two technical interviewers, three questions of its own. */
const NIMBUS = {
  es: { words: ["equipo de modelos de lenguaje para soporte", "Dos entrevistadores técnicos"], first: "¿Cómo evaluarías un modelo antes y después de llevarlo a producción?" },
  en: { words: ["language-model team for support", "Two technical interviewers"], first: "How would you evaluate a model before and after it goes to production?" },
};

/** The same dossier read in both languages: every item of the English one is not the Spanish one at the same place (the body is translated,
 * not only the headings). */
function translated(es, en, at) {
  es.questions.forEach((q, i) => {
    assert.notEqual(en.questions[i].q, q.q, `${at}: likely question ${i + 1} in both languages`);
    assert.notEqual(en.questions[i].why, q.why, `${at}: the why of question ${i + 1} in both languages`);
  });
  es.stories.forEach((st, i) => {
    assert.notEqual(en.stories[i].title, st.title, `${at}: story ${i + 1} in both languages`);
    assert.notEqual(en.stories[i].evidence, st.evidence, `${at}: the evidence of story ${i + 1} in both languages`);
  });
  es.gaps.forEach((g, i) => {
    assert.notEqual(en.gaps[i].topic, g.topic, `${at}: gap ${i + 1} in both languages`);
    assert.notEqual(en.gaps[i].question, g.question, `${at}: the question of gap ${i + 1} in both languages`);
  });
}

test("APRENDER reads the dossier of the default demo's interview in Spanish and in English: the same dossier, each language in its own words, also after the language toggle", async () => {
  const read = {};
  for (const lang of LANGS) {
    fresh(lang);
    const demo = loadDemo({ lang });
    const snap = await snapshot(demo);
    const nowMs = Date.parse(snap.generated_at);
    const eve = eveOf(snap, nowMs);
    assert.ok(eve && eve.card.has_dossier, `${lang}: the demo has an interview with a dossier`);
    const text = (await ask(demo, "/api/job?key=" + encodeURIComponent(eve.card.key))).data.dossier;
    const parsed = parseDossier(text);
    assert.ok(parsed, `${lang}: the dossier has the headings APRENDER reads`);
    assert.ok(lf(text).startsWith(titleLine(eve.card)), `${lang}: the engine's title line, "${lf(text).split("\n")[0]}"`);
    const v = aprenderView(model(snap), { nowMs, dossiers: new Map([[eve.card.key, { state: "ok", parsed, at: nowMs }]]) });
    assert.equal(v.eve.state, "ok", `${lang}: the eve says "${v.eve.text}"`);
    assert.ok(v.eve.summary && v.eve.questions.length > 0 && v.eve.stories.length > 0, `${lang}: a summary, likely questions and stories on screen`);
    assert.ok(parsed.questions.every((q) => q.q && q.why), `${lang}: every likely question carries its why`);
    assert.ok(parsed.stories.every((s) => s.title && s.evidence), `${lang}: every story carries its literal evidence`);
    assert.ok(parsed.gaps.length > 0 && parsed.gaps.every((g) => g.topic && g.question), `${lang}: the gaps to review, each with its question`);
    assert.deepEqual(foreign(lang, text), [], `${lang}: the words of the other language in the dossier`);
    const second = lf(text).split("\n")[2];
    const day = GENERATED[lang].exec(second);
    assert.ok(day, `${lang}: the engine's second line, "${second}"`);
    assert.equal(day[1], santiagoDay(snap.generated_at), `${lang}: the day it was written, Santiago's date of the demo's now`);
    assert.match(text, EVIDENCE[lang], `${lang}: each story's evidence with its label and its quotes`);
    assert.equal(eve.card.key, "demo:4", `${lang}: the default interview is Nimbus Health`);
    for (const word of NIMBUS[lang].words) assert.ok(text.includes(word), `${lang}: Nimbus Health keeps its own words: "${word}"`);
    assert.equal(parsed.questions.length, 3, `${lang}: Nimbus Health's three questions`);
    assert.equal(parsed.questions[0].q, NIMBUS[lang].first, `${lang}: Nimbus Health's own first question`);
    read[lang] = { text, parsed };
  }
  const shape = (p) => [p.questions.length, p.stories.length, p.gaps.length];
  assert.deepEqual(shape(read.en.parsed), shape(read.es.parsed), "the same dossier in both languages");
  assert.notEqual(read.en.parsed.summary, read.es.parsed.summary, "each in its own language");
  translated(read.es.parsed, read.en.parsed, "default");
  // one page, opened in Spanish, after the l key: the drawer asks again and gets the English dossier
  const { demo, page } = loadDemoPage({ lang: "es" });
  const key = cardsOf(await snapshot(demo)).find((c) => c.state === "interview").key;
  const job = "/api/job?key=" + encodeURIComponent(key);
  assert.equal((await ask(demo, job)).data.dossier, read.es.text, "opened in Spanish");
  page.lang = "en";
  const after = (await ask(demo, job)).data.dossier;
  assert.deepEqual(foreign("en", after), [], "after the language toggle, the words of the Spanish dossier");
  assert.equal(after, read.en.text, "after the language toggle, the dossier of a page opened in English");
});

/** Every interview on the demo's board, with what /api/job answers for it. */
async function interviewsOf(demo) {
  const out = [];
  for (const card of cardsOf(await snapshot(demo)).filter((c) => c.state === "interview")) out.push({ card, answer: await ask(demo, "/api/job?key=" + encodeURIComponent(card.key)) });
  return out;
}

test("every interview has a dossier APRENDER reads, about its own application and with the gaps on file for it or none, in both languages: the ones of ?scenario=heavy and the ones marked by hand", async () => {
  const read = { heavy: {}, marked: {} };
  for (const lang of LANGS) {
    const heavy = loadDemo({ search: "?scenario=heavy", lang });
    // by hand: Lumen Pay (replied, no gap on file) and Cobalto Systems (technical test, a gap on file) move on to an interview
    const marked = loadDemo({ lang });
    for (const key of ["demo:2", "demo:3"]) {
      const r = await ask(marked, "/api/mark", { key, state: "interview", note: null });
      assert.ok(r.ok, `${lang}: ${key} can be marked as an interview: ${JSON.stringify(r.data)}`);
    }
    for (const [where, demo] of [["heavy", heavy], ["marked", marked]]) {
      const interviews = await interviewsOf(demo);
      assert.ok(interviews.length >= 3, `${lang} ${where}: ${interviews.length} interviews on the board`);
      for (const { card, answer } of interviews) {
        const at = `${lang} ${where} ${card.key} (${card.company})`;
        assert.ok(answer.ok && answer.status === 200, `${at}: /api/job answers ${answer.status}: ${String(answer.data && answer.data.message || answer.data).slice(0, 300)}`);
        const text = answer.data.dossier;
        const parsed = parseDossier(text);
        assert.ok(parsed, `${at}: the dossier has the headings APRENDER reads`);
        assert.ok(parsed.summary && parsed.questions.length > 0 && parsed.questions.every((q) => q.q && q.why), `${at}: a summary and likely questions, each with its why`);
        assert.ok(parsed.stories.length > 0 && parsed.stories.every((s) => s.title && s.evidence), `${at}: stories, each with its evidence`);
        assert.ok(lf(text).startsWith(titleLine(card)), `${at}: the engine's title line, "${lf(text).split("\n")[0]}"`);
        assert.match(lf(text).split("\n")[2], GENERATED[lang], `${at}: the engine's second line`);
        assert.match(text, EVIDENCE[lang], `${at}: each story's evidence with its label and its quotes`);
        assert.ok(text.includes(card.company), `${at}: the dossier names its own company`);
        const others = interviews.map((x) => x.card.company).filter((c) => c !== card.company && !card.company.includes(c));
        assert.deepEqual(others.filter((c) => text.includes(c)), [], `${at}: the dossier of another interview`);
        assert.deepEqual(parsed.gaps.map((g) => g.topic), answer.data.gaps.map((g) => g.topic), `${at}: the gaps on file for this posting, and only those`);
        assert.ok(parsed.gaps.every((g) => g.question && g.question !== g.topic && /\?$/.test(g.question)), `${at}: each gap with its own question: ${JSON.stringify(parsed.gaps)}`);
        assert.deepEqual(foreign(lang, text), [], `${at}: the words of the other language in the dossier`);
        (read[where][card.key] ||= {})[lang] = parsed;
      }
    }
  }
  for (const [where, byKey] of Object.entries(read)) {
    for (const [key, { es, en }] of Object.entries(byKey)) translated(es, en, `${where} ${key}`);
  }
  assert.ok(Object.keys(read.marked).length >= 3, "the marked board keeps its interviews in both languages");
});

test("APRENDER reads the engine's dossier when a section comes empty, in Spanish and in English: no summary, no stories, no gaps, a question without its why, a heading that is not its own", () => {
  const DOSSIERS = {
    es: "# Dossier — Kestrel Labs · Staff Backend Engineer\r\n\r\nGenerado el 2026-11-06 (hora de Santiago) · aviso: https://example.com/demo/7\r\n\r\n"
      + "## Qué dice el aviso de la empresa\r\n\r\nSin resumen.\r\n\r\n"
      + "## Preguntas probables\r\n\r\n1. ¿Por qué Rust?\r\n2. ¿Qué harías el primer mes?\r\n   *Por qué:* el aviso habla de un equipo nuevo.\r\n\r\n"
      + "## Historias (solo hechos de tu CV maestro)\r\n\r\nSin historias verificadas: cuéntame hechos tuyos y las completo.\r\n\r\n"
      + "## Huecos a estudiar\r\n\r\nNinguno detectado.\r\n\r\n## Notas\r\n\r\n1. Esto no es una pregunta.\r\n- **Ni esto** — un hueco\r\n",
    en: "# Dossier — Kestrel Labs · Staff Backend Engineer\r\n\r\n"
      + "## What the company's posting says\r\n\r\nNo summary.\r\n\r\n"
      + "## Likely questions\r\n\r\n1. Why Rust?\r\n2. What would you do in your first month?\r\n   *Why:* the posting talks about a new team.\r\n\r\n"
      + "## Stories (only facts from your master CV)\r\n\r\nNo verified stories: tell me facts of yours and I will fill them in.\r\n\r\n"
      + "## Gaps to study\r\n\r\nNone found.\r\n\r\n## Notes\r\n\r\n1. This is not a question.\r\n- **Nor this** — a gap\r\n",
  };
  const QUESTIONS = {
    es: [{ q: "¿Por qué Rust?", why: "" }, { q: "¿Qué harías el primer mes?", why: "el aviso habla de un equipo nuevo." }],
    en: [{ q: "Why Rust?", why: "" }, { q: "What would you do in your first month?", why: "the posting talks about a new team." }],
  };
  for (const lang of LANGS) {
    const parsed = parseDossier(DOSSIERS[lang]);
    assert.ok(parsed, `${lang}: a dossier with empty sections is still a dossier`);
    assert.equal(parsed.summary, "", `${lang}: "no summary" is no summary`);
    assert.deepEqual(parsed.questions, QUESTIONS[lang], `${lang}: a question that starts like the why is a question, and one without its why keeps an empty one`);
    assert.deepEqual([parsed.stories, parsed.gaps], [[], []], `${lang}: no stories and no gaps, and nothing taken from a heading that is not the dossier's`);
  }
  const GAPS = {
    es: "## Huecos a estudiar\n\n- **Colas y backpressure** — ¿Qué haces cuando el consumidor no da abasto? (repasa con `jobbot prep next --topic colas`)\n",
    en: "## Gaps to study\n\n- **Queues and backpressure** — What do you do when the consumer cannot keep up? (review with `jobbot prep next --topic queues`)\n",
  };
  const STORY = {
    es: "## Historias (solo hechos de tu CV maestro)\n\n### Ocho años en producción\n\nEvidencia en el CV: «Llevo ocho años operando servicios.»\n\nÚsala para el incidente.\n",
    en: "## Stories (only facts from your master CV)\n\n### Eight years in production\n\nEvidence in the CV: “For eight years I have run services.”\n\nUse it for the incident.\n",
  };
  assert.deepEqual(parseDossier(STORY.es).stories, [{ title: "Ocho años en producción", evidence: "Llevo ocho años operando servicios." }], "es: the evidence between « »");
  assert.deepEqual(parseDossier(STORY.en).stories, [{ title: "Eight years in production", evidence: "For eight years I have run services." }], "en: the evidence between “ ”");
  assert.deepEqual(parseDossier(GAPS.es).gaps, [{ topic: "Colas y backpressure", question: "¿Qué haces cuando el consumidor no da abasto?" }], "es: the gap without its review command");
  assert.deepEqual(parseDossier(GAPS.en).gaps, [{ topic: "Queues and backpressure", question: "What do you do when the consumer cannot keep up?" }], "en: the gap without its review command");
});

// ---- (3) Questions with names

// The question the demo's stuck forms asked, as the form of an English posting wrote it. The reason is the engine's line as
// crates/jobbot-app/src/apply/handoff.rs writes it, with the form's required asterisk; the panel shows the label without it.
const QUESTION = "How many years have you run Kubernetes in production?";
const ENGINE_LINE = "faltan respuestas: " + QUESTION + " *";

test("every question the form-filler could not answer has a name, in both languages and in every scenario; the same question across forms is one row", async () => {
  for (const lang of LANGS) {
    for (const search of ["", "?scenario=stale", "?scenario=heavy"]) {
      fresh(lang);
      const at = `${lang} ${search || "default"}`;
      const snap = await snapshot(loadDemo({ search, lang }));
      const m = missingOf(snap);
      assert.ok(m.forms > 0, `${at}: the demo has a form stopped on a question`);
      assert.deepEqual(m.unnamed.map((c) => c.company), [], `${at}: forms whose reason names no question`);
      assert.ok(m.groups.length > 0, `${at}: each question by its name`);
      assert.equal(m.groups.reduce((n, g) => n + g.forms, 0), m.forms, `${at}: every stopped form under its question`);
      assert.deepEqual(m.groups.map((g) => g.label), [QUESTION], `${at}: the question the form asked, by its own name`);
      const reasons = [...new Set(snap.needs_gonza.cards.filter((c) => c.reason_kind === "missing_answers").map((c) => c.reason))];
      assert.deepEqual(reasons, [ENGINE_LINE], `${at}: the reason is the engine's line, the same in both languages, asterisk included`);
      const v = aprenderView(model(snap), { nowMs: Date.parse(snap.generated_at), expanded: true });
      assert.ok(!v.missing.rows.some((r) => r.id === "miss:unnamed"), `${at}: no "${t("aprender.row.unnamed")}" row`);
    }
    const heavy = missingOf(await snapshot(loadDemo({ search: "?scenario=heavy", lang })));
    assert.ok(heavy.forms >= 2 && heavy.groups.length === 1, `${lang}: in ?scenario=heavy the ${heavy.forms} forms stopped on the same question are one row (${heavy.groups.length})`);
  }
});

// ---- (4) APRENDER says what to do today

test("APRENDER says how to answer today and promises no panel route that does not exist", () => {
  const how = {};
  for (const lang of LANGS) {
    fresh(lang);
    how[lang] = t("aprender.how");
    assert.match(how[lang], /profile\/answers\.toml/, `${lang}: it still says where the answer goes`);
    assert.match(how[lang], /\[\[entry\]\]/, `${lang}: and what to add there`);
    assert.match(how[lang], /\bu\b/, `${lang}: and which key sends the forms back`);
    assert.doesNotMatch(how[lang], /exist|ruta|route/i, `${lang}: "${how[lang]}"`);
    assert.equal(how[lang], HOW[lang], `${lang}: the sentence the encargo fixes`);
  }
  assert.notEqual(how.en, how.es, "each language in its own words");
});

// ---- (5) Product English, with the signature kept

// The calques of the Spanish that the review of 2026-11-06 found in en/ (forja, docs/research/2026-11-06-panel-obra-de-arte.md, PO-08). A closed list:
// it guards against these coming back; the rest of the pass is read in the diff.
// The encargo's table (PO-08): for every key, its English before this pass (en/ at 9e04aee) and the English the encargo fixes. The copy is
// decided in the encargo, so the test asks for it word for word; a new wording goes through the encargo, not around it.
const TABLE = {
  "sistema.result.norecord": ["no record of the result: only a discovery was seen", "no result recorded: only a discovery run was logged"],
  "sistema.task.missing.hint": ["It is registered with scripts/schedule.ps1, whenever you want the bot to work on its own.", "Register it with scripts/schedule.ps1 when you want the bot to work on its own."],
  "sistema.account.title": ["Claude, the own account", "Your Claude account"],
  "sistema.account.fix.bad": ["Run “jobbot claude-login” to sign in with the own account.", "Run “jobbot claude-login” to sign in with your own account."],
  "sistema.spend.over": ["Limit spent: it does not call Claude until the count goes down.", "Daily limit reached: no Claude calls until the count goes down."],
  "sistema.limit.score.sub": ["from there a posting becomes “prepared”", "at or above it, a posting gets prepared"],
  "sistema.limit.batch.set": ["a preference of this browser: what {key} approves in QUEUE", "a browser setting: what {key} approves in Queue"],
  "sistema.limit.batch.none": ["a preference of this browser, not set yet", "a browser setting, not set yet"],
  "sistema.errors.title": ["Errors of the night", "Overnight errors"],
  "aprender.sub.missing.one": ["One form question the bank cannot answer.", "One form question your saved answers do not cover."],
  "aprender.sub.missing.other": ["{n} form questions the bank cannot answer.", "{n} form questions your saved answers do not cover."],
  "aprender.sub.back.one": ["With your answer in the bank, u sends {n} application back to the queue; the engine prepares it on its next run.", "Once your answer is saved, u sends {n} application back to the queue; the engine prepares it on its next run."],
  "aprender.sub.back.other": ["With your answer in the bank, u sends {n} applications back to the queue; the engine prepares them on its next run.", "Once your answer is saved, u sends {n} applications back to the queue; the engine prepares them on its next run."],
  "aprender.miss.none": ["The bank is missing no answer.", "No answer is missing."],
  "aprender.bank.bank": ["answers in the bank", "saved answers"],
  "aprender.bank.note": ["The bank lives in profile/answers.toml and is not in the snapshot: how many answers it holds and how often they were used, no data.", "Saved answers live in profile/answers.toml, outside the snapshot: how many there are and how often each was used, no data."],
  "aprender.eve.head": ["The eve: {company}, {when}", "Interview prep: {company}, {when}"],
  "aprender.eve.head.none": ["The eve", "Interview prep"],
  "aprender.eve.none": ["No interview in progress. The eve of one, its dossier shows up here.", "No interview coming up. When there is one, its dossier shows up here."],
  "aprender.card.from": ["from the offer {offer}", "from the {offer} posting"],
  "aprender.err.next": ["Could not bring the next question.", "Could not load the next question."],
  "aprender.err.answer": ["Could not bring the answer.", "Could not load the answer."],
  "procesos.head.quiet.one": ["{n} no news", "{n} gone quiet"],
  "procesos.head.quiet.other": ["{n} no news", "{n} gone quiet"],
  "procesos.filter.quiet": ["No news", "Gone quiet"],
  "procesos.lane.ghosted": ["No news", "Gone quiet"],
  "procesos.tomb.ghosted": ["→ no news", "→ gone quiet"],
  "procesos.cad.lost": ["no news, day {day}", "gone quiet, day {day}"],
  "procesos.cad.quiet": ["no news, closes in {n} d", "gone quiet, closes in {n} d"],
  "procesos.turn.quiet": ["no news", "gone quiet"],
  "procesos.ruler.rule": ["Follow up at {ask} and {insist} days; no news at {close}; the median reply takes {median} days", "Follow up at {ask} and {insist} days; gone quiet at {close}; the median reply takes {median} days"],
  "procesos.next.approve": ["Approve in COLA", "Approve in Queue"],
  "procesos.drawer.next.approve": ["waits for your approval in COLA", "waits for your approval in Queue"],
  "procesos.next.lost": ["Give it up for lost", "Write it off"],
  "procesos.legend.tomb": ["a dotted gap is the place of the one that left, until you load the page again", "a dotted gap marks one that moved on, until you reload the page"],
  "procesos.closed.show": ["bring them", "show them"],
  "procesos.closed.more": ["{n} more that the panel does not bring: they are in jobbot", "{n} more the panel does not load: they are in jobbot"],
  "prologo.eyebrow": ["The night in one screen, {from} to {to}", "The night on one screen, {from} to {to}"],
  "prologo.eyebrow.plain": ["The night in one screen", "The night on one screen"],
  "prologo.go": ["Go to TODAY", "Go to Today"],
  "cola.row.cap": ["{sent} of {cap} of the cap", "{sent} of {cap} allowed in 24 h"],
  "keys.reveal": ["Show the answer of the review", "Show the review answer"],
};
// aprender.how is the encargo's point 4, in both languages
const HOW = {
  es: "Hoy se contesta a mano: añade una [[entry]] a profile/answers.toml y pulsa u.",
  en: "Answer by hand: add an [[entry]] to profile/answers.toml, then press u.",
};

const CALQUES = [
  [/the own account/i, "la cuenta propia"], [/\bof the night\b/i, "de la noche"], [/\bthe eve\b/i, "la víspera"], [/\{n\} no news/i, "{n} sin noticias"],
  [/\bbring\b/i, "traer"], [/\b(?:COLA|HOY|PROCESOS|APRENDER|SISTEMA|QUEUE|TODAY)\b/, "a stop named in Spanish or in capitals"], [/\bthe bank\b/i, "el banco"],
  [/\bfrom the offer\b/i, "de la oferta"], [/\ba preference of this browser\b/i, "preferencia de este navegador"], [/\bin one screen\b/i, "en una pantalla"],
  [/\bof the cap\b/i, "del tope"], [/\bgive it up for lost\b/i, "darla por perdida"],
];

test("the English of the panel has none of the calques the review found, one name for the quiet state across PROCESOS, and the signature stays as it is", async () => {
  const en = await dictionary("en");
  const found = [];
  for (const [file, key, text] of en) for (const [pattern, spanish] of CALQUES) if (pattern.test(text)) found.push(`${file} ${key}: "${text}" (${spanish})`);
  assert.deepEqual(found, []);
  const word = Object.fromEntries(en.map(([, key, text]) => [key, text]));
  assert.equal(Object.keys(TABLE).length, 42, "the keys of the table");
  const off = Object.entries(TABLE).filter(([key, [, neu]]) => word[key] !== neu).map(([key, [old, neu]]) => `${key}: "${word[key]}"${word[key] === old ? " (the old English)" : ""}, the encargo says "${neu}"`);
  assert.deepEqual(off, [], "keys of the table that do not say what the encargo fixes");
  // what PROCESOS calls "sin noticias" in Spanish (a lane, a filter, a count, a cadence, the ruler) has one English name, so its table never mixes two
  const es = await dictionary("es");
  const quiet = es.filter(([file, , text]) => file === "procesos.js" && /sin noticias/i.test(text)).map(([, key]) => key);
  assert.ok(quiet.length >= 5, `the Spanish of PROCESOS says "sin noticias" in ${quiet.length} places`);
  assert.deepEqual(quiet.filter((key) => !/gone quiet/i.test(word[key])).map((key) => `${key}: "${word[key]}"`), [], 'PROCESOS in English, where it does not say "gone quiet"');
  // the gold italic and the headline are Gonza's voice, not calques: they are decided by him, not by this pass
  assert.equal(word["cola.head.ask"], "Let it pass?");
  assert.equal(word["prologo.voice"], "While you slept.");
  assert.equal(word["hoy.head.other"], "You have {word}.");
  assert.equal(word["hoy.head.one"], "You have {word}.");
  assert.match(word["procesos.legend.short"], /^Each pile: /);
  assert.match(word["procesos.legend.piles"], /^Each pile: /);
  // and in Spanish the eve keeps its name: only the English of APRENDER becomes "Interview prep"
  const palabra = Object.fromEntries(es.map(([, key, text]) => [key, text]));
  assert.equal(palabra["aprender.eve.head.none"], "La víspera");
  assert.match(palabra["aprender.eve.head"], /^La víspera: /);
});

// ---- (6) The demo names itself; the real panel does not

test("the demo calls itself jobbot in every scenario and in both languages, and the real panel still says Panel", () => {
  for (const lang of LANGS) {
    for (const search of ["", "?scenario=empty", "?scenario=stale", "?scenario=heavy"]) {
      const at = `${lang} ${search || "default"}`;
      assert.equal(loadDemo({ search, lang }).brand, "jobbot", `${at}: the name the page shows instead of Panel`);
    }
    // the name lives in the demo: the dictionaries are the real page's too
    fresh(lang);
    assert.equal(t("hud.brand"), "Panel", `${lang}: the mark of the real panel`);
    assert.equal(t("app.title"), "Panel", `${lang}: the tab of the real panel`);
  }
  // after the language toggle of the page the name is the same
  const { demo, page } = loadDemoPage({ lang: "es" });
  page.lang = "en";
  assert.equal(demo.brand, "jobbot", "switched to English");
});

test("the tab of the real panel says Panel when it gets no brand, and the demo's tab says its brand, in both languages", async () => {
  const saved = globalThis.document;
  globalThis.document = { title: "" };   // one property: the icon fails inside its own try, as a courtesy icon does
  try {
    const { createTitle } = await import("../src/ui/title.js");
    for (const lang of LANGS) {
      fresh(lang);
      createTitle().update(null);
      assert.equal(globalThis.document.title, "Panel", `${lang}: no brand`);
      createTitle(undefined).update(null);
      assert.equal(globalThis.document.title, "Panel", `${lang}: the brand main.js hands over when there is no __DEMO__`);
      createTitle("jobbot").update(null);
      assert.equal(globalThis.document.title, "jobbot", `${lang}: the demo's brand`);
    }
  } finally { globalThis.document = saved; }
});

// ---- (7) The brand's wiring, in the source

const SRC = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "src");
const source = (file) => fs.readFileSync(path.join(SRC, file), "utf8").replace(/\r\n?/g, "\n");

test("the demo's brand reaches the mark and the tab: main.js reads __DEMO__ once and hands its brand to the shell, the shell to the HUD and the tab, and the HUD says it", () => {
  const main = source("main.js");
  const reads = main.match(/window\.__DEMO__/g) || [];
  assert.equal(reads.length, 1, `main.js reads window.__DEMO__ ${reads.length} times; once, into one constant`);
  const one = /const\s+(\w+)\s*=\s*window\.__DEMO__\s*\|\|\s*null\s*;/.exec(main);
  assert.ok(one, "main.js keeps the demo in one constant: const <name> = window.__DEMO__ || null;");
  const name = one[1];
  const call = /createShell\(\{([^}]*)\}\)/.exec(main);
  assert.ok(call, "main.js calls createShell({ ... })");
  assert.match(call[1], new RegExp(`\\bbrand:\\s*(${name}\\s*&&\\s*${name}\\.brand|${name}\\?\\.brand)\\b`), `main.js hands createShell the brand of ${name}: ${call[1].trim()}`);

  const shell = source("ui/shell.js");
  const sig = /export function createShell\(\{([\s\S]*?)\}\s*(?:=\s*\{\}\s*)?\)\s*\{/.exec(shell);
  assert.ok(sig && /\bbrand\b/.test(sig[1]), "createShell takes a brand");
  assert.match(shell, /\bcreateTitle\(\s*brand\s*\)/, "the shell hands its brand to the tab: createTitle(brand)");
  const hudCall = /createHud\(\{([^}]*)\}\)/.exec(shell);
  assert.ok(hudCall && /(^|[\s,])brand(\s*:\s*brand)?\s*(,|$)/.test(hudCall[1]), `the shell hands its brand to the HUD: createHud({ ${hudCall ? hudCall[1].trim() : ""} })`);

  const hud = source("ui/hud.js");
  const params = /export function createHud\(\{([\s\S]*?)\}\s*(?:=\s*\{\}\s*)?\)\s*\{/.exec(hud);
  assert.ok(params, "hud.js exports createHud({ ... })");
  assert.match(params[1], /(^|[\s,])brand\s*(=|,|$)/, `createHud takes brand under its own name, not renamed: { ${params[1].trim()} }`);
  assert.doesNotMatch(params[1], /\bbrand\s*:/, `createHud takes brand under its own name, not renamed: { ${params[1].trim()} }`);
  const local = "brand";
  const fallbacks = hud.match(/t\(\s*"hud\.brand"\s*\)/g) || [];
  assert.equal(fallbacks.length, 1, `hud.js says t("hud.brand") ${fallbacks.length} times; once, as the fallback of the brand`);
  assert.match(hud, new RegExp(`textContent\\s*=\\s*${local}\\s*(\\|\\||\\?\\?)\\s*t\\(\\s*"hud\\.brand"\\s*\\)`), `the HUD's mark (its own node, not named brand) says brand when it is given, and t("hud.brand") only when it is absent`);
});
