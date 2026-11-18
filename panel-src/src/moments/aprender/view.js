// APRENDER as data (spec §5.4): everything the page says, worked out from the model, with no DOM. render.js only draws it. Every number
// is the model's or the snapshot's (A16: the figure of the bar and the one of the page are the same function), every word is from i18n,
// everything another person wrote goes through `clean` and `safeHref`, and what the snapshot does not carry says "sin dato" instead of a
// number. Pure: the same model, clock and loaded dossiers give the same view, which is how render.js knows nothing changed.

import { t, tn, fmtDate, fmtNumber } from "../../i18n/index.js";
import { stopCounts } from "../../core/model.js";
import { ago, nextWindowLabel, pointsOf } from "../../core/format.js";
import { serverDay } from "../../core/time.js";
import { keyShown } from "../../ui/keys.js";
import { missingOf, learnOf, eveOf, rejectionsOf, whole, MIN_REJECTED } from "./data.js";
import { clean, safeHref } from "../shared/util.js";
import { askOf } from "../shared/stops.js";

export const MISSING_ROWS = 5;             // the questions shown before "y N más"
export const TOPIC_ROWS = 7;               // the topics of the review
const GAP_ROWS = 3;                 // the gaps a dossier left
const QUESTIONS_SHOWN = 3;          // the likely questions of the dossier
const STORIES_SHOWN = 3;            // and the stories with their evidence
export const SPELLED_UP_TO = 20;           // the headline spells the number as a word up to here ("cinco."), a figure above it
const WHO_SHOWN = 2;                       // companies named in the line of a question

/** The number of the headline: a word up to twenty (feminine: it counts questions), the figure after that. */
export const numWord = (n) => (Number.isInteger(n) && n >= 0 && n <= SPELLED_UP_TO ? t("aprender.num." + n) : String(n));

/** What the key of an action looks like on a button: ui/keys.js's one function, naming APRENDER in the error of an id that is not in the table. */
export const keyOf = (id) => keyShown(id, { room: "aprender" });

/** The "view the posting" verb of a row, or nothing when the card has no safe link. */
const linkVerb = (link) => (link ? [{ id: "link", key: keyOf("link"), label: t("act.viewPosting"), primary: false, visible: false, permanent: false }] : []);

const GAP_ROW_PREFIX = "gap:";
/** The id of the row of a gap, and the test render.js uses to find those rows again: one place writes the prefix. */
export const gapRowId = (id) => GAP_ROW_PREFIX + id;
export const isGapRowId = (rowId) => String(rowId).startsWith(GAP_ROW_PREFIX);

/** The engine's window when it is shut: when it opens again, if the snapshot knows. */
function closedLabel(c, nowMs) {
  if (!c.next_window) return t("aprender.engine.closed");
  return t("aprender.engine.closedOpens", { when: nextWindowLabel(c.next_window, serverDay(nowMs)) });
}

function engineOf(controls, nowMs) {
  const c = controls || {};
  const parts = [];
  if (c.stopped) parts.push(t("aprender.engine.stop"));
  if (c.window_open === true) parts.push(t("aprender.engine.open"));
  else if (c.window_open === false) parts.push(closedLabel(c, nowMs));
  if (c.last_run_at) parts.push(t("aprender.engine.run", { ago: ago(c.last_run_at, nowMs) }));
  else if (c.engine === "never") parts.push(t("aprender.engine.never"));
  return parts;
}

/** A15 in the sentence under the headline: an engine that does not run prepares nothing, and the page says it. Null when it runs. */
function engineWarning(controls, nowMs) {
  const c = controls || {};
  if (c.engine === "never") return t("aprender.sub.engine.never");
  if (c.engine === "stale" && c.last_run_at) return t("aprender.sub.engine.stale", { ago: ago(c.last_run_at, nowMs) });
  return null;
}

function missingRow(group, forms) {
  const names = [...new Set(group.cards.map((c) => c.company).filter(Boolean))];
  const first = group.cards[0];
  const who = names.length === 0 ? "" : names.length > WHO_SHOWN
    ? t("aprender.row.blocks.more", { who: names.slice(0, WHO_SHOWN).join(", "), n: names.length - WHO_SHOWN })
    : t("aprender.row.blocks", { who: names.join(", ") });
  const link = safeHref(first && first.url);
  const verbs = [
    { id: "resolve", key: keyOf("resolve"), label: t("aprender.act.answered"), primary: true, visible: true, permanent: true },
    { id: "open", key: keyOf("open"), label: t("act.open"), primary: false, visible: false, permanent: false },
    ...linkVerb(link),
  ];
  return {
    id: "miss:" + group.id,
    data: { card: first ? first.key : null },
    keys: group.cards.map((c) => c.key),
    label: group.label,
    forms,
    formsLabel: tn("aprender.forms", forms),
    who,
    open: first ? first.key : null,
    link,
    actions: verbs,
  };
}

function missingBlock(snap, expanded) {
  const m = missingOf(snap);
  const rows = m.groups.map((g) => missingRow(g, g.forms));
  if (m.unnamed.length) rows.push({ ...missingRow({ id: "unnamed", label: t("aprender.row.unnamed"), cards: m.unnamed }, m.unnamed.length), who: t("aprender.row.unnamed.why") });
  const shown = expanded ? rows : rows.slice(0, MISSING_ROWS);
  const total = rows.length;
  const forms = tn("aprender.forms", m.forms);
  return {
    count: m.groups.length + (m.unnamed.length ? 1 : 0),
    forms: m.forms,
    partial: m.partial,
    head: total === 0 ? t("aprender.miss.head.none") : t(m.partial ? "aprender.miss.head.atLeast" : "aprender.miss.head", { n: total, forms }),
    rows: shown,
    more: Math.max(0, total - shown.length),
    moreLabel: total > MISSING_ROWS ? (expanded ? t("aprender.miss.less") : t("aprender.miss.more", { n: total - MISSING_ROWS })) : null,
    expanded: !!expanded,
    none: total === 0 ? t("aprender.miss.none") : null,
    how: t("aprender.how"),
  };
}

function bankBlock(snap, missing, learn) {
  const p = snap.pipeline || {};
  const figure = (id, n, label) => {
    const count = pointsOf(n);
    return { id, n: count === null ? null : whole(count), label, nodata: count === null };
  };
  return {
    head: t("aprender.bank.head"),
    figures: [
      figure("complete", p.complete, t("aprender.bank.complete", { days: whole(pointsOf(p.days)) })),
      figure("blocked", missing.forms, t("aprender.bank.blocked")),
      figure("seen", learn.known ? learn.seenValue : null, t("aprender.bank.seen", { total: learn.total })),
      // the bank itself: profile/answers.toml is read by the engine alone and is not in the snapshot
      figure("bank", null, t("aprender.bank.bank")),
    ],
    nodata: t("aprender.nodata"),
    note: t("aprender.bank.note"),
  };
}

function rejectsBlock(snap) {
  const r = rejectionsOf(snap);
  if (r.n === 0 && r.total === 0) return null;
  const rows = r.rows.map((row) => (row.id === "score"
    ? { id: "score", text: t("aprender.rej.score", { rejected: row.rejected, alive: row.alive, peers: row.alivePeers }) }
    : { id: "source", text: t("aprender.rej.source", { k: row.k, n: row.n, source: row.source }) }));
  return {
    head: r.total > r.n ? t("aprender.rej.head.cut", { n: r.n, total: r.total }) : t("aprender.rej.head", { total: r.total }),
    rows,
    few: r.few ? t("aprender.rej.few", { n: r.n, min: MIN_REJECTED }) : null,
    none: !r.few && rows.length === 0 ? t("aprender.rej.none", { n: r.n }) : null,
    nodata: t("aprender.rej.nodata"),
    note: rows.length ? t("aprender.rej.note", { n: r.n }) : null,
  };
}

/** How the time of the interview reads: the mailed date, the card's own note, or "unknown". */
function whenLabelOf(when) {
  if (when.kind === "task") return fmtDate(when.at, "datetime");
  return when.kind === "note" ? when.text : t("aprender.eve.when.none");
}

/** `dossiers` is what the moment has fetched: key -> { state: "loading" | "ok" | "error" | "none", parsed?, why? }. */
function eveBlock(snap, nowMs, learn, dossiers) {
  const eve = eveOf(snap, nowMs);
  if (!eve) return { state: "none", head: t("aprender.eve.head.none"), text: t("aprender.eve.none"), row: null };
  const card = eve.card;
  const when = whenLabelOf(eve.when);
  const loaded = dossiers.get(card.key) || null;
  let state;
  let text = null;
  if (!card.has_dossier || (loaded && loaded.state === "none")) { state = "nodossier"; text = t("aprender.eve.nodossier"); }
  else if (!loaded || loaded.state === "loading") { state = "loading"; text = t("aprender.eve.loading"); }
  else if (loaded.state === "error") { state = "error"; text = t("aprender.eve.error", { why: clean(loaded.why || t("aprender.nodata"), 120) }); }
  else if (!loaded.parsed) { state = "unreadable"; text = t("aprender.eve.unreadable"); }
  else state = "ok";
  const parsed = state === "ok" ? loaded.parsed : null;
  const gaps = learn.gaps.filter((g) => g.offerKey === card.key && !g.seen).length;
  const link = safeHref(card.url);
  return {
    state,
    head: t("aprender.eve.head", { company: clean(card.company, 60), when }),
    role: clean(card.title, 90),
    others: eve.others > 0 ? tn("aprender.eve.others", eve.others) : null,
    text,
    summary: parsed ? parsed.summary : "",
    questionsLabel: t("aprender.eve.questions"),
    questions: parsed ? parsed.questions.slice(0, QUESTIONS_SHOWN).map((q) => q.q) : [],
    storiesLabel: t("aprender.eve.stories"),
    stories: parsed ? parsed.stories.slice(0, STORIES_SHOWN).map((s) => ({ title: s.title, evidence: s.evidence })) : [],
    gaps: gaps > 0 ? tn("aprender.eve.gaps", gaps) : null,
    row: {
      id: "eve:" + card.key,
      data: { card: String(card.key) },
      open: String(card.key),
      link,
      actions: [
        { id: "open", key: keyOf("open"), label: t("aprender.eve.open"), primary: true, visible: true, permanent: false },
        ...linkVerb(link),
      ],
    },
  };
}

/** Where the review stands: nothing in the bank, questions due, or all up to date. */
function drillStateOf(total, due) {
  if (total === 0) return "empty";
  return due > 0 ? "due" : "calm";
}

/** The line under the review's headline: none while questions are due, because the start button says what to do. */
function drillTextOf(state) {
  if (state === "empty") return t("aprender.card.empty");
  return state === "calm" ? t("aprender.drill.calm") : null;
}

function drillBlock(snap, learn, due) {
  const base = { start: { key: keyOf("open"), label: t("aprender.drill.start"), aria: t("aprender.drill.start.aria", { topic: t("aprender.drill.topic.all") }) } };
  if (!learn.known) return { ...base, state: "nodata", head: t("aprender.drill.head.nodata"), text: t("aprender.drill.nodata"), topics: [], gaps: null };
  const topics = learn.topics.slice(0, TOPIC_ROWS).map((x) => ({
    topic: x.topic,
    raw: x.name,                    // the name as the server wrote it: only ever sent back (?topic=) or kept in a data attribute, never shown
    due: x.due,
    seen: x.seen,
    total: x.total,
    wSeen: x.total ? Math.min(1, x.seen / x.total) : 0,
    wDue: x.total ? Math.min(1, x.due / x.total) : 0,
    dueLabel: t("aprender.topic.due", { n: x.due }),
    avgLabel: x.average == null ? null : t("aprender.topic.avg", { avg: fmtNumber(x.average, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) }),
    aria: t("aprender.topic.aria", { topic: x.topic, due: x.due, seen: x.seen, total: x.total }),
  }));
  const gapRows = learn.gaps.slice(0, GAP_ROWS).map((g) => ({
    id: gapRowId(g.id), data: g.offerKey ? { card: g.offerKey } : null, open: g.offerKey, topic: g.topic, offer: g.offer, seen: g.seen, state: t(g.seen ? "aprender.gap.seen" : "aprender.gap.todo"),
  }));
  const state = drillStateOf(learn.total, due);
  return {
    ...base,
    state,
    head: t("aprender.drill.head", { due: tn("aprender.drill.due", due), seen: learn.seen, total: learn.total }),
    due,
    text: drillTextOf(state),
    topics,
    gaps: gapRows.length ? { head: t("aprender.gaps.head", { n: learn.gaps.length }), rows: gapRows } : null,
  };
}

/**
 * aprenderView(model, { nowMs, dossiers, expanded }) ->
 *   { state, calm, eyebrow, engine[], lead, turn, sub[], missing, bank, rejects, eve, drill, foot }
 * `state` is "work" (something blocks a form, or the review has questions due) or "calm". The headline is the real number of questions the
 * bank could not answer (never cut: the list is, behind "y N más").
 */
export function aprenderView(m, { nowMs, dossiers = new Map(), expanded = false } = {}) {
  const snap = m.snap;
  const learn = learnOf(snap);
  const due = stopCounts(m).aprender;
  const missing = missingBlock(snap, expanded);
  const calm = missing.count === 0 && due === 0;
  const warning = engineWarning(snap.controls, nowMs);
  let lead;
  let turn;
  const sub = [];
  if (missing.count > 0) {
    lead = t("aprender.lead.missing");
    turn = numWord(missing.count) + ".";
    sub.push(tn("aprender.sub.missing", missing.count), tn("aprender.sub.back", missing.forms));
  } else if (due > 0) {
    lead = t("aprender.lead.review");
    turn = numWord(due) + ".";
    sub.push(t("aprender.sub.review", { n: due }));
  } else {
    lead = t("aprender.calm.title");
    turn = "";
    sub.push(t("aprender.sub.calm"));
  }
  if (warning && (missing.count > 0 || calm)) sub.push(warning);
  return {
    state: calm ? "calm" : "work",
    calm,
    eyebrow: t("aprender.eyebrow", { ask: t(askOf("aprender")), hora: fmtDate(nowMs, "time") }),
    engine: engineOf(snap.controls, nowMs),
    lead,
    turn,
    sub,
    missing,
    bank: bankBlock(snap, missing, learn),
    rejects: rejectsBlock(snap),
    eve: eveBlock(snap, nowMs, learn, dossiers),
    drill: drillBlock(snap, learn, due),
    foot: t("aprender.foot"),
  };
}
