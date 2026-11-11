// The content of the drawer for one application: "everything about this one application, and the buttons where it
// stopped". The drawer (ui/drawer.js) owns everything around it: the step buttons with "n of N", the close button, focus,
// history, and the fetch of /api/job. This file turns the answer into DOM; the parts live in ui/detail/*.
//
//   renderDetail(detail, card, ctx) -> Node
//     detail  the JobDetail the server sent for /api/job?key= (FEATURES.md section 2.6 lists what the drawer must show)
//     card    `detail.card`, the card as the server describes it now
//     ctx     the room context (t, tn, lang, session, sound, drawer, toast, go, model, reduced, ...); `ctx.drawer.reload()`
//             re-reads and redraws the drawer after an action; `ctx.session.act(path, body, opts)` does the action and
//             refreshes the snapshot; `ctx.session.blob(path)` fetches the dry-run capture with the token
//   It returns ONE node (a div) that contains the application's title as an <h2>. The drawer calls it again after every
//   action, when the language changes and when it steps to another application, and NEVER when a snapshot refreshes (that
//   would lose the picture, the half-typed note and the open folds). Its words follow the active language.
//   releaseDetail() is how the drawer says "this application is gone" (it closed): the picture's blob URL is revoked and
//   what was remembered about it is dropped. Nothing here keeps a timer or a listener alive after a drawing is replaced,
//   except the one that disarms a two-step button.
//
// Top to bottom, so that the answer to "where is this and what do I do now?" is the first thing and the evidence follows:
//   header     title, state, score, place, salary, links
//   answer     ONE sentence (state + whose turn + the next task, with the rules of the Procesos rows), one line under it, the
//              next task when it is what is next, and under that the actions row, which stays in view while the drawer scrolls
//   Recorrido  the five layers (or, once sent, the five stops), where it stopped and what is missing
//   evidence   the reason, the tasks and emails of this application, the dry-run capture, "Registrar una respuesta",
//              the history, how the score was worked out, what to review, the dossier and the posting's own text

import { h } from "../core/dom.js";
import { actButton, outLink } from "./act.js";
import { t, fmtMoney } from "../i18n/index.js";
import { PERMANENT_ACTIONS } from "../core/model.js";
import { actionHint, actionLabel, ago, countdown, kindLabel, pointsOf, reasonShort, stateLabel } from "../core/format.js";
import { memo, adopt, release } from "./detail/memo.js";
import { answerOf } from "./detail/now.js";
import { journeyView } from "./detail/journey.js";
import { shotView, noShotView } from "./detail/shot.js";
import { markView } from "./detail/mark.js";
import { timelineView } from "./detail/timeline.js";
import { scoreView } from "./detail/score.js";
import { armed, perform, sayArming } from "./detail/act.js";

export function releaseDetail() {
  release();
}

const httpUrl = (url) => typeof url === "string" && /^https?:\/\//i.test(url);
const upperFirst = (text) => text.charAt(0).toUpperCase() + text.slice(1);

/** A fold (`details`) that remembers whether it is open while the drawer is drawn again. */
function fold(id, open, label, ...kids) {
  const isOpen = memo.folds.has(id) ? memo.folds.get(id) : open;
  return h("details", { class: "d-fold", "data-fold": "d:" + id, open: isOpen, ontoggle: (e) => memo.folds.set(id, e.currentTarget.open) },
    h("summary", {}, label), ...kids);
}

function section(id, title, ...kids) {
  return h("section", { class: "d-sec", "aria-labelledby": "d-" + id + "-h" }, h("h3", { id: "d-" + id + "-h" }, title), ...kids);
}

// ---- header

function salaryText(detail) {
  const money = (x) => fmtMoney(Math.round(x), { digits: 0 });
  const known = (x) => (x !== null && x > 0 ? x : null); // zero or less is "not published"
  const lo = known(pointsOf(detail.salary_min));
  const hi = known(pointsOf(detail.salary_max));
  if (lo !== null && hi !== null) return t("drawer.salary.range", { range: t("drawer.salary.between", { from: money(lo), to: money(hi) }) });
  if (lo !== null) return t("detail.salary.min", { amount: money(lo) });
  if (hi !== null) return t("detail.salary.max", { amount: money(hi) });
  return t("drawer.salary.none");
}

function headerView(detail, card) {
  const place = typeof detail.location === "string" ? detail.location.trim() : "";
  const remote = detail.remote && !/remot/i.test(place) ? t("drawer.remote") : "";
  const posting = httpUrl(card.url) ? outLink(card.url, t("act.viewPosting")) : null;
  const formWords = t("act.openForm");
  const form = httpUrl(detail.apply_url) && detail.apply_url !== card.url ? outLink(detail.apply_url, posting ? formWords : upperFirst(formWords)) : null;
  return h("header", { class: "d-head" },
    h("h2", { id: "detail-title" }, [card.company, card.title].filter(Boolean).join(" — ")),
    h("p", { class: "d-meta" },
      h("span", { class: "chip" }, stateLabel(card.state)),
      h("span", { class: "d-score" }, t("drawer.score", { score: card.score })),
      place ? h("span", {}, place) : null,
      remote ? h("span", {}, remote) : null),
    h("p", { class: "d-salary muted" }, salaryText(detail)),
    posting || form ? h("p", { class: "d-links" }, posting, posting && form ? " " + t("common.and") + " " : null, form) : null);
}

// ---- the answer and the actions row

function taskLine(task, nowMs) {
  const left = countdown(task, nowMs);
  return h("p", { class: "d-task", "data-dtask": task.id },
    h("span", { id: "d-now-task", class: "d-task-text wrap-any" }, task.text),
    left ? h("span", { class: "chip " + (left.tone === "late" ? "late" : left.tone === "soon" ? "you" : "") }, left.text) : null);
}

function nowView(a, inline) {
  return h("section", { id: "d-now", class: "d-now", "data-tone": a.tone, "aria-label": t("detail.now.aria") },
    a.turn ? h("p", { class: "d-turn" }, h("span", { class: "chip " + a.turn.cls }, a.turn.text)) : null,
    h("h3", { class: "d-sentence" }, a.sentence),
    a.sub ? h("p", { class: "d-sub" }, a.sub) : null,
    inline ? taskLine(inline, a.nowMs) : null);
}

function actionButton(ctx, card, action, primary) {
  const label = actionLabel(action);
  const permanent = PERMANENT_ACTIONS.includes(action);
  const run = (button) => perform(ctx, button, "/api/" + action, { key: card.key }, {
    cue: action === "resolve" ? "undo" : action === "approve" ? "approve" : action === "confirm" ? "sent" : null,
    staged: action === "approve" ? "act.approved" : action === "confirm" ? "act.noted" : null,
  });
  return actButton({
    id: "act-" + action, label, primary, "data-action": action, title: actionHint(action) || null,
    onclick: permanent ? armed(label, run) : (e) => run(e.currentTarget),
  });
}

function doneButton(ctx, task, described) {
  const label = t("act.done");
  const run = (button) => perform(ctx, button, "/api/task/done", { id: task.id }, { cue: "done" });
  return actButton({
    id: "d-done-" + task.id, label, primary: true, "data-done": task.id, title: t("act.done.hint"), "aria-describedby": described || null, onclick: armed(label, run),
  });
}

/** The jump to "Registrar una respuesta": its first answer (or, after one went through, the way to record another) is focused and brought into view. */
function jumpButton(sole) {
  return actButton({
    id: "d-jump", label: t("drawer.mark.title"), primary: sole, cls: sole ? null : "quiet", "data-jump": "mark",
    onclick: () => {
      const target = document.querySelector("#d-mark [data-mark], #mark-again, #mark-undo");
      if (!target) return;
      target.scrollIntoView({ block: "center" });
      target.focus({ preventScroll: true });
    },
  });
}

function actionsView(ctx, card, inline) {
  const buttons = card.actions.map((action, i) => actionButton(ctx, card, action, i === 0));
  if (card.actions.length === 0 && inline) {
    buttons.push(doneButton(ctx, inline, "d-now-task"));
    if (httpUrl(inline.link)) buttons.push(outLink(inline.link, t("act.openLink")));
  }
  if (card.actions.length === 0 && card.marks.length > 0) buttons.push(jumpButton(buttons.length === 0));
  if (buttons.length === 0) return null;
  return h("div", { class: "d-actions", role: "group", "aria-label": t("detail.actions.aria") }, buttons);
}

// ---- evidence

function reasonView(card, a) {
  const onTheWay = a.layer.at >= 0 && a.layer.who !== "done";
  const short = reasonShort(card);
  const parts = [];
  if (!onTheWay && short) parts.push(h("p", { class: "d-reason" }, short));
  if (card.reason) parts.push(fold("reason", card.state === "needs_gonza", t("drawer.reason.full"), h("pre", { class: "d-pre" }, card.reason)));
  else if (card.note) parts.push(h("p", { class: "d-note wrap-any" }, t("drawer.note", { note: card.note })));
  return parts.length ? h("div", { class: "d-block" }, parts) : null;
}

function tasksView(ctx, a, shownAbove) {
  const list = a.tasks.filter((task) => task !== shownAbove);
  if (list.length === 0) return null;
  return section("tasks", t(shownAbove ? "detail.tasks.other" : "drawer.tasks"),
    h("ul", { class: "d-items" }, list.map((task) => {
      const left = countdown(task, a.nowMs);
      const textId = "d-task-" + task.id;
      return h("li", { class: "d-item", "data-dtask": task.id },
        h("div", { class: "d-item-main" },
          h("span", { id: textId, class: "wrap-any" }, task.text),
          left ? h("span", { class: "chip " + (left.tone === "late" ? "late" : left.tone === "soon" ? "you" : "") }, left.text) : null,
          httpUrl(task.link) ? outLink(task.link, t("act.openLink")) : null),
        doneButton(ctx, task, textId));
    })));
}

function mailsView(card, m, nowMs) {
  const mails = m ? m.snap.today.emails.filter((mail) => mail.job_key === card.key) : [];
  if (mails.length === 0) return null;
  return section("mails", t("drawer.mails"),
    h("ul", { class: "d-items" }, mails.map((mail) => h("li", { class: "d-item", "data-email": mail.kind },
      h("div", { class: "d-item-main" },
        h("span", { class: "chip" }, kindLabel(mail.kind)),
        h("span", { class: "wrap-any" }, mail.subject || t("common.noSubject"))),
      mail.summary ? h("p", { class: "muted small wrap-any" }, mail.summary) : null,
      h("p", { class: "muted small wrap-any" }, ago(mail.received_at, nowMs) + (mail.from ? " · " + t("detail.mail.from", { from: mail.from }) : ""))))));
}

function gapsView(ctx, detail) {
  const gaps = detail.gaps || [];
  if (gaps.length === 0) return null;
  const pending = gaps.some((gap) => !gap.seen);
  return section("gaps", t("drawer.gaps"),
    h("ul", { class: "d-list" }, gaps.map((gap) => h("li", { class: "wrap-any" }, t(gap.seen ? "drawer.gap.seen" : "drawer.gap.todo", { topic: gap.topic })))),
    pending ? h("div", {}, actButton({ label: t("detail.gaps.go"), onclick: () => ctx.go("aprender") })) : null);
}

function textFolds(detail) {
  return [
    detail.dossier
      ? fold("dossier", true, t("drawer.dossier"), h("pre", { class: "d-pre" }, detail.dossier))
      : h("p", { class: "muted" }, t("drawer.dossier.none")),
    detail.description ? fold("description", false, t("drawer.description"), h("pre", { class: "d-pre" }, detail.description)) : null,
  ];
}

export function renderDetail(detail, card, ctx) {
  adopt(card.key);
  sayArming(ctx.announce, ctx.sound);
  const m = ctx.model();
  const a = answerOf(card, m);
  const inline = card.actions.length === 0 ? a.task : null; // the task that is next sits with the answer, with its own button
  const awaiting = card.state === "prepared" && a.layer.who === "you";
  // a capture is expected from the dry run on; before that, and once the application went out, its absence says nothing
  const shot = card.has_screenshot ? shotView(ctx, card) : a.layer.at >= 3 && a.layer.who !== "done" ? noShotView() : null;

  const now = nowView(a, inline);
  if (memo.seen && memo.seen !== a.sig && !(ctx.reduced && ctx.reduced())) now.classList.add("fresh"); // the flash is motion: under reduced motion it is a cut
  memo.seen = a.sig;

  return h("div", { class: "detail-body d-body", "data-job": card.key },
    headerView(detail, card),
    now,
    actionsView(ctx, card, inline),
    journeyView(detail, card, m, a),
    awaiting ? shot : null,
    reasonView(card, a),
    tasksView(ctx, a, inline),
    mailsView(card, m, a.nowMs),
    awaiting ? null : shot,
    markView(ctx, card),
    timelineView(detail),
    scoreView(detail, card),
    gapsView(ctx, detail),
    textFolds(detail));
}
