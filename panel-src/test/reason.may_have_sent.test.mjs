// The reason kind `may_have_sent`: an application that needs Gonza because its form MAY ALREADY HAVE GONE OUT (the page never confirmed
// the click, or he finished an assisted form and closed it, or a run died after the click). The panel has to say so in its own
// sentence, because the one thing he must not do is apply again by hand before checking.
//
// The page only knows a CLOSED list of kinds, and a card whose kind is not in it leaves the queue without a word (model.js, "legacy quirk"),
// so the first test reads the server's own list (crates/jobbot-panel/src/snapshot.rs, `reason_kind`) and holds the page to it. Fictional data only.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { setLang, t, setTimeZone } from "../src/i18n/index.js";
import esCommon from "../src/i18n/es/common.js";
import enCommon from "../src/i18n/en/common.js";
import esHoy from "../src/i18n/es/hoy.js";
import enHoy from "../src/i18n/en/hoy.js";
import { model, candidates, turnCount, layerOf, REASON_KINDS, KIND_IDS } from "../src/core/model.js";
import { reasonShort, reasonGroupTitle, asSentence } from "../src/core/format.js";
import { answerOf } from "../src/ui/detail/now.js";
import { hoyView } from "../src/moments/hoy/view.js";
import { ASKS_AFTER } from "../src/moments/hoy/prompt.js";
import { loadDemo, snapshotOf } from "./helpers/demo-host.mjs";

const SENTENCE_ES = "Puede que ya se haya enviado: revisa antes de volver a postular";
const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/snapshot.${name}.json`, import.meta.url), "utf8"));
const fresh = (lang = "es") => { setLang(lang); setTimeZone("America/Santiago"); };

/** An empty night with these cards waiting for him: { n, kind } per card, in the order given. */
function waiting(...specs) {
  const s = fixture("empty");
  const cards = specs.map(({ n, kind, ...extra }) => ({
    key: `fx:${n}`, company: `Test Co ${n}`, title: "Backend Engineer", score: 70 + n, state: "needs_gonza", source: "greenhouse",
    url: `https://example.com/jobs/${n}`, reason: kind ? `${kind}: text` : "falta email", reason_kind: kind, reason_short: "texto del servidor", note: null,
    approved: false, has_screenshot: false, has_cv: true, has_dossier: false, has_handoff: false, updated_at: s.generated_at,
    // the server leads with "it went out" when it may have (snapshot.rs), with the retry second
    actions: kind === "may_have_sent" ? ["confirm", "resolve"] : ["resolve", "confirm"],
    marks: [], posted_at: s.generated_at, first_seen: s.generated_at, salary_min: null, salary_max: null, days_in_state: 0, snoozed_until: null,
    waiting_since: s.generated_at, age_days: 1, prune_in_days: 10, ...extra,
  }));
  s.needs_gonza = { total: cards.length, cards };
  const column = s.board.find((c) => c.state === "needs_gonza");
  column.cards = cards;
  column.total = cards.length;
  return s;
}

// ---- the server's list and the page's list are the same list

/** [kind, sentence] for every kind `reason_kind` of snapshot.rs can answer. */
function serverKinds() {
  const source = readFileSync(new URL("../../crates/jobbot-panel/src/snapshot.rs", import.meta.url), "utf8");
  const start = source.indexOf("fn reason_kind(");
  assert.ok(start > 0, "snapshot.rs has reason_kind");
  const end = source.slice(start).search(/\r?\n}\r?\n/); // the closing brace of the function (the file may have CRLF line ends)
  const body = source.slice(start, start + end);
  return [...body.matchAll(/\(\s*"([a-z_]+)",\s*"([^"]+)"\s*,?\s*\)/g)].map((m) => [m[1], m[2]]);
}

test("every kind the server can write is a kind of the page (or its cards would silently leave the queue), with the server's own sentence", () => {
  const kinds = serverKinds();
  assert.deepEqual(kinds.map(([k]) => k).sort(), ["blocked", "error", "handoff", "manual_apply", "may_have_sent", "missing_answers", "other"], "the kinds snapshot.rs knows");
  for (const [kind, sentence] of kinds) {
    assert.ok(REASON_KINDS.includes(kind), `${kind} is not in REASON_KINDS`);
    assert.ok(KIND_IDS.includes(kind), `${kind} is not in KIND_IDS`);
    assert.equal(esCommon["reason.short." + kind], sentence, `${kind}: the dictionary says something else than the server`);
  }
});

test("may_have_sent has its words in both languages, and the English is not the Spanish", () => {
  for (const key of ["reason.group.may_have_sent", "reason.short.may_have_sent"]) {
    assert.ok(esCommon[key] && enCommon[key], key);
    assert.notEqual(esCommon[key], enCommon[key], key);
  }
  assert.ok(esHoy["hoy.kind.may_have_sent"] && enHoy["hoy.kind.may_have_sent"]);
  assert.notEqual(esHoy["hoy.kind.may_have_sent"], enHoy["hoy.kind.may_have_sent"]);
  // no word of the engine reaches the sentence
  for (const text of [esCommon["reason.short.may_have_sent"], enCommon["reason.short.may_have_sent"], esCommon["reason.group.may_have_sent"], enCommon["reason.group.may_have_sent"]]) {
    assert.doesNotMatch(text, /unconfirmed|assist_|interrupted|submit/i, text);
  }
  // every kind of the closed list has all three names
  for (const [common, hoy] of [[esCommon, esHoy], [enCommon, enHoy]]) {
    for (const kind of REASON_KINDS) {
      assert.ok(common["reason.group." + kind], `reason.group.${kind}`);
      if (kind !== "approve") assert.ok(common["reason.short." + kind], `reason.short.${kind}`);
      assert.ok(hoy["hoy.kind." + kind], `hoy.kind.${kind}`);
    }
  }
});

test("reasonShort and reasonGroupTitle give the dictionary's words for may_have_sent, whatever the server wrote, in both languages", () => {
  fresh("es");
  const card = { reason_kind: "may_have_sent", reason_short: "texto del servidor" };
  assert.equal(reasonShort(card), SENTENCE_ES);
  assert.equal(reasonGroupTitle("may_have_sent"), "Puede que ya se haya enviado");
  fresh("en");
  assert.equal(reasonShort(card), "It may already have gone out: check before applying again");
  assert.equal(reasonGroupTitle("may_have_sent"), "It may already have gone out");
  fresh("es");
});

// ---- where it sits in the queue

test("REASON_KINDS: may_have_sent comes right after blocked and before the generic kinds", () => {
  const at = (k) => REASON_KINDS.indexOf(k);
  assert.equal(at("may_have_sent"), at("blocked") + 1, "as visible as blocked");
  assert.ok(at("may_have_sent") < at("error") && at("may_have_sent") < at("manual_apply") && at("may_have_sent") < at("other"));
  assert.equal(at("other"), REASON_KINDS.length - 1, "other stays last");
});

test("candidates: a form that may have gone out is in the queue between the blocked ones and the rest, with its own title and sentence", () => {
  fresh("es");
  const snap = waiting(
    { n: 1, kind: null }, // an old row with no kind: "other"
    { n: 2, kind: "error" },
    { n: 3, kind: "may_have_sent" },
    { n: 4, kind: "blocked" },
    { n: 5, kind: "manual_apply" },
    { n: 6, kind: "may_have_sent" },
    { n: 7, kind: "missing_answers" },
    { n: 8, kind: "handoff" },
  );
  const m = model(snap);
  const list = candidates(m);
  assert.deepEqual(list.map((c) => c.id), ["card:fx:8", "card:fx:7", "card:fx:4", "card:fx:3", "card:fx:6", "card:fx:2", "card:fx:5", "card:fx:1"]);
  const may = list.filter((c) => c.kindId === "may_have_sent");
  assert.equal(may.length, 2);
  for (const c of may) {
    assert.equal(c.kind, "Puede que ya se haya enviado");
    assert.equal(c.why, SENTENCE_ES);
    assert.deepEqual(c.actions, ["confirm", "resolve"], "the same two answers as any waiting card, 'it went out' first");
  }
  assert.equal(turnCount(m), 8, "every waiting form counts, none is lost");
  fresh("en");
  assert.equal(candidates(m).find((c) => c.id === "card:fx:3").why, "It may already have gone out: check before applying again");
  fresh("es");
});

test("a kind nobody knows is still dropped from the queue (the old quirk), so the closed list above is what keeps may_have_sent in it", () => {
  fresh("es");
  const ids = candidates(model(waiting({ n: 1, kind: "may_have_sent" }, { n: 2, kind: "something_new" }))).map((c) => c.id);
  assert.deepEqual(ids, ["card:fx:1"]);
});

// ---- the drawer, the layers and Hoy

test("layerOf: a form that may have gone out stopped at the gate, in his hands, and says the sentence", () => {
  fresh("es");
  const l = layerOf({ state: "needs_gonza", approved: false, actions: ["confirm", "resolve"], reason_kind: "may_have_sent", reason_short: "x" });
  assert.deepEqual([l.at, l.who, l.text], [4, "you", SENTENCE_ES]);
});

test("the drawer's answer for a form that may have gone out is the sentence, with the first button's hint under it", () => {
  fresh("es");
  const card = waiting({ n: 1, kind: "may_have_sent" }).needs_gonza.cards[0];
  const a = answerOf(card, model(waiting({ n: 1, kind: "may_have_sent" })));
  assert.equal(a.tone, "you");
  assert.equal(a.sentence, asSentence(SENTENCE_ES));
  assert.equal(a.sub, t("act.confirm.hint"), "the first answer offered is 'it went out', never the retry");
  // the long reason stays behind its fold, word for word (ui/detail.js draws `card.reason` in a <pre>)
  assert.equal(card.reason, "may_have_sent: text");
});

test("HOY: the row is looked at (never opened on its posting), its kind tag says to check, and confirm is a two-step", () => {
  for (const [lang, tag] of [["es", "Revisa si salió"], ["en", "Check if sent"]]) {
    fresh(lang);
    const snap = waiting({ n: 1, kind: "may_have_sent" });
    const v = hoyView(model(snap), { nowMs: Date.parse(snap.generated_at) });
    const row = v.next;
    assert.equal(row.kindId, "may_have_sent");
    assert.equal(row.kind, tag);
    assert.equal(row.actions[0].id, "open", "the first thing to do is to check, not to apply");
    assert.ok(row.actions.some((a) => a.id === "confirm" && a.permanent), "marking it as sent cannot be undone");
    assert.ok(row.actions.some((a) => a.id === "resolve"));
  }
  fresh("es");
  assert.ok(!ASKS_AFTER.has("may_have_sent"), "no '¿lo hiciste?' after looking at the posting: he opens it to check, not to apply");
});

// ---- the demo shows it

test("the demo has one form that may have gone out (a made-up company), with both answers, in both languages", async () => {
  for (const lang of ["es", "en"]) {
    const snap = await snapshotOf({ lang });
    const may = snap.needs_gonza.cards.filter((c) => c.reason_kind === "may_have_sent");
    assert.equal(may.length, 1, lang);
    const [card] = may;
    assert.deepEqual([...card.actions], ["confirm", "resolve"]); // spread: the demo runs in its own realm
    assert.equal(card.company, "Brújula Freight");
    assert.equal(card.state, "needs_gonza");
    assert.ok(card.reason && card.reason_short, lang);
    assert.doesNotMatch(card.reason + card.reason_short, /unconfirmed|assist_|interrupted/i, "plain words, no engine words");
    assert.ok(snap.board.find((c) => c.state === "needs_gonza").cards.some((c) => c.key === card.key), "also in the board column");
    fresh(lang);
    const ids = candidates(model(snap)).filter((c) => c.kindId === "may_have_sent").map((c) => c.key);
    assert.deepEqual(ids, [card.key], `${lang}: it is in the queue`);
  }
  fresh("es");
});

test("the demo's heavy scenario keeps its eight stuck forms and shows the new kind among them", async () => {
  const snap = await snapshotOf({ search: "?scenario=heavy" });
  assert.equal(snap.needs_gonza.total, 8);
  assert.equal(snap.needs_gonza.cards.filter((c) => c.reason_kind === "may_have_sent").length, 1);
});

test("in the demo, confirming it marks it as sent and resolving it sends it back to the queue", async () => {
  for (const [path, state] of [["/api/confirm", "submitted"], ["/api/resolve", "prepared"]]) {
    const demo = loadDemo({});
    const before = (await demo.api("/api/snapshot")).data.needs_gonza.cards.find((c) => c.reason_kind === "may_have_sent");
    const done = await demo.api(path, { key: before.key });
    assert.equal(done.ok, true, path);
    const after = (await demo.api("/api/snapshot")).data;
    assert.ok(!after.needs_gonza.cards.some((c) => c.key === before.key), `${path}: it left the stuck forms`);
    assert.ok(after.board.find((c) => c.state === state).cards.some((c) => c.key === before.key), `${path}: it is now ${state}`);
  }
});
