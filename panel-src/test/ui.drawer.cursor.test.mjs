// Auditoria J2-1, J2-7, J2-8 and the shell's contract with them, without a browser (test/helpers/fake-dom.mjs): while the drawer is open the cursor of the
// list is the row of the application it shows, so the keys of a row (s, x, c, r...) never act on another one; a failed re-read leaves the drawer as it
// was; and the focus goes back to whoever opened it.
import test from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
const { h } = await import("../src/core/dom.js");
const { createCursor } = await import("../src/ui/cursor.js");
const { createDrawer } = await import("../src/ui/drawer.js");

const rowsHost = (keys) => {
  dom.reset();
  const host = h("main", { id: "app" }, h("ul", {}, keys.map((k) => h("li", {}, h("button", { class: "cell", "data-card": k, "data-row": "card:" + k }, k)))));
  dom.document.body.append(host);
  return host;
};
const rowOf = (host, key) => host.querySelector(`[data-card="${key}"]`);

function cursorRig(keys) {
  const host = rowsHost(keys);
  const view = { cursor: null };
  const cursor = createCursor({ view, sound: { play() {} }, reduced: () => true, root: () => host });
  return { host, view, cursor };
}

test("J2-1: follow(key) puts the cursor on that application's row", () => {
  const { host, cursor } = cursorRig(["A", "B", "C"]);
  cursor.set("c:A");
  assert.equal(cursor.current(), rowOf(host, "A"));
  cursor.follow("B");
  assert.equal(cursor.key, "c:B");
  assert.equal(cursor.current(), rowOf(host, "B"), "the keys of a row now reach B");
  assert.equal(rowOf(host, "B").getAttribute("data-cursor"), "true");
  assert.equal(rowOf(host, "A").hasAttribute("data-cursor"), false);
});

test("J2-1: an application whose row is not drawn (a filter hides it, or it belongs to another stop) leaves the cursor on NO row, not on the old one", () => {
  const { host, cursor, view } = cursorRig(["A", "B"]);
  cursor.set("c:A");
  cursor.follow("Z");
  assert.equal(cursor.current(), null, "the keys of a row do not act on A while the drawer shows Z");
  assert.equal(view.cursor, null);
  assert.equal(host.querySelector('[data-cursor="true"]'), null);
});

// HOY: a task's row stands for its application but carries the task's id, not the application's key.
function taskRig() {
  dom.reset();
  const task = h("li", { class: "row", "data-task": "7", "data-row": "task:7" }, h("button", { "data-action": "open" }, "open"));
  const card = h("li", { class: "row", "data-card": "A", "data-row": "card:A" }, "A");
  const host = h("main", { id: "app" }, h("ul", {}, task, card));
  dom.document.body.append(host);
  const view = { cursor: null };
  const cursor = createCursor({ view, sound: { play() {} }, reduced: () => true, root: () => host });
  return { host, task, card, view, cursor };
}

test("an application opened from the row the cursor is on leaves the cursor there, even when that row is a task's (given as the element, a child of it, or its data-row id)", () => {
  for (const origin of [(r) => r.task, (r) => r.task.querySelector("button"), () => "task:7"]) {
    const rig = taskRig();
    rig.cursor.set("t:7");
    rig.cursor.follow("Z", origin(rig));
    assert.equal(rig.cursor.current(), rig.task, "the task's row keeps the cursor: its keys are about the application the drawer shows");
  }
});

test("an application opened from somewhere else (another row, the palette, an address) still takes the cursor off a task's row", () => {
  const rig = taskRig();
  rig.cursor.set("t:7");
  rig.cursor.follow("A", rig.card);
  assert.equal(rig.cursor.current(), rig.card, "opened from A's row: the cursor is A's");
  rig.cursor.set("t:7");
  rig.cursor.follow("Z");
  assert.equal(rig.cursor.current(), null, "opened with no row behind it and no row of its own: no row takes the keys");
});

function drawerRig({ keys = ["A", "B", "C"], failOn = [] } = {}) {
  const failing = new Set(failOn);
  const log = { shown: [], toasts: [], writes: [] };
  const el = h("aside", { id: "detail", tabindex: -1, inert: true });
  dom.document.body.append(el);
  const drawer = createDrawer({
    el,
    session: {
      api: async (path) => {
        const key = decodeURIComponent(path.split("key=")[1]);
        log.reads = (log.reads || 0) + 1;
        if (failing.has(key)) return { ok: false, data: null };
        return { ok: true, data: { card: { key }, note: "" } };
      },
      said: (r, fallback) => fallback,
    },
    router: { write: (stop, key, opts) => { log.writes.push([stop, key, !!(opts && opts.push)]); return true; }, back() {} },
    stop: () => "procesos",
    sound: { play() {} },
    getCtx: () => ({ announce() {}, reduced: () => true }),
    jobKeys: () => keys,
    toast: (text, ok) => log.toasts.push([text, ok]),
    onShow: (key) => log.shown.push(key),
    content: (detail) => h("div", { class: "detail-body", "data-job": detail.card.key }, h("h2", {}, "App " + detail.card.key), h("textarea", { id: "note" })),
  });
  return { drawer, el, log, failing };
}

test("J2-1: whichever way the drawer opens an application (a click, the palette, an address) or moves to another (j/k), it announces the one it now shows", async () => {
  dom.reset();
  const { drawer, log } = drawerRig();
  await drawer.open("B", null); // a click on row B, the palette or #procesos/B all end here
  assert.deepEqual(log.shown, ["B"]);
  await drawer.open("C", null);
  assert.deepEqual(log.shown, ["B", "C"]);
  drawer.step(1);
  await new Promise((r) => setTimeout(r, 0));
  assert.deepEqual(log.shown, ["B", "C", "A"], "j wraps to the first");
});

test("a re-read of the application the drawer already shows announces nothing: the cursor stays where it is", async () => {
  dom.reset();
  const { drawer, log } = drawerRig();
  await drawer.open("B", null);
  await drawer.reload();
  await drawer.open("B", null);
  assert.deepEqual(log.shown, ["B"]);
});

test("J2-1: the first read failing opens nothing and moves no cursor", async () => {
  dom.reset();
  const { drawer, log } = drawerRig({ failOn: ["B"] });
  await drawer.open("B", null);
  assert.deepEqual(log.shown, []);
  assert.equal(drawer.isOpen(), false);
});

