// "Historial": every move of the application, the newest first, each with when, from which state to which and the note that
// came with it. Notes are text written by the server or by Gonza and are shown as text.

import { h } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { stateLabel, when } from "../../core/format.js";

const at = (event) => Date.parse(event.at) || 0;
const name = (state) => (state == null ? "" : stateLabel(state));

export function timelineView(detail) {
  const events = (detail.events || []).slice().sort((a, b) => at(b) - at(a));
  const body = events.length === 0
    ? h("p", { class: "muted" }, t("drawer.history.none"))
    : h("ol", { class: "d-timeline" }, events.map((event, i) => h("li", { class: i === 0 ? "d-ev first" : "d-ev" },
      h("span", { class: "d-evdot", "aria-hidden": "true" }),
      h("time", { class: "d-evwhen small muted", datetime: event.at }, when(event.at)),
      h("span", { class: "d-evwhat" },
        event.from ? t("drawer.move", { from: name(event.from), to: name(event.to) }) : name(event.to)),
      event.note ? h("span", { class: "d-evnote small wrap-any" }, event.note) : null)));
  return h("section", { class: "d-sec", "aria-labelledby": "d-history-h" }, h("h3", { id: "d-history-h" }, t("drawer.history")), body);
}
