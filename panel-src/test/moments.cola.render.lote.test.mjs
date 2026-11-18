// COLA's render over the fake DOM: where the keyboard goes when the drawn lote closes. The confirmation is a part of the page, so while it is open the focus
// is inside it; when it closes (cancelled, confirmed, timed out, the list changed) the focus must not be left on a part that has just gone.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
const { setLang, setTimeZone } = await import("../src/i18n/index.js");
const { model } = await import("../src/core/model.js");
const { h } = await import("../src/core/dom.js");
const { colaView } = await import("../src/moments/cola/view.js");
const { createCola } = await import("../src/moments/cola/render.js");

const snap = JSON.parse(readFileSync(new URL("./fixtures/snapshot.rich.json", import.meta.url), "utf8"));
const nowMs = Date.parse(snap.generated_at);

function rig() {
  setLang("es");
  setTimeZone("America/Santiago");
  dom.reset();
  let open = false;
  const picks = [{ key: "fx:1", company: "Acme", score: 90 }];
  const actions = { armedOf: () => null, isPending: () => false, engineArmed: () => false, batch: { isOpen: () => open, picks: () => (open ? picks : []) } };
  const page = createCola({ actions, shot: { el: h("figure"), show() {}, destroy() {} }, on: {}, reduced: () => true });
  dom.document.body.append(page.el);
  const view = colaView(model(structuredClone(snap)), { nowMs });
  page.patch(view);
  const batchBtn = page.el.querySelector('[data-action="batch"]');
  const confirmBox = page.el.querySelector(".confirm");
  const confirmOk = page.el.querySelector('[data-batch="confirm"]');
  // the fake DOM keeps no focus of its own: an element that takes it says so on the document
  const focusLog = [];
  for (const el of [batchBtn, confirmOk]) el.focus = () => { dom.document.activeElement = el; focusLog.push(el); };
  return {
    page, view, batchBtn, confirmBox, confirmOk, focusLog,
    setOpen: (on) => { open = on; page.repaint("batch"); },
  };
}

test("the lote opens: its confirmation shows, and the focus is where the page put it (nothing is stolen)", () => {
  const r = rig();
  assert.equal(r.confirmBox.hidden, true);
  r.setOpen(true);
  assert.equal(r.confirmBox.hidden, false);
  assert.deepEqual(r.focusLog, []);
});

test("the lote closes with the keyboard inside its confirmation: the focus goes back to the key that opened it, and the confirmation hides", () => {
  const r = rig();
  r.setOpen(true);
  dom.document.activeElement = r.confirmOk;
  r.setOpen(false);
  assert.equal(r.confirmBox.hidden, true);
  assert.deepEqual(r.focusLog, [r.batchBtn]);
  assert.equal(dom.document.activeElement, r.batchBtn);
});

test("the lote closes with the keyboard somewhere else: the focus stays where it was", () => {
  const r = rig();
  r.setOpen(true);
  dom.document.activeElement = r.page.el;
  r.setOpen(false);
  assert.deepEqual(r.focusLog, []);
  assert.equal(dom.document.activeElement, r.page.el);
});

test("a snapshot that arrives while the lote is open does not move the focus", () => {
  const r = rig();
  r.setOpen(true);
  dom.document.activeElement = r.confirmOk;
  r.page.patch(r.view);
  assert.deepEqual(r.focusLog, []);
  assert.equal(r.confirmBox.hidden, false);
});
