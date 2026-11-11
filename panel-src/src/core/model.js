// What the page knows about the applications, worked out from one snapshot: whose turn each process is, the one
// next thing to do (the ticket queue), what each stop counts, and where an application stands among the five layers.
// Pure functions of the snapshot; the few strings they produce come from the dictionaries in the active language.
// Ported from the old page.

import { t, tn } from "../i18n/index.js";
import { countdown, daysSince, norm, escapeRegExp, reasonShort, reasonGroupTitle, stateLabel } from "./format.js";

// The five stops every application goes through, left to right; ids only (the names are `stage.<id>`).
export const STAGES = ["submitted", "replied", "technical_test", "interview", "offer"];
export const LIVE = [...STAGES, "ghosted"];
export const CLOSED = ["accepted", "rejected", "withdrawn"];
export const SENT_STATES = ["submitted", "replied", "technical_test", "interview", "offer", "accepted", "rejected", "ghosted"];
// The kinds of "needs Gonza", in the order they are shown; `approve` is the awaiting-approval list. `may_have_sent` (the form may
// already have gone out) sits right after `blocked`: it is as visible as the other things that stopped the robot, and always
// before the generic `other`. A kind that is not in this list is not in the queue at all, so a new kind of the server is added here.
export const REASON_KINDS = ["approve", "handoff", "missing_answers", "blocked", "may_have_sent", "error", "manual_apply", "other"];
// The five layers between a posting and a send (the story's); the names are `layer.<id>`.
export const LAYERS = ["aviso", "puntaje", "cv", "respuestas", "compuerta"];
export const STALE_DAYS = 10;
const SOON_HOURS = 48;
// Actions and marks that cannot be undone: the button needs a second, deliberate click (see twoStep in dom.js). `resolve` is here because the server has no way
// back from it: it clears the capture and the unconfirmed click before the next run fills the form again.
export const PERMANENT_ACTIONS = ["approve", "confirm", "resolve"];
export const PERMANENT_MARKS = ["withdrawn", "rejected", "accepted"];

const HOUR = 3600000;

// What each id of a candidate's `kindId` is (the label is T16's, by i18n): a task, a task that came from a technical-test or
// an interview email, an offer, a proposal to approve, the reasons a posting needs Gonza (REASON_KINDS), and a follow-up.
export const KIND_IDS = ["task", "test", "interview", "offer", "approve", "handoff", "missing_answers", "blocked", "may_have_sent", "error", "manual_apply", "other", "followup"];
const TASK_KIND = { technical_test: "test", interview_invite: "interview" };

export function countOf(s, state) {
  const col = ((s && s.board) || []).find((c) => c.state === state);
  return col ? col.total : 0;
}

// The application a task belongs to: by its key when it has one, else by a company name that the task text says out
// loud (whole word, at least three letters). A task that names several companies belongs to none of them, so a
// to-do about a batch is never pinned on one process.
export function ownerOf(task, cards) {
  // A task with a key belongs to that application or to none: never to another one that happens
  // to share a company name.
  if (task.job_key) return cards.find((c) => c.key === task.job_key) || null;
  const text = norm(task.text);
  const named = cards.filter((c) => {
    const name = norm(c.company).trim();
    return name.length >= 3 && new RegExp("(^|[^\\p{L}\\p{N}])" + escapeRegExp(name) + "($|[^\\p{L}\\p{N}])", "u").test(text);
  });
  // Exactly one application named, or the text is about something else (a batch, two companies,
  // a company with two open applications) and no process is pinned.
  return named.length === 1 ? named[0] : null;
}

/**
 * The snapshot plus what is worked out from it: `tasks` ({ task, owner }), and `procs`, every live application as
 * { card, task, turn, days, stale } sorted by whose turn it is. `turn` is "mine" (a task is on it, or an offer waits),
 * "check" (no task and the company already answered or went quiet: decide the next step) or "wait" (the company's).
 */
export function model(s) {
  const nowMs = Date.parse(s.generated_at);
  const live = [];
  for (const col of s.board) if (LIVE.includes(col.state)) live.push(...col.cards);
  const tasks = s.today.tasks.map((task) => ({ task, owner: ownerOf(task, live) }));
  const nextByKey = new Map();
  for (const item of tasks) {
    if (item.owner && !nextByKey.has(item.owner.key)) nextByKey.set(item.owner.key, item.task);
  }
  const procs = live.map((card) => {
    const task = nextByKey.get(card.key) || null;
    let turn = "wait";
    if (task) turn = "mine";
    else if (card.state === "offer") turn = "mine";
    else if (card.state === "ghosted") turn = "check";
    else if (card.state !== "submitted") turn = "check";
    const days = daysSince(card.updated_at, nowMs);
    return { card, task, turn, days, stale: card.state === "submitted" && days >= STALE_DAYS };
  });
  // Whose turn first; inside a group the nearest deadline (none last), then the longest wait. Every
  // key is a plain number, so the order is the same whichever pair the sort compares.
  const rank = { mine: 0, check: 1, wait: 2 };
  const due = (p) => (p.task && p.task.due_at ? Date.parse(p.task.due_at) : Number.MAX_SAFE_INTEGER);
  procs.sort((a, b) => rank[a.turn] - rank[b.turn] || Math.sign(due(a) - due(b)) || b.days - a.days);
  return { snap: s, nowMs, tasks, procs };
}

// ---- The next thing to do

// The stub of the ticket for a task: a big word or number and a line under it.
function taskStub(task, nowMs) {
  if (!task.due_at) return { big: t("ticket.stub.nodate"), small: true, sub: t("ticket.stub.anytime") };
  const left = countdown(task, nowMs);
  const ms = Date.parse(task.due_at) - nowMs;
  if (task.overdue || ms < 0) return { big: t("ticket.stub.overdue"), small: true, sub: left.text };
  const hours = Math.floor(ms / HOUR);
  if (hours < 1) return { big: t("ticket.stub.lessHour"), sub: left.text };
  if (hours < SOON_HOURS) return { big: t("ticket.stub.hours", { n: hours }), sub: left.text };
  return { big: t("ticket.stub.days", { n: Math.floor(hours / 24) }), sub: left.text };
}

/**
 * Everything that waits for Gonza, in the order that matters: what is late or due within two days, an offer to
 * answer, the applications that need his stamp, what stopped the robot, the other tasks, and last the follow-ups
 * nobody has put a task on. The ticket shows the first one. Each is { id, tone, kind, big, sub, small, title, why,
 * actions, link, kindId, company, role, task?, key?, open?, extra? }; `extra` marks the ones the server does not count
 * (an offer or a follow-up with no task), which turnCount adds. `kindId` is one of KIND_IDS; `company` and `role` are
 * null for a task nobody owns.
 */
export function candidates(m) {
  const s = m.snap;
  const byDue = (a, b) => Date.parse(a.task.due_at) - Date.parse(b.task.due_at);
  const dated = m.tasks.filter((x) => x.task.due_at).sort(byDue);
  const soon = (x) => x.task.overdue || Date.parse(x.task.due_at) - m.nowMs < SOON_HOURS * HOUR;
  const out = [];
  const ofTask = (item) => {
    const { task, owner } = item;
    const late = task.overdue || (task.due_at && Date.parse(task.due_at) < m.nowMs);
    return Object.assign({ id: "task:" + task.id, tone: late ? "late" : "turn", kind: t("ticket.kind.task"), title: task.text, task,
      why: owner ? owner.company + ": " + stateLabel(owner.state) : null,
      open: owner ? owner.key : null, link: task.link,
      kindId: TASK_KIND[task.kind] || "task", company: owner ? owner.company : null, role: owner ? owner.title : null }, taskStub(task, m.nowMs));
  };
  const ofCard = (c, kind, tone, big, sub, why, small, kindId) => ({
    id: "card:" + c.key, tone, kind, big, sub, small, title: c.company + " — " + c.title, key: c.key,
    why: why || reasonShort(c), actions: c.actions, link: c.url,
    kindId, company: c.company, role: c.title,
  });
  dated.filter(soon).forEach((x) => out.push(ofTask(x)));
  for (const p of m.procs) {
    if (p.card.state === "offer" && !p.task) {
      out.push(Object.assign(ofCard(p.card, t("ticket.kind.offer"), "turn", t("ticket.stub.offer"), t("ticket.stub.decide"), t("ticket.why.offer"), true, "offer"), { extra: true }));
    }
  }
  for (const c of s.awaiting_approval.cards) {
    out.push(ofCard(c, t("ticket.kind.approve"), "approve", null, null, reasonShort(c) || t("ticket.why.approve"), undefined, "approve"));
  }
  for (const kind of REASON_KINDS) {
    if (kind === "approve") continue;
    // legacy quirk: a reason kind the page does not know belongs to no group, so it is not in the queue (nor in Hoy's list)
    for (const c of s.needs_gonza.cards.filter((x) => (x.reason_kind || "other") === kind)) {
      out.push(ofCard(c, reasonGroupTitle(kind), "turn", null, null, reasonShort(c), undefined, kind));
    }
  }
  dated.filter((x) => !soon(x)).forEach((x) => out.push(ofTask(x)));
  m.tasks.filter((x) => !x.task.due_at).forEach((x) => out.push(ofTask(x)));
  for (const p of m.procs) {
    if (p.task || p.card.state === "offer") continue;
    if (p.turn === "check" || p.stale) {
      const why = p.card.state === "ghosted" ? t("ticket.why.ghosted") : p.stale ? tn("ticket.why.stale", p.days) : t("ticket.why.nextstep");
      out.push(Object.assign(ofCard(p.card, t("ticket.kind.followup"), "turn", t("ticket.stub.days", { n: p.days }), t("ticket.stub.silent"), why, undefined, "followup"), { extra: true }));
    }
  }
  return out;
}

/**
 * The one waiting count: the tasks, the approvals and the stuck forms the server counts, plus the offers and
 * follow-ups that have no task and are only in the ticket queue. It is the number in the nav, the tab title and the
 * favicon.
 */
export function turnCount(m) {
  const s = m.snap;
  const extra = candidates(m).filter((c) => c.extra).length;
  return s.today.tasks_total + s.awaiting_approval.total + s.needs_gonza.total + extra;
}

// ---- The night, and what each stop counts (spec §2.6, §5.1, §6.1)

const whole = (x) => Math.max(0, Math.trunc(Number(x) || 0));

/**
 * The figures of the night, from the raw snapshot, for the prologue and for HOY's receipts (one function, so the two can never
 * differ): postings read, those above the threshold, those with a CV and a form, those waiting for his approval, the base
 * of the first three in days, and the night's own totals (new postings in the 48 half-hour slots, state changes).
 */
export function nightFigures(snap) {
  const p = (snap && snap.pipeline) || {};
  const a = (snap && snap.activity) || {};
  return {
    read: whole(p.seen),
    above: whole(p.above),
    threshold: Number.isFinite(p.threshold) ? p.threshold : null,
    complete: whole(p.complete),
    awaiting: whole(snap && snap.awaiting_approval && snap.awaiting_approval.total),
    days: whole(p.days),
    newPostings: (a.slots || []).reduce((n, slot) => n + whole(slot && slot.seen), 0),
    moves: whole(a.moves_total),
  };
}

/**
 * What the bar of stops shows next to each name: the live processes (the five stages and the quiet ones, not what has not
 * been sent), the proposals awaiting approval, the drills due, and the engine's state as "recent", "stale" or "never".
 */
export function stopCounts(m) {
  const s = (m && m.snap) || {};          // before the first snapshot the bar draws zeros
  return {
    procesos: LIVE.reduce((n, state) => n + whole(countOf(s, state)), 0),
    cola: whole(s.awaiting_approval && s.awaiting_approval.total),
    aprender: whole(s.learn && s.learn.due),
    sistema: (s.controls && s.controls.engine) || "never",
  };
}

// ---- Where an application stands among the five layers

/**
 * `at` is the layer it is in (an index into LAYERS; 5 once sent; -1 for what is not on the way), `who` whose turn it
 * is there: the robot's ("robot"), Gonza's ("you"), neither because it already went out ("done") or closed
 * ("closed"). `text` says what happens, in the active language.
 */
export function layerOf(c) {
  if (SENT_STATES.includes(c.state)) return { at: LAYERS.length, who: "done", text: "" };
  if (c.state === "scored") return { at: 2, who: "robot", text: t("layer.note.scored") };
  if (c.state === "prepared") {
    // "approve" is offered exactly while a dry-run capture waits for Gonza
    if (c.approved) return { at: 4, who: "robot", text: t("layer.note.approved") };
    if (c.actions.includes("approve")) return { at: 4, who: "you", text: t("layer.note.awaiting") };
    return { at: 3, who: "robot", text: t("layer.note.dryrun") };
  }
  if (c.state === "needs_gonza") {
    const kind = c.reason_kind || "other";
    const at = kind === "missing_answers" || kind === "blocked" || kind === "error" ? 3 : 4;
    return { at, who: "you", text: reasonShort(c) || t("layer.note.look") };
  }
  return { at: -1, who: "closed", text: "" };
}

/**
 * The little path of five dots under a row of the queue, or null when the application is not on the way or already
 * went out. `dots` are the class of each dot: "done", "here" (or "here you" when it waits for Gonza) and "".
 */
export function pathOf(c) {
  const l = layerOf(c);
  if (l.at < 0 || l.who === "done") return null;
  const id = LAYERS[l.at];
  return {
    at: l.at, who: l.who, id, name: t("layer." + id), text: l.text,
    dots: LAYERS.map((_, i) => (i < l.at ? "done" : i === l.at ? (l.who === "you" ? "here you" : "here") : "")),
  };
}
