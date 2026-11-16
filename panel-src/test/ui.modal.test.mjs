// The veil and the Esc key that the palette, the help and the sound sheet share (ui/modal.js), without a browser (test/helpers/fake-dom.mjs).
import test from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./helpers/fake-dom.mjs";

installFakeDom();
const { h } = await import("../src/core/dom.js");
const { modalVeil, closeOnEscape } = await import("../src/ui/modal.js");

function rig(options = {}) {
  const log = { closed: 0 };
  const inner = h("p", {}, "inside");
  const dialog = h("div", { class: "dlg" }, inner);
  const veil = modalVeil({ cls: "x-back", dialog, onClose: () => { log.closed += 1; }, ...options });
  return { log, inner, dialog, veil };
}

const wheel = (target) => {
  const event = { type: "wheel", bubbles: true, target };
  target.dispatchEvent(event);
  return event.defaultPrevented;
};

test("a press on the veil closes, a press inside the dialog does not", () => {
  const { log, veil, inner } = rig();
  assert.equal(veil.className, "x-back");
  assert.equal(veil.hasAttribute("data-lenis-prevent"), true);
  inner.dispatchEvent({ type: "mousedown", bubbles: true, target: inner });
  assert.equal(log.closed, 0);
  veil.dispatchEvent({ type: "mousedown", bubbles: true, target: veil });
  assert.equal(log.closed, 1);
});

test("the wheel over the veil is nobody's; over the dialog it scrolls it when its content does not fit", () => {
  const { veil, dialog, inner } = rig();
  dialog.scrollHeight = 900;
  dialog.clientHeight = 500;
  assert.equal(wheel(veil), true, "over the veil");
  assert.equal(wheel(inner), false, "content taller than the dialog");
  dialog.scrollHeight = 400;
  assert.equal(wheel(inner), true, "nothing to scroll: it must not move the page");
});

test("a scroller other than the dialog (the palette's list) decides when the wheel is held", () => {
  const list = h("div", { class: "list" }, h("span", {}, "row"));
  const head = h("p", {}, "head");
  const dialog = h("div", {}, head, list);
  const veil = modalVeil({ cls: "x-back", dialog, onClose() {}, scroller: list });
  list.scrollHeight = 900;
  list.clientHeight = 500;
  assert.equal(wheel(list.firstChild), false, "inside the list, which scrolls");
  assert.equal(wheel(head), true, "inside the dialog but outside the list");
  assert.equal(wheel(veil), true);
});

test("with holdsWhenItFits off, the wheel over a dialog that fits is still the dialog's (the sound sheet)", () => {
  const { dialog, inner } = rig({ holdsWhenItFits: false });
  dialog.scrollHeight = 400;
  dialog.clientHeight = 500;
  assert.equal(wheel(inner), false);
});

test("Esc closes and stops there; any other key is left alone", () => {
  let closed = 0;
  const seen = { prevented: 0, stopped: 0 };
  const event = (key) => ({ key, preventDefault: () => { seen.prevented += 1; }, stopPropagation: () => { seen.stopped += 1; } });
  assert.equal(closeOnEscape(event("a"), () => { closed += 1; }), false);
  assert.deepEqual([closed, seen.prevented, seen.stopped], [0, 0, 0]);
  assert.equal(closeOnEscape(event("Escape"), () => { closed += 1; }), true);
  assert.deepEqual([closed, seen.prevented, seen.stopped], [1, 1, 1]);
});
