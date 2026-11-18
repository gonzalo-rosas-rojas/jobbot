// PROCESOS as data (spec §5.2): everything the field and the table say, worked out from the model with no DOM. render.js only draws it.
// Every count comes from the model (`stopCounts`, the board's own totals: A16, nothing is a number written by hand), every word from i18n,
// and everything another person wrote (a company, a role, a subject) goes through `clean`. The order of the piles comes from `place.js`
// (frozen for the session, with tombstones), the rhythm of the follow-ups from `cadence.js`, the search from `search.js`.
//
//   procesosView(model, { nowMs, place, filter, order, query, mode, showClosed }) ->
//     { open, openLabel, head, pieces, filters, orders, mode, lanes, table, closed, empty, stale, run, eyebrow, moves, found, query }
// Pure: the same model, the same clock and the same place give the same view, which is how render.js knows nothing changed.

import { t, tn, fmtDate } from "../../i18n/index.js";
import { stopCounts, layerOf, SENT_STATES, CLOSED } from "../../core/model.js";
import { ago, stateLabel, windowLabel, reasonShort, countdown } from "../../core/format.js";
import { serverDay, MINUTE_MS } from "../../core/time.js";
import { KEYS } from "../../ui/keys.js";
import { clean, safeHref } from "../shared/util.js";
import { cadenceOf, daysInStage } from "./cadence.js";
import { searchOf } from "./search.js";

/**
 * The lanes of the field, top to bottom: what has not gone out yet, then the stages of the tracker as they happen (§5.2).
 * `unsent` marks the lane of what has not gone out (its total is the columns' own, and it is outside the live count); `quiet` the lane of
 * the ones that stopped answering (nobody is waited for there).
 */
export const LANES = [
  { id: "presend", states: ["scored", "prepared", "needs_gonza"], unsent: true },
  { id: "submitted", states: ["submitted"] },
  { id: "replied", states: ["replied"] },
  { id: "technical_test", states: ["technical_test"] },
  { id: "interview", states: ["interview"] },
  { id: "offer", states: ["offer"] },
  { id: "ghosted", states: ["ghosted"], quiet: true },
];
/** `f 1` .. `f 4` of the key table, in the order of the chips. */
export const FILTERS = [
  { id: "all", chord: "f 1" },
  { id: "turn", chord: "f 2" },
  { id: "wait", chord: "f 3" },
  { id: "quiet", chord: "f 4" },
];
const PRESEND_STATES = LANES.find((lane) => lane.unsent).states;
export const ORDERS = ["stage", "age", "score"];
export const STALE_DATA_MS = 10 * MINUTE_MS; // §9.4: after ten minutes without a snapshot the page says how old its data is
export const QUERY_MAX = 60;             // the longest search the field takes (the input stops there and so does the saved query)
export const CLOSED_SHOWN = 25;           // the server lists 25 of each closed column

const whole = (x) => Math.max(0, Math.trunc(Number(x) || 0));

/** What a key of the table looks like on screen ("↵" for Enter), from the one table of keys.js. */
export function keyDisplay(id) {
  const entry = KEYS.find((k) => k.id === id);
  if (!entry) throw new Error(`procesos: "${id}" is not an action of the key table`);
  return entry.display === "Enter" ? "↵" : entry.display;
}

/** Old ids of the shared filter (core/route.js: all, mine, wait, stale) and the ones HOY sends ("turn") are the same four chips. */
export function filterId(id) {
  if (id === "mine" || id === "turn") return "turn";
  if (id === "stale" || id === "ghost" || id === "quiet") return "quiet";
  return id === "wait" ? "wait" : "all";
}

// ---- who each application is waiting for

/** The keys the model says are Gonza's (§5.2: `turn` "mine" of the live ones, plus the proposals and the stuck forms the server lists). */
export function yoursOf(m) {
  const out = new Set();
  for (const p of m.procs || []) if (p.turn === "mine") out.add(p.card.key);
  const snap = m.snap || {};
  for (const list of [snap.awaiting_approval, snap.needs_gonza]) for (const c of (list && list.cards) || []) out.add(c.key);
  return out;
}

/** The gate of a pile: "clear" you approved it, "dim" it went out by itself, "hollow" you sent it by hand, "todo" it has not passed yet. */
export function gateOf(card) {
  if (SENT_STATES.includes(card.state)) return card.source === "manual" ? "hollow" : card.approved ? "clear" : "dim";
  return card.approved ? "clear" : "todo";
}

const LAYERS = ["aviso", "puntaje", "cv", "respuestas", "compuerta"];

/** The five layers of a pile as marks: [{ id, state: "done" | "here" | "todo" }] and, for the second, its length (the points, 0-100). */
export function layersOf(card) {
  const at = layerOf(card).at;
  const done = at >= LAYERS.length || at < 0;
  return LAYERS.map((id, i) => {
    const mark = { id, state: done || i < at ? "done" : i === at ? "here" : "todo" };
    if (id === "puntaje") mark.w = scoreOf(card);
    return mark;
  });
}

/** The points of a card as the page counts them: a whole number from 0 to 100, and 0 when the server sent nothing readable. */
export function scoreOf(card) {
  return Math.max(0, Math.min(100, Math.round(Number(card.score) || 0)));
}

/** Whose turn an application is: snoozed wins, then his, then the lane that waits for nobody, else somebody else's. */
function turnKindOf({ away, you, quiet }) {
  if (away) return "snoozed";
  if (you) return "you";
  return quiet ? "quiet" : "wait";
}

/** Every application in the field, in no order: { key, card, lane, state, days, presend, turn, you, task }. `turn` is you | snoozed | wait | quiet. */
export function entriesOf(m, nowMs) {
  const snap = m.snap || {};
  const yours = yoursOf(m);
  const snoozed = new Set(((snap.snoozed || {}).cards || []).map((c) => c.key));
  const tasks = new Map();
  for (const { task, owner } of m.tasks || []) if (owner && !tasks.has(owner.key)) tasks.set(owner.key, task);
  const out = [];
  for (const lane of LANES) {
    for (const state of lane.states) {
      const col = (snap.board || []).find((c) => c.state === state);
      for (const card of (col && col.cards) || []) {
        const away = snoozed.has(card.key) || (card.snoozed_until && Date.parse(card.snoozed_until) > nowMs);
        const turn = turnKindOf({ away, you: yours.has(card.key), quiet: !!lane.quiet });
        out.push({ key: card.key, card, lane: lane.id, state, days: daysInStage(card), presend: !!lane.unsent, turn, you: turn === "you", task: tasks.get(card.key) || null });
      }
    }
  }
  return out;
}

// "Por enviar" is not in the live count, so the filters that are about the live ones (waiting, no news) leave it out; "yours" keeps what is his.
function matches(filter, e) {
  if (filter === "all") return true;
  if (filter === "turn") return e.turn === "you";
  if (e.presend) return false;
  return e.turn === (filter === "wait" ? "wait" : "quiet");
}

// ---- the words of a line

/** The mails of one application, the newest first. */
export function mailsOfKey(snap, key) {
  return ((snap.today || {}).emails || []).filter((x) => x.job_key === key).sort((a, b) => Date.parse(b.received_at) - Date.parse(a.received_at));
}

/** What the company has done last, from what the snapshot knows (a mail, a move of the last day) and else from the state. */
function lastOf(e, snap) {
  const mails = mailsOfKey(snap, e.key);
  if (mails[0]) return t("procesos.last.mail", { subject: clean(mails[0].subject, 70) || t("common.noSubject") });
  const moves = ((snap.activity || {}).moves || []).filter((x) => x.key === e.key).sort((a, b) => Date.parse(b.at) - Date.parse(a.at));
  if (moves[0]) return t("procesos.last.move", { state: stateLabel(moves[0].to) });
  const c = e.card;
  if (e.state === "needs_gonza") return clean(reasonShort(c) || t("procesos.last.needs_gonza"), 90);
  if (e.state === "prepared") return t(c.has_screenshot ? "procesos.last.prepared.shot" : "procesos.last.prepared");
  return t("procesos.last." + e.state);
}

/** What comes next, as a sentence: the task that is on it, else what the stage and the rhythm of the follow-ups say. */
export function nextStepText(e, cad) {
  if (e.task) return clean(e.task.text, 90);
  const c = e.card;
  if (e.state === "needs_gonza") return clean(reasonShort(c) || t("procesos.next.look"), 90);
  if (e.state === "prepared") return t(c.approved ? "procesos.next.queued" : "procesos.next.approve");
  if (e.state === "scored") return t("procesos.next.scored");
  switch (cad.kind) {
    case "ask": return t("procesos.next.ask");
    case "insist": return t("procesos.next.insist");
    case "lost": return t("procesos.next.lost");
    case "reply": case "decide": return t("procesos.next.decide");
    case "test": return t("procesos.next.test");
    case "interview": return t("procesos.next.interview");
    case "offer": return t("procesos.next.offer");
    case "quiet": return t("procesos.next.quiet");
    default: return t("procesos.next.wait");
  }
}

/** The rhythm of a pile in a few words: "seguimiento 2 en 1 d", "cierra en 27 d", "prueba vence en 30 h". */
export function cadenceText(e, cad, nowMs) {
  if (e.turn === "snoozed") return t("procesos.cad.snoozed", { when: e.card.snoozed_until ? fmtDate(e.card.snoozed_until, "day") : "" }).trim();
  switch (cad.kind) {
    case "soon": return t("procesos.cad.soon", { step: cad.step, n: cad.daysTo });
    case "ask": case "insist": return t("procesos.cad.window", { step: cad.step, day: cad.day, from: cad.window[0], to: cad.window[1] });
    case "closing": return t("procesos.cad.closing", { n: cad.daysTo });
    case "lost": return t("procesos.cad.lost", { day: cad.day });
    case "reply": return cad.day === 0 ? t("procesos.cad.reply.today") : t("procesos.cad.reply", { n: cad.day });
    case "decide": return t("procesos.cad.decide", { day: cad.day });
    case "task": case "test": case "interview": case "offer": {
      const left = e.task ? countdown(e.task, nowMs) : null;
      return left ? left.text : t("procesos.cad." + cad.kind, { day: cad.day });
    }
    case "quiet": return t("procesos.cad.quiet", { n: cad.daysTo });
    default: return t("procesos.cad.pre", { state: stateLabel(e.state).toLowerCase(), day: cad.day });
  }
}

function turnOf(e, cad) {
  if (e.turn === "you") return { kind: "you", text: t("act.open"), key: keyDisplay("open") };
  if (e.turn === "snoozed") return { kind: "snoozed", text: t("procesos.turn.snoozed"), key: null };
  if (e.turn === "quiet") return { kind: "quiet", text: t("procesos.turn.quiet"), key: null };
  if (cad.kind === "decide" || cad.kind === "reply") return { kind: "check", text: t("turn.check"), key: null };
  return { kind: "wait", text: t(e.presend ? "procesos.turn.bot" : "procesos.turn.wait"), key: null };
}

/** The verbs of a pile, in the shape HOY's actions run (`actions.js`): the row of a keyboard or a mouse is the same object. */
export function actionsOf(e) {
  const c = e.card;
  const marks = Array.isArray(c.marks) ? c.marks : [];
  const list = [{ id: "open", permanent: false }];
  if (safeHref(c.url)) list.push({ id: "link", permanent: false });
  if (CLOSED.includes(e.state)) return list; // an ended application is opened and read, never put off, withdrawn or marked
  list.push({ id: "snooze", permanent: false });
  if (marks.includes("withdrawn")) list.push({ id: "withdraw", permanent: false });
  if (marks.some((x) => x !== "withdrawn")) list.push({ id: "register", permanent: false });
  if (Array.isArray(c.actions) && c.actions.includes("confirm")) list.push({ id: "confirm", permanent: true });
  return list;
}

/** The id of the row an application has in the page ("card:" and its key): what `rowView` and the piles are looked up by. */
export const rowIdOf = (key) => "card:" + key;
/** The id of the tombstone a card left behind (the key of its element in the page). */
export const tombIdOf = (key) => "tomb:" + key;

/** The company and the role of a card as the page shows and searches them: cleaned and cut to the one length each has. */
const namesOf = (card) => ({ company: clean(card.company, 60), title: clean(card.title, 90) });

/** What a pile adds to its spoken label when its turn matters: it is his, or it is put off. */
function turnNoteOf(e) {
  if (e.turn === "you") return t("status.you");
  return e.turn === "snoozed" ? t("procesos.turn.snoozed") : null;
}

/** One pile as the page needs it (a cell of the field and the start of a row of the table). */
function cellOf(e, extra = {}) {
  const c = e.card;
  const { company, title } = namesOf(c);
  return {
    id: rowIdOf(e.key),
    key: e.key,
    data: { card: e.key },
    company,
    title,
    state: e.state,
    stage: stateLabel(e.state),
    lane: e.lane,
    turn: e.turn,
    days: e.days,
    age: t("procesos.age", { n: e.days }),
    layers: layersOf(c),
    gate: gateOf(c),
    score: scoreOf(c),
    open: c.key,
    link: safeHref(c.url),
    actions: actionsOf(e),
    label: [t("procesos.cell.aria", { company, title, state: stateLabel(e.state), days: e.days }), turnNoteOf(e)].filter(Boolean).join(" · "),
    ...extra,
  };
}

/**
 * The row of one application worked out from the model alone, for a button of the drawer when the list does not have it drawn (a filter or a
 * search hides it): the same object the list's keys run (`cellOf`), or null when the application is not one of the live ones.
 */
export function rowViewOf(m, key, nowMs) {
  const e = m ? entriesOf(m, nowMs).find((x) => x.key === key) : null;
  return e ? cellOf(e) : null;
}

function tombOf(slot) {
  return { tomb: slot.tomb, to: slot.to, text: slot.to ? t("procesos.tomb." + slot.to) : t("procesos.tomb.out") };
}

// ---- the table

function rowOf(e, snap, nowMs, hl) {
  const cad = cadenceOf(e.card, { hasTask: !!e.task });
  return {
    ...cellOf(e, { hl }),
    last: lastOf(e, snap),
    next: nextStepText(e, cad),
    cadence: cadenceText(e, cad, nowMs),
    turnCell: turnOf(e, cad),
  };
}

/** "Te tocan primero, luego por edad" (§5.2): the tier of a pile, then its age. */
const TIER = { you: 0, snoozed: 1, wait: 2, quiet: 3 };
const byAge = (a, b) => (b.days - a.days) || (a.key < b.key ? -1 : 1);
const byScore = (a, b) => (Number(b.card.score) || 0) - (Number(a.card.score) || 0) || byAge(a, b);
/** `list` sorted by age or by score, or null for "stage", whose order each caller gives its own way (the table by tier, the field by its places). */
function sortedBy(list, order) {
  if (order === "age") return list.slice().sort(byAge);
  if (order === "score") return list.slice().sort(byScore);
  return null;
}
const tableOrder = (list, order) => sortedBy(list, order) || list.slice().sort((a, b) => TIER[a.turn] - TIER[b.turn] || byAge(a, b));

// ---- the whole view

/** The live applications by whose turn it is: { you, snoozed, wait, quiet }. */
function countsOf(live) {
  const group = (name) => live.filter((e) => e.turn === name).length;
  return { you: group("you"), snoozed: group("snoozed"), wait: group("wait"), quiet: group("quiet") };
}

/** The column of the board in `state`, or null. */
const columnOf = (snap, state) => (snap.board || []).find((c) => c.state === state) || null;

/** The letters of a search that matched `e`, as parts of its company and role, or null when nothing is being searched. */
const partsOfHit = (hits, e) => (hits ? hits.get(e.key).parts : null);

/**
 * The lanes of the field that have something to show: with no filter, search or order, each lane as it was (tombstones and all); else only
 * the piles that pass, in the order asked for. Everything it takes is what procesosView has worked out by then.
 */
function lanesOf({ entries, slots, byKey, passes, hits, sort, unfiltered, presendTotal }) {
  return LANES.map((lane) => {
    const own = entries.filter((e) => e.lane === lane.id);
    let cells;
    if (unfiltered) {
      cells = (slots[lane.id] || []).map((slot) => (slot.tomb != null ? tombOf(slot) : cellOf(byKey.get(slot.key), {})));
    } else {
      const shown = (slots[lane.id] || []).filter((slot) => slot.tomb == null).map((slot) => byKey.get(slot.key)).filter((e) => e && passes(e));
      const ordered = sortedBy(shown, sort) || shown;
      cells = ordered.map((e) => cellOf(e, { hl: partsOfHit(hits, e) }));
    }
    return { id: lane.id, label: t("procesos.lane." + lane.id), note: t("procesos.lane." + lane.id + ".note"), quiet: !!lane.quiet, total: lane.unsent ? presendTotal : own.length, you: own.filter((e) => e.turn === "you").length, cells };
  }).filter((lane) => lane.cells.length > 0);
}

/** The applications that ended: the server lists up to 25 of each column and says how many there are; the items are drawn only when `showClosed`. */
function closedOf(snap, showClosed) {
  const closedCols = CLOSED.map((state) => columnOf(snap, state) || { state, total: 0, cards: [] });
  const total = (state) => whole(closedCols.find((c) => c.state === state).total);
  const items = showClosed
    ? closedCols.flatMap((col) => col.cards.slice(0, CLOSED_SHOWN).map((card) => cellOf({ key: card.key, card, lane: "closed", state: col.state, days: daysInStage(card), presend: false, turn: "wait", you: false, task: null })))
    : [];
  const sum = total("accepted") + total("rejected") + total("withdrawn");
  return { total: sum, accepted: total("accepted"), rejected: total("rejected"), withdrawn: total("withdrawn"), items, more: showClosed ? Math.max(0, sum - items.length) : 0 };
}

/** What the page says when nothing is drawn: no application at all, none that passes the filter, or none that matches the search; else null. */
function emptyOf({ entries, closed, found, hits, query }) {
  if (entries.length === 0) return { kind: "none", text: closed.total > 0 ? tn("procesos.empty.none.closed", closed.total) : t("procesos.empty.none") };
  if (found > 0) return null;
  return hits ? { kind: "search", text: t("procesos.empty.search", { q: clean(query, 40) }) } : { kind: "filter", text: t("procesos.empty.filter") };
}

/**
 * procesosView(model, { nowMs, place, filter = "all", order = "stage", query = "", mode = "field", showClosed = false })
 *   open     the live applications (the number of the bar: `stopCounts`)       head   { you, snoozed, wait, quiet, presend, closed }
 *   lanes    [{ id, label, note, total, you, cells: (cell | tomb)[] }] for the field (empty lanes are not listed)
 *   table    [row] for the table, in its own order        found   how many piles pass the filter and the search
 *   closed   { total, accepted, rejected, withdrawn, items: [cell], more }     empty   null | { kind: "none" | "filter" | "search", text }
 */
export function procesosView(m, { nowMs, place, filter = "all", order = "stage", query = "", mode = "field", showClosed = false }) {
  const snap = m.snap || {};
  const want = filterId(filter);
  const sort = ORDERS.includes(order) ? order : "stage";
  const entries = entriesOf(m, nowMs);
  const live = entries.filter((e) => !e.presend);
  const open = whole(stopCounts(m).procesos);
  const counts = countsOf(live);
  const presend = entries.filter((e) => e.presend);
  const presendYou = presend.filter((e) => e.you).length;

  // the board by key, closed ones included: where a pile went when it left the field
  const states = new Map();
  for (const col of snap.board || []) for (const c of col.cards || []) states.set(c.key, col.state);
  // the server lists a column of cards in full only for the live ones: the others (Por enviar's three among them) come cut to 25 with their real total
  const column = (state) => columnOf(snap, state);
  const cut = (state) => { const col = column(state); return !!col && whole(col.total) > (col.cards || []).length; };
  const { lanes: slots, moves } = place.sync(entries.map((e) => ({ key: e.key, lane: e.lane, days: e.days, you: e.you, state: e.state })), (key) => states.get(key) || null, cut);

  const byKey = new Map(entries.map((e) => [e.key, e]));
  const hits = searchOf(entries.map((e) => ({ key: e.key, ...namesOf(e.card) })), query);
  const passes = (e) => matches(want, e) && (!hits || hits.has(e.key));
  const found = entries.filter(passes).length;
  const unfiltered = want === "all" && !hits && sort === "stage"; // the field as it was, tombstones and all

  // what has not gone out: the columns' real totals, never less than the cards that came
  const presendTotal = Math.max(presend.length, PRESEND_STATES.reduce((n, state) => { const col = column(state); return n + (col ? Math.max(whole(col.total), (col.cards || []).length) : 0); }, 0));

  const lanes = mode === "table" ? [] : lanesOf({ entries, slots, byKey, passes, hits, sort, unfiltered, presendTotal });
  const table = mode === "table" ? tableOrder(entries.filter(passes), sort).map((e) => rowOf(e, snap, nowMs, partsOfHit(hits, e))) : [];

  const closed = closedOf(snap, showClosed);
  const empty = emptyOf({ entries, closed, found, hits, query });

  const c = snap.controls || {};
  const runText = c.last_run_at ? t("procesos.run.last", { ago: ago(c.last_run_at, nowMs) }) : c.engine === "never" ? t("procesos.run.never") : "";
  const age = nowMs - Date.parse(snap.generated_at);

  const countOf = { all: open, turn: counts.you + presendYou, wait: counts.wait, quiet: counts.quiet };

  return {
    open,
    mode,
    eyebrow: t(mode === "table" ? "procesos.eyebrow.table" : "procesos.eyebrow", { hora: fmtDate(nowMs, "time") }),
    head: { ...counts, presend: presendTotal, presendYou, closed: closed.total },
    pieces: [
      counts.you > 0 && { id: "you", text: tn("procesos.head.you", counts.you), gold: true },
      counts.snoozed > 0 && { id: "snoozed", text: tn("procesos.head.snoozed", counts.snoozed) },
      counts.wait > 0 && { id: "wait", text: tn("procesos.head.wait", counts.wait) },
      counts.quiet > 0 && { id: "quiet", text: tn("procesos.head.quiet", counts.quiet) },
      closed.total > 0 && { id: "closed", text: tn("procesos.head.closed", closed.total) },
      presendTotal > 0 && { id: "presend", text: tn("procesos.head.presend", presendTotal) },
    ].filter(Boolean),
    openLabel: tn("procesos.head.open", open),
    filters: FILTERS.map((f) => ({ id: f.id, chord: f.chord, label: t("procesos.filter." + f.id), n: countOf[f.id], active: f.id === want })),
    orders: ORDERS.map((id) => ({ id, label: t("procesos.order." + id), active: id === sort })),
    lanes,
    table,
    closed,
    empty,
    found,
    moves,
    query: String(query || ""),
    run: [windowLabel(c, serverDay(nowMs)), runText].filter(Boolean),
    stale: age > STALE_DATA_MS ? ago(snap.generated_at, nowMs) : null,
  };
}
