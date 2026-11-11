// Draws SISTEMA (spec §5.5, board/relay2/sistema-1920.png) from the view of ./view.js. The page is BUILT ONCE and PATCHED after, like HOY:
// every text goes through setText (a write only when it differs), the lists are brought to the new order with `sync` (which moves no node
// that is already where it should be), so a refresh that changes nothing writes nothing and one that changes a number leaves the button,
// the focus and the cursor exactly where they were. Hierarchy, from the biggest down: the headline (when the engine last ran), two figures
// (the next run, the sends of 24 h; what Claude cost is said in the account's row) next to the night's window with its marks, then the instruments in two columns in the
// reading face (Inter) with their labels in mono, the limits as one band, and the keys of the page at the foot.
//
//   const ui = createSistema({ run, open });
//   ui.el                    section.sistema  (host.append(ui.el))
//   ui.patch(view, state)    state = { pending, armed, foot }: what the STOP button is doing right now, and whether the foot of the page has room
//   ui.destroy()

import { h } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { sync, setText, setData, setHidden, setTitle } from "../shared/util.js";
import { actButton } from "../../ui/act.js";

const setVar = (el, name, value) => { if (el.style.getPropertyValue(name) !== value) el.style.setProperty(name, value); };
const pct = (n) => String(Math.round(n * 100) / 100) + "%";

/** One instrument: a mono label, its value in the reading face, and a dim sentence under it. */
function line(id) {
  const label = h("dt", { class: "row-l label" });
  const value = h("dd", { class: "row-v" });
  const sub = h("dd", { class: "row-s" });
  return { id, label, value, sub, el: h("div", { class: "row", "data-row": id }, label, value, sub) };
}

const dot = () => h("span", { class: "dot", "aria-hidden": "true" });

export function createSistema({ run, open }) {
  const skeleton = () => h("div", { class: "row skeleton", "aria-hidden": "true" });

  // ---- the top: where this is, and the window and STOP as the snapshot says them
  const parts = {
    eyebrow: h("p", { class: "eyebrow label" }),
    status: h("p", { class: "status label" }),
    lead: h("span", { class: "lead label" }),
    tail: h("span", { class: "tail cond" }),
    chipText: h("span", { class: "chip-text" }),
    chipStop: h("span", { class: "chip-stop" }),
    figs: h("dl", { class: "figs" }),
    stripTitle: h("p", { class: "strip-title label" }),
    stripBox: h("div", { class: "strip-box", role: "img" }),
    stripTicks: h("div", { class: "strip-ticks mono", "aria-hidden": "true" }),
    stripMarks: h("p", { class: "strip-marks label" }),
  };
  parts.h1 = h("h1", { class: "mega", id: "sistema-title", tabindex: "-1" }, parts.lead, " ", parts.tail);
  const chip = h("p", { class: "chip label" }, dot(), parts.chipText, parts.chipStop);
  const figEls = ["next", "sends"].map((id) => ({ id, n: h("dd", { class: "fig-n cond num" }), label: h("dt", { class: "fig-l label" }) }));
  // dl > div > dt + dd: the label first in the markup (what a screen reader reads first), the number first on the screen (CSS order)
  figEls.forEach((f) => parts.figs.append(h("div", { class: "fig", "data-fig": f.id }, f.label, f.n)));

  // ---- the strip: bars and marks are plain boxes placed by percent (custom properties), so nothing is stretched
  parts.stripBox.append(h("span", { class: "strip-base", "aria-hidden": "true" }));
  const stripLayers = { busy: h("div", { class: "strip-busy" }), bars: h("div", { class: "strip-bars" }), marks: h("div", { class: "strip-dots" }) };
  parts.stripBox.append(stripLayers.busy, stripLayers.bars, stripLayers.marks);

  // ---- the engine
  const rows = { last: line("last"), task: line("task") };
  // the last run and how it ended are one line: "02:04 · hace 56 min" and a dot with "terminó bien" (the window is the line at the top of the sheet)
  const lastWhen = h("span", { class: "when" });
  const lastResult = h("span", { class: "result" });
  rows.last.value.append(lastWhen, lastResult);
  const steps = h("ul", { class: "steps" });
  const stepEls = ["discover", "prepare", "submit", "dossiers"].map((id) => ({ id, el: h("li", { class: "step", "data-step": id }, h("span", { class: "step-l label" }), h("span", { class: "step-v" })) }));
  steps.append(...stepEls.map((s) => s.el));
  const stepsRow = line("steps");
  stepsRow.value.replaceWith(steps);
  stepsRow.value = steps;
  const stopRow = line("stop");
  const stopText = h("span", { class: "stop-text" });
  const stopButton = actButton({ key: "", primary: true, "data-action": "engine" });
  const [stopKey, stopLabel] = stopButton.children;
  stopRow.value.append(stopText, stopButton);
  const engineRows = h("dl", { class: "rows" }, rows.last.el, stepsRow.el, rows.task.el, stopRow.el);
  const engineTitle = h("h2", { class: "block-title label" });
  const engineBlock = h("section", { class: "block run", "data-block": "run" }, engineTitle, engineRows);

  // ---- the account
  const acc = { account: line("account"), folder: line("folder"), access: line("access"), cli: line("cli"), spend: line("spend") };
  const accDetail = h("span", { class: "detail" });
  const accFix = h("span", { class: "fix gold" });
  const accText = h("span", { class: "acc-text" });
  const accWho = h("span", { class: "acc-who" });
  const accAnchor = h("span", { class: "acc-anchor" });
  // an account that nothing pins down is a real warning: a square, its name in the alert colour, and in plain words what is missing
  const looseTitle = h("span", { class: "loose-title" });
  const looseWhy = h("span", { class: "loose-why" });
  const accLoose = h("span", { class: "loose", role: "note" }, looseTitle, looseWhy);
  acc.account.value.append(dot(), accText, accAnchor, accWho);
  acc.account.sub.append(accDetail, accFix, accLoose);
  const meter = h("div", { class: "meter", "aria-hidden": "true" }, h("span", { class: "meter-fill" }));
  const stagesList = h("p", { class: "stages label" });
  const overNote = h("span", { class: "over" });
  const spendText = h("span", { class: "spend-text" });
  acc.spend.value.append(spendText, meter, stagesList, overNote);
  const accountTitle = h("h2", { class: "block-title label" });
  const accountBlock = h("section", { class: "block account", "data-block": "account" }, accountTitle,
    h("dl", { class: "rows" }, acc.account.el, acc.folder.el, acc.access.el, acc.cli.el, acc.spend.el));

  // ---- the sources
  const srcHead = ["board", "posted", "sent", "answered"].map((id) => h("th", { class: id === "board" ? "col-board label" : "col-n label", scope: "col" }));
  const srcBody = h("tbody");
  const srcTable = h("table", { class: "sources-table" }, h("thead", {}, h("tr", {}, ...srcHead)), srcBody);
  const srcWeek = h("span", { class: "src-week" });
  const srcNote = h("p", { class: "note label" });
  const srcEmpty = h("p", { class: "empty" });
  const srcMore = h("p", { class: "more label" });
  const sourcesLabel = h("span", { class: "title-text" });
  const sourcesTitle = h("h2", { class: "block-title label" }, sourcesLabel, srcWeek);
  const sourcesBlock = h("section", { class: "block sources", "data-block": "sources" }, sourcesTitle, srcTable, srcEmpty, srcMore, srcNote);

  // ---- the errors of the night
  const errList = h("ul", { class: "errors-list" });
  const errEmpty = h("p", { class: "empty" });
  const errMore = h("p", { class: "more label" });
  const errSources = h("p", { class: "note label" });
  const errorsLabel = h("span", { class: "title-text" });
  const errorsCount = h("span", { class: "count num" });
  const errorsTitle = h("h2", { class: "block-title label" }, errorsLabel, errorsCount);
  const errorsBlock = h("section", { class: "block errors", "data-block": "errors" }, errorsTitle, errList, errEmpty, errMore, errSources);

  // ---- the limits, read only
  const limitEls = ["sends", "score", "auto", "batch"].map((id) => ({ id, label: h("dt", { class: "cell-l label" }), value: h("dd", { class: "cell-v cond num" }), sub: h("dd", { class: "cell-s" }) }));
  const limitsLabel = h("span", { class: "title-text" });
  const limitsRo = h("span", { class: "ro" });
  const limitsTitle = h("h2", { class: "block-title label" }, limitsLabel, limitsRo);
  const limitsBlock = h("section", { class: "block limits", "data-block": "limits" }, limitsTitle,
    h("dl", { class: "cells" }, ...limitEls.map((c) => h("div", { class: "cell", "data-cell": c.id }, c.label, c.value, c.sub))));

  // ---- the foot
  const footTitle = h("span", { class: "foot-title label" });
  const footKeys = h("span", { class: "foot-keys" });
  const foot = h("footer", { class: "foot" }, footTitle, footKeys);

  const colLeft = h("div", { class: "col" }, engineBlock, accountBlock, foot);
  const colRight = h("div", { class: "col" }, sourcesBlock, errorsBlock, limitsBlock);
  // until the first snapshot: grey lines where the instruments will be (CSS hides the real columns meanwhile), never a spinner
  const loading = h("div", { class: "loading", "aria-hidden": "true" }, ...Array.from({ length: 6 }, skeleton));

  const el = h("section", { class: "sistema", "aria-labelledby": "sistema-title", "data-loading": "true", "aria-busy": "true" },
    h("header", { class: "top" }, parts.eyebrow, parts.status),
    h("div", { class: "headline" }, parts.h1, chip),
    h("div", { class: "receipts" },
      parts.figs,
      h("div", { class: "strip-wrap" }, parts.stripTitle, parts.stripBox, parts.stripTicks, parts.stripMarks)),
    loading,
    h("div", { class: "cols" }, colLeft, colRight));

  // ---- one delegated listener for the button and the "open" of an error: nothing is bound per row, so nothing is lost when a row is rebuilt
  let lastView = null;
  el.addEventListener("click", (e) => {
    const target = e.target;
    const act = target.closest("button[data-action]");
    if (act) { run(act.dataset.action); return; }
    const go = target.closest("button[data-open]");
    if (go) open(go.dataset.open, go.closest("[data-row-id]") ? go.closest("[data-row-id]").dataset.rowId : null);
  });

  // ---- patches
  function patchStrip(strip) {
    setText(parts.stripTitle, strip.title);
    setTitle(parts.stripBox, strip.legend);
    if (parts.stripBox.getAttribute("aria-label") !== strip.aria) parts.stripBox.setAttribute("aria-label", strip.aria);
    const ticks = strip.hours.map((x) => {
      const node = h("span", { class: "tick" }, x.label);
      node.style.setProperty("--at", pct(x.pct));
      return node;
    });
    const tickSig = strip.hours.map((x) => x.label + x.pct).join("|");
    if (parts.stripTicks.dataset.sig !== tickSig) { parts.stripTicks.dataset.sig = tickSig; parts.stripTicks.replaceChildren(...ticks); }
    const barSig = JSON.stringify([strip.busy, strip.bars, strip.marks.map((m) => [m.id, m.pct])]);
    if (stripLayers.bars.dataset.sig !== barSig) {
      stripLayers.bars.dataset.sig = barSig;
      stripLayers.busy.replaceChildren(...strip.busy.map((b) => { const n = h("span", { class: "busy" }); setVar(n, "--at", pct(b.pct)); setVar(n, "--w", pct(b.w)); return n; }));
      stripLayers.bars.replaceChildren(...strip.bars.map((b) => { const n = h("span", { class: "bar" }); setVar(n, "--at", pct(b.pct)); setVar(n, "--w", pct(b.w)); setVar(n, "--h", pct(b.h * 100)); return n; }));
      stripLayers.marks.replaceChildren(...strip.marks.map((m) => { const n = h("span", { class: "mark", "data-mark": m.id }); setVar(n, "--at", pct(m.pct)); return n; }));
    }
    const legend = strip.marks.filter((m) => m.id !== "now");
    const sig = legend.map((m) => m.id + m.label).join("|") + "|" + (strip.empty || "");
    if (parts.stripMarks.dataset.sig !== sig) {
      parts.stripMarks.dataset.sig = sig;
      parts.stripMarks.replaceChildren(...(strip.empty ? [h("span", { class: "none" }, strip.empty)] : []),
        ...legend.map((m) => h("span", { class: "key", "data-mark": m.id }, h("i", { class: "swatch", "aria-hidden": "true" }), m.label)));
    }
  }

  function patchSteps(list) {
    list.forEach((s, i) => {
      const node = stepEls[i].el;
      setData(node, "tone", s.tone);
      setText(node.firstChild, s.label);
      setText(node.lastChild, s.text);
    });
  }

  function patchStop(view, state) {
    const stop = view.stop;
    setData(stopRow.el, "on", stop.on ? "true" : "false");
    setData(stopRow.el, "pending", state.pending ? "true" : null);
    if (state.pending) { if (stopRow.el.getAttribute("aria-busy") !== "true") stopRow.el.setAttribute("aria-busy", "true"); } else if (stopRow.el.hasAttribute("aria-busy")) stopRow.el.removeAttribute("aria-busy");
    setText(stopText, stop.text);
    setText(stopKey, stop.key);
    const armed = state.armed && stop.on;
    const said = armed ? t("act.sure") : stop.label;
    setText(stopLabel, said);
    setData(stopButton, "armed", armed ? "true" : null);
    const aria = t("sistema.row.stop") + ": " + said;
    if (stopButton.getAttribute("aria-label") !== aria) stopButton.setAttribute("aria-label", aria);
    setTitle(stopRow.el, stop.hint);
    setHidden(stopRow.sub, true);
    setText(stopRow.label, t("sistema.row.stop"));
  }

  function patchAccount(view) {
    const a = view.account;
    setText(accountTitle, t("sistema.account.title"));
    setText(acc.account.label, t("sistema.row.account"));
    setData(acc.account.el, "tone", a.state);
    setText(accText, a.text);
    setText(accWho, a.who || "");
    setText(accDetail, a.detail || "");
    setHidden(accDetail, !a.detail);
    setText(accFix, a.fix || "");
    setHidden(accFix, !a.fix);
    const loose = !!a.anchor && a.anchor.state === "loose";
    setData(acc.account.el, "anchor", a.anchor ? a.anchor.state : null);
    setText(accAnchor, a.anchor && !loose ? a.anchor.text : "");
    setText(looseTitle, loose ? a.anchor.text : "");
    setText(looseWhy, loose ? a.anchor.why : "");
    setHidden(accLoose, !loose);
    setHidden(acc.account.sub, !a.detail && !a.fix && !loose);
    for (const [key, row, label] of [["folder", acc.folder, "sistema.row.folder"], ["access", acc.access, "sistema.row.access"], ["version", acc.cli, "sistema.row.cli"]]) {
      setText(row.label, t(label));
      setText(row.value, a[key] || "");
      setHidden(row.el, !a[key]);
    }
    setText(acc.spend.label, t("sistema.row.spend"));
    const s = view.spend;
    setData(acc.spend.el, "over", s && s.over ? "true" : "false");
    setText(spendText, s ? s.text : t("sistema.fig.nodata"));
    setHidden(meter, !s || s.ratio == null);
    if (s && s.ratio != null) setVar(meter, "--w", pct(s.ratio * 100));
    setText(stagesList, s ? s.stages.map((x) => x.text).join(" · ") : "");
    setHidden(stagesList, !s || s.stages.length === 0);
    setText(overNote, s && s.over ? s.overText : "");
    setHidden(overNote, !(s && s.over));
  }

  function patchSources(view) {
    const s = view.sources;
    setText(sourcesLabel, t("sistema.sources.title"));
    ["board", "posted", "sent", "answered"].forEach((id, i) => setText(srcHead[i], t("sistema.sources.head." + id)));
    setText(srcWeek, s.week || "");
    setHidden(srcWeek, !s.week);
    const bodyRows = s.rows.map((r) => {
      let tr = srcBody.querySelector(`tr[data-src="${CSS.escape(r.id)}"]`);
      if (!tr) {
        tr = h("tr", { "data-src": r.id }, h("th", { class: "col-board", scope: "row" }, h("span", { class: "src-name" }), h("span", { class: "src-bar", "aria-hidden": "true" })), h("td", { class: "col-n num" }), h("td", { class: "col-n num" }), h("td", { class: "col-n num" }));
      }
      setText(tr.firstChild.firstChild, r.name);
      setVar(tr.firstChild.lastChild, "--w", pct(r.w * 100));
      // the label of each number, for the narrow page, where the table turns into rows of three numbers under their names (CSS: attr(data-label))
      [r.n, r.sent, r.answered].forEach((n, i) => { setText(tr.children[i + 1], String(n)); setData(tr.children[i + 1], "label", t("sistema.sources.head." + ["posted", "sent", "answered"][i])); });
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

  function patchErrors(view) {
    const e = view.errors;
    setText(errorsLabel, t("sistema.errors.title"));
    setText(errorsCount, String(e.total));
    setData(errorsBlock, "none", e.total === 0 ? "true" : "false");
    const items = e.rows.map((r) => {
      let li = errList.querySelector(`li[data-row-id="${CSS.escape(r.id)}"]`);
      if (!li) {
        li = h("li", { class: "err", "data-row-id": r.id }, h("time", { class: "err-hour mono num" }), h("span", { class: "err-what" }, h("span", { class: "err-co" }), h("span", { class: "err-why" })), h("span", { class: "err-act" }));
      }
      setData(li, "kind", r.id === "run" ? "run" : "card");
      setText(li.children[0], r.hour);
      setText(li.children[1].firstChild, r.text);
      setText(li.children[1].lastChild, r.why || "");
      setHidden(li.children[1].lastChild, !r.why);
      const slot = li.children[2];
      if (r.open) {
        let b = slot.firstChild;
        if (!b) { b = actButton({ label: "" }); slot.append(b); }
        setData(b, "open", r.open);
        setText(b.querySelector(".act-label"), t("act.open"));
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

  function patchLimits(view) {
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

  function patchFoot(view) {
    setText(footTitle, view.foot.title);
    const sig = JSON.stringify(view.foot.keys);
    if (footKeys.dataset.sig === sig) return;
    footKeys.dataset.sig = sig;
    footKeys.replaceChildren(...view.foot.keys.map((k) => h("span", { class: "pref" }, h("kbd", {}, k.key), " ", h("span", { class: "pref-l label" }, k.label))));
  }

  function patch(view, state = { pending: false, armed: false }) {
    lastView = view;
    setData(el, "nofoot", state.foot === false ? "true" : null);       // the keys of the page give way before a list does, when there is no room
    if (el.dataset.loading) { delete el.dataset.loading; el.removeAttribute("aria-busy"); loading.remove(); }
    setData(el, "engine", view.engine);
    setData(el, "stopped", view.chip.stop ? "true" : "false");
    setText(parts.eyebrow, view.eyebrow);
    setText(parts.status, [view.status, view.old].filter(Boolean).join(" · "));
    setText(parts.lead, view.head.lead);
    setText(parts.tail, view.head.tail);
    setData(chip, "tone", view.chip.tone);
    setText(parts.chipText, view.chip.text);
    setText(parts.chipStop, view.chip.stop ? t("run.stopped") : "");
    setHidden(parts.chipStop, !view.chip.stop);
    view.figures.forEach((f, i) => {
      const fe = figEls[i];
      setData(fe.n, "words", f.words ? "true" : null);
      setText(fe.label, f.label);
      setText(fe.n, f.value);
    });
    patchStrip(view.strip);
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
    patchAccount(view);
    patchSources(view);
    patchErrors(view);
    patchLimits(view);
    patchFoot(view);
  }

  return {
    el,
    patch,
    stopButton,
    view: () => lastView,
    destroy() { lastView = null; },
  };
}
