// What COLA asks of the demo's fictional server: the score of a card told by the server's own factor ids with their weights (so the bars
// read "points / what that factor can give"), and the lote, POST /api/approve { keys }, answered like the real one (one result per key,
// 409 when any fails, the others stay done). Nothing here needs a browser: the demo runs in a vm (test/helpers/demo-host.mjs).

import test from "node:test";
import assert from "node:assert/strict";
import { loadDemo } from "./helpers/demo-host.mjs";
import { FACTOR_WEIGHTS } from "../src/moments/cola/view.js";

const WEIGHT = Object.fromEntries(FACTOR_WEIGHTS);
const IDS = FACTOR_WEIGHTS.map(([id]) => id);
const awaitingOf = async (demo) => Array.from((await demo.api("/api/snapshot")).data.awaiting_approval.cards, (c) => c.key);   // from the vm: plain arrays

test("the demo tells the score by the server's ids and weights, in both languages, adding up to the score of the card", async () => {
  for (const lang of ["es", "en"]) {
    const demo = loadDemo({ search: "?scenario=heavy", lang });
    const snap = (await demo.api("/api/snapshot")).data;
    const cards = snap.board.flatMap((col) => col.cards).filter((c) => Number.isFinite(c.score)).slice(0, 40);
    assert.ok(cards.length >= 20, "enough cards to mean something");
    for (const card of cards) {
      const detail = (await demo.api("/api/job?key=" + encodeURIComponent(card.key))).data.score_detail;
      assert.deepEqual(Object.keys(detail), IDS, card.key + " names the six factors of the engine, in its order");
      let sum = 0;
      for (const [id, points] of Object.entries(detail)) {
        assert.ok(Number.isInteger(points) && points >= 0 && points <= WEIGHT[id], `${card.key} ${id}: ${points} of ${WEIGHT[id]}`);
        sum += points;
      }
      assert.equal(sum, card.score, card.key + " adds up to its score");
    }
  }
});

test("the lote: one request for many keys, one result per key in order, every one approved", async () => {
  const demo = loadDemo({ search: "?scenario=heavy" });
  const keys = await awaitingOf(demo);
  assert.ok(keys.length >= 3, "the heavy demo has captures waiting");
  const res = await demo.api("/api/approve", { keys });
  assert.equal(res.ok, true);
  assert.equal(res.status, 200);
  assert.equal(res.data.ok, true);
  assert.deepEqual(res.data.results.map((r) => [r.key, r.ok]), keys.map((k) => [k, true]));
  assert.match(res.data.message, new RegExp(`^${keys.length} de ${keys.length}`));
  assert.ok(res.data.results.every((r) => typeof r.message === "string" && r.message.length > 0));
  assert.deepEqual(await awaitingOf(demo), [], "none of them waits any more");
  const board = (await demo.api("/api/snapshot")).data.board.flatMap((col) => col.cards);
  for (const k of keys) assert.equal(board.find((c) => c.key === k).approved, true, k + " is approved on the board");
});

test("the lote with a key that fails answers 409 and keeps what went through, like the server; the message follows the language", async () => {
  const demo = loadDemo({ search: "?scenario=heavy", lang: "en" });
  const [first, second] = await awaitingOf(demo);
  const res = await demo.api("/api/approve", { keys: [first, "demo:nope", second, first] });
  assert.equal(res.ok, false);
  assert.equal(res.status, 409);
  assert.deepEqual(res.data.results.map((r) => [r.key, r.ok]), [[first, true], ["demo:nope", false], [second, true], [first, false]]);
  assert.equal(res.data.message, "2 of 4 done");
  assert.match(res.data.results[1].message, /demo:nope/);
  assert.ok(!(await awaitingOf(demo)).includes(first) && !(await awaitingOf(demo)).includes(second), "the two that went through stay approved");
});

test("a bad lote is refused whole (400) and does nothing; one key still works as it did", async () => {
  const demo = loadDemo({ search: "?scenario=heavy" });
  const before = await awaitingOf(demo);
  for (const body of [{ keys: [] }, { keys: before.slice(0, 1), key: before[0] }, { keys: Array.from({ length: 201 }, (_, i) => "demo:" + i) }]) {
    const res = await demo.api("/api/approve", body);
    assert.equal(res.ok, false);
    assert.equal(res.status, 400, JSON.stringify(body).slice(0, 40));
  }
  assert.deepEqual(await awaitingOf(demo), before, "a refused request approved nothing");
  const one = await demo.api("/api/approve", { key: before[0] });
  assert.equal(one.ok, true);
  assert.equal(one.data.results, undefined, "one key is answered as before, without a list");
  assert.deepEqual(await awaitingOf(demo), before.slice(1));
});
