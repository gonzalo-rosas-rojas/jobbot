// HOY's four figures (moments/hoy/figures.js) over the fake DOM: they count up on the first drawing of the session of the page and never again, not even when the
// room is mounted again; a figure in gold and a figure of zero never count. Each test passes its own `visit`, so none depends on another.
import test from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
const { createFigures } = await import("../src/moments/hoy/figures.js");

const view = { figures: [
  { id: "read", n: 40, label: "leídos" },
  { id: "above", n: 0, label: "sobre el umbral" },
  { id: "complete", n: 7, label: "completos" },
  { id: "awaiting", n: 3, label: "esperan", gold: true },
] };

function mount({ reduced = false, visit = { counted: false } } = {}) {
  dom.reset();
  const frames = [];
  const figures = createFigures({ reduced: () => reduced, raf: (fn) => frames.push(fn), visit });
  dom.document.body.append(figures.el);
  const nodes = () => [...figures.el.querySelectorAll(".fig-n")];
  return { figures, frames, nodes, visit, numbers: () => nodes().map((n) => n.textContent) };
}

test("the first drawing of the session counts up the figures that can: not the gold one, not a zero; the count ends on the real number", () => {
  const r = mount();
  r.figures.patch(view, true);
  assert.equal(r.frames.length, 2, "40 and 7 count; 0 and the gold 3 are written as they are");
  assert.deepEqual(r.nodes().map((n) => n.dataset.counting), ["true", undefined, "true", undefined]);
  assert.deepEqual([r.numbers()[1], r.numbers()[3]], ["0", "3"]);
  for (const frame of r.frames.splice(0)) frame(performance.now() + 5000);
  assert.deepEqual(r.numbers(), ["40", "0", "7", "3"]);
  assert.deepEqual(r.nodes().map((n) => n.dataset.counting), [undefined, undefined, undefined, undefined]);
  assert.equal(r.visit.counted, true);
});

test("the first drawing of a later mount of the same session does not count up again, and marks gold only the figure that waits", () => {
  const visit = { counted: false };
  mount({ visit }).figures.patch(view, true);
  const later = mount({ visit });
  later.figures.patch(view, true);
  assert.equal(later.frames.length, 0);
  assert.deepEqual(later.numbers(), ["40", "0", "7", "3"]);
  assert.deepEqual(later.nodes().map((n) => n.dataset.gold), ["false", "false", "false", "true"]);
});

test("a repaint that is not the first drawing, and reduced motion, write the numbers as they are", () => {
  const repaint = mount();
  repaint.figures.patch(view, false);
  assert.deepEqual(repaint.numbers(), ["40", "0", "7", "3"]);
  assert.equal(repaint.visit.counted, false, "only a first drawing uses up the count-up");
  const reduced = mount({ reduced: true });
  reduced.figures.patch(view, true);
  assert.deepEqual(reduced.numbers(), ["40", "0", "7", "3"]);
  assert.equal(repaint.frames.length + reduced.frames.length, 0);
});
