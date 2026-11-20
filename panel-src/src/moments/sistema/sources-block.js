// The sources block of SISTEMA: the boards that postings came from, with what each one gave in the funnel, and the response rate of the week.
//
//   const block = createSourcesBlock();
//   block.el               section.block.sources
//   block.patch(view)
//
// One table, three ways to read it (the boards, the origins, the regions): a switch on the title row, per visit. The switch is a row of buttons the sheet's one
// click listener answers (`data-funnel-by`); the table itself is the same nodes, patched.

import { h, setText } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { sync, setData, setHidden } from "../shared/util.js";
import { setVar, pct, keyedRow } from "./parts.js";
import { FUNNEL_BYS } from "./view.js";

const FUNNEL_COLUMNS = ["board", "posted", "sent", "answered", "rejected"];

export function createSourcesBlock() {
  const srcHead = FUNNEL_COLUMNS.map((id) => h("th", { class: id === "board" ? "col-board label" : "col-n label", scope: "col" }));
  const srcBody = h("tbody");
  const srcCells = new WeakMap();       // each row of the table -> the nodes the patch writes to
  const srcTable = h("table", { class: "sources-table" }, h("thead", {}, h("tr", {}, ...srcHead)), srcBody);
  const srcWeek = h("span", { class: "src-week" });
  const srcNote = h("p", { class: "note label funnel-note" });
  const srcEmpty = h("p", { class: "empty" });
  const srcMore = h("p", { class: "more label" });
  const sourcesLabel = h("span", { class: "title-text" });
  const sourcesTitle = h("h2", { class: "block-title label" }, sourcesLabel, srcWeek);
  const switchLabel = h("span", { class: "funnel-by-label label" });
  const switchButtons = FUNNEL_BYS.map((id) => h("button", { class: "funnel-by-pick label", type: "button", "data-funnel-by": id, "aria-pressed": false }));
  const switchBar = h("div", { class: "funnel-by", role: "group" }, switchLabel, ...switchButtons);
  const sourcesBlock = h("section", { class: "block sources", "data-block": "sources" }, h("div", { class: "funnel-head" }, sourcesTitle, switchBar), srcTable, srcEmpty, srcMore, srcNote);

  function patch(view) {
    const s = view.funnel;
    setText(sourcesLabel, s.title);
    setText(switchLabel, s.switchLabel);
    if (switchBar.getAttribute("aria-label") !== s.switchLabel) switchBar.setAttribute("aria-label", s.switchLabel);
    s.switch.forEach((pick, i) => {
      setText(switchButtons[i], pick.label);
      const on = String(pick.on);
      if (switchButtons[i].getAttribute("aria-pressed") !== on) switchButtons[i].setAttribute("aria-pressed", on);
    });
    FUNNEL_COLUMNS.forEach((id, i) => setText(srcHead[i], i === 0 ? s.head : t("sistema.funnel.head." + id)));
    setText(srcWeek, s.week || "");
    setHidden(srcWeek, !s.week);
    const bodyRows = s.rows.map((r) => {
      const { el: tr, cells } = keyedRow(srcBody, "tr", "data-src", r.id, () => {
        const row = h("tr", { "data-src": r.id }, h("th", { class: "col-board", scope: "row" }, h("span", { class: "src-name" }), h("span", { class: "src-bar", "aria-hidden": "true" })), h("td", { class: "col-n num" }), h("td", { class: "col-n num" }), h("td", { class: "col-n num" }), h("td", { class: "col-n num" }));
        return { el: row, cells: { name: row.querySelector(".src-name"), bar: row.querySelector(".src-bar"), numbers: [...row.querySelectorAll("td")] } };
      }, srcCells);
      setText(cells.name, r.name);
      setVar(cells.bar, "--w", pct(r.w * 100));
      // the label of each number, for the narrow page, where the table turns into rows of four numbers under their names (CSS: attr(data-label))
      [["posted", r.n], ["sent", r.sent], ["answered", r.answered], ["rejected", r.rejected]].forEach(([id, n], i) => {
        setText(cells.numbers[i], String(n));
        setData(cells.numbers[i], "label", t("sistema.funnel.head." + id));
      });
      return tr;
    });
    sync(srcBody, bodyRows);
    setHidden(srcTable, s.rows.length === 0);
    setText(srcEmpty, s.empty || "");
    setHidden(srcEmpty, !s.empty);
    setText(srcMore, s.moreLabel || "");
    setHidden(srcMore, !s.moreLabel);
    setText(srcNote, s.note || "");
    setHidden(srcNote, !s.note);
  }

  return { el: sourcesBlock, patch };
}
