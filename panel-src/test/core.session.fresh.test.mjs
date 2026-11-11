import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { useStorage } from "../src/core/store.js";
import { setLang, setTimeZone } from "../src/i18n/index.js";
import { createSession, FRESH_MS } from "../src/core/session.js";

const rich = () => JSON.parse(readFileSync(new URL("./fixtures/snapshot.rich.json", import.meta.url), "utf8"));
const shelf = () => { const m = new Map(); return { getItem: (k) => (m.has(k) ? m.get(k) : null), setItem: (k, v) => m.set(k, String(v)), removeItem: (k) => m.delete(k) }; };

// Timers that only move when the test says so.
function clock() {
  let now = 0, seq = 0;
  const items = [];
  return {
    setTimeout(fn, ms) { const id = ++seq; items.push({ id, at: now + ms, fn }); return id; },
    clearTimeout(id) { const at = items.findIndex((x) => x.id === id); if (at >= 0) items.splice(at, 1); },
    now: () => now,
    to(ms) { for (;;) { items.sort((a, b) => a.at - b.at); const n = items[0]; if (!n || n.at > ms) break; items.shift(); now = n.at; n.fn(); } now = ms; },
  };
}

test("the marks of the second snapshot last the full time from the second, not from the first", async () => {
  useStorage({ local: shelf(), session: shelf() });
  setTimeZone("UTC");
  setLang("es");
  const timers = clock();
  const snap = rich();
  const server = { snapshot: snap };
  const session = createSession({
    timers, now: timers.now, token: "t",
    fetchImpl: async () => ({ ok: true, status: 200, json: async () => structuredClone(server.snapshot) }),
  });
  assert.equal(FRESH_MS, 4000);
  await session.start();
  timers.to(2000);
  const next = structuredClone(snap);
  next.generated_at = new Date(Date.parse(snap.generated_at) + 2000).toISOString();
  next.today.emails.push({ ...next.today.emails[0], subject: "A brand new subject", received_at: next.generated_at });
  server.snapshot = next;
  await session.refresh({ force: true });
  assert.ok(session.fresh.size > 0, "the second snapshot brought marks");
  timers.to(5000);                       // 5 s after the first, 3 s after the second
  assert.ok(session.fresh.size > 0, "the first snapshot's clearing must not take them");
  timers.to(2000 + FRESH_MS - 1);
  assert.ok(session.fresh.size > 0);
  timers.to(2000 + FRESH_MS);            // 4 s after the second
  assert.equal(session.fresh.size, 0);
  session.stop();
});
