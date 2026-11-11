import test from "node:test";
import assert from "node:assert/strict";
import { createColaActions, requestOf, batchRequest, engineRequest, DEFAULT_SNOOZE, MAX_BATCH } from "../src/moments/cola/actions.js";
import { ARM_MIN_MS, ARM_MAX_MS } from "../src/core/dom.js";

// A fake session whose `act` stays open until the test lets it finish, so the state in the middle can be looked at.
function rig(options = {}) {
  const log = { requests: [], said: [], sounds: [], changes: [], opened: [], links: [], went: [], asked: [], toasts: [] };
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
  const actions = createColaActions({
    session,
    sound: { play: (name) => log.sounds.push(name) },
    announce: (text) => log.said.push(text),
    t: (key) => key,
    open: (key, id) => log.opened.push([key, id]),
    go: (stop) => log.went.push(stop),
    openUrl: (url) => log.links.push(url),
    toast: options.noToast ? null : (text, ok, o) => log.toasts.push([text, ok, o]),
    onChange: (id) => log.changes.push(id),
    now: () => clock,
    timers: { set: (fn, ms) => { const id = nextTimer++; timers.set(id, { fn, ms }); return id; }, clear: (id) => timers.delete(id) },
    held: () => !!options.held,
    ask: options.ask ? async (...args) => { log.asked.push(args[0]); return options.ask(...args); } : null,
  });
  return {
    actions, log, timers, gates,
    advance: (ms) => { clock += ms; },
    answer: (r = { ok: true, status: 200, data: { message: "done" } }) => gates.shift()(r),
    fireTimers: () => { for (const [id, t] of [...timers]) { timers.delete(id); t.fn(); } },
  };
}

const act = (id, permanent = false) => ({ id, key: id[0], label: id, primary: false, visible: true, permanent });
const card = { id: "card:fx:12", data: { card: "fx:12" }, open: "fx:12", link: "https://example.com/12", actions: [act("approve", true), act("open"), act("snooze"), act("link"), act("resolve")] };
const blocked = { id: "card:fx:10", data: { card: "fx:10" }, open: "fx:10", link: null, actions: [act("answer"), act("resolve"), act("open")] };

test("each verb makes the request the server expects, and a row only takes what it is", () => {
  assert.deepEqual(requestOf("approve", card), { path: "/api/approve", body: { key: "fx:12" } });
  assert.deepEqual(requestOf("resolve", card), { path: "/api/resolve", body: { key: "fx:12" } });
  assert.deepEqual(requestOf("unsnooze", card), { path: "/api/unsnooze", body: { key: "fx:12" } });
  assert.deepEqual(requestOf("snooze", card), { path: "/api/snooze", body: { key: "fx:12", until: DEFAULT_SNOOZE } });
  assert.deepEqual(requestOf("snooze", card, { until: "7d" }).body, { key: "fx:12", until: "7d" });
  assert.equal(requestOf("open", card), null, "opening is not a request");
  assert.equal(requestOf("approve", { data: { task: "1" } }), null, "a row that is no application is not approved");
  assert.deepEqual(batchRequest(["a", "b"]), { path: "/api/approve", body: { keys: ["a", "b"] } });
  assert.equal(batchRequest([]), null);
  assert.equal(batchRequest(Array.from({ length: MAX_BATCH + 50 }, (_, i) => "k" + i)).body.keys.length, MAX_BATCH, "never more keys than the server takes");
  assert.deepEqual(engineRequest(false), { path: "/api/stop", body: {} });
  assert.deepEqual(engineRequest(true), { path: "/api/resume", body: {} });
});

test("approve takes two presses: the first arms and says so, a second inside 350 ms is the same press, then the row is dim until the server answers", async () => {
  const r = rig();
  assert.equal(await r.actions.run("approve", card), "armed");
  assert.deepEqual([r.actions.isArmed("card:fx:12", "approve"), r.actions.armedOf("card:fx:12")], [true, "approve"]);
  assert.deepEqual(r.log.said, ["act.sure"], "said aloud for the screen reader");
  assert.equal(r.log.requests.length, 0, "nothing was sent");
  assert.equal(await r.actions.run("approve", card), "ignored", "a double click is one press, not two");
  assert.equal(r.log.requests.length, 0);
  r.advance(ARM_MIN_MS + 10);
  const second = r.actions.run("approve", card);
  assert.equal(r.log.requests.length, 1);
  assert.deepEqual(r.log.requests[0], { path: "/api/approve", body: { key: "fx:12" } });
  assert.equal(r.actions.isPending("card:fx:12"), true, "dim from the first instant");
  assert.equal(r.actions.isArmed("card:fx:12"), false);
  assert.equal(await r.actions.run("approve", card), "ignored", "a row that waits for the server takes nothing more");
  r.answer();
  assert.equal(await second, "fired");
  assert.equal(r.actions.isPending("card:fx:12"), false);
  assert.deepEqual(r.log.sounds, ["arm", "confirm", "approve"]);
});

test("a held key is never the second press", async () => {
  const r = rig({ held: true });
  assert.equal(await r.actions.run("approve", card), "armed");
  r.advance(ARM_MIN_MS + 10);
  assert.equal(await r.actions.run("approve", card), "ignored");
  assert.equal(r.log.requests.length, 0);
});

test("an armed press lapses after five seconds, and another verb disarms it", async () => {
  const r = rig();
  await r.actions.run("approve", card);
  assert.ok([...r.timers.values()].some((t) => t.ms === ARM_MAX_MS));
  r.fireTimers();
  assert.equal(r.actions.isArmed("card:fx:12"), false);
  await r.actions.run("approve", card);
  await r.actions.run("link", card);
  assert.equal(r.actions.isArmed("card:fx:12"), true, "the link of the same row is not a decision");
  const other = r.actions.run("resolve", card);
  r.answer({ ok: true, status: 200, data: {} });
  await other;
  assert.equal(r.actions.isArmed("card:fx:12"), false, "doing something else on the row drops the armed approval");
});

test("a no gives the row back: it is not dim, it sounds like an error, and the approval is not repeated by itself", async () => {
  const r = rig();
  await r.actions.run("approve", card);
  r.advance(ARM_MIN_MS + 10);
  const p = r.actions.run("approve", card);
  assert.equal(r.actions.isPending("card:fx:12"), true);
  r.answer({ ok: false, status: 409, data: { message: "no" } });
  assert.equal(await p, "fired");
  assert.equal(r.actions.isPending("card:fx:12"), false);
  assert.deepEqual(r.log.sounds, ["arm", "confirm", "error"]);
  assert.equal(r.actions.isArmed("card:fx:12"), false, "he must press twice again");
});

test("skipping asks until when, sends the answer, and offers the way back; a closed menu sends nothing", async () => {
  const r = rig({ ask: async () => ({ until: "3d" }) });
  const p = r.actions.run("snooze", card);
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(r.log.asked, ["snooze"]);
  assert.deepEqual(r.log.requests, [{ path: "/api/snooze", body: { key: "fx:12", until: "3d" } }]);
  r.answer({ ok: true, status: 200, data: { message: "Pospuesta hasta el sábado" } });
  await p;
  assert.equal(r.log.toasts.length, 1);
  assert.equal(r.log.toasts[0][0], "Pospuesta hasta el sábado");
  assert.equal(r.log.toasts[0][1], true);
  assert.equal(typeof r.log.toasts[0][2].undo, "function", "the toast carries 'Deshacer'");
  const back = r.log.toasts[0][2].undo();
  assert.deepEqual(r.log.requests.at(-1), { path: "/api/unsnooze", body: { key: "fx:12" } }, "the way back is the server's unsnooze");
  r.answer();
  await back;
  assert.deepEqual(r.log.sounds, ["snooze", "undo"]);

  const closed = rig({ ask: async () => null });
  assert.equal(await closed.actions.run("snooze", card), "cancelled");
  assert.equal(closed.log.requests.length, 0);
  const bare = rig({ noToast: true });
  const q = bare.actions.run("snooze", card);
  assert.equal(bare.log.requests[0].body.until, DEFAULT_SNOOZE, "without a menu it is until tomorrow");
  bare.answer();
  await q;
  assert.deepEqual(bare.log.toasts, []);
});

test("a refused skip has no way back to offer", async () => {
  const r = rig({ ask: async () => ({ until: "1d" }) });
  const p = r.actions.run("snooze", card);
  await new Promise((resolve) => setImmediate(resolve));
  r.answer({ ok: false, status: 409, data: { message: "no" } });
  await p;
  assert.deepEqual(r.log.toasts, []);
});

test("open, link and answer do not touch the server; answer takes him to APRENDER", async () => {
  const r = rig();
  assert.equal(await r.actions.run("open", card), "opened");
  assert.deepEqual(r.log.opened, [["fx:12", "card:fx:12"]]);
  assert.equal(await r.actions.run("link", card), "opened");
  assert.deepEqual(r.log.links, ["https://example.com/12"]);
  assert.equal(await r.actions.run("link", { ...card, link: null }), "ignored", "no address, no link");
  assert.equal(await r.actions.run("answer", blocked), "answering");
  assert.deepEqual(r.log.went, ["aprender"]);
  assert.equal(await r.actions.run("approve", blocked), "ignored", "a blocked row cannot be approved");
  assert.equal(await r.actions.run("nope", card), "ignored");
  assert.equal(await r.actions.run("open", null), "ignored");
  assert.equal(r.log.requests.length, 0);
});

test("the lote is drawn first and sent as ONE request with every key after the confirmation", async () => {
  const r = rig();
  const picks = [{ key: "a", company: "A", score: 92 }, { key: "b", company: "B", score: 89 }];
  assert.equal(r.actions.batch.open([]), false, "nothing to approve, nothing to draw");
  assert.equal(r.actions.batch.open(picks), true);
  assert.equal(r.actions.batch.isOpen(), true);
  assert.deepEqual(r.actions.batch.picks().map((p) => p.key), ["a", "b"]);
  assert.ok(r.log.changes.includes("batch"));
  assert.equal(await r.actions.batch.confirm(), "ignored", "↵ held or at the same instant as Shift+A is not the second press");
  assert.equal(r.actions.batch.isOpen(), true);
  r.advance(ARM_MIN_MS + 10);
  const p = r.actions.batch.confirm();
  assert.equal(r.actions.batch.isOpen(), false);
  assert.deepEqual(r.log.requests, [{ path: "/api/approve", body: { keys: ["a", "b"] } }]);
  assert.deepEqual([r.actions.isPending("card:a"), r.actions.isPending("card:b")], [true, true], "every row of the lote is dim");
  r.answer();
  assert.equal(await p, "fired");
  assert.deepEqual([r.actions.isPending("card:a"), r.actions.isPending("card:b")], [false, false]);
  assert.deepEqual(r.log.sounds, ["arm", "confirm", "approve"]);
  assert.equal(await r.actions.batch.confirm(), "ignored", "nothing is waiting any more");
});

test("the lote: Esc drops it, a 409 gives the rows back, and a held ↵ never confirms", async () => {
  const r = rig();
  r.actions.batch.open([{ key: "a", company: "A", score: 90 }]);
  assert.equal(r.actions.batch.cancel(), true);
  assert.equal(r.actions.batch.isOpen(), false);
  assert.equal(r.actions.batch.cancel(), false);
  r.actions.batch.open([{ key: "a", company: "A", score: 90 }, { key: "b", company: "B", score: 90 }]);
  r.advance(ARM_MIN_MS + 10);
  const p = r.actions.batch.confirm();
  r.answer({ ok: false, status: 409, data: { message: "una no pasó" } });
  await p;
  assert.deepEqual([r.actions.isPending("card:a"), r.actions.isPending("card:b")], [false, false]);
  assert.deepEqual(r.log.sounds, ["arm", "arm", "confirm", "error"]);
  const h = rig({ held: true });
  h.actions.batch.open([{ key: "a", company: "A", score: 90 }]);
  h.advance(ARM_MIN_MS + 10);
  assert.equal(await h.actions.batch.confirm(), "ignored");
  assert.equal(h.actions.batch.isOpen(), true);
  // a row that is already waiting for the server is not sent twice
  const d = rig();
  d.actions.batch.open([{ key: "a", company: "A", score: 90 }]);
  d.advance(ARM_MIN_MS + 10);
  const first = d.actions.run("approve", { ...card, id: "card:a", data: { card: "a" }, actions: [act("approve")] });
  assert.equal(await d.actions.batch.confirm(), "ignored", "its only key is already on its way");
  d.answer();
  await first;
});

test("the engine: stopping goes at once, resuming takes two presses (SISTEMA's rule), and the switch is given back", async () => {
  const r = rig();
  const p = r.actions.engine(false);
  assert.deepEqual(r.log.requests, [{ path: "/api/stop", body: {} }], "one press stops: it is the safe direction");
  assert.equal(r.actions.isPending("engine"), true);
  assert.equal(r.actions.engineArmed(), false);
  r.answer();
  assert.equal(await p, "fired");
  assert.deepEqual([r.actions.isPending("engine"), r.actions.engineArmed()], [false, false]);
  assert.equal(await r.actions.engine(true), "armed");
  assert.equal(r.actions.engineArmed(), true);
  assert.equal(r.log.requests.length, 1, "the first press to resume sends nothing");
  r.advance(ARM_MIN_MS + 10);
  const q = r.actions.engine(true);
  assert.equal(r.log.requests.at(-1).path, "/api/resume");
  r.answer({ ok: false, status: 500, data: { message: "falló" } });
  await q;
  assert.deepEqual(r.log.sounds, ["stop", "arm", "confirm", "error"]);
});

test("destroy leaves nothing armed, dim or drawn", async () => {
  const r = rig();
  await r.actions.run("approve", card);
  r.actions.batch.open([{ key: "a", company: "A", score: 90 }]);
  r.actions.destroy();
  assert.deepEqual([r.actions.isArmed("card:fx:12"), r.actions.batch.isOpen(), r.timers.size], [false, false, 0]);
});
