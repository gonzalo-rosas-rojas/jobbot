// R8 §1 gave each foundation library a weight; a version bump or a wider import that blows it should be a decision, not a surprise.
import test from "node:test";
import assert from "node:assert/strict";
import { weigh, BUDGETS, TOTAL_BUDGET } from "../tools/weigh.mjs";

test("each foundation library stays inside its gzip budget, and all together inside the total", async (t) => {
  const rows = await weigh();
  assert.deepEqual(rows.map((r) => r.name), Object.keys(BUDGETS));
  for (const r of rows) {
    t.diagnostic(`${r.name}: ${(r.raw / 1024).toFixed(1)} KB raw, ${(r.gzip / 1024).toFixed(1)} KB gzip`);
    assert.ok(r.gzip <= BUDGETS[r.name], `${r.name} is ${r.gzip} bytes gzipped, over ${BUDGETS[r.name]}`);
  }
  const total = rows.reduce((n, r) => n + r.gzip, 0);
  t.diagnostic(`together: ${(total / 1024).toFixed(1)} KB gzip`);
  assert.ok(total <= TOTAL_BUDGET, `${total} bytes together, over ${TOTAL_BUDGET}`);
});
