// "Cómo se calculó el puntaje": one bar per factor of the score, as long as its points over what it can give, and the total at the end.
// The server names the factors; the ones the page knows are translated and any other name is shown as it comes, so a factor added later
// still reads as something.

import { h } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { factorBars, factorName, factorPoints as points } from "../../core/format.js";

/** The bars of the score as data (core/format.js factorBars, as COLA and PROCESOS draw them); an array is no breakdown. */
export function scoreRows(scoreDetail) {
  if (Array.isArray(scoreDetail)) return [];
  return factorBars(scoreDetail).map((b) => ({ ...b, label: factorName(b.id) }));
}

export function scoreView(detail, card) {
  const bars = scoreRows(detail.score_detail);
  const body = bars.length === 0
    ? h("p", { class: "muted" }, t("drawer.score.none"))
    : (() => {
      const rows = bars.map((b) => {
        const bar = h("i", {});
        bar.style.setProperty("--w", String(b.k));
        return h("li", { class: "d-bar" },
          h("span", { class: "d-bk" }, b.label),
          h("span", { class: "meter", "aria-hidden": "true" }, bar),
          h("span", { class: "d-bv num" }, b.text));
      });
      rows.push(h("li", { class: "d-bar total" },
        h("span", { class: "d-bk" }, t("detail.score.total")),
        h("span", {}),
        h("span", { class: "d-bv num" }, points(card.score))));
      return h("ul", { class: "d-bars", "aria-label": t("drawer.score.aria") }, rows);
    })();
  return h("section", { class: "d-sec", "aria-labelledby": "d-score-h" }, h("h3", { id: "d-score-h" }, t("drawer.score.how")), body);
}
