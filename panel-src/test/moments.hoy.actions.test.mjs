import test from "node:test";
import assert from "node:assert/strict";
import { createActions, requestOf, DEFAULT_SNOOZE } from "../src/moments/hoy/actions.js";
import { ARM_MIN_MS, ARM_MAX_MS } from "../src/core/dom.js";

const flush = () => new Promise((resolve) => setImmediate(resolve));

// A fake session whose `act` stays open until the test lets it finish, so the state in the middle can be looked at.
function rig(options = {}) {
  const log = { requests: [], said: [], sounds: [], changes: [], opened: [], links: [], afterLink: [], asked: [], toasts: [] };
  let clock = 1000;
  const timers = new Map();
  let nextTimer = 1;
  const gates = [];
  const session = {
    said: (r, fallback) => (r.data && r.data.message) || fallback,
    act(path, body, opts = {}) {
      log.requests.push({ path, body });
      return new Promise((resolve) => gates.push(async (r) => {
        if (r.ok && opts.onOk) opts.onOk(r);
        if (opts.beforeRefresh) await opts.beforeRefresh(r);
        resolve({ ...r, message: "m" });
      }));
    },
  };
  const actions = createActions({
    session,
    sound: { play: (name) => log.sounds.push(name) },
    announce: (text) => log.said.push(text),
    t: (key) => key,
    open: (key, id) => log.opened.push([key, id]),
    openUrl: (url) => log.links.push(url),
    afterLink: (row) => log.afterLink.push(row.id),
    onChange: (id) => log.changes.push(id),
    now: () => clock,
    timers: { set: (fn, ms) => { const id = nextTimer++; timers.set(id, { fn, ms }); return id; }, clear: (id) => timers.delete(id) },
    held: () => !!options.held,
    ask: options.ask ? async (...args) => { log.asked.push(args[0]); return options.ask(...args); } : null,
    toast: options.toast ? (text, ok, o) => log.toasts.push([text, ok, o]) : null,
  });
  return {
    actions, log, timers,
    advance: (ms) => { clock += ms; },
    answer: (r = { ok: true, status: 200 }) => gates.shift()(r),
    fireTimers: () => { for (const [id, t] of [...timers]) { timers.delete(id); t.fn(); } },
  };
}

const act = (id, permanent = false) => ({ id, key: id[0], label: id, primary: false, visible: true, permanent });
const card = { id: "card:fx:12", data: { card: "fx:12" }, open: "fx:12", link: "https://example.com/12", actions: [act("approve", true), act("open"), act("snooze"), act("link"), act("resolve"), act("confirm", true)] };
const task = { id: "task:101", data: { task: "101" }, open: "fx:2", link: null, actions: [act("done", true), act("open")] };

test("each verb makes the request the server expects, and a row only takes what it is", () => {
  assert.deepEqual(requestOf("done", task), { path: "/api/task/done", body: { id: 101 } }, "a task id is a number");
  assert.deepEqual(requestOf("approve", card), { path: "/api/approve", body: { key: "fx:12" } });
  assert.deepEqual(requestOf("confirm", card), { path: "/api/confirm", body: { key: "fx:12" } });
  assert.deepEqual(requestOf("resolve", card), { path: "/api/resolve", body: { key: "fx:12" } });
  assert.deepEqual(requestOf("unsnooze", card), { path: "/api/unsnooze", body: { key: "fx:12" } });
  assert.deepEqual(requestOf("snooze", card), { path: "/api/snooze", body: { key: "fx:12", until: DEFAULT_SNOOZE } });
  assert.deepEqual(requestOf("snooze", card, { until: "3d" }).body, { key: "fx:12", until: "3d" });
  assert.deepEqual(requestOf("withdraw", card, { note: "ya no" }), { path: "/api/mark", body: { key: "fx:12", state: "withdrawn", note: "ya no" } });
  assert.equal(requestOf("register", card, {}), null, "a register needs the state he chose");
  assert.deepEqual(requestOf("register", card, { state: "replied" }).body, { key: "fx:12", state: "replied", note: null });
  assert.equal(requestOf("approve", task), null, "a task is not approved");
  assert.equal(requestOf("done", card), null, "a card is not done");
  assert.equal(requestOf("open", card), null);
});

test("a permanent verb needs two deliberate presses: the first arms and says so, a double click is one press, a long wait disarms", async () => {
  const r = rig();
  assert.equal(await r.actions.run("approve", card), "armed");
  assert.deepEqual(r.log.said, ["act.sure"], "armed aloud, for the screen reader");
  assert.equal(r.log.requests.length, 0, "nothing was sent");
  assert.equal(r.actions.isArmed(card.id, "approve"), true);
  assert.equal(r.actions.isArmed(card.id, "confirm"), false);

  r.advance(ARM_MIN_MS - 1);
  assert.equal(await r.actions.run("approve", card), "ignored", "a second press inside 350 ms is the same press");
  assert.equal(r.actions.isArmed(card.id, "approve"), true, "and it does not disarm");

  r.fireTimers(); // the 5 s timer
  assert.equal(r.actions.isArmed(card.id), false, "5 s later it is disarmed by itself");
  assert.equal(await r.actions.run("approve", card), "armed", "and the next press starts over");

  r.advance(ARM_MIN_MS + 50);
  const sent = r.actions.run("approve", card);
  assert.equal(r.actions.isPending(card.id), true, "the row is dim from the first instant, before the server answers");
  assert.deepEqual(r.log.requests, [{ path: "/api/approve", body: { key: "fx:12" } }]);
  r.answer();
  assert.equal(await sent, "fired");
  assert.equal(r.actions.isPending(card.id), false);
  assert.deepEqual(r.log.sounds, ["arm", "arm", "confirm", "approve"]);
  assert.equal(r.timers.size, 0, "no timer is left behind");
  assert.ok(ARM_MAX_MS === 5000 && ARM_MIN_MS === 350, "the numbers of the spec (§3.4)");
});

test("a held key never is the second press; arming one verb or one row disarms the other", async () => {
  const held = rig({ held: true });
  await held.actions.run("done", task);
  held.advance(1000);
  assert.equal(await held.actions.run("done", task), "ignored", "the repeat of a held key");
  assert.equal(held.log.requests.length, 0);

  const r = rig();
  await r.actions.run("approve", card);
  await r.actions.run("confirm", card);
  assert.equal(r.actions.isArmed(card.id, "approve"), false, "one armed verb at a time");
  assert.equal(r.actions.isArmed(card.id, "confirm"), true);
  await r.actions.run("done", task);
  assert.equal(r.actions.isArmed(card.id), false);
  assert.equal(r.actions.isArmed(task.id, "done"), true);
  assert.deepEqual(r.log.changes.slice(0, 2), [card.id, card.id], "the row is told when it is armed and when it is not");
  r.actions.disarm();
  assert.equal(r.timers.size, 0);
});

test("a no gives the row back at once; a yes keeps it dim until the snapshot after it; a second press while it waits is nothing", async () => {
  const r = rig();
  const row = { ...card, actions: [act("resolve")] };
  const first = r.actions.run("resolve", row);
  assert.equal(r.actions.isPending(row.id), true);
  assert.equal(await r.actions.run("resolve", row), "ignored", "one request at a time per row");
  assert.equal(r.log.requests.length, 1);
  r.answer({ ok: false, status: 409 });
  await first;
  assert.deepEqual(r.log.sounds, ["error"], "a refusal sounds like one");
  assert.equal(r.actions.isPending(row.id), false);

  const second = r.actions.run("resolve", row);
  let answered = false;
  const gate = r.answer({ ok: true, status: 200 }).then(() => { answered = true; });
  await flush();
  await gate;
  await second;
  assert.equal(answered, true);
  assert.deepEqual(r.log.sounds, ["error", "undo"]);
});

test("open and link do not touch the data; a row without them ignores the key; a verb the row does not have is ignored", async () => {
  const r = rig();
  assert.equal(await r.actions.run("open", card), "opened");
  assert.deepEqual(r.log.opened, [["fx:12", "card:fx:12"]], "the drawer is asked for the application and told which row it came from");
  assert.equal(await r.actions.run("link", card), "opened");
  assert.deepEqual(r.log.links, ["https://example.com/12"]);
  assert.deepEqual(r.log.afterLink, [card.id], "T19 asks 'did you do it?' after a link of a form the bot could not send");
  assert.equal(await r.actions.run("link", { ...card, link: null }), "ignored");
  assert.equal(await r.actions.run("open", { ...task, open: null }), "ignored");
  assert.equal(await r.actions.run("approve", task), "ignored", "a task has no approve");
  assert.equal(await r.actions.run("approve", null), "ignored", "no row, no action");
  assert.equal(r.log.requests.length, 0);
});

test("snooze asks until when if someone can ask, goes to tomorrow if nobody can, and a cancel sends nothing", async () => {
  const plain = rig();
  const a = plain.actions.run("snooze", card);
  assert.deepEqual(plain.log.requests, [{ path: "/api/snooze", body: { key: "fx:12", until: "1d" } }]);
  plain.answer();
  await a;
  assert.equal(await plain.actions.run("withdraw", { ...card, actions: [act("withdraw")] }), "ignored", "withdraw cannot guess its reason");

  const asked = rig({ ask: async (id) => (id === "snooze" ? { until: "3d" } : null) });
  const b = asked.actions.run("snooze", card);
  await flush();
  assert.deepEqual(asked.log.requests, [{ path: "/api/snooze", body: { key: "fx:12", until: "3d" } }]);
  asked.answer();
  await b;
  assert.equal(await asked.actions.run("withdraw", { ...card, actions: [act("withdraw")] }), "cancelled");
  assert.deepEqual(asked.log.asked, ["snooze", "withdraw"]);
  assert.equal(asked.log.requests.length, 1, "a cancel sends nothing");
});

test("a pick of the menus has a way back: the toast offers Deshacer, an answer or a withdrawal is the server's mark undo, a skip its unsnooze", async () => {
  const marked = { ...card, actions: [act("register"), act("withdraw"), act("snooze")] };
  const r = rig({ toast: true, ask: async (id) => (id === "register" ? { state: "interview" } : id === "withdraw" ? { note: null } : { until: "3d" }) });
  for (const [verb, said, back] of [["register", "Registrado: entrevista.", "/api/mark/undo"], ["withdraw", "", "/api/mark/undo"], ["snooze", "Pospuesta hasta el sábado", "/api/unsnooze"]]) {
    const p = r.actions.run(verb, marked);
    await flush();
    r.answer({ ok: true, status: 200, data: said ? { message: said } : {} });
    await p;
    const [text, ok, opts] = r.log.toasts.at(-1);
    assert.equal(text, said || "hoy.undo." + verb, "the server's words, or the page's own when it said nothing");
    assert.equal(ok, true);
    const undo = opts.undo();
    assert.deepEqual(r.log.requests.at(-1), { path: back, body: { key: "fx:12" } });
    r.answer();
    await undo;
    assert.equal(r.log.sounds.at(-1), "undo");
  }
  assert.equal(r.log.toasts.length, 3);
  // a no has nothing to take back, and without the shell's toast nothing offers it
  const refused = rig({ toast: true, ask: async () => ({ state: "interview" }) });
  const q = refused.actions.run("register", marked);
  await flush();
  refused.answer({ ok: false, status: 409, data: { message: "no" } });
  await q;
  assert.deepEqual(refused.log.toasts, []);
  const bare = rig({ ask: async () => ({ state: "interview" }) });
  const s = bare.actions.run("register", marked);
  await flush();
  bare.answer();
  await s;
  assert.deepEqual(bare.log.toasts, []);
});
