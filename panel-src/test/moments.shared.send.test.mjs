// moments/shared/send.js: the request of a verb on a row, the dim row while the server answers, the sound of the answer and the toast with "Deshacer".
import test from "node:test";
import assert from "node:assert/strict";
import { createSender } from "../src/moments/shared/send.js";

function rig(options = {}) {
  const log = { requests: [], sounds: [], changes: [], toasts: [] };
  const pending = new Set();
  const session = {
    said: options.noSaid ? undefined : (r) => (r.data && r.data.message) || "",
    act: async (path, body, opts = {}) => {
      log.requests.push({ path, body, during: [...pending] });
      if (options.throws) throw new Error("boom");
      const r = options.answer || { ok: true, status: 200 };
      if (r.ok && opts.onOk) opts.onOk(r);
      if (opts.beforeRefresh) await opts.beforeRefresh(r);
      return r;
    },
  };
  const send = createSender({
    session,
    sound: { play: (name) => log.sounds.push(name) },
    pending,
    onChange: (id) => log.changes.push([id, pending.has(id)]),
    requestOf: (actionId, row, extra = {}) => (row.data.card ? { path: "/api/" + actionId, body: { key: row.data.card, ...extra } } : null),
    cue: { approve: "approve", snooze: "snooze" },
    undoOf: (actionId, row) => (actionId === "snooze" ? { path: "/api/unsnooze", body: { key: row.data.card }, words: "undo.words" } : null),
    toast: options.noToast ? null : (text, ok, o) => log.toasts.push({ text, ok, undo: o.undo }),
    t: (key) => key,
  });
  return { send, log, pending };
}

const row = { id: "card:x", data: { card: "x" } };

test("a verb the row cannot take sends nothing and is ignored", async () => {
  const { send, log } = rig();
  assert.equal(await send("approve", { id: "task:1", data: { task: "1" } }, {}), "ignored");
  assert.deepEqual(log.requests, []);
  assert.deepEqual(log.changes, []);
});

test("the row is pending from the first instant, the cue of the verb sounds on a yes, and the row is freed after", async () => {
  const { send, log, pending } = rig();
  assert.equal(await send("approve", row, {}), "fired");
  assert.deepEqual(log.requests, [{ path: "/api/approve", body: { key: "x" }, during: ["card:x"] }]);
  assert.deepEqual(log.sounds, ["approve"]);
  assert.equal(pending.size, 0);
  assert.deepEqual(log.changes, [["card:x", true], ["card:x", false]]);
});

test("a verb with no cue of its own sounds as a plain press", async () => {
  const { send, log } = rig();
  await send("confirm", row, {});
  assert.deepEqual(log.sounds, ["press"]);
});

test("a no from the server gives the row back at once, sounds the error, and the row is freed once more at the end", async () => {
  const { send, log, pending } = rig({ answer: { ok: false, status: 409 } });
  await send("approve", row, {});
  assert.deepEqual(log.sounds, ["error"]);
  assert.equal(pending.size, 0);
  assert.deepEqual(log.changes, [["card:x", true], ["card:x", false], ["card:x", false]]);
});

test("a request that throws still frees the row", async () => {
  const { send, pending, log } = rig({ throws: true });
  await assert.rejects(send("approve", row, {}), /boom/);
  assert.equal(pending.size, 0);
  assert.equal(log.changes.at(-1)[1], false);
});

test("a verb that has a way back leaves a toast with Deshacer: the server's words or the fallback; the undo asks the way back and sounds", async () => {
  const fallback = rig();
  await fallback.send("snooze", row, { until: "3d" });
  assert.equal(fallback.log.toasts.length, 1);
  assert.equal(fallback.log.toasts[0].text, "undo.words", "no words from the server: the fallback sentence");
  assert.equal(fallback.log.toasts[0].ok, true);
  await fallback.log.toasts[0].undo();
  assert.deepEqual(fallback.log.requests.map((r) => r.path), ["/api/snooze", "/api/unsnooze"]);
  assert.deepEqual(fallback.log.requests[1].body, { key: "x" });
  assert.deepEqual(fallback.log.sounds, ["snooze", "undo"]);

  const said = rig({ answer: { ok: true, status: 200, data: { message: "Pospuesta" } } });
  await said.send("snooze", row, {});
  assert.equal(said.log.toasts[0].text, "Pospuesta");
});

test("a verb with no way back, or no toast to offer it in, leaves none", async () => {
  const plain = rig();
  await plain.send("approve", row, {});
  assert.deepEqual(plain.log.toasts, []);
  const mute = rig({ noToast: true });
  await mute.send("snooze", row, {});
  assert.deepEqual(mute.log.toasts, []);
});

test("without session.said the toast falls back to its own sentence", async () => {
  const { send, log } = rig({ noSaid: true });
  await send("snooze", row, {});
  assert.equal(log.toasts[0].text, "undo.words");
});

test("an undo the server refuses sounds the error", async () => {
  const state = { answer: { ok: true, status: 200 } };
  const log = { sounds: [], toasts: [] };
  const session = {
    act: async (path, body, opts = {}) => {
      if (state.answer.ok && opts.onOk) opts.onOk(state.answer);
      if (opts.beforeRefresh) await opts.beforeRefresh(state.answer);
      return state.answer;
    },
  };
  const send = createSender({
    session, sound: { play: (name) => log.sounds.push(name) }, pending: new Set(), onChange() {},
    requestOf: () => ({ path: "/api/snooze", body: {} }), cue: {}, undoOf: () => ({ path: "/api/unsnooze", body: {}, words: "w" }),
    toast: (text, ok, o) => log.toasts.push(o), t: (key) => key,
  });
  await send("snooze", { id: "card:x", data: {} }, {});
  state.answer = { ok: false, status: 409 };
  await log.toasts[0].undo();
  assert.deepEqual(log.sounds, ["press", "error"]);
});
