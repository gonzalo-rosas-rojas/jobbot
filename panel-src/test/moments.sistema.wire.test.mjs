// SISTEMA's mount (moments/sistema.js) over the fake DOM: which clicks reach the engine switch. Only the button of the engine is the switch's.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
const { setLang, setTimeZone } = await import("../src/i18n/index.js");
const { model } = await import("../src/core/model.js");
const sistema = (await import("../src/moments/sistema.js")).default;

const snap = JSON.parse(readFileSync(new URL("./fixtures/snapshot.rich.json", import.meta.url), "utf8"));
const nowMs = Date.parse(snap.generated_at);

function rig() {
  setLang("es");
  setTimeZone("America/Santiago");
  dom.reset();
  const requests = [];
  const ctx = {
    session: { act: async (path) => { requests.push(path); return { ok: true }; } },
    sound: { play() {} },
    announce() {},
    toast: null,
    now: () => nowMs,
    open() {},
  };
  const host = dom.document.createElement("div");
  dom.document.body.append(host);
  sistema.mount(host, ctx);
  sistema.update(model(snap), ctx);
  return { host, requests };
}

test("a button with another data-action than the engine's does not touch the engine; the engine's button does", async () => {
  const { host, requests } = rig();
  try {
    const stray = dom.document.createElement("button");
    stray.setAttribute("data-action", "other");
    host.querySelector(".sistema").append(stray);
    stray.click();
    await new Promise((resolve) => setImmediate(resolve));
    assert.deepEqual(requests, [], "a stray button[data-action] is not STOP");

    const engine = host.querySelector('button[data-action="engine"]');
    assert.ok(engine, "the engine's button is drawn");
    engine.click();
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(requests.length, 1, "the engine's button reaches the switch");
  } finally {
    sistema.unmount();
  }
});
