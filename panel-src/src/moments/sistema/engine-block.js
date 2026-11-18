// The engine block of SISTEMA: the last run and how it ended, the four steps, the scheduled task, and the STOP button with what it is doing.
//
//   const block = createEngineBlock();
//   block.el                    section.block.run
//   block.patch(view, state)    state = { pending, armed }: what the STOP button is doing right now

import { h, setText } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { setData, setHidden, setTitle, setBusy } from "../shared/util.js";
import { actButton, setActKey, setActLabel } from "../../ui/act.js";
import { makeRow } from "./parts.js";

export function createEngineBlock() {
  const rows = { last: makeRow("last"), task: makeRow("task") };
  // the last run and how it ended are one line: "02:04 · hace 56 min" and a dot with "terminó bien" (the window is the line at the top of the sheet)
  const lastWhen = h("span", { class: "when" });
  const lastResult = h("span", { class: "result" });
  rows.last.value.append(lastWhen, lastResult);
  const steps = h("ul", { class: "steps" });
  const stepEls = ["discover", "prepare", "submit", "dossiers"].map((id) => {
    const label = h("span", { class: "step-l label" });
    const text = h("span", { class: "step-v" });
    return { id, label, text, el: h("li", { class: "step", "data-step": id }, label, text) };
  });
  steps.append(...stepEls.map((s) => s.el));
  // dl > div takes only dt and dd: the list goes inside a plain dd (not .row-v, whose column is the second one of the other rows)
  const stepsRow = makeRow("steps", { value: h("dd", {}, steps) });
  const stopRow = makeRow("stop", { withSub: false });
  const stopText = h("span", { class: "stop-text" });
  const stopButton = actButton({ key: "", primary: true, "data-action": "engine" });
  stopRow.value.append(stopText, stopButton);
  const engineRows = h("dl", { class: "rows" }, rows.last.el, stepsRow.el, rows.task.el, stopRow.el);
  const engineTitle = h("h2", { class: "block-title label" });
  const el = h("section", { class: "block run", "data-block": "run" }, engineTitle, engineRows);

  function patchSteps(list) {
    list.forEach((s, i) => {
      setData(stepEls[i].el, "tone", s.tone);
      setText(stepEls[i].label, s.label);
      setText(stepEls[i].text, s.text);
    });
  }

  function patchStop(view, state) {
    const stop = view.stop;
    setData(stopRow.el, "on", stop.on ? "true" : "false");
    setData(stopRow.el, "pending", state.pending ? "true" : null);
    setBusy(stopRow.el, state.pending);
    setText(stopText, stop.text);
    setActKey(stopButton, stop.key);
    const armed = state.armed && stop.on;
    const said = armed ? t("act.sure") : stop.label;
    setActLabel(stopButton, said);
    setData(stopButton, "armed", armed ? "true" : null);
    const aria = t("sistema.row.stop") + ": " + said;
    if (stopButton.getAttribute("aria-label") !== aria) stopButton.setAttribute("aria-label", aria);
    setTitle(stopRow.el, stop.hint);
    setText(stopRow.label, t("sistema.row.stop"));
  }

  function patch(view, state) {
    setText(engineTitle, t("sistema.run.title"));
    setText(rows.last.label, t("sistema.row.last"));
    setText(lastWhen, view.run.last ? view.run.last.text : t("sistema.row.last.none"));
    // a run that never happened has no result: "sin dato" next to "ninguna todavía" would say it twice
    setData(lastResult, "tone", view.run.result.tone);
    setText(lastResult, view.run.last ? view.run.result.text : "");
    setHidden(lastResult, !view.run.last);
    setText(rows.last.sub, view.run.result.detail || "");
    setHidden(rows.last.sub, !view.run.result.detail);
    setText(stepsRow.label, t("sistema.row.steps"));
    patchSteps(view.run.steps);
    setText(rows.task.label, t("sistema.row.task"));
    setData(rows.task.el, "state", view.task.state);
    setText(rows.task.value, view.task.text);
    // the way out of "sin dato" or "sin tarea" is said under it; a task that exists needs no explanation on the page (it is its tooltip)
    setText(rows.task.sub, view.task.state === "registered" ? "" : view.task.hint);
    setHidden(rows.task.sub, view.task.state === "registered");
    setTitle(rows.task.el, view.task.state === "registered" ? view.task.hint : "");
    patchStop(view, state);
  }

  return { el, patch };
}
