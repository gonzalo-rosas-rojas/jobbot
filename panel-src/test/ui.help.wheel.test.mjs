// Auditoria J2-9 without a browser (test/helpers/fake-dom.mjs): the help sheet is a dialog like the palette and the sound sheet, so the wheel over it or its veil never reaches the
// camera behind (Lenis ignores `data-lenis-prevent` and the browser scrolls what the wheel is over), while the sheet itself scrolls when its content does not fit.
import test from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
const { createHelp } = await import("../src/ui/help.js");

function opened() {
  dom.reset();
  const help = createHelp({ root: dom.document.body, sound: { play() {} }, getStop: () => "procesos" });
  help.open();
  const box = dom.document.body.querySelector(".help-back");
  const dialog = box.querySelector(".help");
  return { help, box, dialog };
}

const wheel = (target) => {
  const event = { type: "wheel", bubbles: true, target };
  target.dispatchEvent(event);
  return event.defaultPrevented;
};

test("J2-9: the veil of the help is data-lenis-prevent and a wheel over it does not move the page behind", () => {
  const { box, dialog } = opened();
  assert.equal(box.hasAttribute("data-lenis-prevent"), true);
  dialog.scrollHeight = 900;
  dialog.clientHeight = 500;
  assert.equal(wheel(box), true, "over the veil: nobody's");
});

test("J2-9: over the sheet, the wheel scrolls it when its content does not fit and is nobody's when it fits", () => {
  const { dialog } = opened();
  dialog.scrollHeight = 900;
  dialog.clientHeight = 500;
  assert.equal(wheel(dialog.querySelector(".help-head")), false, "content taller than the sheet: the sheet scrolls");
  dialog.scrollHeight = 400;
  assert.equal(wheel(dialog.querySelector(".help-head")), true, "nothing to scroll: it must not move the page");
});
