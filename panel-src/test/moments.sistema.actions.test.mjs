import test from "node:test";
import assert from "node:assert/strict";
import { createActions, REQUESTS } from "../src/moments/sistema/actions.js";
import { ARM_MIN_MS, ARM_MAX_MS } from "../src/core/dom.js";

const flush = () => new Promise((resolve) => setImmediate(resolve));

// A fake session whose `act` stays open until the test answers, so the state in the middle can be looked at.
function rig(options = {}) {
  const log = { requests: [], said: [], sounds: [], changes: 0, toasts: [] };
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
    toast: options.noToast ? null : (text, ok, extra) => log.toasts.push({ text, ok, undo: extra && extra.undo }),
    onChange: () => { log.changes += 1; },
    now: () => clock,
    timers: { set: (fn, ms) => { const id = nextTimer++; timers.set(id, { fn, ms }); return id; }, clear: (id) => timers.delete(id) },
    held: () => !!options.held,
  });
  return {
    actions, log, timers,
    advance: (ms) => { clock += ms; },
    answer: (r = { ok: true, status: 200 }) => gates.shift()(r),
    fireTimers: () => { for (const [id, t] of [...timers]) { timers.delete(id); t.fn(); } },
  };
}

const RUNNING = { on: false };      // what view.stop says while the engine runs
const STOPPED = { on: true };

test("the two requests are the real routes, with an empty body", () => {
  assert.deepEqual(REQUESTS, { stop: { path: "/api/stop" }, resume: { path: "/api/resume" } });
});

test("STOP is instant: one press sends it, the page shows it at once, the dim lasts until the server answers, and it can be taken back from the toast", async () => {
  const r = rig();
  assert.equal(await Promise.race([r.actions.run(RUNNING), flush().then(() => "waiting")]), "waiting", "run waits for the server");
  assert.deepEqual(r.log.requests, [{ path: "/api/stop", body: {} }], "no arming, no confirmation: stopping is the safe direction");
  assert.deepEqual(r.actions.state(), { intent: true, pending: true, armed: false }, "STOP is already on the page, and the button is dim");
  await r.answer();
  await flush();
  assert.deepEqual(r.actions.state(), { intent: true, pending: false, armed: false }, "it keeps showing STOP until a snapshot says so too");
  assert.deepEqual(r.log.sounds, ["stop"]);
  assert.equal(r.log.toasts.length, 1);
  assert.deepEqual([r.log.toasts[0].text, r.log.toasts[0].ok], ["sistema.stop.done", true]);
  assert.equal(typeof r.log.toasts[0].undo, "function", "the toast carries 'deshacer'");
  assert.deepEqual(r.log.said, [], "with a toast the toast is what is read aloud: nothing twice");
});

test("a snapshot that arrives after the answer is the truth: it takes the intent away; one that arrives during the request does not", async () => {
  const r = rig();
  const run = r.actions.run(RUNNING);
  r.actions.settle();
  assert.equal(r.actions.state().intent, true, "the snapshot of the refresh that follows the 200 still comes before `act` returns");
  await r.answer();
  await run;
  r.actions.settle();
  assert.equal(r.actions.state().intent, null);
});

test("quitting STOP asks twice: the first press arms it and says so, a second inside 350 ms is the same press, a held key is not a second, and the arming lapses", async () => {
  const r = rig();
  assert.equal(await r.actions.run(STOPPED), "armed");
  assert.deepEqual(r.actions.state(), { intent: null, pending: false, armed: true });
  assert.deepEqual(r.log.said, ["act.sure"]);
  assert.deepEqual(r.log.requests, []);
  assert.equal(await r.actions.run(STOPPED), "ignored", "a double click is one press");
  assert.equal(r.actions.state().armed, true);
  r.advance(ARM_MIN_MS + 50);
  const held = rig({ held: true });
  await held.actions.run(STOPPED);
  held.advance(ARM_MIN_MS + 50);
  assert.equal(await held.actions.run(STOPPED), "ignored", "a repeating key is not a decision");
  assert.deepEqual(held.log.requests, []);
  const second = r.actions.run(STOPPED);
  await flush();
  assert.deepEqual(r.log.requests, [{ path: "/api/resume", body: {} }]);
  assert.deepEqual(r.actions.state(), { intent: false, pending: true, armed: false });
  await r.answer();
  assert.equal(await second, "fired");
  assert.deepEqual(r.log.sounds, ["arm", "confirm", "resume"]);
  assert.equal(r.log.toasts[0].text, "sistema.resume.done");
  const lapse = rig();
  await lapse.actions.run(STOPPED);
  assert.deepEqual([...lapse.timers.values()].map((x) => x.ms), [ARM_MAX_MS]);
  lapse.fireTimers();
  assert.equal(lapse.actions.state().armed, false, "five seconds later it is as if nothing was pressed");
  lapse.advance(ARM_MIN_MS + 50);
  assert.equal(await lapse.actions.run(STOPPED), "armed", "and the next press starts again");
});

test("a no gives the state back: a 409, a 500 or no network leave the page as it was, sound like an error and can be tried again; the toast with the reason is the session's", async () => {
  for (const status of [409, 500, 0]) {
    const r = rig();
    const run = r.actions.run(RUNNING);
    await r.answer({ ok: false, status });
    assert.equal(await run, "fired");
    assert.deepEqual(r.actions.state(), { intent: null, pending: false, armed: false }, String(status));
    assert.deepEqual(r.log.sounds, ["error"], String(status));
    assert.deepEqual(r.log.toasts, [], "the session already said why");
    const again = r.actions.run(RUNNING);
    await flush();
    assert.equal(r.log.requests.length, 2, "it can be tried again");
    await r.answer();
    await again;
  }
});

test("a press while the server is still answering is ignored: one request, never two", async () => {
  const r = rig();
  const first = r.actions.run(RUNNING);
  assert.equal(await r.actions.run(RUNNING), "ignored");
  assert.equal(await r.actions.run(STOPPED), "ignored", "not even the opposite one");
  assert.equal(r.log.requests.length, 1);
  await r.answer();
  await first;
});

test("'deshacer' sends the opposite at once, with no arming (it is his own undo), and works the other way round", async () => {
  const r = rig();
  const stop = r.actions.run(RUNNING);
  await r.answer();
  await stop;
  const undo = r.log.toasts[0].undo();
  await flush();
  assert.deepEqual(r.log.requests[1], { path: "/api/resume", body: {} }, "undoing STOP quits STOP without asking twice");
  await r.answer();
  await undo;
  assert.equal(r.log.toasts[1].text, "sistema.resume.done");
  const back = rig();
  await back.actions.run(STOPPED);
  back.advance(ARM_MIN_MS + 50);
  const resume = back.actions.run(STOPPED);
  await back.answer();
  await resume;
  back.log.toasts[0].undo();
  await flush();
  assert.deepEqual(back.log.requests[1], { path: "/api/stop", body: {} });
  await back.answer();
});

test("without a toast the sentence is said aloud instead, and destroy leaves no timer behind", async () => {
  const r = rig({ noToast: true });
  const run = r.actions.run(RUNNING);
  await r.answer();
  await run;
  assert.deepEqual(r.log.said, ["sistema.stop.done"]);
  const armed = rig();
  await armed.actions.run(STOPPED);
  assert.equal(armed.timers.size, 1);
  armed.actions.destroy();
  assert.equal(armed.timers.size, 0);
  assert.deepEqual(armed.actions.state(), { intent: null, pending: false, armed: false });
});
