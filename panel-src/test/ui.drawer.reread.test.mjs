// Auditoria J2-7 and J2-8 without a browser (test/helpers/fake-dom.mjs): the drawer survives a failed re-read, and the focus goes back to who opened it.
import test from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
const { h } = await import("../src/core/dom.js");
const { createDrawer } = await import("../src/ui/drawer.js");

function rig({ keys = ["A", "B", "C"] } = {}) {
  dom.reset();
  const log = { toasts: [], writes: [] };
  const failing = new Set();
  const gates = new Map(); // key -> a promise the read waits for (to hold a read back)
  const el = h("aside", { id: "detail", tabindex: -1, inert: true });
  dom.document.body.append(el);
  const drawer = createDrawer({
    el,
    session: {
      api: async (path) => {
        const key = decodeURIComponent(path.split("key=")[1]);
        if (gates.has(key)) await gates.get(key);
        if (failing.has(key)) return { ok: false, data: null };
        return { ok: true, data: { card: { key } } };
      },
      said: (r, fallback) => fallback,
    },
    router: { write: (stop, key, opts) => { log.writes.push([stop, key, !!(opts && opts.push)]); return true; }, back() {} },
    stop: () => "procesos",
    sound: { play() {} },
    getCtx: () => ({ announce() {}, reduced: () => true }),
    jobKeys: () => keys,
    toast: (text, ok) => log.toasts.push([text, ok]),
    content: (detail) => h("div", { class: "detail-body", "data-job": detail.card.key }, h("h2", {}, "App " + detail.card.key), h("textarea", { id: "note" }), h("details", { "data-fold": "dossier", open: true }, h("summary", {}, "Dossier"))),
  });
  return { drawer, el, log, failing, gates };
}

const tick = () => new Promise((r) => setTimeout(r, 0));

test("J2-7: a re-read that fails after an action leaves the drawer open with what it had (the half-written note, the folds) and says the error", async () => {
  const { drawer, el, log, failing } = rig();
  await drawer.open("B", null);
  const body = el.querySelector(".detail-body");
  const note = el.querySelector("#note");
  note.value = "half a sentence";
  log.toasts.length = 0;
  failing.add("B");
  await drawer.reload();
  assert.equal(drawer.isOpen(), true, "still open");
  assert.equal(drawer.key, "B");
  assert.equal(el.querySelector(".detail-body"), body, "the content was not redrawn");
  assert.equal(el.querySelector("#note"), note);
  assert.equal(el.querySelector("#note").value, "half a sentence");
  assert.equal(el.querySelector("details").hasAttribute("open"), true);
  assert.equal(log.toasts.length, 1, "and it says the error");
  assert.equal(log.toasts[0][1], false);
});

test("J2-7: a j/k step whose read fails stays on the application on screen, says the error, and the next step starts from there", async () => {
  const { drawer, el, log, failing } = rig();
  await drawer.open("B", null);
  log.toasts.length = 0;
  failing.add("C");
  drawer.step(1);
  await tick();
  assert.equal(drawer.isOpen(), true);
  assert.equal(drawer.key, "B", "the drawer is still B");
  assert.equal(el.querySelector(".detail-body").getAttribute("data-job"), "B");
  assert.equal(log.toasts.length, 1);
  failing.clear();
  drawer.step(-1);
  await tick();
  assert.equal(drawer.key, "A", "k goes to the one before B, the one on screen");
});

test("J2-7: only the first opening failing leaves the drawer closed", async () => {
  const { drawer, el, log, failing } = rig();
  failing.add("B");
  await drawer.open("B", null);
  assert.equal(drawer.isOpen(), false);
  assert.equal(drawer.key, null);
  assert.equal(el.querySelector(".detail-body"), null);
  assert.equal(log.toasts.length, 1);
  assert.deepEqual(log.writes.at(-1), ["procesos", null, false], "the address does not name it");
});

test("J2-8: the focus goes back, on close, to the element that had it when open() was called, even if the focus moved while the read was on its way", async () => {
  const { drawer, el, gates } = rig();
  const opener = h("button", { id: "opener" }, "row");
  const other = h("button", { id: "other" }, "elsewhere");
  dom.document.body.append(opener, other);
  opener.focus();
  let release;
  gates.set("B", new Promise((r) => { release = r; }));
  const opening = drawer.open("B", null);
  other.focus(); // the person moves on (or the page moves the focus) while /api/job is on its way
  release();
  await opening;
  assert.equal(drawer.isOpen(), true);
  drawer.close();
  assert.equal(dom.document.activeElement, opener, "back to the one that opened it, not to the one that happened to have the focus after the read");
  assert.ok(el);
});

test("J2-8: with no element focused when it opens, the focus goes back to the origin the caller named", async () => {
  const { drawer } = rig();
  const row = h("button", { "data-card": "B" }, "B");
  dom.document.body.append(row);
  dom.document.body.focus?.();
  await drawer.open("B", row);
  drawer.close();
  assert.equal(dom.document.activeElement, row);
});
