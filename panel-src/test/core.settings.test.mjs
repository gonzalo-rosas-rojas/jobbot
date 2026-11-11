// The settings of the page: defaults, saving, bad values, change events and the overrides the address may ask for. (This test lived in the tower's
// shell test, which is archived with the tower; the settings are the page's too, so it stays.)
import test from "node:test";
import assert from "node:assert/strict";
import { createSettings, overridesOf } from "../src/core/settings.js";

const memory = () => {
  const box = new Map();
  return { box, store: { get: (k) => (box.has(k) ? box.get(k) : null), set: (k, v) => box.set(k, v) } };
};

test("settings: defaults, saving, bad values, change events and address overrides", () => {
  const { box, store } = memory();
  const s = createSettings({ store });
  assert.deepEqual(s.get(), { sound: false, motion: "auto", batchMin: 85 });
  const heard = [];
  s.onChange((c) => heard.push([c.key, c.value, c.previous]));
  s.set("motion", "full");
  s.set({ motion: "calm", sound: true });
  s.set("motion", "calm"); // no change, no event
  s.set("nothing", 1);
  assert.deepEqual(JSON.parse(box.get("settings")), { motion: "calm", sound: true });
  // a new instance reads what was saved; broken JSON and junk values fall back to the defaults
  assert.deepEqual(createSettings({ store }).get(), { sound: true, motion: "calm", batchMin: 85 });
  s.set("sound", "nonsense"); // only true is on
  s.set("motion", "nonsense");
  assert.deepEqual(heard, [["motion", "full", "auto"], ["motion", "calm", "full"], ["sound", true, false], ["sound", false, true], ["motion", "auto", "calm"]]);
  box.set("settings", "{not json");
  assert.deepEqual(createSettings({ store }).get(), { sound: false, motion: "auto", batchMin: 85 });
  box.set("settings", JSON.stringify({ sound: "yes", motion: 5 }));
  assert.deepEqual(createSettings({ store }).get(), { sound: false, motion: "auto", batchMin: 85 });
  box.set("settings", JSON.stringify({ sound: "yes", motion: "calm" }));
  assert.deepEqual(createSettings({ store }).get(), { sound: false, motion: "calm", batchMin: 85 });
  // an override from the address wins, is never saved, and is cleared by choosing the setting by hand
  box.clear();
  const o = createSettings({ store, search: "?motion=calm&other=1" });
  assert.equal(o.get("motion"), "calm");
  assert.equal(o.overridden("motion"), true);
  assert.equal(box.has("settings"), false, "nothing was saved by an override");
  o.set("motion", "full");
  assert.equal(o.get("motion"), "full");
  assert.equal(o.overridden("motion"), false);
  assert.deepEqual(overridesOf("?motion=weird&sound=1"), {});
  assert.deepEqual(overridesOf("?motion=auto"), {});
  assert.deepEqual(overridesOf("?motion=full"), { motion: "full" });
  // no storage at all: it still works in memory
  const none = createSettings({ store: { get() { throw new Error("blocked"); }, set() { throw new Error("blocked"); } } });
  none.set("motion", "calm");
  assert.equal(none.get("motion"), "calm");
});

test("settings: the `world` an earlier page saved, or an address asks for, is ignored without error and dropped on the next save", () => {
  for (const old of ["off", "high", 5, null, { deep: true }]) {
    const { box, store } = memory();
    box.set("settings", JSON.stringify({ world: old, motion: "calm", sound: true }));
    const s = createSettings({ store, search: "?world=off" });
    assert.deepEqual(s.get(), { sound: true, motion: "calm", batchMin: 85 }, `world ${JSON.stringify(old)} is not a setting any more`);
    assert.equal(s.get("world"), undefined);
    assert.equal(s.overridden("world"), false);
    const heard = [];
    s.onChange((c) => heard.push(c.key));
    assert.deepEqual(s.set("world", "high"), { sound: true, motion: "calm", batchMin: 85 }, "setting it does nothing");
    assert.deepEqual(heard, []);
    s.set("motion", "full");
    assert.deepEqual(JSON.parse(box.get("settings")), { motion: "full", sound: true }, "the stale key is not written back");
  }
  assert.deepEqual(overridesOf("?world=off&world=low"), {});
});
