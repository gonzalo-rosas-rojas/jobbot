// The limits block of SISTEMA: the caps and thresholds the engine works under, read only.
//
//   const block = createLimitsBlock();
//   block.el               section.block.limits
//   block.patch(view)

import { h, setText } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { setData, setHidden } from "../shared/util.js";

export function createLimitsBlock() {
  const limitEls = ["sends", "score", "auto", "batch"].map((id) => ({ id, label: h("dt", { class: "cell-l label" }), value: h("dd", { class: "cell-v cond num" }), sub: h("dd", { class: "cell-s" }) }));
  const limitsLabel = h("span", { class: "title-text" });
  const limitsRo = h("span", { class: "ro" });
  const limitsTitle = h("h2", { class: "block-title label" }, limitsLabel, limitsRo);
  const limitsBlock = h("section", { class: "block limits", "data-block": "limits" }, limitsTitle,
    h("dl", { class: "cells" }, ...limitEls.map((c) => h("div", { class: "cell", "data-cell": c.id }, c.label, c.value, c.sub))));

  function patch(view) {
    setText(limitsLabel, t("sistema.limits.title"));
    setText(limitsRo, t("sistema.limits.ro"));
    view.limits.rows.forEach((row, i) => {
      const c = limitEls[i];
      setText(c.label, row.label);
      setText(c.value, row.value);
      setData(c.value, "words", /\d/.test(row.value) ? null : "true");
      setText(c.sub, row.sub || "");
      setHidden(c.sub, !row.sub);
    });
  }

  return { el: limitsBlock, patch };
}
