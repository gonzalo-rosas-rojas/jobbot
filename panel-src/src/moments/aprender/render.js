// Draws APRENDER (spec §5.4, board/relay2/aprender-1920.png) from the view of ./view.js. The page is BUILT ONCE and PATCHED after: every
// text goes through setText (a write only when it differs), the rows are kept in place by id and brought to the new order with `sync`, and
// the parts that change rarely (the dossier, the topics) are rebuilt only when what they say changed. A refresh that changes nothing
// writes nothing, and one that changes a number leaves the buttons, the focus, the armed press and the half-typed answer as they were.
//
//   const page = createPage({ actions, drill, drillUi, onMore, onTopic, onStart, t });
//   page.el                                  section.aprender  (host.append(page.el))
//   page.patch(view)                         the new view of the moment
//   page.rowView(id) / page.firstQuestion()  the view of a row by its id (what a key acts on), and of the first question
//   page.repaint(id)                         paints one row again (its armed or pending state changed)

import { h } from "../../core/dom.js";
import { sync, setText, setData, setHidden, setTitle } from "../shared/util.js";
import { opens } from "../shared/opens.js";
import { actButton } from "../../ui/act.js";

const rowSignature = (view, state) => JSON.stringify([view.label, view.formsLabel, view.who, view.open, view.link, view.actions.map((a) => [a.id, a.key, a.label, a.primary, a.visible]), state.armed, state.pending]);

/** One question that blocks forms: its words, how many forms, whom it holds back and the one button (u). */
function createRow(first, t) {
  const parts = {
    q: h("span", { class: "ap-q wrap-any" }),
    n: h("span", { class: "ap-n mono num" }),
    who: h("span", { class: "ap-who wrap-any" }),
    acts: h("span", { class: "acts" }),
  };
  const el = h("li", { class: "ap-row" }, parts.q, parts.n, parts.who, parts.acts);
  el.dataset.row = first.id;
  // the cursor's identity is the row's own (a card that misses two questions is in two rows): "t:" + id, never "c:" + card
  el.dataset.task = first.id;
  let sig = "";
  let structure = "";
  const row = { el, view: first, patch };

  function patch(view, state = { armed: null, pending: false }) {
    const next = rowSignature(view, state);
    if (next === sig) return false;
    sig = next;
    row.view = view;
    setData(el, "actions", view.actions.map((a) => a.id).join(","));
    setData(el, "opens", opens(view) ? "true" : null);   // a click on the row opens it: the hand and the underline say so
    setData(el, "pending", state.pending ? "true" : null);
    if (state.pending) { if (el.getAttribute("aria-busy") !== "true") el.setAttribute("aria-busy", "true"); } else if (el.hasAttribute("aria-busy")) el.removeAttribute("aria-busy");
    setData(el, "armed", state.armed);
    setText(parts.q, view.label);
    setText(parts.n, view.formsLabel);
    setText(parts.who, view.who);
    setHidden(parts.who, !view.who);
    setTitle(parts.q, view.label);
    setTitle(parts.who, view.who);
    const shown = view.actions.filter((a) => a.visible);
    const shape = JSON.stringify(shown.map((a) => [a.id, a.key, a.label, a.primary]));
    if (shape !== structure) {
      structure = shape;
      const focused = parts.acts.contains(document.activeElement) ? document.activeElement.dataset.action : null;
      parts.acts.replaceChildren(...shown.map((a) => actButton({ key: a.key, label: a.label, primary: a.primary, "data-action": a.id })));
      if (focused) { const again = parts.acts.querySelector(`[data-action="${focused}"]`); if (again) again.focus({ preventScroll: true }); }
    }
    for (const b of parts.acts.children) {
      const action = shown.find((a) => a.id === b.dataset.action);
      const armed = state.armed === action.id;
      const said = armed ? t("act.sure") : action.label; // the name a screen reader gives is the one the eyes read
      setText(b.lastChild, said);
      setData(b, "armed", armed ? "true" : null);
      const aria = t("aprender.act.aria", { action: said, what: view.label });
      if (b.getAttribute("aria-label") !== aria) b.setAttribute("aria-label", aria);
    }
    return true;
  }
  patch(first);
  return row;
}

export function createPage({ actions, drill, drillUi, onMore, onTopic, onStart, onOpen, t }) {
  const rowCtls = new Map();          // the questions: id -> row
  const views = new Map();            // every row that has verbs, by id: what a key acts on
  let eveActSig = "";
  let eveRowId = null;
  let eveSig = "";
  let topicsSig = "";
  let gapsSig = "";
  let pattsSig = "";

  const P = {
    eyebrow: h("p", { class: "eyebrow label" }),
    engine: h("p", { class: "ap-engine label" }),
    lead: h("span", { class: "lead" }),
    turn: h("em", { class: "turn" }),
    sub: h("div", { class: "ap-sub" }),
    // the questions
    missH: h("h2", { class: "ap-h label", id: "ap-miss-h" }),
    list: h("ul", { class: "ap-rows", "data-lenis-prevent": "" }),
    more: h("button", { class: "ap-more label", type: "button", "data-more": "" }),
    none: h("p", { class: "ap-none" }),
    how: h("p", { class: "ap-how" }),
    // the bank
    bankH: h("h2", { class: "ap-h label", id: "ap-bank-h" }),
    figs: h("dl", { class: "ap-figs" }),
    bankNote: h("p", { class: "ap-note" }),
    // the rejections
    rejH: h("h2", { class: "ap-h label", id: "ap-rej-h" }),
    patts: h("ul", { class: "ap-pats" }),
    rejNote: h("p", { class: "ap-note" }),
    rejNo: h("p", { class: "ap-note" }),
    // the eve
    eveH: h("h2", { class: "ap-h label", id: "ap-eve-h" }),
    eveRole: h("p", { class: "ap-eve-role wrap-any" }),
    eveBody: h("div", { class: "ap-eve-body" }),
    eveOthers: h("p", { class: "ap-others label" }),
    eveActs: h("span", { class: "acts" }),
    eveAct: h("div", { class: "ap-eve-act" }),
    // the review
    drillH: h("h2", { class: "ap-h label", id: "ap-drill-h" }),
    drillSum: h("div", { class: "ap-drill-sum" }),
    drillText: h("p", { class: "ap-none" }),
    topics: h("ul", { class: "ap-topics" }),
    gapsH: h("h3", { class: "ap-h2 label" }),
    gaps: h("ul", { class: "ap-gaps" }),
    start: actButton({ key: "↵", primary: true, cls: "ap-start", "data-ap": "start" }),
    foot: h("p", { class: "ap-foot label" }),
  };
  P.h1 = h("h1", { class: "mega cond", id: "aprender-title", tabindex: "-1" }, P.lead, " ", P.turn);
  P.figEls = ["complete", "blocked", "seen", "bank"].map((id) => ({ id, label: h("dt", { class: "fig-l" }), n: h("dd", { class: "fig-n cond num" }) }));
  P.figEls.forEach((f) => P.figs.append(h("div", { class: "fig", "data-fig": f.id }, f.label, f.n)));
  P.eveAct.append(P.eveActs, P.eveOthers); // the buttons of the eve and, on the same line, what is waiting behind them
  P.drillSum.append(P.drillText, P.topics, P.gapsH, P.gaps, P.start);
  P.list.append(h("li", { class: "ap-row skeleton", "aria-hidden": "true" }), h("li", { class: "ap-row skeleton", "aria-hidden": "true" }), h("li", { class: "ap-row skeleton", "aria-hidden": "true" }));

  const block = (name, labelledBy, ...kids) => h("section", { class: "ap-block", "data-block": name, "aria-labelledby": labelledBy }, ...kids);
  const blocks = {
    missing: block("missing", "ap-miss-h", P.missH, P.list, P.more, P.none, P.how),
    bank: block("bank", "ap-bank-h", P.bankH, P.figs, P.bankNote),
    rejects: block("rejects", "ap-rej-h", P.rejH, P.patts, P.rejNo, P.rejNote),
    eve: block("eve", "ap-eve-h", P.eveH, P.eveRole, P.eveBody, P.eveAct),
    drill: block("drill", "ap-drill-h", P.drillH, P.drillSum, drillUi.el),
  };
  const el = h("section", { class: "aprender", "aria-labelledby": "aprender-title", "data-loading": "true" },
    h("header", { class: "ap-top" }, P.eyebrow, P.engine),
    h("div", { class: "ap-head" }, P.h1, P.sub),
    h("div", { class: "ap-cols" },
      h("div", { class: "ap-col" }, blocks.missing, blocks.bank, blocks.rejects),
      h("div", { class: "ap-col" }, blocks.eve, blocks.drill)),
    P.foot);

  // ---- one delegated listener for every button, row and link of the page: nothing is bound per row, so nothing is lost when one is rebuilt
  el.addEventListener("click", (e) => {
    const target = e.target;
    const button = target.closest("button[data-action]");
    if (button) {
      const rowEl = button.closest("[data-row]");
      const view = rowEl && views.get(rowEl.dataset.row);
      if (view) onOpen(button.dataset.action, view);
      return;
    }
    if (target.closest("button[data-more]")) { onMore(); return; }
    const topic = target.closest("button[data-topic]");
    if (topic) { onTopic(topic.dataset.topic); return; }
    if (target.closest("button[data-ap='start']")) { onStart(); return; }
    const rowEl = target.closest("[data-row]");
    if (rowEl && !target.closest("a, button, summary, textarea")) {
      const view = views.get(rowEl.dataset.row);
      if (view) onOpen("open", view);
    }
  });

  const stateOf = (id) => ({ armed: actions.armedOf(id), pending: actions.isPending(id) });

  function patchMissing(m) {
    setText(P.missH, m.head);
    const wanted = new Set(m.rows.map((r) => r.id));
    for (const id of [...rowCtls.keys()]) if (!wanted.has(id)) { rowCtls.delete(id); views.delete(id); }
    const els = m.rows.map((row) => {
      let ctl = rowCtls.get(row.id);
      if (!ctl) { ctl = createRow(row, t); rowCtls.set(row.id, ctl); }
      views.set(row.id, row);
      ctl.patch(row, stateOf(row.id));
      return ctl.el;
    });
    sync(P.list, els);
    setHidden(P.list, els.length === 0);
    setData(P.list, "expanded", m.expanded ? "true" : null);
    setText(P.more, m.moreLabel || "");
    setHidden(P.more, !m.moreLabel);
    const expanded = m.expanded ? "true" : "false";
    if (P.more.getAttribute("aria-expanded") !== expanded) P.more.setAttribute("aria-expanded", expanded);
    setText(P.none, m.none || "");
    setHidden(P.none, !m.none);
    setText(P.how, m.how);
    setHidden(P.how, !!m.none);
  }

  function patchBank(b) {
    setText(P.bankH, b.head);
    b.figures.forEach((fig, i) => {
      const f = P.figEls[i];
      setData(f.n, "nodata", fig.nodata ? "true" : null);
      setText(f.label, fig.label);
      setText(f.n, fig.nodata ? b.nodata : String(fig.n));
    });
    setText(P.bankNote, b.note);
  }

  function patchRejects(r) {
    setHidden(blocks.rejects, !r);
    if (!r) return;
    setText(P.rejH, r.head);
    const sig = JSON.stringify(r.rows.map((x) => x.text));
    if (sig !== pattsSig) { pattsSig = sig; P.patts.replaceChildren(...r.rows.map((x) => h("li", { class: "ap-pat" }, x.text))); }
    const none = r.few || r.none || "";
    setText(P.rejNo, none);
    setHidden(P.rejNo, !none);
    setText(P.rejNote, [r.note, r.nodata].filter(Boolean).join(" "));
    setHidden(P.rejNote, !(r.note || r.nodata));
  }

  function patchEve(e) {
    setText(P.eveH, e.head);
    setText(P.eveRole, e.role || "");
    setHidden(P.eveRole, !e.role);
    setData(blocks.eve, "state", e.state);
    const questions = e.questions || [];
    const stories = e.stories || [];
    const sig = JSON.stringify([e.state, e.text, e.summary, questions, stories, e.questionsLabel, e.storiesLabel]);
    if (sig !== eveSig) {
      eveSig = sig;
      const kids = [];
      if (e.text) kids.push(h("p", { class: "ap-none", "data-kind": e.state }, e.text));
      if (e.summary) kids.push(h("p", { class: "ap-eve-sum wrap-any" }, e.summary));
      if (questions.length) kids.push(h("h3", { class: "ap-h2 label" }, e.questionsLabel), h("ol", { class: "ap-qs" }, questions.map((q) => h("li", { class: "wrap-any" }, q))));
      if (stories.length) kids.push(h("h3", { class: "ap-h2 label" }, e.storiesLabel), h("ul", { class: "ap-stories" }, stories.map((s) => h("li", { class: "wrap-any" }, h("strong", {}, s.title), s.evidence ? h("span", { class: "ap-ev wrap-any" }, s.evidence) : null))));
      P.eveBody.replaceChildren(...kids);
    }
    setText(P.eveOthers, [e.gaps, e.others].filter(Boolean).join(" · "));
    setHidden(P.eveOthers, !(e.gaps || e.others));
    // the one row of the eve: the button that opens the dossier (the cursor reaches it with j k, Enter opens it)
    if (eveRowId && (!e.row || e.row.id !== eveRowId)) { views.delete(eveRowId); eveRowId = null; eveActSig = ""; P.eveActs.replaceChildren(); delete P.eveAct.dataset.row; delete P.eveAct.dataset.card; }
    if (!e.row) { setHidden(P.eveAct, true); return; }
    setHidden(P.eveAct, false);
    eveRowId = e.row.id;
    views.set(e.row.id, e.row);
    setData(P.eveAct, "row", e.row.id);
    setData(P.eveAct, "card", e.row.data.card);
    const shown = e.row.actions.filter((a) => a.visible);
    const shape = JSON.stringify(shown.map((a) => [a.id, a.key, a.label]));
    if (shape !== eveActSig) {
      eveActSig = shape;
      P.eveActs.replaceChildren(...shown.map((a) => actButton({ key: a.key, label: a.label, primary: true, "data-action": a.id })));
    }
  }

  function patchDrill(d) {
    setText(P.drillH, d.head);
    setText(P.start.lastChild, d.start.label);
    if (P.start.getAttribute("aria-label") !== d.start.aria) P.start.setAttribute("aria-label", d.start.aria);
    const active = drill.state.active;
    setHidden(P.drillSum, active);
    setText(P.drillText, d.text || "");
    setHidden(P.drillText, !d.text);
    const sig = JSON.stringify(d.topics.map((x) => [x.raw, x.topic, x.dueLabel, x.avgLabel, x.wSeen, x.wDue]));
    if (sig !== topicsSig) {
      topicsSig = sig;
      P.topics.replaceChildren(...d.topics.map((x) => {
        const bar = h("span", { class: "ap-bar", "aria-hidden": "true" }, h("span", { class: "ap-bar-seen" }), h("span", { class: "ap-bar-due" }));
        bar.style.setProperty("--seen", String(Math.round(x.wSeen * 100)) + "%");
        bar.style.setProperty("--due", String(Math.round(x.wDue * 100)) + "%");
        const b = h("button", { class: "ap-topic", type: "button", "aria-label": x.aria },
          h("span", { class: "ap-t-name label" }, x.topic), bar, h("span", { class: "ap-t-due mono num" }, x.dueLabel), h("span", { class: "ap-t-avg mono num" }, x.avgLabel || ""));
        b.dataset.topic = x.raw;
        return h("li", {}, b);
      }));
    }
    setHidden(P.topics, d.topics.length === 0 || d.state !== "due");
    setHidden(P.start, d.state !== "due");
    const gsig = JSON.stringify(d.gaps);
    setHidden(P.gapsH, !d.gaps);
    setHidden(P.gaps, !d.gaps);
    if (gsig !== gapsSig) {
      gapsSig = gsig;
      for (const key of [...views.keys()]) if (key.startsWith("gap:")) views.delete(key);
      if (d.gaps) {
        setText(P.gapsH, d.gaps.head);
        P.gaps.replaceChildren(...d.gaps.rows.map((g) => {
          const li = h("li", { class: "ap-gap", "data-seen": g.seen ? "true" : "false" }, h("span", { class: "ap-g-topic label wrap-any" }, g.topic), h("span", { class: "ap-g-offer wrap-any" }, g.offer), h("span", { class: "ap-g-state label" }, g.state));
          if (g.data) {
            li.dataset.row = g.id;
            li.dataset.card = g.data.card;
            views.set(g.id, { id: g.id, data: g.data, open: g.open, link: null, actions: [{ id: "open", key: "↵", label: "", primary: false, visible: false, permanent: false }] });
          }
          return li;
        }));
      } else {
        P.gaps.replaceChildren();
      }
    }
  }

  function patch(view) {
    if (el.dataset.loading) { delete el.dataset.loading; P.list.replaceChildren(); }
    setData(el, "state", view.state);
    setText(P.eyebrow, view.eyebrow);
    setText(P.engine, view.engine.join(" · "));
    setHidden(P.engine, view.engine.length === 0);
    setText(P.lead, view.lead);
    setText(P.turn, view.turn);
    setHidden(P.turn, !view.turn);
    const subs = view.sub;
    while (P.sub.childNodes.length > subs.length) P.sub.lastChild.remove();
    while (P.sub.childNodes.length < subs.length) P.sub.append(h("p", { class: "ap-sub-line" }));
    subs.forEach((text, i) => setText(P.sub.childNodes[i], text));
    patchMissing(view.missing);
    patchBank(view.bank);
    patchRejects(view.rejects);
    patchEve(view.eve);
    patchDrill(view.drill);
    setHidden(blocks.eve, drill.state.active);
    setData(el, "drill", drill.state.active ? "on" : null);
    setText(P.foot, view.foot);
  }

  return {
    el,
    patch,
    rowView: (id) => views.get(id) || null,
    /** The first question (what `Enter` or `u` would act on with the cursor on none): its view, or null. */
    firstQuestion: () => { const first = P.list.querySelector("[data-row]"); return first ? views.get(first.dataset.row) || null : null; },
    repaint(id) { const ctl = rowCtls.get(id); if (ctl) ctl.patch(ctl.view, stateOf(id)); },
    destroy() { rowCtls.clear(); views.clear(); },
  };
}
