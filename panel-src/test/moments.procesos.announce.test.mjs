// PROCESOS says which filter is on when it changes: the keys 1-4, the chip and the "clear" button all announce it, a search or an order does not.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
globalThis.requestAnimationFrame ??= () => 0;
const { setLang, setTimeZone } = await import("../src/i18n/index.js");
const { model } = await import("../src/core/model.js");
const procesos = (await import("../src/moments/procesos.js")).default;
const { resetSession } = await import("../src/moments/procesos.js");

const snap = JSON.parse(readFileSync(new URL("./fixtures/snapshot.rich.json", import.meta.url), "utf8"));
const nowMs = Date.parse(snap.generated_at);

function rig() {
  setLang("es");
  setTimeZone("America/Santiago");
  dom.reset();
  resetSession();
  const m = model(snap);
  const said = [];
  const ctx = {
    session: { act: async () => ({ ok: true }), said: () => "" },
    sound: { play() {} },
    announce: (text) => said.push(text),
    reduced: () => true,
    now: () => nowMs,
    open() {},
    openUrl() {},
    model: () => m,
  };
  const host = dom.document.createElement("div");
  dom.document.body.append(host);
  procesos.mount(host, ctx);
  procesos.update(m, ctx);
  return { host, said, done: () => procesos.unmount() };
}

test("a filter key announces the filter that is on, with its count", () => {
  const { said, done } = rig();
  try {
    assert.equal(procesos.onKey("filter", null, { key: "2" }), true);
    assert.equal(said.length, 1);
    assert.match(said[0], /^.+ · \d+$/);
  } finally { done(); }
});

test("a click on a filter chip announces it, and so does the clear button", () => {
  const { host, said, done } = rig();
  try {
    const chips = host.querySelectorAll("button[data-filter]");
    assert.ok(chips.length >= 2, "the chips are drawn");
    chips[1].click();
    assert.equal(said.length, 1);
    assert.match(said[0], /^.+ · \d+$/);
    const find = host.querySelector("input.find-input");
    find.value = "zzzz-nothing-matches-this";
    find.dispatchEvent({ type: "input", bubbles: true });
    assert.equal(said.length, 1, "a search alone says nothing");
    const clear = host.querySelector("button[data-reset]");
    assert.ok(clear, "a search that finds nothing offers to clear");
    clear.click();
    assert.equal(said.length, 2, "clearing goes back to all and says so");
  } finally { done(); }
});

test("an order and the mode do not announce a filter", () => {
  const { host, said, done } = rig();
  try {
    const order = host.querySelector("button[data-order]");
    assert.ok(order, "the order chips are drawn");
    order.click();
    const mode = host.querySelector("button[data-mode-toggle]");
    assert.ok(mode, "the mode button is drawn");
    mode.click();
    assert.deepEqual(said, []);
  } finally { done(); }
});

test("arriving with a filter (HOY's \"y M más\") changes it without announcing", () => {
  const { said, done } = rig();
  try {
    procesos.arrive({ filter: "quiet" });
    assert.deepEqual(said, []);
  } finally { done(); }
});
