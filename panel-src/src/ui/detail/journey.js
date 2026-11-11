// The Recorrido: where one application stands among the five layers between a posting and a send (aviso, puntaje, CV,
// respuestas, compuerta), as a line you can follow from the top. What is done says how (saved when and from where, the score
// against the threshold, the CV, the form, the gate), the layer it stopped at says what is missing and whose turn it is, and
// what is still ahead is quiet. Once it went out, the line goes on with the five stops of Procesos ("Después del envío").
// The buttons that act on it are not drawn here: they are in the actions row under the answer (ui/detail.js), which stays in
// view while this list scrolls, so there is one set of them and it is always within reach.

import { h } from "../../core/dom.js";
import { t, fmtDate } from "../../i18n/index.js";
import { LAYERS, STAGES, layerOf, pathOf } from "../../core/model.js";
import { pointsOf, stageLabel, stateLabel, statusWord } from "../../core/format.js";
import { say } from "./now.js";

/** The class a dot of `pathOf` stands for, as a state of the list. */
const stateOfDot = (dot) => (dot === "done" ? "done" : dot === "here you" ? "you" : dot === "here" ? "robot" : "todo");

/** What a finished layer says about how it went. */
function doneLines(detail, card, m) {
  const limit = pointsOf(m && m.snap.pipeline ? m.snap.pipeline.threshold : null);
  const threshold = limit === null ? null : Math.round(limit);
  return {
    aviso: detail.first_seen
      ? t("journey.savedOn", { date: fmtDate(detail.first_seen, "day"), source: card.source })
      : t("journey.saved", { source: card.source }),
    puntaje: threshold != null ? t("journey.scoreThreshold", { score: card.score, threshold }) : t("journey.score", { score: card.score }),
    cv: t(card.has_cv ? "journey.cv.ready" : "journey.cv.done"),
    respuestas: t("journey.answers"),
    compuerta: t(card.approved ? "journey.gate.approved" : "journey.gate.passed"),
  };
}

/** The five stops of Procesos with the current one marked; a state outside them (answered "no", no reply) is a chip. */
function stepper(card) {
  const here = STAGES.indexOf(card.state);
  if (here < 0) return h("span", { class: "chip" }, stateLabel(card.state));
  return h("ol", { class: "d-steps", role: "img", "aria-label": t("detail.stage.aria", { name: stageLabel(card.state) }) },
    STAGES.map((stage, i) => h("li", { class: i < here ? "done" : i === here ? "here" : "" },
      h("span", { class: "d-dot" }),
      h("span", { class: "d-slabel" }, stageLabel(stage)))));
}

/**
 * The section for `card`, or null when it is not on the way and has not been sent (a withdrawn application has no path).
 * `m` is the model (for the threshold of the week); without it the score line has no threshold.
 */
export function journeyView(detail, card, m, answer) {
  const l = layerOf(card);
  if (l.at < 0) return null;
  const beyond = l.who === "done";
  const dots = beyond ? null : pathOf(card).dots;
  const lines = doneLines(detail, card, m);
  const items = LAYERS.map((id, i) => {
    const st = beyond ? "done" : stateOfDot(dots[i]);
    const current = !beyond && i === l.at;
    const said = current ? say(l.text) : null;
    // the answer above may already say it in the same words (a stuck form says why once)
    const line = st === "done" ? lines[id] : said && !(answer && answer.sentence === said) ? said : null;
    return h("li", { class: "j-" + st, "data-layer": id, "aria-current": current ? "step" : null },
      h("span", { class: "d-node", "aria-hidden": "true" }),
      h("div", { class: "d-lbody" },
        h("span", { class: "d-lname" }, t("layer." + id), h("span", { class: "sr" }, " (" + statusWord(st) + ")")),
        st === "you" ? h("span", { class: "chip you" }, t("turn.mine")) : null,
        st === "robot" ? h("span", { class: "chip robot" }, t("status.robot")) : null,
        line ? h("span", { class: "d-ldetail" }, line) : null));
  });
  return h("section", { id: "journey", class: "d-sec", "aria-labelledby": "d-journey-h" },
    h("h3", { id: "d-journey-h" }, t("drawer.journey")),
    h("ol", { class: "d-layers", "aria-label": t("drawer.journey.aria") }, items),
    beyond ? h("div", { class: "j-after" }, h("p", { class: "d-after" }, t("drawer.afterSend")), stepper(card)) : null);
}
