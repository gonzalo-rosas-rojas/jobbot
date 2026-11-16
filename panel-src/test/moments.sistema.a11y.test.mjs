// SISTEMA's definition lists without a browser (test/helpers/fake-dom.mjs, test/helpers/a11y.mjs). axe-core on the demo reported
// definition-list on section[data-block="run"] > .rows: the row of the steps had a <ul> where its <dd> belongs, so a dl > div held
// something that is neither dt nor dd. The steps stay a list (four items) but inside the row's dd.
import test from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./helpers/fake-dom.mjs";
import { problems } from "./helpers/a11y.mjs";

const dom = installFakeDom();
const { setLang, setTimeZone } = await import("../src/i18n/index.js");
const { createSistema } = await import("../src/moments/sistema/render.js");

const rig = () => {
  setLang("es");
  setTimeZone("America/Santiago");
  dom.reset();
  const ui = createSistema({ run() {}, open() {} });
  dom.document.body.append(ui.el);
  return ui;
};

test("every dl of SISTEMA holds only dt, dd and divs that group them", () => {
  const ui = rig();
  assert.deepEqual(problems(ui.el), []);
});

test("the run block's row of steps is a dt (the label) and a dd that holds the list of four steps", () => {
  const ui = rig();
  const row = ui.el.querySelector('[data-block="run"] .row[data-row="steps"]');
  assert.deepEqual(row.children.map((k) => k.tagName).slice(0, 2), ["DT", "DD"], "the label, then the dd with the list");
  assert.ok(row.children.every((k) => k.tagName === "DT" || k.tagName === "DD"), "nothing else in the row");
  const list = row.querySelector("dd > ul");
  assert.ok(list, "the steps are still a list, inside the dd");
  assert.deepEqual(list.children.map((li) => li.dataset.step), ["discover", "prepare", "submit", "dossiers"]);
});
