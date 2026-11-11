// The hygiene of the refresh loop (core/session.js), the part a slow server shows: a base so big that the first answer takes ten seconds must never make
// the page look broken or the server pay twice. One request in flight at a time whatever asks (the timer, the person coming back, three actions that
// end together), a request that never answers is cut off and says so, the 30 seconds count from the END of the last answer, an action reads the
// snapshot once more and not in a chain, a hidden page asks nothing, and coming back reads at once. Everything is a fake: the timers, the server
// and its latency; nothing here waits for real time.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createSession, POLL_MS } from "../src/core/session.js";
import { t } from "../src/i18n/index.js";

const rich = () => structuredClone(JSON.parse(readFileSync(new URL("./fixtures/snapshot.rich.json", import.meta.url), "utf8")));

const settle = async () => {
  for (let i = 0; i < 30; i++) await Promise.resolve();
  await new Promise((resolve) => setImmediate(resolve));
};

function fakeTimers() {
  let now = 0;
  let seq = 0;
  const items = [];
  return {
    setTimeout(fn, ms) { const id = ++seq; items.push({ id, at: now + ms, fn }); return id; },
    clearTimeout(id) { const at = items.findIndex((x) => x.id === id); if (at >= 0) items.splice(at, 1); },
    now: () => now,
    waits: () => items.map((x) => x.at - now).sort((a, b) => a - b),
    async advance(ms) {
      const end = now + ms;
      for (;;) {
        items.sort((a, b) => a.at - b.at || a.id - b.id);
        const next = items[0];
        if (!next || next.at > end) break;
        items.shift();
        now = next.at;
        next.fn();
        await settle();
      }
      now = end;
      await settle();
    },
  };
}

/** A server with a latency in fake time (`Infinity`: never answers) that counts its snapshot reads and how many were in flight at once. */
function slowServer(timers, latency = 0) {
  const s = { latency, snapshotCalls: 0, posts: 0, inflight: 0, maxInflight: 0, aborted: 0, snapshot: rich() };
  s.fetchImpl = async (path, init = {}) => {
    const post = init.method === "POST";
    if (post) s.posts += 1;
    else { s.snapshotCalls += 1; s.inflight += 1; s.maxInflight = Math.max(s.maxInflight, s.inflight); }
    try {
      if (!post && s.latency > 0) {                                   // an action answers at once; a read takes `latency`
        await new Promise((resolve, reject) => {
          const wait = s.latency === Infinity ? null : timers.setTimeout(resolve, s.latency);
          if (init.signal) init.signal.addEventListener("abort", () => { if (wait != null) timers.clearTimeout(wait); s.aborted += 1; reject(Object.assign(new Error("aborted"), { name: "AbortError" })); });
        });
      }
    } finally { if (!post) s.inflight -= 1; }
    const body = post ? { ok: true, message: "Aprobada: la próxima corrida la envía." } : s.snapshot;
    return { ok: true, status: 200, json: async () => structuredClone(body) };
  };
  return s;
}

function rig({ latency = 0, ...options } = {}) {
  const timers = fakeTimers();
  const server = slowServer(timers, latency);
  const flags = { visible: true };
  const events = [];
  const session = createSession({ fetchImpl: server.fetchImpl, token: "tok", timers, now: timers.now, isVisible: () => flags.visible, ...options });
  session.subscribe((e) => events.push(e));
  return { session, server, timers, flags, events };
}

test("a request that never answers is cut off at the limit, the error says so in words, and the loop goes on", async () => {
  const { session, server, timers, events } = rig({ latency: Infinity, requestTimeoutMs: 5000 });
  const first = session.start();
  await timers.advance(4999);
  assert.equal(server.inflight, 1, "still waiting");
  await timers.advance(1);
  assert.equal(await first, false);
  assert.equal(server.aborted, 1, "the request was aborted, not forgotten");
  assert.equal(server.inflight, 0);
  assert.equal(session.error.kind, "timeout");
  assert.equal(session.error.fatal, false);
  assert.equal(session.error.message, t("err.timeout"));
  assert.doesNotMatch(session.error.message, /undefined|null|NaN|⟦/);
  assert.ok(events.some((e) => e.type === "error"));
  // the server wakes up: the next try (after the wait of one failure) works and the error is gone
  server.latency = 0;
  await timers.advance(POLL_MS * 2);
  assert.equal(server.snapshotCalls, 2);
  assert.equal(session.error, null);
  assert.ok(session.snapshot, "the picture arrived");
  session.stop();
});

test("never two snapshot reads at once, whatever asks: the timer, the person coming back, an ask that joins, three actions that end together", async () => {
  const { session, server, timers } = rig({ latency: 4000 });
  const first = session.start();
  await timers.advance(1000);
  const joined = [session.refresh({ force: false }), session.refresh({ force: false }), session.wake(), session.refresh()];
  await timers.advance(3000);
  await first;
  await Promise.all(joined);
  assert.equal(server.snapshotCalls, 1, "all of them were the one read in flight");
  const actions = [session.act("/api/approve", { key: "a" }), session.act("/api/approve", { key: "b" }), session.act("/api/approve", { key: "c" })];
  await timers.advance(1);
  assert.equal(server.inflight, 1);
  await timers.advance(4000);
  await timers.advance(4000);
  await Promise.all(actions);
  assert.equal(server.maxInflight, 1, "never two in flight");
  assert.equal(server.snapshotCalls, 3, "the first read, one for the first action and ONE more for the other two: not one each, and no chain");
  assert.equal(server.posts, 3);
  await timers.advance(POLL_MS * 4);
  assert.equal(server.maxInflight, 1);
  session.stop();
});

test("the 30 seconds count from the end of the last answer: a four-second answer is followed by the next read at 34 s, not at 30", async () => {
  const { session, server, timers } = rig({ latency: 4000 });
  const first = session.start();
  await timers.advance(4000);
  await first;
  assert.equal(server.snapshotCalls, 1);
  await timers.advance(POLL_MS - 1);
  assert.equal(server.snapshotCalls, 1, "30 s after the answer, not before");
  await timers.advance(1);
  assert.equal(server.snapshotCalls, 2);
  assert.equal(server.inflight, 1, "the second is in flight now; the third waits for its end");
  await timers.advance(4000 + POLL_MS - 1);
  assert.equal(server.snapshotCalls, 2);
  await timers.advance(1);
  assert.equal(server.snapshotCalls, 3);
  assert.equal(server.maxInflight, 1);
  session.stop();
});

test("an action reads once and moves the next poll a whole interval away from it", async () => {
  const { session, server, timers } = rig();
  await session.start();
  await timers.advance(25000);
  await session.act("/api/approve", { key: "fx:12" });
  assert.equal(server.snapshotCalls, 2, "the first read and the one after the action");
  assert.equal(server.posts, 1);
  await timers.advance(POLL_MS - 1);
  assert.equal(server.snapshotCalls, 2, "the poll that was due at 30 s did not come: the action had just read");
  await timers.advance(1);
  assert.equal(server.snapshotCalls, 3, "one interval after the action");
  session.stop();
});

test("a hidden page asks nothing; coming back reads at once when the last read is not fresh, and not twice in a row", async () => {
  const { session, server, timers, flags } = rig();
  await session.start();
  session.pause();
  flags.visible = false;
  await timers.advance(POLL_MS * 4);
  assert.equal(server.snapshotCalls, 1, "a hidden page pays nothing");
  assert.deepEqual(timers.waits().filter((ms) => ms > 4000), [], "and there is no timer left to wake it");
  flags.visible = true;
  assert.equal(await session.wake(), true, "back: read now, not in half a minute");
  assert.equal(server.snapshotCalls, 2);
  assert.equal(await session.wake(), false, "a second return a moment later does not read again");
  assert.equal(server.snapshotCalls, 2);
  assert.deepEqual(timers.waits().filter((ms) => ms > 4000), [POLL_MS], "and the loop is back, a whole interval from this read");
  await timers.advance(POLL_MS);
  assert.equal(server.snapshotCalls, 3);
  session.stop();
});

test("coming back after a few seconds (under the floor) does not read, but the loop is running again", async () => {
  const { session, server, timers, flags } = rig();
  await session.start();
  await timers.advance(2000);
  session.pause();
  flags.visible = false;
  flags.visible = true;
  assert.equal(await session.wake(), false, "the picture is two seconds old");
  assert.equal(server.snapshotCalls, 1);
  assert.deepEqual(timers.waits().filter((ms) => ms > 4000), [POLL_MS], "but polling is back");
  session.stop();
});
