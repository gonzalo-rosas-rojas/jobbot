// findCard (moments/shared/cards.js, the one PROCESOS uses) over snapshots with a column or a list that has no `cards`: it must not throw, and the first appearance wins.
import test from "node:test";
import assert from "node:assert/strict";
import { findCard } from "../src/moments/shared/cards.js";

test("a column or a list without cards is skipped, and the first appearance of a key wins", () => {
  const snap = {
    board: [{ state: "scored" }, { state: "submitted", cards: [{ key: "a", n: 1 }] }],
    awaiting_approval: {},
    needs_gonza: { cards: [{ key: "a", n: 2 }, { key: "b", n: 3 }] },
  };
  assert.equal(findCard({ snap }, "a").n, 1);
  assert.equal(findCard({ snap }, "b").n, 3);
  assert.equal(findCard({ snap }, "zzz"), null);
  assert.equal(findCard(null, "a"), null);
});
