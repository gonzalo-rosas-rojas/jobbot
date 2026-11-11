// The one action button of the panel, and the one link that leaves it. Every verb of HOY, COLA, APRENDER, SISTEMA, PROCESOS and the drawer is an
// actButton; styles/actions.css is the only sheet that gives it shape. A button is never underlined at rest (that is what a link looks like), so
// the drawer's links are outLink: words, underline and the exit icon.

import { h, link } from "../core/dom.js";
import { icon } from "./icons.js";

/**
 * <button type=button class="act [primary] [cls]"><kbd class=k>key</kbd><span class=act-label>label</span></button>.
 *   key      the mono key cap; "" leaves an empty one for the stop to fill later, and leaving it out draws none
 *   label    the verb; the two-press helper of core/dom.js rewrites it in place when the button arms, so the key stays
 *   primary  the next verb (ink, with the key in gold)
 *   cls      a class of the caller's own (it may place the button, never redraw it)
 * Every other property (id, title, aria-*, data-*, onclick) goes to the button as it is.
 */
export function actButton({ key, label, primary = false, cls = null, ...attrs }) {
  return h("button", { ...attrs, type: "button", class: ["act", primary && "primary", cls] },
    key == null ? null : h("kbd", { class: "k" }, key),
    h("span", { class: "act-label" }, label));
}

/** A link to another page, in a new tab, with the exit icon after its words; anything that is not an http(s) address is plain text. */
export function outLink(url, text) {
  const out = link(url, text);
  if (out.tagName === "A") out.append(icon("external-link"));
  return out;
}
