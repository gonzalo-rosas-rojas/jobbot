// The fictional postings behind the heavy fixture (test/fixtures/fake.mjs): the same lines every run, and nothing that looks real.
import test from "node:test";
import assert from "node:assert/strict";
import { fakePostings, seeded } from "./fixtures/fake.mjs";

test("fake data: 300 lines, the same every run, nothing that looks real", () => {
  const a = fakePostings();
  assert.equal(a.length, 300);
  assert.deepEqual(a, fakePostings());
  assert.notDeepEqual(a, fakePostings(300, 1));
  assert.ok(a.every((line) => /^[A-Za-z() -]+$/.test(line) && !/https?:|@/.test(line)));
  const r = seeded(1);
  const first = [r(), r(), r()];
  const again = seeded(1);
  assert.deepEqual(first, [again(), again(), again()]);
  assert.ok(first.every((v) => v >= 0 && v < 1));
});
