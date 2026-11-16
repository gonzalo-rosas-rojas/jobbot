// COLA's head (spec §5.3): the eyebrow with the engine's line, the headline with the number of the proposal on screen, the line of an empty gate and the engine's warnings.
// Built once, patched after.
//
//   const head = createHead();   head.els  the four nodes in the order they go at the top of the section   head.patch(view)

import { h, setText } from "../../core/dom.js";
import { setData, setHidden } from "../shared/util.js";

export function createHead() {
  const parts = {
    eyebrow: h("p", { class: "eyebrow label" }),
    dot: h("span", { class: "dot", "aria-hidden": "true" }),
    engine: h("span", { class: "engine-text" }),
    lead: h("span", { class: "lead" }),
    n: h("b", { class: "n num" }),
    of: h("span", { class: "of" }),
    ask: h("p", { class: "ask serif" }),
    sub: h("p", { class: "sub label" }),
    warn: h("p", { class: "warn label", role: "status" }),
  };
  const h1 = h("h1", { class: "mega cond", id: "cola-title", tabindex: "-1" }, parts.lead, " ", parts.n, " ", parts.of);
  const els = [
    h("div", { class: "head-row" }, parts.eyebrow, h("p", { class: "engine label" }, parts.dot, parts.engine)),
    h("div", { class: "headline" }, h1, parts.ask),
    parts.sub,
    parts.warn,
  ];

  return {
    els,
    patch(view) {
      setText(parts.eyebrow, view.eyebrow);
      setText(parts.engine, view.engine.line);
      setData(parts.dot, "engine", view.engine.kind);
      setData(parts.dot, "stopped", view.engine.stopped ? "true" : null);
      setText(parts.lead, view.head.lead);
      setText(parts.n, view.head.n == null ? "" : String(view.head.n));
      setHidden(parts.n, view.head.n == null);
      setText(parts.of, view.head.of);
      setHidden(parts.of, view.empty);
      setText(parts.ask, view.head.ask);
      setHidden(parts.ask, view.empty);
      setText(parts.sub, view.empty ? view.head.sub : "");
      setHidden(parts.sub, !view.empty);
      setText(parts.warn, view.engine.warn.join(" "));
      setHidden(parts.warn, view.engine.warn.length === 0);
    },
  };
}
