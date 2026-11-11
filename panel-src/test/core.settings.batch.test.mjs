// The lote's threshold as a preference of the page: `batchMin`, a whole score from 1 to 100, 85 until the person says otherwise.
// COLA proposes the lote from it and SISTEMA shows it, so both read the one constant and the one stored value.

import test from "node:test";
import assert from "node:assert/strict";
import { createSettings, DEFAULTS, DEFAULT_BATCH_MIN, BATCH_MIN_RANGE } from "../src/core/settings.js";
import { DEFAULT_BATCH_MIN as COLA_DEFAULT } from "../src/moments/cola/view.js";

const memory = () => {
  const box = new Map();
  return { box, store: { get: (k) => (box.has(k) ? box.get(k) : null), set: (k, v) => box.set(k, v) } };
};

test("batchMin: 85 by default, one constant shared by the settings and COLA", () => {
  assert.equal(DEFAULT_BATCH_MIN, 85);
  assert.equal(DEFAULTS.batchMin, DEFAULT_BATCH_MIN);
  assert.equal(COLA_DEFAULT, DEFAULT_BATCH_MIN, "COLA reads the settings' constant, it does not keep its own");
  assert.deepEqual(BATCH_MIN_RANGE, [1, 100]);
  const { store } = memory();
  const s = createSettings({ store });
  assert.equal(s.get("batchMin"), 85);
  assert.equal(s.get().batchMin, 85);
});

test("batchMin: saved, announced, and always a whole number inside the range", () => {
  const { box, store } = memory();
  const s = createSettings({ store });
  const heard = [];
  s.onChange((c) => heard.push([c.key, c.value, c.previous]));
  s.set("batchMin", 90);
  assert.equal(s.get("batchMin"), 90);
  assert.deepEqual(JSON.parse(box.get("settings")), { batchMin: 90 });
  s.set("batchMin", 90); // no change, no event
  s.set("batchMin", 89.6); // a whole score
  assert.equal(s.get("batchMin"), 90);
  s.set("batchMin", 250);
  assert.equal(s.get("batchMin"), 100);
  s.set("batchMin", -4);
  assert.equal(s.get("batchMin"), 1);
  s.set("batchMin", "70"); // a field answers in text
  assert.equal(s.get("batchMin"), 70);
  for (const junk of ["", "abc", null, undefined, NaN, Infinity, {}, [], true]) {
    s.set("batchMin", 60);
    s.set("batchMin", junk);
    assert.equal(s.get("batchMin"), DEFAULT_BATCH_MIN, "not a score: " + String(junk));
  }
  assert.deepEqual(heard.slice(0, 4), [["batchMin", 90, 85], ["batchMin", 100, 90], ["batchMin", 1, 100], ["batchMin", 70, 1]]);
  // the next visit reads what was saved; a broken saved value is the default, not a crash
  s.set("batchMin", 92);
  assert.equal(createSettings({ store }).get("batchMin"), 92);
  box.set("settings", JSON.stringify({ batchMin: "ninety" }));
  assert.equal(createSettings({ store }).get("batchMin"), 85);
  box.set("settings", JSON.stringify({ batchMin: 5000 }));
  assert.equal(createSettings({ store }).get("batchMin"), 100);
});

test("batchMin: the other settings keep their way, and the address cannot override it", () => {
  const { store } = memory();
  const s = createSettings({ store, search: "?batchMin=10&motion=calm" });
  assert.equal(s.get("batchMin"), 85, "an address does not move the lote's threshold");
  assert.equal(s.get("motion"), "calm");
  assert.deepEqual(Object.keys(s.get()).sort(), ["batchMin", "motion", "sound"]);
});
