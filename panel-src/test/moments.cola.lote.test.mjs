import test from "node:test";
import assert from "node:assert/strict";
import { createColaActions, enterIsLots, BATCH_MAX_MS } from "../src/moments/cola/actions.js";
import { ARM_MAX_MS, ARM_MIN_MS } from "../src/core/dom.js";

// The drawn lote ("¿Aprobar N?") closes by itself and takes Enter only where nothing else would.
function rig() {
  const log = { said: [], changes: [], requests: [] };
  let clock = 1000;
  const timers = new Map();
  let next = 1;
  const actions = createColaActions({
    session: { act: (path, body) => { log.requests.push({ path, body }); return Promise.resolve({ ok: true }); } },
    sound: { play() {} },
    announce: (text) => log.said.push(text),
    t: (key) => key,
    open() {}, go() {}, openUrl() {},
    onChange: (id) => log.changes.push(id),
    now: () => clock,
    timers: { set: (fn, ms) => { const id = next++; timers.set(id, { fn, ms }); return id; }, clear: (id) => timers.delete(id) },
    held: () => false,
  });
  return { actions, log, timers, advance: (ms) => { clock += ms; }, fire: () => { for (const [id, t] of [...timers]) { timers.delete(id); t.fn(); } } };
}
const picks = [{ key: "a", company: "A", score: 90 }, { key: "b", company: "B", score: 88 }];
const card = (key, actions = ["approve"]) => ({ key, actions });

test("the lote has a limit longer than an armed verb's, and closes itself when it passes, saying so", () => {
  const r = rig();
  assert.ok(BATCH_MAX_MS > ARM_MAX_MS, "he reads a list of names first");
  r.actions.batch.open(picks);
  const waiting = [...r.timers.values()].map((t) => t.ms);
  assert.ok(waiting.includes(BATCH_MAX_MS), "the lote's own limit is set when it opens");
  r.fire();
  assert.equal(r.actions.batch.isOpen(), false);
  assert.deepEqual(r.log.said, ["cola.batch.closed"]);
  assert.equal(r.log.changes.at(-1), "batch", "the page paints the closing");
  assert.equal(r.timers.size, 0);
});

test("confirming, cancelling or destroying the lote leaves no limit waiting, and a late confirm sends nothing", async () => {
  for (const end of [(r) => { r.advance(ARM_MIN_MS + 10); return r.actions.batch.confirm(); }, (r) => r.actions.batch.cancel(), (r) => r.actions.destroy()]) {
    const r = rig();
    r.actions.batch.open(picks);
    await end(r);
    assert.equal(r.timers.size, 0);
  }
  const r = rig();
  r.actions.batch.open(picks);
  r.fire();
  r.advance(ARM_MIN_MS + 10);
  assert.equal(await r.actions.batch.confirm(), "ignored");
  assert.deepEqual(r.log.requests, []);
});

test("a snapshot in which a pick is no longer an approvable proposal closes the lote; one that still holds them leaves it", () => {
  const r = rig();
  r.actions.batch.open(picks);
  assert.equal(r.actions.batch.sync([card("a"), card("b"), card("c")]), false);
  assert.equal(r.actions.batch.isOpen(), true);
  assert.equal(r.actions.batch.sync([card("a")]), true, "b was approved or skipped elsewhere");
  assert.equal(r.actions.batch.isOpen(), false);
  assert.deepEqual(r.log.said, ["cola.batch.closed"]);
  r.actions.batch.open(picks);
  assert.equal(r.actions.batch.sync([card("a"), card("b", ["open"])]), true, "b can no longer be approved");
  assert.equal(r.actions.batch.sync([]), false, "nothing drawn, nothing to close");
});

test("Enter is the lote's yes inside the lote or on no control; any other control keeps its Enter", () => {
  const node = (matches = []) => ({ closest: (sel) => (matches.some((m) => sel.includes(m)) ? { sel } : null) });
  const body = node();
  const inside = node(["[data-batch=\"confirm\"]"]);
  const cancel = node(["[data-batch=\"cancel\"]", "button"]);
  const opts = { inConfirm: (n) => n === inside || n === cancel, bare: [body] };
  assert.equal(enterIsLots(body, opts), true, "focus on the body");
  assert.equal(enterIsLots(null, opts), true, "no focus at all");
  assert.equal(enterIsLots(node(), opts), true, "a plain container without an action");
  assert.equal(enterIsLots(inside, opts), true, "its own confirm button");
  assert.equal(enterIsLots(cancel, opts), false, "its cancel button keeps its Enter");
  assert.equal(enterIsLots(node(["button"]), opts), false, "another button");
  assert.equal(enterIsLots(node(["a[href]"]), opts), false, "a link");
  assert.equal(enterIsLots(node(["[data-action]"]), opts), false, "a control of the drawer");
});
