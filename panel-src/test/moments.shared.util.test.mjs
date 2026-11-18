// The small helpers of moments/shared/util.js that two or more rooms rely on: the one clock of "fresh", and the rule of what setData writes.
import test from "node:test";
import assert from "node:assert/strict";
import { FRESH_MS as SESSION_FRESH_MS } from "../src/core/session.js";
import { FRESH_MS, setData, setAttr, isTime } from "../src/moments/shared/util.js";

test("the rooms and the session mark a changed row for the same time: there is one FRESH_MS", () => {
  assert.equal(FRESH_MS, SESSION_FRESH_MS);
});

test("setData writes a value as text, removes the attribute for null, undefined and false, and keeps a zero", () => {
  const el = { dataset: {} };
  setData(el, "n", 0);
  assert.equal(el.dataset.n, "0", "a zero is a value");
  setData(el, "n", "");
  assert.equal(el.dataset.n, "", "an empty text is a value");
  for (const gone of [null, undefined, false]) {
    el.dataset.n = "x";
    setData(el, "n", gone);
    assert.ok(!("n" in el.dataset), `${String(gone)} removes it`);
  }
  setData(el, "on", true);
  assert.equal(el.dataset.on, "true");
});

test("setData writes nothing when the value is already there", () => {
  let writes = 0;
  const store = { n: "1" };
  const el = { dataset: new Proxy(store, { set(target, key, value) { writes += 1; target[key] = value; return true; } }) };
  setData(el, "n", 1);
  assert.equal(writes, 0);
  setData(el, "n", 2);
  assert.equal(writes, 1);
});

test("isTime is true for text a date can be made of and false for what is missing or is not a date", () => {
  assert.equal(isTime("2026-10-07T06:00:00Z"), true);
  assert.equal(isTime("2026-10-07 18:00"), true);
  for (const bad of [null, undefined, "", "soon", 0]) assert.equal(isTime(bad), false, String(bad));
});

test("setAttr writes a value as text, removes the attribute for null, undefined and false, keeps an empty text and a zero, and writes nothing when it is already there", () => {
  const attrs = new Map();
  let writes = 0;
  const el = {
    hasAttribute: (name) => attrs.has(name),
    getAttribute: (name) => (attrs.has(name) ? attrs.get(name) : null),
    setAttribute: (name, value) => { writes += 1; attrs.set(name, value); },
    removeAttribute: (name) => attrs.delete(name),
  };
  setAttr(el, "aria-label", "Hola");
  setAttr(el, "aria-label", "Hola");
  assert.equal(writes, 1);
  setAttr(el, "hidden", "");
  assert.equal(attrs.get("hidden"), "", "an empty text is a value");
  setAttr(el, "n", 0);
  assert.equal(attrs.get("n"), "0");
  for (const gone of [null, undefined, false]) {
    attrs.set("x", "1");
    setAttr(el, "x", gone);
    assert.equal(attrs.has("x"), false, String(gone));
  }
  setAttr(el, "never", null);
  assert.equal(attrs.has("never"), false);
});
