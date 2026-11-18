// The small pieces every block of SISTEMA is built from: an instrument row (label, value, sentence), a box placed by custom properties, the dot
// of a state, and the rule for writing a custom property only when it differs.

import { h } from "../../core/dom.js";

export const setVar = (el, name, value) => { if (el.style.getPropertyValue(name) !== value) el.style.setProperty(name, value); };

/** A percentage (0-100, already multiplied: `pct(0.5)` is "0.5%") as the CSS percentage a custom property carries: two decimals at most. */
export const pct = (n) => String(Math.round(n * 100) / 100) + "%";

export const dot = () => h("span", { class: "dot", "aria-hidden": "true" });

/**
 * One instrument: a mono label, its value in the reading face, and a dim sentence under it. `value` brings a value node of the caller's own
 * (the steps list needs a plain dd); `withSub: false` leaves out the sentence for a row that never has one.
 */
export function makeRow(id, { value = h("dd", { class: "row-v" }), withSub = true } = {}) {
  const label = h("dt", { class: "row-l label" });
  const sub = withSub ? h("dd", { class: "row-s" }) : null;
  return { id, label, value, sub, el: h("div", { class: "row", "data-row": id }, label, value, sub) };
}

/**
 * The row of `container` (a `tag` element whose `attr` is `id`), built by `create()` the first time: `create` answers { el, cells }, and the
 * `cells` (the nodes the patch writes to) are kept in `store`, a WeakMap by row. Answers { el, cells }.
 */
export function keyedRow(container, tag, attr, id, create, store) {
  let el = container.querySelector(`${tag}[${attr}="${CSS.escape(id)}"]`);
  if (!el) {
    const made = create();
    el = made.el;
    store.set(el, made.cells);
  }
  return { el, cells: store.get(el) };
}

/** A box placed by custom properties (`{ "--at": "12%" }`), the way the strip draws its bars and marks. */
export function placed(cls, vars, attrs = {}) {
  const node = h("span", { class: cls, ...attrs });
  for (const [name, value] of Object.entries(vars)) setVar(node, name, value);
  return node;
}
