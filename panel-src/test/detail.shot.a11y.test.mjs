// The dry-run capture's scrolling box in the drawer, without a browser (test/helpers/fake-dom.mjs). axe-core on the demo reported
// scrollable-region-focusable on .d-shotscroll: a box that scrolls (max-height with overflow: auto, a tall capture inside) that the
// keyboard could not reach, so someone without a mouse could not read the lower part of what jobbot filled in. The box is now a
// named region in the tab order, so the arrow keys scroll it once it has the focus.
import test from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
const { setLang, t } = await import("../src/i18n/index.js");
const { memo, release } = await import("../src/ui/detail/memo.js");
const { showShot } = await import("../src/ui/detail/shot.js");

const tick = () => new Promise((r) => setTimeout(r, 0));

async function shown() {
  setLang("es");
  dom.reset();
  release();
  memo.key = "fx:1";
  const ctx = { session: { blob: async () => new Blob(["x"], { type: "image/png" }) } };
  const section = showShot(ctx, { key: "fx:1" });
  dom.document.body.append(section);
  await tick();
  return section;
}

test("the scrolling box of the capture can be reached with the keyboard and has a name", async () => {
  const section = await shown();
  const box = section.querySelector(".d-shotscroll");
  assert.ok(box, "the capture is drawn once it arrives");
  assert.equal(box.getAttribute("tabindex"), "0", "in the tab order");
  assert.equal(box.getAttribute("role"), "region", "a focusable box says what it is");
  assert.equal(box.getAttribute("aria-label"), t("drawer.shot.alt"), "and what it holds");
  release();
});

test("the zoom button still follows the box and keeps its pressed state", async () => {
  const section = await shown();
  const button = section.querySelector("button");
  assert.equal(button.getAttribute("aria-pressed"), "false");
  button.click();
  const again = section.querySelector(".d-shotscroll");
  assert.equal(again.getAttribute("tabindex"), "0", "a redraw (Ampliar) keeps it reachable");
  assert.equal(section.querySelector("button").getAttribute("aria-pressed"), "true");
  release();
});
