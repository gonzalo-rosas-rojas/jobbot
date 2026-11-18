// How the drawer closes, in its three ways, without a browser (test/helpers/fake-dom.mjs): Esc or the button (the focus goes back and the history
// entry the drawer added is taken back), Back (the address already moved: the focus goes back, the history is left alone) and a change of stop
// (closeQuietly: no focus, no history). And what the first opening, a step and a re-read do with the focus and the address.
import test from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
const { h } = await import("../src/core/dom.js");
const { createDrawer } = await import("../src/ui/drawer.js");

function rig() {
  dom.reset();
  const log = { router: [], sounds: [], open: [], said: [] };
  const el = h("aside", { id: "detail", tabindex: -1, inert: true });
  dom.document.body.append(el);
  const opener = h("button", { id: "opener" }, "row");
  dom.document.body.append(opener);
  const drawer = createDrawer({
    el,
    session: { api: async (path) => ({ ok: true, data: { card: { key: decodeURIComponent(path.split("key=")[1]) } } }), said: (r, fallback) => fallback },
    router: {
      pushStop: (stop, key) => { log.router.push(["write", stop, key, true]); return true; },
      replaceStop: (stop, key) => { log.router.push(["write", stop, key, false]); return false; },
      back: () => log.router.push(["back"]),
    },
    stop: () => "procesos",
    sound: { play: (cue) => log.sounds.push(cue) },
    getCtx: () => ({ announce: (text) => log.said.push(text), reduced: () => true }),
    jobKeys: () => ["A", "B", "C"],
    toast() {},
    onOpenChange: (open) => log.open.push(open),
    content: (detail) => h("div", { class: "detail-body", "data-job": detail.card.key }, h("h2", {}, "App " + detail.card.key), h("button", { id: "inside" }, "x")),
  });
  opener.focus();
  return { drawer, el, log, opener };
}

test("the first opening adds one history entry, moves the focus into the drawer and tells the shell", async () => {
  const { drawer, el, log } = rig();
  await drawer.open("A", null);
  assert.equal(drawer.isOpen(), true);
  assert.equal(dom.document.activeElement, el);
  assert.deepEqual(log.router, [["write", "procesos", "A", true]]);
  assert.deepEqual(log.open, [true]);
  assert.deepEqual(log.sounds, ["open"]);
});

test("closeAndRestore() (Esc, the button): the focus goes back to what opened it and the history entry is taken back", async () => {
  const { drawer, el, log, opener } = rig();
  await drawer.open("A", null);
  log.router.length = 0;
  drawer.closeAndRestore();
  assert.equal(drawer.isOpen(), false);
  assert.equal(el.inert, true);
  assert.equal(dom.document.activeElement, opener);
  assert.deepEqual(log.router, [["back"]]);
  assert.deepEqual(log.open, [true, false]);
  assert.deepEqual(log.sounds, ["open", "close"]);
});

test("closeFromHistory() (Back): the focus goes back, the address is only rewritten, never stepped back again", async () => {
  const { drawer, log, opener } = rig();
  await drawer.open("A", null);
  log.router.length = 0;
  drawer.closeFromHistory();
  assert.equal(dom.document.activeElement, opener);
  assert.deepEqual(log.router, [["write", "procesos", null, false]]);
});

test("closeQuietly() (the stop changes): no focus moves and the history is left to whoever moved it", async () => {
  const { drawer, el, log, opener } = rig();
  await drawer.open("A", null);
  log.router.length = 0;
  const before = dom.document.activeElement;
  drawer.closeQuietly();
  assert.equal(drawer.isOpen(), false);
  assert.equal(dom.document.activeElement, before, "the focus stays exactly where it was");
  assert.notEqual(before, opener);
  assert.deepEqual(log.router, []);
  assert.deepEqual(log.sounds, ["open", "close"]);
  drawer.closeAndRestore();
  assert.deepEqual(log.router, [], "closing what is closed does nothing");
  assert.equal(el.classList.contains("open"), false);
});

test("a close right after a step goes back to the row of the application on screen, not the one it was opened from", async () => {
  const { drawer, log } = rig();
  const rowB = h("div", { "data-card": "B", tabindex: -1 }, "B");
  dom.document.body.append(rowB);
  await drawer.open("A", null);
  drawer.step(1);
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(drawer.key, "B");
  assert.deepEqual(log.router.at(-1), ["write", "procesos", "B", false], "a step rewrites the address, it adds no entry");
  assert.deepEqual(log.said, ["App B"], "and says the title of the new one");
  drawer.closeAndRestore();
  assert.equal(dom.document.activeElement, rowB);
});
