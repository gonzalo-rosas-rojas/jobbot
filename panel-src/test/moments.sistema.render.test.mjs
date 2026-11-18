// SISTEMA's page (moments/sistema.js over render.js and its blocks) without a browser, over test/helpers/fake-dom.mjs: every block is on the
// page, the page fills in from the snapshot, and a second snapshot reuses the rows of the sources table and of the errors list instead of rebuilding them.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
globalThis.requestAnimationFrame ??= () => 0;
const { setLang, setTimeZone } = await import("../src/i18n/index.js");
const { model } = await import("../src/core/model.js");
const sistema = (await import("../src/moments/sistema.js")).default;

const snap = JSON.parse(readFileSync(new URL("./fixtures/snapshot.rich.json", import.meta.url), "utf8"));
const nowMs = Date.parse(snap.generated_at);

function rig() {
  setLang("es");
  setTimeZone("America/Santiago");
  dom.reset();
  const ctx = {
    session: { act: async () => ({ ok: true }), said: () => "" },
    sound: { play() {} },
    announce() {},
    reduced: () => true,
    now: () => nowMs,
    open() {},
    model: () => model(snap),
  };
  const host = dom.document.createElement("div");
  dom.document.body.append(host);
  sistema.mount(host, ctx);
  return { host, ctx, done: () => sistema.unmount() };
}

test("every block of the sheet is drawn once the first snapshot arrives", () => {
  const { host, ctx, done } = rig();
  try {
    assert.equal(host.querySelector(".sistema").dataset.loading, "true", "a skeleton until the first snapshot");
    sistema.update(model(snap), ctx);
    const page = host.querySelector(".sistema");
    assert.equal(page.dataset.loading, undefined);
    for (const block of ["run", "account", "sources", "errors", "limits"]) assert.ok(host.querySelector(`[data-block="${block}"]`), block + " is on the page");
    assert.ok(host.querySelector(".top .eyebrow").textContent.length > 0, "the top says where this is");
    assert.equal(host.querySelectorAll(".figs .fig").length, 2, "two figures");
    assert.equal(host.querySelectorAll(".steps .step").length, 4, "four steps");
    assert.equal(host.querySelectorAll(".limits .cell, .cells .cell").length, 4, "four limits");
    assert.ok(host.querySelector(".foot .foot-keys").childNodes.length > 0, "the keys of the page at the foot");
  } finally { done(); }
});

test("a second snapshot keeps the rows of the sources table and of the errors list", () => {
  const { host, ctx, done } = rig();
  try {
    sistema.update(model(snap), ctx);
    const rowsOf = (sel) => host.querySelectorAll(sel);
    const sources = rowsOf("tr[data-src]");
    const errors = rowsOf("li.err");
    assert.ok(sources.length > 0, "the fixture has sources");
    sistema.update(model(snap), ctx);
    const again = rowsOf("tr[data-src]");
    assert.equal(again.length, sources.length);
    sources.forEach((tr, i) => assert.equal(again[i], tr, "the same row node"));
    const errorsAgain = rowsOf("li.err");
    assert.equal(errorsAgain.length, errors.length);
    errors.forEach((li, i) => assert.equal(errorsAgain[i], li, "the same error node"));
  } finally { done(); }
});
