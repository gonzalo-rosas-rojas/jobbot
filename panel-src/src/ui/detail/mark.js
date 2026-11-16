// "Registrar una respuesta": what the company told him, recorded by hand. No change of state is one click away: on
// 2026-10-24 three clicks in nine seconds took an application from "respondió" to "oferta", because each button acted at
// once and the button of the next stage then stood under the pointer. So it goes in three steps:
//   choose   one button per answer the server allows from here, said the way it happened ("Me citaron a entrevista");
//            a click only picks it (aria-pressed) and asks
//   ask      under the choices: the question in plain words, what will change ("Agility IO pasa de «Respondió» a
//            «Oferta».") and "Sí, registrarlo" / "No, volver". The yes ignores a press in its first ARM_MIN_MS and a held
//            key, so a double click on a choice cannot answer for him
//   receipt  once it went through: what changed, "Deshacer" while the server allows it (`card.undo`, POST /api/mark/undo),
//            and "Registrar otra respuesta", which brings the choices back; they never come back on their own
// When the drawer opens on an application whose last change can still be undone, that line and its "Deshacer" stand above
// the choices. One note field (200 characters at most) goes with the answer. The note is kept as it is typed, per
// application, so a redraw of the drawer (after an action, a new language), stepping away and coming back, or an accidental
// Esc does not lose it; it is cleared only when a mark went through. When the server refuses a mark or an undo (the state
// moved meanwhile), the note stays and the refusal is written under the buttons in the server's own words.

import { h, ARM_MIN_MS, heldKey } from "../../core/dom.js";
import { t, fmtDate } from "../../i18n/index.js";
import { PERMANENT_MARKS } from "../../core/model.js";
import { markPhrase, markQuestion, stateLabel } from "../../core/format.js";
import { memo, draftOf, setDraft } from "./memo.js";
import { perform } from "./act.js";
import { actButton } from "../act.js";

/** The remembered slot (`memo.noted`, `memo.asking`) when it belongs to the application `key`, else null. */
const ownSlot = (slot, key) => (slot && slot.key === key ? slot : null);

const NOTE_MAX = 200; // the most characters the note field takes: the panel's own choice (the server cleans a note to 500)

/** The first thing to do in "Registrar una respuesta": an answer to pick, or the way back after one went through; null when the section is not drawn. */
export const firstMarkTarget = () => document.querySelector("#d-mark [data-mark], #mark-again, #mark-undo");

/** The last change of `card` the server still takes back ({ from, to, at, until }), or null. */
const undoOf = (card) => (card.undo && card.undo.from && card.undo.to === card.state ? card.undo : null);

const nameOf = (card) => card.company || t("detail.mark.it");

/** Remembers the server's refusal of a mark or an undo, in its own words, to write it under the buttons. */
function refuse(ctx, key, r) {
  memo.refusal = { key, text: ctx.session.said(r, t("err.action")) };
}

function undoButton(ctx, card) {
  const key = card.key;
  return actButton({
    id: "mark-undo", label: t("detail.mark.undo"),
    onclick: (e) => perform(ctx, e.currentTarget, "/api/mark/undo", { key }, {
      cue: "undo",
      onOk: () => { memo.noted = null; memo.asking = null; memo.refusal = null; },
      onFail: (r) => refuse(ctx, key, r),
    }),
  });
}

/** After a mark went through: what changed, the way back while there is one, and the way to record another answer. */
function receiptView(ctx, card, noted, undo) {
  const again = (card.marks || []).length > 0
    ? actButton({
      id: "mark-again", label: t("detail.mark.again"), cls: "quiet",
      onclick: (e) => {
        memo.noted = null;
        const fresh = markView(ctx, card);
        e.currentTarget.closest("#d-mark").replaceWith(fresh);
        const first = fresh.querySelector("[data-mark]");
        if (first) first.focus();
      },
    })
    : null;
  const buttons = [undo ? undoButton(ctx, card) : null, again].filter(Boolean);
  return [
    h("p", { id: "mark-done", class: "d-done", tabindex: "-1" }, t("detail.mark.done", { company: nameOf(card), from: stateLabel(noted.from), to: stateLabel(noted.to) })),
    undo ? h("p", { class: "muted small" }, t("detail.mark.undo_until", { hora: fmtDate(undo.until, "time") })) : null,
    buttons.length ? h("div", { class: "cluster" }, buttons) : null,
  ];
}

/** The note field; typing keeps the draft for `key` and takes the old refusal away. */
function noteField(key, refusal) {
  return h("input", {
    id: "mark-note", class: "input", type: "text", maxlength: String(NOTE_MAX), autocomplete: "off", value: draftOf(key), placeholder: t("drawer.mark.placeholder"),
    oninput: (e) => {
      setDraft(key, e.target.value);
      memo.refusal = null;
      refusal.hidden = true;
    },
  });
}

/** The line that says the last change can still be taken back, with its button. */
function lastChangeView(ctx, card, undo) {
  return h("div", { class: "d-last" },
    h("p", { class: "muted small" }, t("detail.mark.last", { from: stateLabel(undo.from), to: stateLabel(undo.to), hora: fmtDate(undo.until, "time") })),
    undoButton(ctx, card));
}

/** The question for the answer he picked and what it will change: the two paragraphs under the choices. */
function questionView(card, mark, from) {
  const change = t(PERMANENT_MARKS.includes(mark) ? "detail.mark.change.closes" : "detail.mark.change", { company: nameOf(card), from: stateLabel(from), to: stateLabel(mark) });
  return [
    h("p", { id: "mark-question", class: "d-question" }, markQuestion(mark, from)),
    h("p", { class: "d-change" }, change),
  ];
}

export function markView(ctx, card) {
  const key = card.key;
  const marks = card.marks || [];
  const noted = ownSlot(memo.noted, key);
  const undo = undoOf(card);
  if (!noted && !undo && marks.length === 0) return null;
  const refused = memo.refusal && memo.refusal.key === key ? memo.refusal.text : "";
  const refusal = h("p", { id: "mark-refusal", class: "d-refusal", hidden: !refused }, refused);
  const section = h("section", { id: "d-mark", class: "d-sec", "aria-labelledby": "d-mark-h" }, h("h3", { id: "d-mark-h" }, t("drawer.mark.title")));
  if (noted) {
    section.append(...receiptView(ctx, card, noted, undo).filter(Boolean), refusal);
    return section;
  }

  const from = card.state;
  const note = noteField(key, refusal);
  const ask = h("div", { id: "mark-ask", class: "d-ask", role: "group", "aria-labelledby": "mark-question", hidden: true });

  const run = (button, mark) => perform(ctx, button, "/api/mark", { key, state: mark, note: note.value.trim() || null }, {
    cue: mark === "withdrawn" ? "withdraw" : "register",
    onOk: () => { setDraft(key, ""); memo.refusal = null; memo.asking = null; memo.noted = { key, from, to: mark }; },
    onFail: (r) => { refuse(ctx, key, r); memo.asking = null; },
  }).then(() => {
    const done = document.getElementById("mark-done");
    if (done && ownSlot(memo.noted, key)) done.focus(); // and into view: the section shrank under him (its scroll-margin clears the sticky actions row)
  });

  /** Draws the question for the answer he picked, or takes it away. */
  function paintAsk() {
    const asking = ownSlot(memo.asking, key);
    const mark = asking && marks.includes(asking.state) ? asking.state : null;
    for (const b of section.querySelectorAll("[data-mark]")) b.setAttribute("aria-pressed", String(b.dataset.mark === mark));
    if (!mark) {
      ask.hidden = true;
      ask.replaceChildren();
      return;
    }
    const shownAt = Date.now();
    const yes = actButton({
      id: "mark-yes", label: t("detail.mark.yes"), primary: true,
      onclick: (e) => {
        if (heldKey() || Date.now() - shownAt < ARM_MIN_MS) return; // the second half of a double click, or Enter held down
        run(e.currentTarget, mark);
      },
    });
    const no = actButton({
      id: "mark-no", label: t("detail.mark.no"), cls: "quiet",
      onclick: () => {
        memo.asking = null;
        paintAsk();
        const picked = section.querySelector(`[data-mark="${CSS.escape(mark)}"]`);
        if (picked) picked.focus();
      },
    });
    ask.replaceChildren(...questionView(card, mark, from), h("div", { class: "cluster" }, yes, no));
    ask.hidden = false;
  }

  const choices = marks.map((mark) => actButton({
    id: "mark-" + mark, label: markPhrase(mark, from), "data-mark": mark, "aria-pressed": "false",
    onclick: () => {
      if (heldKey()) return;
      memo.asking = { key, state: mark };
      memo.refusal = null;
      refusal.hidden = true;
      if (ctx.sound) ctx.sound.play("arm");
      paintAsk();
      const yes = ask.querySelector("#mark-yes");
      if (yes) yes.focus({ preventScroll: true });
    },
  }));

  const parts = [
    undo ? lastChangeView(ctx, card, undo) : null,
    marks.length ? h("p", { class: "muted small" }, t("detail.mark.hint")) : null,
    marks.length ? h("div", { class: "field" }, h("label", { for: "mark-note" }, t("detail.mark.note")), note) : null,
    marks.length ? h("div", { class: "d-marks", role: "group", "aria-labelledby": "d-mark-h" }, choices) : null,
    ask,
    refusal,
  ];
  section.append(...parts.filter(Boolean));
  paintAsk(); // a redraw (a new language, the drawer read again) keeps the question he had open
  return section;
}
