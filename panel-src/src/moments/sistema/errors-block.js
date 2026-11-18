// The errors block of SISTEMA: what went wrong in the night (a run that ended badly, the forms that stuck), each with the way to open it.
//
//   const block = createErrorsBlock();
//   block.el               section.block.errors
//   block.patch(view)

import { h, setText } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { sync, setData, setHidden } from "../shared/util.js";
import { actButton, setActLabel } from "../../ui/act.js";
import { keyedRow } from "./parts.js";

export function createErrorsBlock() {
  const errList = h("ul", { class: "errors-list" });
  const errCells = new WeakMap();       // each error row -> the nodes the patch writes to
  const errEmpty = h("p", { class: "empty" });
  const errMore = h("p", { class: "more label" });
  const errSources = h("p", { class: "note label" });
  const errorsLabel = h("span", { class: "title-text" });
  const errorsCount = h("span", { class: "count num" });
  const errorsTitle = h("h2", { class: "block-title label" }, errorsLabel, errorsCount);
  const errorsBlock = h("section", { class: "block errors", "data-block": "errors" }, errorsTitle, errList, errEmpty, errMore, errSources);

  function patch(view) {
    const e = view.errors;
    setText(errorsLabel, t("sistema.errors.title"));
    setText(errorsCount, String(e.total));
    setData(errorsBlock, "none", e.total === 0 ? "true" : "false");
    const items = e.rows.map((r) => {
      const { el: li, cells } = keyedRow(errList, "li", "data-row-id", r.id, () => {
        const made = { hour: h("time", { class: "err-hour mono num" }), co: h("span", { class: "err-co" }), why: h("span", { class: "err-why" }), slot: h("span", { class: "err-act" }) };
        return { el: h("li", { class: "err", "data-row-id": r.id }, made.hour, h("span", { class: "err-what" }, made.co, made.why), made.slot), cells: made };
      }, errCells);
      const { hour, co, why, slot } = cells;
      setData(li, "kind", r.id === "run" ? "run" : "card");
      setText(hour, r.hour);
      setText(co, r.text);
      setText(why, r.why || "");
      setHidden(why, !r.why);
      if (r.open) {
        let b = slot.firstChild;
        if (!b) { b = actButton({ label: "" }); slot.append(b); }
        setData(b, "open", r.open);
        setActLabel(b, t("act.open"));
        const aria = t("act.open") + ": " + r.text;
        if (b.getAttribute("aria-label") !== aria) b.setAttribute("aria-label", aria);
      } else if (slot.firstChild) slot.replaceChildren();
      return li;
    });
    sync(errList, items);
    setHidden(errList, items.length === 0);
    setText(errEmpty, e.empty || "");
    setHidden(errEmpty, !e.empty);
    setText(errMore, e.moreLabel || "");
    setHidden(errMore, !e.moreLabel);
    setText(errSources, e.sources);
  }

  return { el: errorsBlock, patch };
}
