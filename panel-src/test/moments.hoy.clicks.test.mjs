// HOY's one delegated click listener (moments/hoy/clicks.js) over the fake DOM: which verb a click runs, on which row, and what it leaves alone.
import test from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
const { h } = await import("../src/core/dom.js");
const { bindClicks } = await import("../src/moments/hoy/clicks.js");

function rig() {
  dom.reset();
  const calls = [];
  const views = { "card:a": { id: "card:a" } };
  const verb = h("button", { "data-action": "approve", type: "button" });
  const label = h("span", {}, "text");
  const open = h("button", { "data-action": "open", type: "button" }, label);
  const link = h("a", { href: "#" });
  const row = h("li", { "data-row": "card:a" }, verb, open, link, h("span", { class: "role" }));
  const ghost = h("li", { "data-row": "card:gone" }, h("button", { "data-action": "approve", type: "button" }));
  const back = h("button", { "data-unsnooze": "fx:7", type: "button" });
  const more = h("button", { "data-more": "", type: "button" }, h("b", {}));
  const root = h("section", {}, row, ghost, back, more, h("p", {}));
  dom.document.body.append(root);
  bindClicks(root, {
    viewOf: (id) => views[id] || null,
    run: (actionId, view) => calls.push(["run", actionId, view.id]),
    more: () => calls.push(["more"]),
    unsnooze: (key) => calls.push(["unsnooze", key]),
  });
  return { root, row, ghost, verb, open, label, link, back, more, calls };
}

test("a verb button runs its verb on the row it is in", () => {
  const r = rig();
  r.verb.click();
  r.label.click();
  assert.deepEqual(r.calls, [["run", "approve", "card:a"], ["run", "open", "card:a"]], "a click on the button's own text is a click on the button");
});

test("a click on the row itself opens it, but not on a link, a button's gap or a row the page no longer has", () => {
  const r = rig();
  r.row.querySelector(".role").click();
  r.row.click();
  assert.deepEqual(r.calls, [["run", "open", "card:a"], ["run", "open", "card:a"]]);
  r.calls.length = 0;
  r.link.click();
  r.ghost.click();
  r.ghost.querySelector("button").click();
  assert.deepEqual(r.calls, [], "a link keeps its click; a row with no view does nothing");
});

test("the button of a snoozed card takes it back, and the 'more' button goes to PROCESOS", () => {
  const r = rig();
  r.back.click();
  r.more.querySelector("b").click();
  assert.deepEqual(r.calls, [["unsnooze", "fx:7"], ["more"]]);
});

test("a click on nothing that does anything does nothing", () => {
  const r = rig();
  r.root.querySelector("p").click();
  r.root.click();
  assert.deepEqual(r.calls, []);
});
