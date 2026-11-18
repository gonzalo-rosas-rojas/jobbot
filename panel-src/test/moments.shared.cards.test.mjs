// moments/shared/cards.js: the places a card is looked up in, written once, and the two rules for a key found in more than one of them.
import test from "node:test";
import assert from "node:assert/strict";
import { cardLists, indexCards, findCard } from "../src/moments/shared/cards.js";

const snap = {
  board: [{ state: "prepared", cards: [{ key: "a", from: "board" }, { key: "dup", from: "board" }] }, { state: "found", cards: [{ key: "b", from: "board" }] }],
  awaiting_approval: { cards: [{ key: "dup", from: "awaiting" }, { key: "c", from: "awaiting" }] },
  needs_gonza: { cards: [{ key: "d", from: "needs" }] },
};

test("the places of a card are the board's columns, then the proposals, then the ones that need him", () => {
  assert.deepEqual(cardLists(snap).map((c) => c.key), ["a", "dup", "b", "dup", "c", "d"]);
});

test("a snapshot with none of the places has no cards, and no place is required", () => {
  assert.deepEqual(cardLists({}), []);
  assert.equal(indexCards({}).size, 0);
  assert.equal(findCard({ snap: {} }, "a"), null);
});

test("the index keeps the last card of a key", () => {
  const index = indexCards(snap);
  assert.equal(index.size, 5);
  assert.equal(index.get("dup").from, "awaiting");
  assert.equal(index.get("a").from, "board");
  assert.equal(index.get("d").from, "needs");
});

test("the single lookup keeps the first card of a key, finds the others, and answers null for no model or no card", () => {
  const model = { snap };
  assert.equal(findCard(model, "dup").from, "board");
  assert.equal(findCard(model, "c").from, "awaiting");
  assert.equal(findCard(model, "nope"), null);
  assert.equal(findCard(null, "a"), null);
});
