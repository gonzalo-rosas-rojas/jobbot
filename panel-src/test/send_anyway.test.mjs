// "Send anyway" is the one act that clears the region check of a card parked outside the region. It is its own action (its own server route),
// and neither a plain resolve nor a batch ever sends it. No browser: the requests of the verbs and the words are checked.
import test from "node:test";
import assert from "node:assert/strict";
import esCommon from "../src/i18n/es/common.js";
import enCommon from "../src/i18n/en/common.js";
import { readFileSync } from "node:fs";
import { setLang, setTimeZone } from "../src/i18n/index.js";
import { model, PERMANENT_ACTIONS } from "../src/core/model.js";
import { hoyView, ACTION_LABEL } from "../src/moments/hoy/view.js";
import { colaView } from "../src/moments/cola/view.js";
import { requestOf as hoyRequestOf } from "../src/moments/hoy/actions.js";
import { requestOf as colaRequestOf, batchRequest } from "../src/moments/cola/actions.js";

const row = { data: { card: "fx:1" } };

test("send anyway has its own request in HOY and in COLA, one key, and resolve keeps its own", () => {
  for (const requestOf of [hoyRequestOf, colaRequestOf]) {
    assert.deepEqual(requestOf("send_anyway", row), { path: "/api/send_anyway", body: { key: "fx:1" } });
    assert.deepEqual(requestOf("resolve", row), { path: "/api/resolve", body: { key: "fx:1" } });
    assert.equal(requestOf("send_anyway", { data: {} }), null, "no card, no request");
  }
});

test("batchRequest always posts to /api/approve", () => {
  assert.equal(batchRequest(["fx:1", "fx:2"]).path, "/api/approve");
});

// ---- drawing a card that offers it (an action with no key is drawn with no key)

const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/snapshot.${name}.json`, import.meta.url), "utf8"));
const fresh = () => { setLang("es"); setTimeZone("America/Santiago"); };

/** An empty night with one card waiting for him, as the server sends a region card. */
function withWaitingCard(kind, actions) {
  const s = fixture("empty");
  const card = {
    key: "fx:1", company: "Test Co", title: "Backend Engineer", score: 80, state: "needs_gonza", source: "greenhouse",
    url: "https://example.com/jobs/1", reason: kind + ": text", reason_kind: kind, reason_short: "texto del servidor", note: null,
    approved: false, has_screenshot: false, has_cv: true, has_dossier: false, has_handoff: false, updated_at: s.generated_at,
    actions, marks: [], posted_at: s.generated_at, first_seen: s.generated_at, salary_min: null, salary_max: null, days_in_state: 0,
    snoozed_until: null, waiting_since: s.generated_at, age_days: 1, prune_in_days: 10,
  };
  s.needs_gonza = { total: 1, cards: [card] };
  const column = s.board.find((c) => c.state === "needs_gonza");
  column.cards = [card];
  column.total = 1;
  return { snap: s, card };
}

const rowsOfHoy = (snap) => {
  const v = hoyView(model(snap), { nowMs: Date.parse(snap.generated_at) });
  return [v.next, ...v.rows].filter(Boolean);
};

test("HOY draws a region card with its send-anyway button and no key for it", () => {
  fresh();
  const { snap } = withWaitingCard("out_of_region", ["send_anyway", "confirm"]);
  const rows = rowsOfHoy(snap);
  const row = rows.find((r) => r.data && r.data.card === "fx:1");
  assert.ok(row, "the region card is a row of HOY");
  const send = row.actions.find((a) => a.id === "send_anyway");
  assert.ok(send, "it offers send anyway");
  assert.equal(send.key, "", "no key: the act is deliberate");
  assert.equal(send.permanent, true);
  assert.equal(send.label, "Enviar igual");
});

test("COLA draws a blocked row that offers send anyway without throwing, with no key for it", () => {
  fresh();
  const { snap } = withWaitingCard("missing_answers", ["send_anyway", "resolve"]);
  const v = colaView(model(snap), { nowMs: Date.parse(snap.generated_at) });
  const group = v.queue.groups.find((g) => g.id === "blocked");
  const send = group.rows[0].actions.find((a) => a.id === "send_anyway");
  assert.ok(send);
  assert.equal(send.key, "");
});

test("every action HOY can label is drawn without throwing and has a request", () => {
  fresh();
  // done is a task's, open and link never make a request: the page handles them itself
  for (const id of Object.keys(ACTION_LABEL).filter((id) => !["done", "open", "link"].includes(id))) {
    const kind = id === "send_anyway" ? "out_of_region" : "missing_answers";
    const { snap } = withWaitingCard(kind, [id, "confirm"]);
    const rows = rowsOfHoy(snap);
    const row = rows.find((r) => r.data && r.data.card === "fx:1");
    assert.ok(row, `${id}: the card is a row`);
    if (row.actions.some((a) => a.id === id)) {
      assert.notEqual(hoyRequestOf(id, row, { until: "1d", state: "rejected" }), null, `${id} has a request in HOY`);
    }
  }
});

test("send anyway is permanent (two presses) and has its words in both languages", () => {
  assert.ok(PERMANENT_ACTIONS.includes("send_anyway"));
  assert.equal(esCommon["act.send_anyway"], "Enviar igual");
  assert.equal(esCommon["act.send_anyway.hint"], "La revisé y quiero postular aunque quede fuera de la región.");
  assert.equal(enCommon["act.send_anyway"], "Send anyway");
  assert.equal(enCommon["act.send_anyway.hint"], "I reviewed it and want to apply although it is outside the region.");
});
