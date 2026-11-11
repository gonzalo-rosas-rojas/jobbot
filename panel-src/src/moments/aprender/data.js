// What APRENDER knows, worked out from one snapshot (spec §5.4), with no DOM and no words: the questions the form-filler could not answer
// (from the reason it left on each stuck application), the interview of the next evening (the "víspera"), what its dossier says, the
// patterns of the rejections and the state of the review. Pure functions: the same snapshot gives the same answer.
//
// What the snapshot does NOT carry, and so is never invented here: the answer bank itself (profile/answers.toml: how many answers, how
// many times each was used), the time between sending and a rejection, and the text of any answer. The view says "sin dato" for them.

import { clean, safeHref } from "../shared/util.js";
import { pointsOf } from "../../core/format.js";

const LABEL_MAX = 120;
export const MIN_REJECTED = 5;            // fewer rejections than this show no pattern, only how many there are
const LIVE = ["submitted", "replied", "technical_test", "interview", "offer", "ghosted", "accepted"];
const EVE_LOOKBACK_MS = 36 * 3600000;     // a mailed interview date older than this is not the eve of anything
const MARKER = "faltan respuestas:";

const whole = (x) => Math.max(0, Math.trunc(Number(x) || 0));
const list = (x) => (Array.isArray(x) ? x : []);
const columnOf = (snap, state) => list(snap && snap.board).find((col) => col && col.state === state) || { total: 0, cards: [] };

// ---- the questions the form-filler could not answer

/**
 * The labels listed after "faltan respuestas:" in the reason the filler left (the same reading as `missing_labels` of src/handoff.rs):
 * wherever that text sits, stopping at the dash that starts the failures that are not questions. Each label is cleaned (it came from a
 * web page) and cut; blank ones are dropped.
 */
export function missingLabels(reason) {
  const text = String(reason == null ? "" : reason);
  const start = text.indexOf(MARKER);
  if (start < 0) return [];
  const listed = text.slice(start + MARKER.length).split(" — ")[0];
  return listed.split(";").map((label) => clean(label, LABEL_MAX)).filter(Boolean);
}

/** The identity of a question: lower case, no accents, letters and digits only, so "Salary (CLP) *" and "salary clp" are one. */
export function questionKey(label) {
  const plain = String(label == null ? "" : label).normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase().replace(/[^\p{L}\p{N}]+/gu, " ").trim();
  return plain || String(label == null ? "" : label).trim().toLowerCase();
}

/** The spelling shown: the label as the form wrote it, without the trailing asterisk of a required field. */
const shownLabel = (label) => clean(label.replace(/[\s*:]+$/u, ""), LABEL_MAX) || label;

/**
 * The forms stuck on questions, by question: { groups: [{ id, label, forms, cards: [{ key, company, title, score, url }] }], unnamed, forms, partial }.
 * `groups` go the most blocking first (then the best score); `unnamed` are the stuck forms whose reason lists no question; `forms` is
 * every stuck form once, however many questions it waits on; `partial` says the queue was cut at its cap, so the numbers are "at least".
 */
export function missingOf(snap) {
  const queue = (snap && snap.needs_gonza) || {};
  const cards = list(queue.cards).filter((c) => c && c.reason_kind === "missing_answers");
  const groups = new Map();
  const unnamed = [];
  const stuck = new Set();
  for (const card of cards) {
    const brief = { key: String(card.key), company: clean(card.company, 60), title: clean(card.title, 90), score: whole(card.score), url: safeHref(card.url) };
    const labels = missingLabels(card.reason);
    stuck.add(brief.key);
    if (labels.length === 0) { unnamed.push(brief); continue; }
    for (const label of labels) {
      const id = questionKey(label);
      const group = groups.get(id) || { id, label: shownLabel(label), cards: [] };
      if (!group.cards.some((c) => c.key === brief.key)) group.cards.push(brief);
      groups.set(id, group);
    }
  }
  const sorted = [...groups.values()].map((g) => ({ ...g, forms: g.cards.length }));
  const best = (g) => g.cards.reduce((n, c) => Math.max(n, c.score), 0);
  sorted.sort((a, b) => b.forms - a.forms || best(b) - best(a) || a.label.localeCompare(b.label));
  return { groups: sorted, unnamed, forms: stuck.size, partial: whole(queue.total) > list(queue.cards).length };
}

// ---- the review

/** The snapshot's `learn`, with every list a list (an older server or a hostile one never breaks the page). `known` is false when it has none. */
export function learnOf(snap) {
  const l = (snap && snap.learn) || null;
  const raw = l || {};
  const average = new Map(list(raw.weak_topics).filter((w) => w && w.topic != null).map((w) => [String(w.topic), pointsOf(w.average)]));
  const topics = list(raw.topics).filter((x) => x && x.topic != null).map((x) => ({
    topic: clean(x.topic, 40), name: String(x.topic), total: whole(x.total), seen: whole(x.seen), due: whole(x.due), average: average.has(String(x.topic)) ? average.get(String(x.topic)) : null,
  }));
  topics.sort((a, b) => b.due - a.due || (b.total - b.seen) - (a.total - a.seen) || a.topic.localeCompare(b.topic));
  const gaps = list(raw.gaps).filter((g) => g && g.topic != null).map((g) => ({
    id: String(g.id), topic: clean(g.topic, 60), offerKey: g.offer_key ? String(g.offer_key) : null, offer: clean(g.offer, 90), seen: !!g.seen,
  }));
  gaps.sort((a, b) => Number(a.seen) - Number(b.seen));
  return { known: !!l, total: whole(raw.total), seen: whole(raw.seen), seenValue: pointsOf(raw.seen), due: whole(raw.due), topics, gaps };
}

/** A card from the server, or null (nothing to ask), or undefined (not a card at all: the caller treats it as a failure). */
export function cardOf(raw) {
  if (raw == null) return null;
  if (typeof raw !== "object" || raw.id == null) return undefined;
  return {
    id: String(raw.id), topic: clean(raw.topic, 60), question: clean(raw.question, 600), offer: raw.offer ? clean(raw.offer, 90) : null,
  };
}

// ---- the dossier

const stripLabel = (line, label) => line.replace(new RegExp("^\\s*\\*?" + label + ":?\\*?\\s*", "i"), "");

/**
 * The dossier markdown of the engine, in Spanish or in English (src/dossier.rs `render_markdown`), as { summary, questions: [{ q, why }], stories: [{ title, evidence }],
 * gaps: [{ topic, question }] }, or null when it has none of the headings (the page then says "sin dato" and offers the whole file).
 * Everything in it was written by a model reading a web page: it is cleaned and cut.
 */
export function parseDossier(md) {
  const text = String(md == null ? "" : md).replace(/\r\n?/g, "\n");
  const sections = new Map();
  let current = null;
  for (const line of text.split("\n")) {
    const heading = /^##\s+(.*)$/.exec(line);
    if (heading) {
      const name = heading[1].toLowerCase();
      current = /^(qué dice|what the)/.test(name) ? "summary" : /^(preguntas|likely)/.test(name) ? "questions" : /^(historias|stories)/.test(name) ? "stories" : /^(huecos|gaps)/.test(name) ? "gaps" : null;
      if (current) sections.set(current, []);
    } else if (current) {
      sections.get(current).push(line);
    }
  }
  if (sections.size === 0) return null;
  const lines = (name) => sections.get(name) || [];

  const questions = [];
  for (const line of lines("questions")) {
    const item = /^\s*\d+\.\s+(.*)$/.exec(line);
    if (item) { questions.push({ q: clean(item[1], 200), why: "" }); continue; }
    if (questions.length && /^\s*\*?(?:por qu[ée]|why):?\*?/i.test(line)) questions[questions.length - 1].why = clean(stripLabel(line, "(?:por qu[ée]|why)"), 200);
  }

  const stories = [];
  for (const line of lines("stories")) {
    const title = /^###\s+(.*)$/.exec(line);
    if (title) { stories.push({ title: clean(title[1], 120), evidence: "" }); continue; }
    const evidence = /[«“](.*)[»”]/.exec(line);
    if (stories.length && !stories[stories.length - 1].evidence && /evidencia|evidence/i.test(line) && evidence) stories[stories.length - 1].evidence = clean(evidence[1], 200);
  }

  const gaps = [];
  for (const line of lines("gaps")) {
    const item = /^\s*-\s+\*\*(.+?)\*\*\s*(?:—|-)\s*(.*)$/.exec(line);
    if (item) gaps.push({ topic: clean(item[1], 60), question: clean(item[2].replace(/\((?:repasa con|review with)[^)]*\)\s*$/i, ""), 200) });
  }

  const summary = clean(lines("summary").join(" "), 400);
  return { summary: /^(sin resumen|no summary)\.?$/i.test(summary) ? "" : summary, questions, stories, gaps };
}

// ---- the eve of an interview

/**
 * The interview whose dossier is worth reading tonight: { card, when: { kind: "task", at } | { kind: "note", text } | { kind: "none" }, others }, or
 * null when no process is in the interview stage. The date comes from the recruiter's mail (an `interview_invite` task of that process with a
 * date), else from what the card's own note says when it holds a digit, else it is unknown. The nearest mailed date first, then a dossier, then the
 * newest.
 */
export function eveOf(snap, nowMs) {
  const cards = list(columnOf(snap, "interview").cards).filter((c) => c && c.key != null);
  if (cards.length === 0) return null;
  const tasks = list(snap && snap.today && snap.today.tasks);
  const mailed = (card) => tasks
    .filter((t) => t && t.job_key === card.key && t.kind === "interview_invite" && Number.isFinite(Date.parse(t.due_at)) && Date.parse(t.due_at) >= nowMs - EVE_LOOKBACK_MS)
    .sort((a, b) => Date.parse(a.due_at) - Date.parse(b.due_at))[0] || null;
  const rows = cards.map((card) => ({ card, invite: mailed(card) }));
  rows.sort((a, b) => {
    if (a.invite && b.invite) return Date.parse(a.invite.due_at) - Date.parse(b.invite.due_at);
    if (a.invite || b.invite) return a.invite ? -1 : 1;
    if (!!a.card.has_dossier !== !!b.card.has_dossier) return a.card.has_dossier ? -1 : 1;
    return Date.parse(b.card.updated_at) - Date.parse(a.card.updated_at) || 0;
  });
  const { card, invite } = rows[0];
  const note = clean(card.note, 80);
  const when = invite ? { kind: "task", at: invite.due_at } : /\d/.test(note) ? { kind: "note", text: note } : { kind: "none" };
  return { card, when, others: rows.length - 1 };
}

// ---- the rejections

const median = (numbers) => {
  const sorted = [...numbers].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
};

/**
 * What the rejections have in common, as counts and never as rates: { total, n, few, rows }. `n` is the rejected cards the snapshot lists (its
 * column is cut at 25), `total` all of them. With fewer than MIN_REJECTED there is no pattern (`few`); with enough, a row for the score (the
 * median of the rejected against the median of the processes still alive, when there are three of those) and one for the board most of
 * them came from (at least three, at least half). A hypothesis to tune the threshold with, not a prediction: the view says so.
 */
export function rejectionsOf(snap) {
  const rejected = columnOf(snap, "rejected");
  const scored = (cards) => list(cards).flatMap((c) => { const score = c ? pointsOf(c.score) : null; return score === null ? [] : [{ ...c, score }]; });
  const cards = scored(rejected.cards);
  const n = cards.length;
  const out = { total: Math.max(whole(rejected.total), n), n, few: n > 0 && n < MIN_REJECTED, rows: [] };
  if (n < MIN_REJECTED) return out;
  const alive = LIVE.flatMap((state) => scored(columnOf(snap, state).cards)).map((c) => c.score);
  if (alive.length >= 3) out.rows.push({ id: "score", rejected: Math.round(median(cards.map((c) => c.score))), alive: Math.round(median(alive)), alivePeers: alive.length });
  const bySource = new Map();
  for (const c of cards) bySource.set(String(c.source), (bySource.get(String(c.source)) || 0) + 1);
  const [source, k] = [...bySource.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))[0];
  if (k >= 3 && k * 2 >= n) out.rows.push({ id: "source", source: clean(source, 24), k, n });
  return out;
}
