// The four figures of the night in HOY (what the engine read, what passed, what is complete, what waits for his yes). They count up once per session of the page
// (§5.1 "Anima"), never again and never in gold. Built once, patched after.
//
//   const figures = createFigures({ reduced, raf, visit });   figures.el (dl.figs)   figures.patch(view, first)
// `first` is the first drawing of this mount: only then (and only if no earlier mount counted) the numbers animate; `raf` is requestAnimationFrame, handed in for tests.
// `visit` ({ counted }) remembers whether the count-up already happened; by default it is the page's own, one for the whole session, and a test passes its own.

import { h, setText } from "../../core/dom.js";
import { setData } from "../shared/util.js";

const COUNT_MS = 700;
const easeOutCubic = (p) => 1 - Math.pow(1 - p, 3);
const SESSION = { counted: false }; // module state on purpose: the count-up happens once per session of the page, not once per mount of the room

export function createFigures({ reduced, raf, visit = SESSION }) {
  const el = h("dl", { class: "figs" });
  const figEls = ["read", "above", "complete", "awaiting"].map((id) => ({
    id, n: h("dd", { class: "fig-n cond num" }), label: h("dt", { class: "fig-l label" }),
  }));
  // dl > div > dt + dd: the label first in the markup (what a screen reader reads first), the number first on the screen (CSS order)
  figEls.forEach((f) => el.append(h("div", { class: "fig", "data-fig": f.id }, f.label, f.n)));

  function countUp(node, n) {
    const start = performance.now();
    node.dataset.counting = "true";
    const step = (now) => {
      const p = Math.min(1, (now - start) / COUNT_MS);
      node.textContent = String(Math.round(n * easeOutCubic(p)));
      if (p < 1 && node.isConnected) raf(step);
      else { node.textContent = String(n); delete node.dataset.counting; }
    };
    raf(step);
  }

  return {
    el,
    patch(view, first) {
      const animate = first && !visit.counted && !reduced();
      view.figures.forEach((fig, i) => {
        const f = figEls[i];
        setData(f.n, "gold", fig.gold ? "true" : "false");
        setText(f.label, fig.label);
        if (f.n.dataset.counting) return;
        if (animate && !fig.gold && fig.n > 0) countUp(f.n, fig.n);
        else setText(f.n, String(fig.n));
      });
      if (first) visit.counted = true;
    },
  };
}
