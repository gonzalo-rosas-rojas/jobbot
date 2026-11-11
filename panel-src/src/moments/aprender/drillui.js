// The box of the review on screen (spec §5.4): the question in display type, a field for his own attempt (never sent, never saved), the
// reference answer when he asks, and six grades. It is a "mode, announced": it opens with Enter on "Empezar el repaso", and inside it
// Enter or Space show the answer, 0-5 grade it and Esc leaves. Once the answer is out the box marks itself `data-local-keys`, so the
// page's own single-key shortcuts step aside until the next card (before that, 1-5 still travel: a person who got here by key is
// never stuck). The card is built ONCE per question and patched after, so what he is typing and where the caret is survive every
// snapshot (the shell calls `update` every 30 s).
//
//   const ui = createDrillUi({ drill, t, reduced, onLeave });   ui.el   ui.paint()   ui.focusStart()   ui.contains(node)

import { h } from "../../core/dom.js";
import { setHidden, setData } from "../shared/util.js";
import { GRADES } from "./drill.js";
import { actButton } from "../../ui/act.js";

export function createDrillUi({ drill, t, reduced = () => false, onLeave = () => {} }) {
  const { state } = drill;
  let builtFor = null; // the id of the card whose nodes are on screen
  let refs = null;

  const parts = {
    body: h("div", { class: "ap-box-body", "aria-live": "off" }),
    keys: h("p", { class: "ap-keys label" }),
  };
  const el = h("div", { class: "ap-box", role: "group", "aria-labelledby": "ap-drill-h", tabindex: "-1", hidden: true, onkeydown: (e) => onKey(e) }, parts.body, parts.keys);

  const kbd = (text) => h("kbd", { class: "k" }, text);
  const inside = () => el.contains(document.activeElement);

  // Whoever had the keyboard in the box keeps it: the button that was pressed is replaced, so the focus goes where the next thing is.
  async function reveal() {
    const had = inside();
    await drill.reveal();
    if (had && state.shown && refs) { const answer = refs.act.querySelector("#ap-answer"); if (answer) answer.focus({ preventScroll: true }); }
  }
  async function grade(g) {
    const had = inside();
    const before = state.card;
    await drill.grade(g);
    if (had && state.card !== before) focusStart();
  }

  function hints() {
    parts.keys.replaceChildren(
      h("span", { class: "ap-keys-label" }, t("aprender.hint.label") + ":"),
      kbd("↵"), " ", t("aprender.hint.reveal"), " · ", kbd("0–5"), " ", t("aprender.hint.grade"), " · ", kbd("Esc"), " ", t("aprender.hint.leave"),
    );
  }

  // ---- the card: built once per question
  function buildCard(card) {
    const question = h("p", { class: "ap-bigq", id: "ap-question", tabindex: "-1", "data-size": card.question.length <= 90 ? "short" : card.question.length <= 190 ? "mid" : "long" }, card.question);
    const mine = h("textarea", { class: "ap-mine", id: "ap-mine", rows: 4, "aria-describedby": "ap-mine-hint", spellcheck: "false" });
    const act = h("div", { class: "ap-act" });
    const root = h("div", { class: "ap-card" },
      h("p", { class: "ap-tab label" }, h("span", { class: "ap-ctopic" }, card.topic), card.offer ? h("span", { class: "ap-from" }, t("aprender.card.from", { offer: card.offer })) : null),
      question,
      h("div", { class: "ap-field" },
        h("label", { class: "ap-label label", for: "ap-mine" }, t("aprender.card.mine")),
        h("span", { class: "ap-hint", id: "ap-mine-hint" }, t("aprender.card.mine.hint")),
        mine),
      act);
    refs = { root, question, mine, act, shownBuilt: null };
    return root;
  }

  function revealButton() {
    return actButton({ key: "↵", label: t("aprender.card.reveal"), primary: true, cls: "ap-reveal", "data-ap": "reveal", onclick: () => reveal() });
  }

  function paintAct() {
    if (!refs) return;
    const key = state.shown ? "answer" : "question";
    if (refs.shownBuilt === key + ":" + state.answer) return;
    refs.shownBuilt = key + ":" + state.answer;
    if (!state.shown) { refs.act.replaceChildren(revealButton()); return; }
    const grades = GRADES.map((g) => h("button", { class: "ap-grade", type: "button", "data-grade": g, onclick: () => grade(g) }, h("span", { class: "g-n num" }, String(g)), h("span", { class: "g-w" }, t("aprender.grade." + g))));
    refs.act.replaceChildren(
      h("div", { class: "ap-ref" },
        h("h3", { class: "ap-label label", id: "ap-answer-title" }, t("aprender.card.answer")),
        h("pre", { class: "ap-answer", id: "ap-answer", tabindex: "-1", "aria-labelledby": "ap-answer-title" }, state.answer || t("aprender.card.answer.none"))),
      h("div", { class: "ap-grading" },
        h("h3", { class: "ap-label label", id: "ap-grade-title" }, t("aprender.card.grade")),
        h("div", { class: "ap-grades", role: "group", "aria-labelledby": "ap-grade-title" }, grades)));
  }

  function note(text, kind) {
    return h("p", { class: "ap-note-line", "data-kind": kind, tabindex: "-1" }, text);
  }

  let noteKind = null;
  function paint() {
    setHidden(el, !state.active);
    if (!state.active) { builtFor = null; refs = null; noteKind = null; parts.body.replaceChildren(); return; }
    el.toggleAttribute("data-local-keys", !!state.shown);
    setData(el, "busy", state.busy);
    if (parts.keys.childNodes.length === 0) hints();
    if (state.card) {
      noteKind = null;
      if (builtFor !== state.card.id) {
        builtFor = state.card.id;
        parts.body.replaceChildren(buildCard(state.card));
        if (!reduced()) refs.root.dataset.enter = "true";
      }
      paintAct();
      if (refs) refs.root.setAttribute("aria-busy", state.busy === "grade" ? "true" : "false");
      return;
    }
    builtFor = null;
    refs = null;
    const kind = state.waiting ? "waiting" : state.failed ? "failed" : state.empty ? "done" : "waiting";
    if (noteKind === kind) return;
    noteKind = kind;
    const text = kind === "done" ? t("aprender.card.done") : kind === "failed" ? t("aprender.err.next") : t("aprender.card.waiting");
    parts.body.replaceChildren(note(text, kind));
    if (kind === "failed") parts.body.append(actButton({ key: "↵", label: t("common.retry"), primary: true, onclick: () => drill.start(state.topic) }));
  }

  /** The keyboard goes to the question (or to the note when there is none), without scrolling the page: the camera is the scene's. */
  function focusStart() {
    const target = refs ? refs.question : parts.body.querySelector(".ap-note-line");
    if (target) target.focus({ preventScroll: true });
  }

  function onKey(e) {
    if (e.defaultPrevented || e.altKey || e.metaKey) return;
    const tag = e.target && e.target.tagName;
    const field = tag === "TEXTAREA" || tag === "INPUT" || tag === "SELECT";
    const own = (fn) => { e.preventDefault(); e.stopPropagation(); fn(); };
    if (e.key === "Escape") return own(() => onLeave());
    if (e.key === "Enter" || e.key === " " || e.key === "Spacebar") {
      if (e.key === "Enter" && e.ctrlKey && tag === "TEXTAREA") return own(() => reveal());
      // a focused button keeps its own Enter and Space (it is a click), and so does a field; Shift+Enter is a new line
      if (tag === "BUTTON" || tag === "A" || field || e.shiftKey || e.ctrlKey) return;
      if (state.card && !state.shown) return own(() => reveal());
      return;
    }
    if (e.ctrlKey || field || e.key.length !== 1) return;
    if (e.key >= "0" && e.key <= "5" && state.shown) own(() => grade(Number(e.key)));
  }

  return {
    el,
    paint,
    focusStart,
    reveal,
    contains: (node) => !!node && el.contains(node),
    setLabel: (text) => el.setAttribute("aria-label", text),
    /** A language change: the card is built again in the new words (what he typed in it is gone, and that is rare). */
    relabel() { parts.keys.replaceChildren(); builtFor = null; noteKind = null; if (state.active) paint(); },
  };
}
