// The answer at the top of the drawer: where this application is and what comes now, as ONE sentence and one line under
// it, worked out with the same rules as the rows of Procesos (core/model.js: whose turn it is, the task that is next, the
// days without news) and the five layers for what has not been sent yet. Pure: a card, the model and the language in; no
// DOM, so it is tested in Node.
//
//   answerOf(card, model) -> { tone, turn, sentence, sub, task, tasks, late, sig, nowMs, layer }
//     tone      "you" (amber: it is his turn) | "late" (red) | "robot" (blue: jobbot is on it) | "ok" (green) | "calm"
//     turn      { key, text, cls } the chip that says whose turn it is, or null (closed)
//     task      the next task of this application (the one the Procesos row shows), or null
//     tasks     every open task of this application (by its key, or by the company named in the text)
//     late      one of the tasks is overdue
//     sig       a language-free signature of the situation, to see that an action changed it

import { layerOf, CLOSED, LIVE, STALE_DAYS } from "../../core/model.js";
import { t, tn, has } from "../../i18n/index.js";
import { actionHint, ago, asSentence, daysSince, reasonShort, stateLabel } from "../../core/format.js";

const isLate = (task, nowMs) => !!task.overdue || (task.due_at != null && Date.parse(task.due_at) < nowMs);

/** The open tasks of one application: the ones that carry its key and the ones that name its company (model.js `ownerOf`). */
export function tasksOf(card, m) {
  if (!m) return [];
  return m.tasks.filter(({ task, owner }) => task.job_key === card.key || (owner != null && owner.key === card.key)).map((x) => x.task);
}

/** A phrase made into a sentence. The product name stays as it is written (lower case) when the phrase starts with it. */
export const say = (text) => (/^jobbot\b/.test(text) ? (/[.!?»]$/.test(text) ? text : text + ".") : asSentence(text));

const word = (prefix, state, fallback) => (has(prefix + state) ? t(prefix + state) : t(prefix + fallback));

export function answerOf(card, m) {
  const nowMs = m ? m.nowMs : Date.now();
  const tasks = tasksOf(card, m);
  const proc = m ? m.procs.find((p) => p.card.key === card.key) || null : null;
  const task = (proc && proc.task) || tasks[0] || null;
  const layer = layerOf(card);
  const late = tasks.some((x) => isLate(x, nowMs));
  const out = { tone: "calm", turn: null, sentence: "", sub: null, task, tasks, late, nowMs, layer, sig: "" };
  const last = () => t("detail.now.last", { when: ago(card.updated_at, nowMs) });

  if (layer.at >= 0 && layer.who !== "done") {
    // not sent yet: the five layers say where it stopped
    if (card.state === "needs_gonza") {
      out.tone = "you";
      out.turn = { key: "mine", text: t("turn.mine"), cls: "you" };
      out.sentence = say(reasonShort(card) || t("layer.note.look"));
      out.sub = card.actions.length ? actionHint(card.actions[0]) || null : null;
    } else if (layer.who === "you") {
      out.tone = "you";
      out.turn = { key: "mine", text: t("turn.mine"), cls: "you" };
      out.sentence = t("detail.now.approve");
      out.sub = t("ticket.why.approve");
    } else {
      const kind = card.state === "scored" ? "scored" : card.approved ? "approved" : "dryrun";
      out.tone = "robot";
      out.turn = { key: "robot", text: t("status.robot"), cls: "robot" };
      out.sentence = t("detail.now.robot." + kind);
      out.sub = t("detail.now.robot." + kind + ".sub");
    }
  } else if (LIVE.includes(card.state)) {
    const days = proc ? proc.days : daysSince(card.updated_at, nowMs);
    const stale = proc ? proc.stale : card.state === "submitted" && days >= STALE_DAYS;
    let turn = "wait";
    if (proc) turn = proc.turn;
    else if (task || card.state === "offer") turn = "mine";
    else if (card.state !== "submitted") turn = "check";
    if (turn === "mine") {
      out.tone = late ? "late" : "you";
      out.turn = { key: turn, text: t("turn.mine"), cls: late ? "late" : "you" };
      out.sentence = word("detail.now.mine.", card.state, "other");
      if (!task && card.state === "offer") out.sub = t("ticket.why.offer");
    } else if (turn === "check") {
      out.tone = "you";
      out.turn = { key: turn, text: t("turn.check"), cls: "you" };
      out.sentence = word("detail.now.check.", card.state, "other");
      out.sub = t("detail.now.check.sub");
    } else if (stale) {
      out.tone = "late";
      out.turn = { key: turn, text: t("turn.wait"), cls: "" };
      out.sentence = tn("detail.now.stale", days);
      out.sub = t("detail.now.stale.sub");
    } else {
      out.turn = { key: turn, text: t("turn.wait"), cls: "" };
      out.sentence = t("detail.now.wait");
      out.sub = last();
    }
  } else if (CLOSED.includes(card.state)) {
    out.tone = card.state === "accepted" ? "ok" : "calm";
    out.sentence = t("detail.now." + card.state);
    out.sub = last();
  } else {
    out.sentence = t("detail.now.state", { state: stateLabel(card.state) });
    out.sub = last();
  }
  out.sig = [card.state, layer.who, layer.at, card.approved ? 1 : 0, out.turn ? out.turn.key : "-", out.tone].join("|");
  return out;
}
