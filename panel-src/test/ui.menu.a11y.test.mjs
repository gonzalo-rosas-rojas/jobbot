// The small menu's accessible structure without a browser (test/helpers/fake-dom.mjs, test/helpers/a11y.mjs). axe-core on the demo reported
// aria-required-children on .menu and aria-required-parent on its four items: the `menu` role sat on the box that also holds the title and
// the foot, and the items were inside list items that said they were list items. The menu is the list of items; the box around it is a
// labelled group, and what the mouse and the keys do is the same as before.
import test from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./helpers/fake-dom.mjs";
import { problems } from "./helpers/a11y.mjs";

const dom = installFakeDom();
globalThis.window = { innerWidth: 1280, innerHeight: 800 }; // only for placing the box beside what opened it
const { createMenu } = await import("../src/ui/menu.js");

const items = (picked = []) => ["1", "2", "3", "4"].map((key) => ({ key, label: "Opcion " + key, run: () => picked.push(key) }));
const rig = () => { dom.reset(); return createMenu({ sound: { play() {} }, root: () => dom.document.body }); };

test("the items live in a menu that holds only items, and every item has that menu as its parent", () => {
  const menu = rig();
  const box = menu.open({ title: "Posponer hasta cuándo", items: items() });
  assert.deepEqual(problems(box), []);
  const list = box.querySelector('[role="menu"]');
  assert.ok(list, "there is a menu");
  assert.equal(list.getAttribute("aria-label"), "Posponer hasta cuándo", "named by the question it asks");
  assert.equal(list.querySelectorAll('[role="menuitem"]').length, 4);
  assert.equal(box.querySelectorAll('[role="menu"]').length, 1, "one menu, not the box and the list");
  assert.notEqual(box.getAttribute("role"), "menu", "the box also holds the title and the foot, which a menu may not");
});

test("the box around the menu is a labelled group, and the list items are out of the way of the reader", () => {
  const menu = rig();
  const box = menu.open({ title: "Cuál respuesta", items: items() });
  assert.equal(box.getAttribute("role"), "group");
  assert.equal(box.getAttribute("aria-label"), "Cuál respuesta");
  for (const li of box.querySelectorAll("li")) assert.equal(li.getAttribute("role"), "none", "an li between a menu and its items is not announced");
});

test("keys and focus are unchanged: focus on the first item, ArrowDown walks, a digit picks and closes", () => {
  const menu = rig();
  const picked = [];
  const box = menu.open({ title: "t", items: items(picked) });
  const buttons = box.querySelectorAll(".menu-item");
  assert.equal(dom.document.activeElement, buttons[0]);
  box.dispatchEvent({ type: "keydown", key: "ArrowDown", bubbles: true });
  assert.equal(dom.document.activeElement, buttons[1]);
  box.dispatchEvent({ type: "keydown", key: "3", bubbles: true });
  assert.deepEqual(picked, ["3"]);
  assert.equal(menu.isOpen(), false);
});

test("a menu with a form (body) stays a dialog with the form in it", () => {
  const menu = rig();
  const body = dom.document.createElement("form");
  const box = menu.open({ title: "Nueva tarea", body });
  assert.equal(box.getAttribute("role"), "dialog");
  assert.equal(box.getAttribute("aria-label"), "Nueva tarea");
  assert.deepEqual(problems(box), []);
});

test("with the focus on no item, ArrowDown lands on the first and ArrowUp on the last", () => {
  const menu = rig();
  const box = menu.open({ title: "t", items: items() });
  const buttons = box.querySelectorAll(".menu-item");
  dom.document.activeElement = dom.document.body; // the focus left the items (a click on the box's title, say)
  box.dispatchEvent({ type: "keydown", key: "ArrowUp", bubbles: true });
  assert.equal(dom.document.activeElement, buttons[buttons.length - 1], "up from nowhere is the last item, not the one before it");
  dom.document.activeElement = dom.document.body;
  box.dispatchEvent({ type: "keydown", key: "ArrowDown", bubbles: true });
  assert.equal(dom.document.activeElement, buttons[0]);
});
