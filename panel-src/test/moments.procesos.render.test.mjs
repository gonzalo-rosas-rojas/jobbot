// PROCESOS' render (moments/procesos/render.js) without a browser, over test/helpers/fake-dom.mjs: what a mode draws is what a repaint touches and what a key reaches.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
const { setLang, setTimeZone } = await import("../src/i18n/index.js");
const { model } = await import("../src/core/model.js");
const { createPlace } = await import("../src/moments/procesos/place.js");
const { procesosView } = await import("../src/moments/procesos/view.js");
const { createProcesos } = await import("../src/moments/procesos/render.js");

const snap = JSON.parse(readFileSync(new URL("./fixtures/snapshot.rich.json", import.meta.url), "utf8"));
const nowMs = Date.parse(snap.generated_at);

function rig() {
  setLang("es");
  setTimeZone("America/Santiago");
  dom.reset();
  const place = createPlace();
  const memory = { filter: "all", order: "stage", query: "", mode: "field", showClosed: false };
  const states = new Map();
  const ui = {
    get: () => memory,
    set: (patch) => Object.assign(memory, patch),
    stateOf: (id) => states.get(id) || { armed: null, pending: false },
    retry() {},
    setRetry() {},
    toList() {},
  };
  const page = createProcesos({ ui, run() {}, reduced: () => true, raf() {} });
  dom.document.body.append(page.el);
  const show = (mode) => {
    memory.mode = mode;
    page.patch(procesosView(model(snap), { nowMs, place, ...memory }), { first: false });
  };
  return { page, show, states, el: page.el };
}

const rowEl = (el, id) => el.querySelector(`.trow[data-row="${id}"]`);
const pileEl = (el, id) => el.querySelector(`.cell[data-row="${id}"]`);

test("J2-2: in the table, a verb armed on a row is drawn on that row (data-armed) and a pending one too, even though the page started in the field", () => {
  const { page, show, states, el } = rig();
  show("field");
  assert.ok(pileEl(el, "card:fx:9"), "the field drew the pile");
  show("table");
  assert.ok(rowEl(el, "card:fx:9"), "the table drew the row");
  states.set("card:fx:9", { armed: "confirm", pending: false });
  page.repaint("card:fx:9");
  assert.equal(rowEl(el, "card:fx:9").dataset.armed, "confirm", "the row that is on screen shows it");
  states.set("card:fx:9", { armed: null, pending: true });
  page.repaint("card:fx:9");
  assert.equal(rowEl(el, "card:fx:9").dataset.pending, "true");
  assert.equal(rowEl(el, "card:fx:9").dataset.armed, undefined);
});

test("J2-2: in each mode the view a key reaches is the one that mode drew, and none survives the change to the other", () => {
  const { page, show, el } = rig();
  show("field");
  assert.equal("turnCell" in page.rowView("card:fx:9"), false, "a pile of the field has no turn cell");
  show("table");
  const row = page.rowView("card:fx:9");
  assert.equal("turnCell" in row, true, "the table's view of the row, not the old pile's");
  assert.ok(row.actions.some((a) => a.id === "confirm"), "with the actions of this snapshot");
  show("field");
  assert.equal("turnCell" in page.rowView("card:fx:9"), false, "and back");
  assert.ok(pileEl(el, "card:fx:9"));
});

test("J2-2: a repaint of a row that is not drawn in this mode touches nothing", () => {
  const { page, show, states, el } = rig();
  show("field");
  show("table");
  show("field");
  states.set("card:fx:9", { armed: "confirm", pending: false });
  page.repaint("card:fx:9");
  assert.equal(pileEl(el, "card:fx:9").dataset.armed, "confirm", "the pile on screen");
  assert.equal(rowEl(el, "card:fx:9"), null, "the table is not in the page");
});
