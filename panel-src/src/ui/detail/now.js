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

const endWithPeriod = (text) => (/[.!?»]$/.test(text) ? text : text + ".");

/** A phrase made into a sentence. The product name stays as it is written (lower case) when the phrase starts with it. */
export const say = (text) => (/^jobbot\b/.test(text) ? endWithPeriod(text) : asSentence(text));

const phraseFor = (prefix, state, fallback) => (has(prefix + state) ? t(prefix + state) : t(prefix + fallback));

const updatedAgo = (card, nowMs) => t("detail.now.last", { when: ago(card.updated_at, nowMs) });

const mineTurn = (cls) => ({ key: "mine", text: t("turn.mine"), cls });

/** Which of the three texts of "jobbot is on it" fits: only scored, approved but not yet run, or run dry. */
function robotKind(card) {
  if (card.state === "scored") return "scored";
  if (card.approved) return "approved";
  return "dryrun";
}

/** Not sent yet: the five layers say where it stopped. */
function answerUnsent(card, layer) {
  if (card.state === "needs_gonza") {
    return {
      tone: "you",
      turn: mineTurn("you"),
      sentence: say(reasonShort(card) || t("layer.note.look")),
      sub: card.actions.length ? actionHint(card.actions[0]) || null : null,
    };
  }
  if (layer.who === "you") {
    return { tone: "you", turn: mineTurn("you"), sentence: t("detail.now.approve"), sub: t("ticket.why.approve") };
  }
  const kind = robotKind(card);
  return {
    tone: "robot",
    turn: { key: "robot", text: t("status.robot"), cls: "robot" },
    sentence: t("detail.now.robot." + kind),
    sub: t("detail.now.robot." + kind + ".sub"),
  };
}

/** Whose turn it is on a live application: the Procesos row's own answer when there is one, else worked out from the card. */
function turnOf(card, proc, task) {
  if (proc) return proc.turn;
  if (task || card.state === "offer") return "mine";
  if (card.state !== "submitted") return "check";
  return "wait";
}

/** Sent and still open: his turn, a check of his, a long silence, or simply waiting. */
function answerLive(card, { proc, task, late, nowMs }) {
  const days = proc ? proc.days : daysSince(card.updated_at, nowMs);
  const stale = proc ? proc.stale : card.state === "submitted" && days >= STALE_DAYS;
  const turn = turnOf(card, proc, task);
  if (turn === "mine") {
    return {
      tone: late ? "late" : "you",
      turn: { key: turn, text: t("turn.mine"), cls: late ? "late" : "you" },
      sentence: phraseFor("detail.now.mine.", card.state, "other"),
      sub: !task && card.state === "offer" ? t("ticket.why.offer") : null,
    };
  }
  if (turn === "check") {
    return {
      tone: "you",
      turn: { key: turn, text: t("turn.check"), cls: "you" },
      sentence: phraseFor("detail.now.check.", card.state, "other"),
      sub: t("detail.now.check.sub"),
    };
  }
  if (stale) {
    return { tone: "late", turn: { key: turn, text: t("turn.wait"), cls: "" }, sentence: tn("detail.now.stale", days), sub: t("detail.now.stale.sub") };
  }
  return { tone: "calm", turn: { key: turn, text: t("turn.wait"), cls: "" }, sentence: t("detail.now.wait"), sub: updatedAgo(card, nowMs) };
}

/** Closed: accepted is the good ending, the others are calm. */
function answerClosed(card, nowMs) {
  return { tone: card.state === "accepted" ? "ok" : "calm", turn: null, sentence: t("detail.now." + card.state), sub: updatedAgo(card, nowMs) };
}

/** A state none of the groups names: it says the state and when it last changed. */
function answerOther(card, nowMs) {
  return { tone: "calm", turn: null, sentence: t("detail.now.state", { state: stateLabel(card.state) }), sub: updatedAgo(card, nowMs) };
}

export function answerOf(card, m) {
  const nowMs = m ? m.nowMs : Date.now();
  const tasks = tasksOf(card, m);
  const proc = m ? m.procs.find((p) => p.card.key === card.key) || null : null;
  const task = (proc && proc.task) || tasks[0] || null;
  const layer = layerOf(card);
  const late = tasks.some((x) => isLate(x, nowMs));
  let said;
  if (layer.at >= 0 && layer.who !== "done") said = answerUnsent(card, layer);
  else if (LIVE.includes(card.state)) said = answerLive(card, { proc, task, late, nowMs });
  else if (CLOSED.includes(card.state)) said = answerClosed(card, nowMs);
  else said = answerOther(card, nowMs);
  const sig = [card.state, layer.who, layer.at, card.approved ? 1 : 0, said.turn ? said.turn.key : "-", said.tone].join("|");
  return { tone: said.tone, turn: said.turn, sentence: said.sentence, sub: said.sub, task, tasks, late, nowMs, layer, sig };
}
