// HOY as data (spec §5.1): everything the page says, worked out from the model, with no DOM. render.js only draws it. Every
// count comes from `turnCount`, `candidates` and `nightFigures` (A16: no number written by hand), every word from i18n, and
// everything another person wrote goes through `clean` and `safeHref`. Pure: the same model and the same two clocks give the
// same view, which is how render.js knows nothing changed and leaves the nodes alone.

import { t, tn, lang, fmtDate, fmtMoney } from "../../i18n/index.js";
import { candidates, turnCount, nightFigures, PERMANENT_ACTIONS } from "../../core/model.js";
import { ago, kindLabel, stateLabel, windowLabel, spendLabel, scoreText, pointsOf } from "../../core/format.js";
import { dayOf, serverDay } from "../../core/time.js";
import { mailId } from "../../core/diff.js";
import { KEYS } from "../../ui/keys.js";
import { safeHref, clean } from "../shared/util.js";
import { askOf } from "../shared/stops.js";

export const NEXT_ROWS = 6;                 // "Y después · 6": with LO SIGUIENTE, at most seven items have an action (R7 rule 14)
export const SINCE_ROWS = 5;
export const SPELLED_UP_TO = 20;            // the headline spells the number as a word up to here ("Te tocan siete."), a figure above it

/** The number of the headline: a word up to twenty (from i18n, so each language spells its own), the figure after that. */
export const turnWord = (n) => (Number.isInteger(n) && n >= 0 && n <= SPELLED_UP_TO ? t("hoy.num." + n) : String(n));

// The label of each action (words that common.js already has) and the verb that is primary for each kind of item (§5.1):
// a task is done, a proposal is approved, a form the bot could not send is opened (`o`), the rest ask to be looked at (Enter).
// A form that may already have gone out (`may_have_sent`) is looked at and never opened on its posting: first check whether it was sent.
export const ACTION_LABEL = { done: "act.done", approve: "act.approve", confirm: "act.confirm", resolve: "act.resolve", open: "act.open", link: "act.viewPosting", snooze: "act.skip", withdraw: "hoy.act.withdraw", register: "hoy.act.register" };
const PRIMARY = { task: "done", test: "done", interview: "done", approve: "approve", handoff: "link", manual_apply: "link", blocked: "link", may_have_sent: "open", error: "link", missing_answers: "open", offer: "open", followup: "open", other: "open" };
const PERMANENT = new Set([...PERMANENT_ACTIONS, "done"]);          // what the drawer asks twice for, and finishing a task
const MOVES_SHOWN = new Set(["submitted", "replied", "technical_test", "interview", "offer", "rejected", "needs_gonza"]);

// A partial snapshot (a card with no score, a mail with no subject) must never put the word "null", "undefined" or "NaN" on the page.
const SAYS_NOTHING = /\b(?:null|undefined|NaN)\b/;
const sane = (text) => (SAYS_NOTHING.test(text) ? "" : text);
const whole = (x) => Math.max(0, Math.trunc(Number(x) || 0));

// What the key of an action looks like on a button, from the one table of keys.js (so a renamed key is renamed here).
export function keyShown(id) {
  const entry = KEYS.find((k) => k.id === id);
  if (!entry) throw new Error(`hoy: "${id}" is not an action of the key table`);
  return entry.display === "Enter" ? "↵" : entry.display;
}

function indexCards(snap) {
  const map = new Map();
  for (const col of snap.board || []) for (const c of col.cards) map.set(c.key, c);
  for (const c of [...((snap.awaiting_approval || {}).cards || []), ...((snap.needs_gonza || {}).cards || [])]) map.set(c.key, c);
  return map;
}

/** The ids of what can be done to a candidate, in the order the keys are listed; `first` leads. */
function actionIds(c, card) {
  const ids = [];
  if (c.task) ids.push("done");
  for (const a of c.actions || []) if (ACTION_LABEL[a] && !ids.includes(a)) ids.push(a);
  if (c.key || c.open) ids.push("open");
  if (safeHref(c.link) || (card && safeHref(card.url))) ids.push("link");
  if (c.key) ids.push("snooze");
  // What the server allows for this application (`marks`): withdrawing it, and writing down the answer a company gave. Key-only verbs of a card row (a task row
  // has neither: its `data` is the task, not the application): they never get a button.
  const marks = c.key && card && Array.isArray(card.marks) ? card.marks : [];
  if (marks.includes("withdrawn")) ids.push("withdraw");
  if (marks.some((m) => m !== "withdrawn")) ids.push("register");
  return ids;
}

/** [{ id, key, label, primary, visible, permanent }]: the primary first; `visible` are the buttons drawn, the rest work by key only. */
function actionsOf(c, card, shown) {
  const ids = actionIds(c, card);
  const first = ids.includes(PRIMARY[c.kindId]) ? PRIMARY[c.kindId] : ids[0];
  const wanted = [first, ...(shown > 1 ? ["open", "snooze"] : [])].filter((id, i, all) => id && ids.includes(id) && all.indexOf(id) === i);
  for (const id of ids) if (wanted.length < shown && !wanted.includes(id)) wanted.push(id);
  const visible = wanted.slice(0, shown);
  const order = [...visible, ...ids.filter((id) => !visible.includes(id))];
  return order.map((id) => ({ id, key: keyShown(id), label: t(ACTION_LABEL[id]), primary: id === first, visible: visible.includes(id), permanent: PERMANENT.has(id) }));
}

/** The mono line at the right of a row: what makes it pressing, in numbers the data has. */
function instrumentOf(c, card) {
  if (c.task) return c.sub;
  if (c.kindId === "followup") return [c.big, c.sub].filter(Boolean).join(" · ");
  if (c.kindId === "offer") {
    const pay = card ? pointsOf(card.salary_max) ?? pointsOf(card.salary_min) : null;
    return pay !== null ? t("hoy.row.salary", { amount: fmtMoney(pay, { digits: 0 }) }) : c.sub;
  }
  const score = scoreText(card && card.score);
  if (c.kindId === "approve") return [score, card && card.source, card && card.has_screenshot ? t("hoy.row.capture") : ""].filter(Boolean).join(" · ");
  return [score, card && card.days_in_state > 0 ? t("hoy.row.days", { n: card.days_in_state }) : ""].filter(Boolean).join(" · ");
}

function rowOf(c, cards, shown) {
  const card = c.key ? cards.get(c.key) : c.open ? cards.get(c.open) : null;
  return {
    id: c.id,
    data: c.task ? { task: String(c.task.id) } : { card: c.key },
    mark: c.task ? "task:" + c.task.id : "card:" + c.key,
    tone: c.tone,
    kindId: c.kindId,
    kind: t("hoy.kind." + c.kindId),
    company: c.task ? null : clean(c.company, 60),
    text: clean(c.task ? c.title : c.role, 140),
    why: c.why ? clean(c.why, 180) : null,
    instrument: sane(clean(instrumentOf(c, card), 90)),
    open: c.open || c.key || null,
    link: safeHref(c.link) || (card && safeHref(card.url)) || null,
    actions: actionsOf(c, card, shown),
  };
}

/** What happened since `lastMs`: new mail and the moves that matter, newest first. Nothing at all on a first visit (no last visit). */
function sinceOf(snap, lastMs, nowMs) {
  if (!(lastMs > 0)) return null;
  const quoted = (text) => (text ? "«" + text + "»" : "");
  const mails = ((snap.today || {}).emails || []).filter((m) => Date.parse(m.received_at) > lastMs).map((m) => ({
    at: Date.parse(m.received_at), mark: "mail:" + mailId(m), kind: "mail", typeId: m.kind, type: kindLabel(m.kind),
    company: clean(m.company || m.from, 50), line: clean([quoted(clean(m.subject, 100)), m.summary].filter(Boolean).join(" · "), 150),
  }));
  const moves = ((snap.activity || {}).moves || []).filter((x) => Date.parse(x.at) > lastMs && MOVES_SHOWN.has(x.to)).map((x) => ({
    at: Date.parse(x.at), mark: "card:" + x.key, kind: "move", typeId: x.to === "submitted" ? "sent" : x.to,
    type: x.to === "submitted" ? t("hoy.since.type.sent") : x.to === "needs_gonza" ? t("hoy.since.type.needs") : stateLabel(x.to),
    company: clean(x.company, 50), line: clean(x.title, 150),
  }));
  const all = [...mails, ...moves].sort((a, b) => b.at - a.at);
  const sent = moves.filter((x) => x.typeId === "sent").length;
  return {
    head: t("hoy.since.head", { ago: ago(new Date(lastMs).toISOString(), nowMs), mails: tn("hoy.since.mails", mails.length), sent: tn("hoy.since.sent", sent) }),
    total: all.length,
    rows: all.slice(0, SINCE_ROWS).map((r) => ({ ...r, hour: fmtDate(r.at, "time") })),
  };
}

/** The 48 half hours of the night. `busy` is Gonza's time: the run window was closed (an unknown window is not drawn as busy). */
function stripOf(activity, nowMs) {
  const slots = (activity && activity.slots) || [];
  const max = slots.reduce((m, s) => Math.max(m, s.seen || 0), 0);
  const day = (iso) => (dayOf(iso) === dayOf(nowMs) ? t("hoy.strip.today") : t("hoy.strip.yesterday"));
  return {
    max,
    slots: slots.map((s) => ({ at: s.at, seen: s.seen || 0, busy: s.open === false, h: max ? (s.seen || 0) / max : 0 })),
    // a night with nothing seen draws no strip, so it has no title either (the receipts already say 0)
    title: max > 0 && activity && activity.from && activity.to
      ? t("hoy.strip.title", { from: fmtDate(activity.from, "time") + " " + day(activity.from), to: fmtDate(activity.to, "time") + " " + day(activity.to) }) : null,
  };
}

function footOf(snap, nowMs) {
  const c = snap.controls || {};
  const parts = [windowLabel(c, serverDay(nowMs))];
  if (c.last_run_at) parts.push(t("hoy.foot.run", { ago: ago(c.last_run_at, nowMs) }));
  else if (c.engine === "never") parts.push(t("hoy.foot.never"));
  if (c.stopped === true) parts.push(t("hoy.foot.stopOn"));          // STOP off is the normal state and says nothing
  if (snap.claude) parts.push(spendLabel(snap.claude));
  return parts;
}

/** The demo's line about itself in the language of the page now (each text is a pair [es, en]); a link that is not http(s), or has no words, is dropped. Null without it. */
function aboutOf(about) {
  if (!about) return null;
  const say = (pair) => (Array.isArray(pair) ? pair[lang() === "en" ? 1 : 0] : pair);
  return {
    line: clean(say(about.line), 200),
    note: clean(say(about.note), 200),
    links: (about.links || []).map((l) => ({ href: safeHref(l.href), text: clean(say(l.text), 60) })).filter((l) => l.href && l.text),
  };
}

/**
 * hoyView(model, { nowMs, lastVisitMs, flat, about }) ->
 *   { about, calm, eyebrow, turn, headline, awaiting, calmLine, figures, base, strip, next, after, rows, restLabel, restHint, more, moreLabel, since, foot, snoozed }
 * `flat` is the page with no keyboard (a phone): `restHint` tells it to tap and names no key, where a desk is told what a click does.
 * `about` is what the public demo says about itself ({ line, note, links: [{ href, text }] }), null on the real panel.
 * `turn` is the real number of what waits (7 or 60: the headline is never cut), `headline` is the one run of the title ("Te tocan siete.",
 * with the number spelled by `turnWord`; the calm title when nothing waits), `next` is LO SIGUIENTE with its three buttons,
 * `rows` are the NEXT_ROWS that follow with one button each, and `more` is what does not fit ("y {M} más", which opens
 * PROCESOS filtered to what is yours). `since` is null on a first visit.
 */
export function hoyView(m, { nowMs, lastVisitMs = 0, flat = false, about = null }) {
  const snap = m.snap;
  const turn = whole(turnCount(m));
  const cands = candidates(m);
  const calm = turn === 0 && cands.length === 0;
  const cards = indexCards(snap);
  const night = nightFigures(snap);
  const first = cands[0] ? rowOf(cands[0], cards, 3) : null;
  const rows = cands.slice(1, 1 + NEXT_ROWS).map((c) => rowOf(c, cards, 1));
  const more = Math.max(0, turn - ((first ? 1 : 0) + rows.length));
  const c = snap.controls || {};
  const snoozed = snap.snoozed || { total: 0, cards: [] };
  return {
    about: aboutOf(about),
    calm,
    eyebrow: t("hoy.eyebrow", { ask: t(askOf("hoy")), hora: fmtDate(nowMs, "time") }),
    turn,
    headline: calm ? t("hoy.calm.title") : tn("hoy.head", turn, { word: turnWord(turn) }),
    awaiting: night.awaiting > 0 ? tn("hoy.awaiting", night.awaiting) : null,
    calmLine: c.last_run_at ? t("hoy.calm.run", { ago: ago(c.last_run_at, nowMs) }) : t("hoy.calm.never"),
    figures: [
      { id: "read", n: night.read, label: t("hoy.fig.read") },
      { id: "above", n: night.above, label: night.threshold == null ? t("hoy.fig.aboveAny") : t("hoy.fig.above", { threshold: night.threshold }) },
      { id: "complete", n: night.complete, label: t("hoy.fig.complete") },
      { id: "awaiting", n: night.awaiting, label: t("hoy.fig.awaiting"), gold: true },
    ],
    base: night.days > 0 ? tn("hoy.fig.base", night.days) : null,
    strip: stripOf(snap.activity, nowMs),
    next: first,
    after: first ? tn("hoy.next.after", Math.max(0, turn - 1)) : null,
    rows,
    restLabel: t("hoy.rest.label", { n: rows.length }),
    restHint: t(flat ? "hoy.rest.hint.flat" : "hoy.rest.hint"),
    more,
    moreLabel: more > 0 ? t("hoy.rest.more", { n: more }) : null,
    since: sinceOf(snap, lastVisitMs, nowMs),
    foot: footOf(snap, nowMs),
    snoozed: {
      total: whole(snoozed.total),
      items: (snoozed.cards || []).map((x) => ({ key: x.key, company: clean(x.company, 60), role: clean(x.title, 90), until: x.snoozed_until ? fmtDate(x.snoozed_until, "datetime") : "" })),
    },
  };
}
