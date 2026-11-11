import test from "node:test";
import assert from "node:assert/strict";
import { createActions, requestOf } from "../src/moments/aprender/actions.js";
import { ARM_MIN_MS, ARM_MAX_MS } from "../src/core/dom.js";

// A fake session whose `act` stays open until the test lets it finish, so the state in the middle can be looked at.
function rig(options = {}) {
  const log = { requests: [], said: [], sounds: [], changes: [], opened: [], links: [] };
  let clock = 1000;
  const timers = new Map();
  let nextTimer = 1;
  const gates = [];
  const session = {
    act(path, body, opts) {
      log.requests.push({ path, body });
      return new Promise((resolve) => gates.push(async (r) => { await opts.beforeRefresh(r); resolve({ ...r, message: "m" }); }));
    },
  };
  const actions = createActions({
    session,
    sound: { play: (name) => log.sounds.push(name) },
    announce: (text) => log.said.push(text),
    t: (key) => key,
    open: (key, id) => log.opened.push([key, id]),
    openUrl: (url) => log.links.push(url),
    onChange: (id) => log.changes.push(id),
    now: () => clock,
    timers: { set: (fn, ms) => { const id = nextTimer++; timers.set(id, { fn, ms }); return id; }, clear: (id) => timers.delete(id) },
    held: () => !!options.held,
  });
  return { actions, log, timers, advance: (ms) => { clock += ms; }, answer: (r = { ok: true, status: 200 }) => gates.shift()(r), fireTimers: () => { for (const [id, t] of [...timers]) { timers.delete(id); t.fn(); } } };
}

const act = (id, permanent = false) => ({ id, key: id[0], label: id, primary: false, visible: true, permanent });
const one = { id: "miss:salary", data: { card: "fx:10" }, keys: ["fx:10"], open: "fx:10", link: "https://example.com/10", actions: [act("resolve", true), act("open"), act("link")] };
const two = { ...one, id: "miss:kube", keys: ["fx:21", "fx:22"], data: { card: "fx:21" }, open: "fx:21" };

test("u gives the forms a question blocks back to the queue: one key for one form, a list for several", () => {
  assert.deepEqual(requestOf("resolve", one), { path: "/api/resolve", body: { key: "fx:10" } });
  assert.deepEqual(requestOf("resolve", two), { path: "/api/resolve", body: { keys: ["fx:21", "fx:22"] } });
  assert.equal(requestOf("resolve", { ...one, keys: [] }), null, "a row that blocks nothing gives nothing back");
  assert.equal(requestOf("open", one), null);
  assert.equal(requestOf("link", one), null);
  assert.equal(requestOf("approve", one), null, "APRENDER approves nothing");
});

test("u is a claim ('it is in the bank'), so it needs two deliberate presses; a double click is one, a long wait disarms", async () => {
  const r = rig();
  assert.equal(await r.actions.run("resolve", one), "armed");
  assert.deepEqual(r.log.said, ["act.sure"], "armed aloud, for the screen reader");
  assert.equal(r.log.requests.length, 0, "nothing was sent");
  assert.equal(r.actions.armedOf(one.id), "resolve");
  r.advance(ARM_MIN_MS - 1);
  assert.equal(await r.actions.run("resolve", one), "ignored", "a second press inside 350 ms is the same press");
  r.fireTimers();
  assert.equal(r.actions.armedOf(one.id), null, "5 s later it disarms by itself");
  assert.equal(await r.actions.run("resolve", one), "armed");
  r.advance(ARM_MIN_MS + 50);
  const sent = r.actions.run("resolve", one);
  assert.equal(r.actions.isPending(one.id), true, "the row is dim from the first instant");
  assert.deepEqual(r.log.requests, [{ path: "/api/resolve", body: { key: "fx:10" } }]);
  r.answer();
  assert.equal(await sent, "fired");
  assert.equal(r.actions.isPending(one.id), false);
  assert.deepEqual(r.log.sounds, ["arm", "arm", "confirm", "undo"]);
  assert.equal(r.timers.size, 0, "no timer is left behind");
  assert.ok(ARM_MAX_MS === 5000 && ARM_MIN_MS === 350);
});

test("a held key is never the second press; arming one row disarms the other", async () => {
  const held = rig({ held: true });
  await held.actions.run("resolve", one);
  held.advance(1000);
  assert.equal(await held.actions.run("resolve", one), "ignored");
  assert.equal(held.log.requests.length, 0);
  const r = rig();
  await r.actions.run("resolve", one);
  await r.actions.run("resolve", two);
  assert.equal(r.actions.armedOf(one.id), null);
  assert.equal(r.actions.armedOf(two.id), "resolve");
  assert.deepEqual(r.log.changes.slice(0, 3), [one.id, one.id, two.id], "each row is told when it is armed and when it is not");
  r.actions.disarm();
  assert.equal(r.timers.size, 0);
});

test("a no gives the row back at once; a yes keeps it dim until the snapshot after it; a second press while it waits is nothing", async () => {
  const r = rig();
  const row = { ...one, actions: [act("resolve")] };
  const first = r.actions.run("resolve", row);
  assert.equal(r.actions.isPending(row.id), true);
  assert.equal(await r.actions.run("resolve", row), "ignored");
  assert.equal(r.log.requests.length, 1);
  r.answer({ ok: false, status: 409 });
  await first;
  assert.deepEqual(r.log.sounds, ["error"]);
  assert.equal(r.actions.isPending(row.id), false);
});

test("Enter opens the posting of the first form, o opens its address; a row that has neither does nothing; a verb the row does not have is ignored", async () => {
  const r = rig();
  assert.equal(await r.actions.run("open", one), "opened");
  assert.deepEqual(r.log.opened, [["fx:10", "miss:salary"]]);
  assert.equal(await r.actions.run("link", one), "opened");
  assert.deepEqual(r.log.links, ["https://example.com/10"]);
  const bare = { ...one, open: null, link: null };
  assert.equal(await r.actions.run("open", bare), "ignored");
  assert.equal(await r.actions.run("link", bare), "ignored");
  assert.equal(await r.actions.run("resolve", { ...one, actions: [act("open")] }), "ignored", "the row does not take u");
  assert.equal(await r.actions.run("resolve", null), "ignored");
  assert.equal(r.log.requests.length, 0);
});
