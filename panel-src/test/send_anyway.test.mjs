// "Send anyway" is the one act that clears the region check of a card parked outside the region. It is its own action (its own server route),
// and neither a plain resolve nor a batch ever sends it. No browser: the requests of the verbs and the words are checked.
import test from "node:test";
import assert from "node:assert/strict";
import esCommon from "../src/i18n/es/common.js";
import enCommon from "../src/i18n/en/common.js";
import { PERMANENT_ACTIONS } from "../src/core/model.js";
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

test("the lote only approves: no bulk request carries send anyway", () => {
  assert.equal(batchRequest(["fx:1", "fx:2"]).path, "/api/approve");
});

test("send anyway is permanent (two presses) and has its words in both languages", () => {
  assert.ok(PERMANENT_ACTIONS.includes("send_anyway"));
  assert.equal(esCommon["act.send_anyway"], "Enviar igual");
  assert.equal(esCommon["act.send_anyway.hint"], "La revisé y quiero postular aunque quede fuera de la región.");
  assert.equal(enCommon["act.send_anyway"], "Send anyway");
  assert.equal(enCommon["act.send_anyway.hint"], "I reviewed it and want to apply although it is outside the region.");
});
