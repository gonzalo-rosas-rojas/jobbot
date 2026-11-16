// COLA mounted over the fake DOM: when the request for the detail of a proposal fails, the page keeps its ordinary "error" state for that proposal
// and the reason is not lost: it is left in the console, like every other place of the page that swallows an exception to keep going.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
const { setLang, setTimeZone } = await import("../src/i18n/index.js");
const { model } = await import("../src/core/model.js");
const cola = (await import("../src/moments/cola.js")).default;

const base = JSON.parse(readFileSync(new URL("./fixtures/snapshot.rich.json", import.meta.url), "utf8"));
const nowMs = Date.parse(base.generated_at);

test("a refused detail request shows the ordinary error with its retry and leaves its reason in the console", async () => {
  setLang("es");
  setTimeZone("America/Santiago");
  dom.reset();
  const reason = new Error("the wire is cut");
  const asked = [];
  const ctx = {
    session: { act: async () => ({ ok: true }), fresh: new Set(), api: (path) => { asked.push(path); return Promise.reject(reason); }, blob: async () => null, said: () => false },
    sound: { play() {} }, announce() {}, toast: null, open() {}, go() {}, reduced: () => true, now: () => nowMs,
  };
  const host = dom.document.createElement("div");
  dom.document.body.append(host);
  const logged = [];
  let shown = "";
  const original = console.error;
  console.error = (...args) => { logged.push(args); };
  try {
    cola.mount(host, ctx);
    cola.update(model(structuredClone(base)), ctx);
    await new Promise((resolve) => setImmediate(resolve));
    await new Promise((resolve) => setImmediate(resolve));
    shown = host.textContent;
  } finally {
    console.error = original;
    cola.unmount();
  }
  assert.ok(asked.some((p) => p.startsWith("/api/job?key=")), "the detail of the proposal in focus was asked for");
  assert.ok(shown.includes("No se pudo cargar cómo se armó el puntaje."), "the page says the detail could not be loaded");
  assert.ok(!shown.includes(reason.message), "and never the words of the failure");
  assert.ok(logged.some((args) => args.includes(reason)), "the reason of the refusal was logged");
});
