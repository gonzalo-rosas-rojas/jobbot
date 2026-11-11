// The buttons of PROCESOS' drawer (moments/procesos.js `drawer`) without a browser, over test/helpers/fake-dom.mjs: a button the drawer draws always runs.
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
const cardOf = (key) => snap.board.flatMap((c) => c.cards).find((c) => c.key === key);
const tick = () => new Promise((r) => setTimeout(r, 0));

function rig() {
  setLang("es");
  setTimeZone("America/Santiago");
  dom.reset();
  resetSession();
  const m = model(snap);
  const acts = [];
  const reloads = [];
  const ctx = {
    session: { act: async (path, body) => { acts.push({ path, body }); return { ok: true }; }, said: () => "" },
    sound: { play() {} },
    announce() {},
    reduced: () => true,
    now: () => nowMs,
    open() {},
    openUrl() {},
    ask: async (id) => (id === "register" ? { state: "replied", note: null } : id === "withdraw" ? { note: null } : {}),
    model: () => m,
    drawer: { reload: () => reloads.push(1) },
  };
  const host = dom.document.createElement("div");
  dom.document.body.append(host);
  procesos.mount(host, ctx);
  procesos.update(m, ctx);
  const detailOf = (key) => ({ card: cardOf(key), location: "Remote", remote: true, events: [], score_detail: {}, gaps: [], dossier: null });
  const body = (key) => procesos.drawer(detailOf(key), cardOf(key), ctx);
  return { host, ctx, acts, reloads, body, done: () => procesos.unmount() };
}

test("J2-3: a button the drawer draws runs even when the list does not draw that application (a filter hides it)", async () => {
  const { host, acts, reloads, body, done } = rig();
  try {
    procesos.arrive({ filter: "quiet" }); // only the applications with no news: fx:2 (replied, his) is not in the list
    assert.equal(host.querySelector('[data-card="fx:2"]'), null, "the list does not draw it");
    const content = body("fx:2");
    const button = content.querySelector('[data-act="register"]');
    assert.ok(button, "the drawer offers Registrar");
    button.click();
    await tick();
    await tick();
    assert.deepEqual(acts, [{ path: "/api/mark", body: { key: "fx:2", state: "replied", note: null } }], "the request went out");
    assert.equal(reloads.length, 1, "and the drawer reads the application again");
  } finally { done(); }
});

test("J2-3: the same button with the application drawn in the list still runs on the list's own row (nothing changes when it is there)", async () => {
  const { acts, body, done } = rig();
  try {
    body("fx:2").querySelector('[data-act="register"]').click();
    await tick();
    await tick();
    assert.equal(acts.length, 1);
    assert.equal(acts[0].body.key, "fx:2");
  } finally { done(); }
});
