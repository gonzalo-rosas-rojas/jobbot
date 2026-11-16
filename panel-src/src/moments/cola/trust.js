// COLA's trust block (spec §5.3): how many approvals are his (a row of squares) and how much of the sends cap is used (a meter). Built once, patched after.
//
//   const trust = createTrust();   trust.el (div.trust)   trust.patch(view)

import { h, setText } from "../../core/dom.js";
import { setHidden, setAttr } from "../shared/util.js";

export function createTrust() {
  const parts = {
    reviewLabel: h("p", { class: "label" }),
    squares: h("p", { class: "sq", role: "img" }),
    reviewSub: h("p", { class: "label dim" }),
    sentLabel: h("p", { class: "label" }),
    meter: h("p", { class: "cap-meter", role: "img" }, h("i", {})),
    sentSub: h("p", { class: "label dim" }),
  };
  const el = h("div", { class: "trust" },
    h("div", { class: "trust-a" }, parts.reviewLabel, parts.squares, parts.reviewSub),
    h("div", { class: "trust-b" }, parts.sentLabel, parts.meter, parts.sentSub));
  let squaresSig = "";

  return {
    el,
    patch(view) {
      const tr = view.trust;
      setHidden(el, !tr);
      if (!tr) return;
      setText(parts.reviewLabel, tr.reviewLabel);
      setAttr(parts.squares, "aria-label", tr.reviewAria);
      const sig = JSON.stringify(tr.squares);
      if (sig !== squaresSig) {
        squaresSig = sig;
        parts.squares.replaceChildren(...tr.squares.map((lit) => h("i", { "data-on": lit ? "true" : null })));
      }
      setText(parts.reviewSub, tr.reviewSub);
      setText(parts.sentLabel, tr.sentLabel);
      setAttr(parts.meter, "aria-label", tr.sentAria);
      const bar = parts.meter.firstChild;
      const k = tr.k.toFixed(3);
      if (bar.style.getPropertyValue("--k") !== k) bar.style.setProperty("--k", k);
      setText(parts.sentSub, tr.sentSub);
    },
  };
}
