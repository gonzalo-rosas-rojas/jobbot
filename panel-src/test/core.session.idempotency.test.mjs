// Acceptance test of the encargo docs/encargos/idempotency-key-en-cada-post.md. The panel server runs a POST once per attempt id
// (`Idempotency-Key`, crates/jobbot-panel/src/http.rs: attempt_id and once; ADR 0006): a repeat of the id gets the first reply back,
// marked `Idempotent-Replayed`, and the action runs once. The page has to send that id on every POST the session makes: a new one for
// every action Gonza asks for, and the same one, with the same body, when the page sends that action again (one more try after a request
// that got no answer, or the same action asked again while the first is in flight). A GET carries none. Everything is a fake server
// handed to createSession: no browser, no DOM.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { setLang } from "../src/i18n/index.js";
import { createSession } from "../src/core/session.js";

const SNAPSHOT = JSON.parse(readFileSync(new URL("./fixtures/snapshot.rich.json", import.meta.url), "utf8"));
const HEADER = "Idempotency-Key";
// What the server takes (http.rs, attempt_id): 1 to 128 visible ASCII characters, no quote. The page's ids are random and at least
// 16 characters long (a crypto.randomUUID() is 36): the server keeps them a week, across reloads of the page.
const WELL_FORMED = /^[\x21\x23-\x7e]{16,128}$/;

/** The value of a header in a fetch init, whatever shape the headers come in (object, Headers, pairs). */
function headerOf(init, name) {
  const h = init && init.headers;
  if (!h) return undefined;
  if (typeof h.get === "function") return h.get(name) ?? undefined;
  const pairs = Array.isArray(h) ? h : Object.entries(h);
  const hit = pairs.find(([k]) => String(k).toLowerCase() === name.toLowerCase());
  return hit ? hit[1] : undefined;
}

const answer = (status, body) => ({
  ok: status >= 200 && status < 300, status,
  json: async () => structuredClone(body),
  blob: async () => new Blob(["png"]),
});

/**
 * A server that records every request. Each POST takes the next step of `plan` ("ok" when it is empty): "throw" (the network failed),
 * "hang" (no answer until the page gives up and aborts), or { status, body }. While `hold` is a promise, every POST waits for it.
 */
function fakeServer() {
  const s = { calls: [], plan: [], hold: null };
  s.fetchImpl = async (path, init = {}) => {
    const method = String(init.method || "GET").toUpperCase();
    s.calls.push({ path, method, key: headerOf(init, HEADER), body: init.body });
    if (method !== "POST") return answer(200, path.startsWith("/api/snapshot") ? SNAPSHOT : { ok: true });
    if (s.hold) await s.hold;
    const step = s.plan.length ? s.plan.shift() : "ok";
    if (step === "throw") throw new TypeError("network down");
    if (step === "hang") {
      return new Promise((resolve, reject) => {
        if (init.signal) init.signal.addEventListener("abort", () => reject(Object.assign(new Error("aborted"), { name: "AbortError" })));
      });
    }
    if (typeof step === "object") return answer(step.status, step.body);
    return answer(200, { ok: true, message: "Hecho en el servidor." });
  };
  s.posts = (path) => s.calls.filter((c) => c.method === "POST" && (!path || c.path === path));
  return s;
}

function rig(options = {}) {
  setLang("es");
  const server = fakeServer();
  const session = createSession({ fetchImpl: server.fetchImpl, token: "tok", requestTimeoutMs: 50, freshMs: 1, ...options });
  return { server, session };
}

const keysOf = (calls) => calls.map((c) => c.key);
const settle = () => new Promise((resolve) => setImmediate(resolve));

test("idempotency: every POST of the session carries a well-formed key, and no GET carries one", async () => {
  const { server, session } = rig();
  await session.start();
  await session.act("/api/approve", { key: "fx:12" });
  await session.api("/api/task/add", { text: "Llamar" });
  await session.api("/api/job?key=" + encodeURIComponent("fx:12"));
  await session.blob("/api/shot?key=" + encodeURIComponent("fx:12"));
  session.stop();
  const posts = server.posts();
  assert.deepEqual(posts.map((c) => c.path), ["/api/approve", "/api/task/add"], "one POST per action, none repeated when the server answered");
  for (const post of posts) assert.match(String(post.key), WELL_FORMED, `${post.path} carries ${HEADER}`);
  assert.notEqual(posts[0].key, posts[1].key, "two actions, two keys");
  const gets = server.calls.filter((c) => c.method !== "POST");
  assert.ok(gets.length >= 4, "the snapshot (twice), the job and the screenshot were read");
  for (const get of gets) assert.equal(get.key, undefined, `GET ${get.path} carries no ${HEADER}`);
});

test("idempotency: a POST that got no answer is sent once more with the same key and the same body", async () => {
  const { server, session } = rig();
  server.plan = ["throw"];
  const result = await session.act("/api/mark", { key: "fx:3", state: "replied", note: null });
  const posts = server.posts("/api/mark");
  assert.equal(posts.length, 2, "the network failed once: one more try");
  assert.match(String(posts[0].key), WELL_FORMED);
  assert.equal(posts[1].key, posts[0].key, "the retry is the same action: same key");
  assert.equal(posts[1].body, posts[0].body, "and the same bytes, or the server refuses the key with 422");
  assert.deepEqual([result.ok, result.message], [true, "Hecho en el servidor."]);
});

test("idempotency: a POST cut off at the time limit is sent once more with the same key", async () => {
  const { server, session } = rig({ requestTimeoutMs: 40 });
  server.plan = ["hang"];
  const result = await session.act("/api/learn/grade", { id: "rust-1", grade: 4 });
  const posts = server.posts("/api/learn/grade");
  assert.equal(posts.length, 2, "the first try was given up: one more try");
  assert.match(String(posts[0].key), WELL_FORMED);
  assert.equal(posts[1].key, posts[0].key, "the server may still be running the first one: the retry gets its reply instead of a second run");
  assert.equal(posts[1].body, posts[0].body);
  assert.equal(result.ok, true);
});

test("idempotency: a server that never answers is asked twice in all, and the next action has a key of its own", async () => {
  const { server, session } = rig();
  server.plan = ["throw", "throw", "throw"];
  const failed = await session.act("/api/task/done", { id: 101 });
  const tries = server.posts("/api/task/done");
  assert.equal(tries.length, 2, "one retry, not a loop");
  assert.match(String(tries[0].key), WELL_FORMED);
  assert.equal(tries[1].key, tries[0].key);
  assert.deepEqual([failed.ok, failed.status], [false, 0]);
  server.plan = [];
  const again = await session.act("/api/task/done", { id: 101 });
  const all = server.posts("/api/task/done");
  assert.equal(all.length, 3, "asked again by hand: one request");
  assert.equal(again.ok, true);
  assert.match(String(all[2].key), WELL_FORMED);
  assert.notEqual(all[2].key, tries[0].key, "a new action, not a retry: a new key");
});

test("idempotency: an answer from the server is never sent again, and asking again is a new action with a new key", async () => {
  const { server, session } = rig();
  server.plan = [{ status: 500, body: { ok: false, message: "Error interno: boom" } }];
  const broken = await session.act("/api/stop", {});
  server.plan = [{ status: 409, body: { ok: false, message: "Una petición con este Idempotency-Key empezó y no terminó" } }];
  const refused = await session.act("/api/stop", {});
  const ok = await session.act("/api/stop", {});
  const posts = server.posts("/api/stop");
  assert.equal(posts.length, 3, "500 and 409 are answers: each action made one request");
  assert.deepEqual([broken.status, refused.status, ok.status], [500, 409, 200]);
  for (const post of posts) assert.match(String(post.key), WELL_FORMED);
  assert.equal(new Set(keysOf(posts)).size, 3, "three actions with the same body, three keys");
});

test("idempotency: the same action asked again while the first is in flight carries the first one's key; another action has its own", async () => {
  const { server, session } = rig();
  let release;
  server.hold = new Promise((resolve) => { release = resolve; });
  const first = session.act("/api/task/add", { text: "Llamar" });
  const twice = session.act("/api/task/add", { text: "Llamar" });
  const other = session.act("/api/task/add", { text: "Escribir" });
  await settle();
  server.hold = null;
  release();
  const results = await Promise.all([first, twice, other]);
  for (const r of results) assert.equal(r.ok, true);
  const llamar = server.posts("/api/task/add").filter((c) => c.body === JSON.stringify({ text: "Llamar" }));
  const escribir = server.posts("/api/task/add").filter((c) => c.body === JSON.stringify({ text: "Escribir" }));
  assert.ok(llamar.length >= 1 && escribir.length >= 1);
  assert.match(String(llamar[0].key), WELL_FORMED);
  assert.match(String(escribir[0].key), WELL_FORMED);
  assert.deepEqual([...new Set(keysOf(llamar))], [llamar[0].key], "the double click is one action: every request of it has one key");
  assert.deepEqual([...new Set(keysOf(escribir))], [escribir[0].key]);
  assert.notEqual(llamar[0].key, escribir[0].key, "a different body is a different action");
});

test("idempotency: keys are random, never repeated within a page or across two loads of it", async () => {
  const keys = [];
  for (const load of [1, 2]) {
    const { server, session } = rig();
    for (let i = 0; i < 10; i++) await session.act("/api/task/add", { text: "Tarea " + (i % 2) });
    keys.push(...keysOf(server.posts()));
  }
  assert.equal(keys.length, 20);
  for (const key of keys) assert.match(String(key), WELL_FORMED);
  assert.equal(new Set(keys).size, 20, "the server keeps keys a week: a counter that starts again on every load would collide");
  for (let i = 1; i < keys.length; i++) {
    const a = String(keys[i - 1]);
    const b = String(keys[i]);
    let differ = 0;
    for (let j = 0; j < Math.min(a.length, b.length); j++) if (a[j] !== b[j]) differ += 1;
    assert.ok(differ >= 8, `consecutive keys ${a} and ${b} differ in ${differ} places: they look counted, not random`);
  }
});

test("idempotency: a read that got no answer is sent once and never again, and carries no key", async () => {
  setLang("es");
  const calls = [];
  const fetchImpl = async (path, init = {}) => {
    calls.push({ path, method: String(init.method || "GET").toUpperCase(), key: headerOf(init, HEADER) });
    if (path.startsWith("/api/learn/")) {
      return new Promise((resolve, reject) => {
        if (init.signal) init.signal.addEventListener("abort", () => reject(Object.assign(new Error("aborted"), { name: "AbortError" })));
      });
    }
    throw new TypeError("network down");
  };
  const session = createSession({ fetchImpl, token: "tok", requestTimeoutMs: 40, freshMs: 1 });
  const job = await session.api("/api/job?key=" + encodeURIComponent("fx:12"));
  const learn = await session.api("/api/learn/next");
  const shot = await session.blob("/api/shot?key=" + encodeURIComponent("fx:12"));
  const read = await session.refresh({ force: true });
  session.stop();
  assert.deepEqual([job.ok, job.status], [false, 0], "the network failed: the read says so");
  assert.equal(learn.timedOut, true, "the read that never answered was cut off at the time limit");
  assert.deepEqual([shot, read], [null, false]);
  assert.deepEqual(calls.map((c) => c.method + " " + c.path.split("?")[0]), ["GET /api/job", "GET /api/learn/next", "GET /api/shot", "GET /api/snapshot"], "each read went out exactly once: a GET is never retried");
  for (const c of calls) assert.equal(c.key, undefined, `GET ${c.path} carries no ${HEADER}`);
});
