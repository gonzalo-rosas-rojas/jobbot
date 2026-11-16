import test from "node:test";
import assert from "node:assert/strict";
import { setLang, t } from "../src/i18n/index.js";
import { createSession } from "../src/core/session.js";

test("a demo server that throws shows the person 'no network' and leaves the error in the console", async () => {
  setLang("en");
  const boom = new Error("demo bug");
  const seen = [];
  const original = console.error;
  console.error = (...args) => seen.push(args);
  try {
    const session = createSession({ demo: { api: async () => { throw boom; } } });
    const reply = await session.api("/api/snapshot");
    assert.equal(reply.ok, false);
    assert.equal(reply.status, 0);
    assert.equal(reply.data.message, t("err.network"), "the person still sees the 'no network' text");
    assert.ok(seen.some((args) => args.includes(boom)), "a programming mistake in the demo must not vanish");
  } finally {
    console.error = original;
  }
});

test("a reply that fails to arrive is sent once more with the same key, and a read is not repeated", async () => {
  const calls = [];
  const session = createSession({
    token: "t",
    fetchImpl: async (path, init) => {
      calls.push({ path, method: init.method || "GET", key: init.headers["Idempotency-Key"] || null });
      throw new Error("offline");
    },
  });
  const post = await session.api("/api/do", { a: 1 });
  assert.equal(post.status, 0);
  assert.equal(calls.length, 2, "a POST that got no answer is tried twice");
  assert.equal(calls[0].key, calls[1].key, "with the same Idempotency-Key");
  assert.ok(calls[0].key);
  calls.length = 0;
  await session.api("/api/snapshot");
  assert.equal(calls.length, 1, "a GET is not repeated");
  assert.equal(calls[0].key, null);
});

test("blob() gives the bytes, or null when the file is not there", async () => {
  const bytes = { tag: "bytes" };
  const ok = createSession({ fetchImpl: async () => ({ ok: true, blob: async () => bytes }) });
  assert.equal(await ok.blob("/shots/a.png"), bytes);
  const missing = createSession({ fetchImpl: async () => ({ ok: false }) });
  assert.equal(await missing.blob("/shots/a.png"), null);
  const down = createSession({ fetchImpl: async () => { throw new Error("offline"); } });
  assert.equal(await down.blob("/shots/a.png"), null);
  const demo = createSession({ demo: { api: async () => ({}), blob: async () => ({ tag: "drawn" }) } });
  assert.deepEqual(await demo.blob("/shots/a.png"), { tag: "drawn" });
});
