// moments/shared/act-buttons.js without a browser (test/helpers/fake-dom.mjs): the buttons of a row stay the same nodes while only an armed press or a name changes.
import test from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
const { setLang } = await import("../src/i18n/index.js");
const { t } = await import("../src/i18n/index.js");
const { h } = await import("../src/core/dom.js");
const { patchActButtons } = await import("../src/moments/shared/act-buttons.js");

const act = (id, extra = {}) => ({ id, key: id[0], label: id.toUpperCase(), primary: false, visible: true, ...extra });

function rig() {
  setLang("es");
  dom.reset();
  const box = h("span", { class: "acts" });
  dom.document.body.append(box);
  return { box, memo: { shape: "" }, buttons: () => [...box.querySelectorAll("button[data-action]")] };
}

test("only the visible verbs are drawn, one button each, named by the room's sentence", () => {
  const { box, memo, buttons } = rig();
  patchActButtons(box, memo, [act("approve", { primary: true }), act("open", { visible: false }), act("snooze", { title: "hint" })], { armed: null, what: "Acme", ariaKey: "hoy.act.aria" });
  assert.deepEqual(buttons().map((b) => b.dataset.action), ["approve", "snooze"]);
  assert.equal(buttons()[0].getAttribute("aria-label"), t("hoy.act.aria", { action: "APPROVE", what: "Acme" }));
  assert.equal(buttons()[1].getAttribute("title"), "hint", "a verb's own hint is its title");
  assert.equal(buttons()[0].getAttribute("title"), null);
});

test("the room chooses the sentence: the same buttons are named by cola.act.aria in COLA", () => {
  const { box, memo, buttons } = rig();
  patchActButtons(box, memo, [act("approve")], { armed: null, what: "Acme", ariaKey: "cola.act.aria" });
  assert.equal(buttons()[0].getAttribute("aria-label"), t("cola.act.aria", { action: "APPROVE", what: "Acme" }));
});

test("a repaint of the same set keeps the nodes, and arming one verb changes only its text, mark and name", () => {
  const { box, memo, buttons } = rig();
  const actions = [act("approve", { primary: true }), act("snooze")];
  patchActButtons(box, memo, actions, { armed: null, what: "Acme", ariaKey: "hoy.act.aria" });
  const [approve, snooze] = buttons();
  patchActButtons(box, memo, actions, { armed: "approve", what: "Acme", ariaKey: "hoy.act.aria" });
  assert.deepEqual(buttons(), [approve, snooze], "the node he is pressing is the same node");
  assert.equal(approve.lastChild.textContent, t("act.sure"));
  assert.equal(approve.dataset.armed, "true");
  assert.equal(approve.getAttribute("aria-label"), t("hoy.act.aria", { action: t("act.sure"), what: "Acme" }), "the name says what the eyes read");
  assert.equal(snooze.dataset.armed, undefined);
  assert.equal(snooze.lastChild.textContent, "SNOOZE");
  patchActButtons(box, memo, actions, { armed: null, what: "Acme", ariaKey: "hoy.act.aria" });
  assert.equal(approve.lastChild.textContent, "APPROVE");
  assert.equal(approve.dataset.armed, undefined);
});

test("a verb that arrives or goes, or a label that changes, draws the buttons again", () => {
  const { box, memo, buttons } = rig();
  patchActButtons(box, memo, [act("approve")], { armed: null, what: "Acme", ariaKey: "hoy.act.aria" });
  const first = buttons()[0];
  patchActButtons(box, memo, [act("approve"), act("open")], { armed: null, what: "Acme", ariaKey: "hoy.act.aria" });
  assert.equal(buttons().length, 2);
  assert.notEqual(buttons()[0], first);
  const second = buttons()[0];
  patchActButtons(box, memo, [act("approve", { label: "Aprobar" }), act("open")], { armed: null, what: "Acme", ariaKey: "hoy.act.aria" });
  assert.notEqual(buttons()[0], second);
  assert.equal(buttons()[0].lastChild.textContent, "Aprobar");
});
