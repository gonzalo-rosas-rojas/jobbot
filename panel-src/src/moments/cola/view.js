// COLA as data (spec §5.3, board/relay2/cola-1920.png): everything the gate says, worked out from the model, with no DOM. render.js only
// draws it. Every figure comes from the snapshot (A16: `awaiting_approval`, `pipeline`, `trust`, `controls`, `funnel`, `activity`), every
// word from i18n, everything another person wrote goes through `clean` and `safeHref`, and the engine is told as it is (A15): "the next run
// sends it" while it runs, what it really waits for when it is stopped, stale or has never run. Pure: the same model and the same clocks
// give the same view, which is how render.js knows nothing changed and leaves the nodes alone.

import { t, tn, has, fmtDate, fmtMoney, fmtPercent } from "../../i18n/index.js";
import { ago, factorBars, factorName, pointsOf, reasonShort, scoreText, FACTOR_WEIGHTS } from "../../core/format.js";
import { serverDay } from "../../core/time.js";
import { nightFigures } from "../../core/model.js";
import { DEFAULT_BATCH_MIN } from "../../core/settings.js";
import { KEYS } from "../../ui/keys.js";
import { safeHref, clean } from "../shared/util.js";
import { askOf } from "../shared/stops.js";

export { FACTOR_WEIGHTS };                       // what the score is made of, in the order of the sheet's two columns (core/format.js)
export { DEFAULT_BATCH_MIN };                    // the lote's own threshold (a preference of the page, `panel.settings.batchMin`), not the engine's
export const PENDING_SHOWN = 4;                  // rows of "preparada" under the proposal; the rest are one j away
const APPROVED_SHOWN = 3;
const BLOCKED_SHOWN = 3;
const BATCH_NAMES = 4;                    // companies named in the drawn confirmation before "y N más"
export const MIN_BASE = 10;                      // under this a share of the funnel is noise: it is said as "n of base"
const SQUARES_MAX = 12;
export const ANSWER_KEY = "4";                   // APRENDER is where the missing answers are written (the `go` key of spec §3.4)
const DAY = 24 * 3600000;

const SHOWN = { Enter: "↵", "Shift A": "⇧A" };
/** What the key of an action looks like on a button, from the one table of keys.js (so a renamed key is renamed here). */
export function keyShown(id) {
  const entry = KEYS.find((k) => k.id === id);
  if (!entry) throw new Error(`cola: "${id}" is not an action of the key table`);
  return SHOWN[entry.display] || entry.display;
}

const whole = (x) => Math.max(0, Math.trunc(Number(x) || 0));
const SAYS_NOTHING = /\b(?:null|undefined|NaN)\b/;
const sane = (text) => (SAYS_NOTHING.test(text) ? "" : text);

// ---- the engine, honestly (A15)

/**
 * { kind, stopped, approve, window, run, warn[] } from `controls` alone. `kind` is the server's `engine` ("recent", "stale", "never") or
 * "unknown"; `approve` picks the label of the verb ("recent" says the next run sends it, "stopped" and "idle" say what it waits for).
 * `scheduled_task: null` is "sin dato" and is only named when the engine is not running; "no task" needs `exists === false`.
 * `intent` is STOP as the page should show it before the server's snapshot (true on, false off, null: ask the snapshot), as in SISTEMA.
 */
export function engineOf(controls, nowMs, intent = null) {
  const c = controls || {};
  const kind = c.engine === "recent" || c.engine === "stale" || c.engine === "never" ? c.engine : "unknown";
  const stopped = typeof intent === "boolean" ? intent : !!c.stopped;
  let window;
  if (c.window_open === true) window = t("cola.win.open");
  else if (c.window_open === false) window = c.next_window ? t("cola.win.closed", { when: whenOf(c.next_window, nowMs) }) : t("cola.win.closedNoNext");
  else window = t("cola.win.unknown");
  const lastOk = c.last_run_at && Number.isFinite(Date.parse(c.last_run_at));
  let run;
  if (lastOk) run = t("cola.run.last", { at: fmtDate(c.last_run_at, "datetime"), ago: ago(c.last_run_at, nowMs) });
  else run = kind === "never" ? t("cola.run.never") : t("cola.run.unknown");
  const warn = [];
  if (stopped) warn.push(t("cola.warn.stopped"));
  else if (kind === "stale" && lastOk) warn.push(t("cola.warn.stale", { ago: ago(c.last_run_at, nowMs) }));
  else if (kind === "stale" || kind === "never") warn.push(t("cola.warn.never"));
  if (!stopped && (kind === "stale" || kind === "never")) {
    const task = c.scheduled_task;
    if (task == null) warn.push(t("cola.warn.task.unknown"));
    else if (task.exists === false) warn.push(t("cola.warn.task.none"));
    else if (task.exists === true && task.next_run_at && Number.isFinite(Date.parse(task.next_run_at))) warn.push(t("cola.warn.task.next", { when: fmtDate(task.next_run_at, "datetime") }));
  }
  return { kind, stopped, approve: stopped ? "stopped" : kind === "stale" || kind === "never" ? "idle" : "recent", window, run, warn };
}

/** `controls.next_window` is the server's own clock text ("2026-10-07 18:00"): the hour alone when it is today there, else the day and the hour. */
function whenOf(next, nowMs) {
  const [day, hour] = String(next || "").split(" ");
  if (day && hour && day !== serverDay(nowMs)) return fmtDate(day + "T12:00:00Z", "day", "UTC") + " " + hour;
  return hour || String(next || "");
}

// ---- a proposal, its facts and its path

const payOf = (card, row = false) => {
  const lo = pointsOf(card.salary_min);
  const hi = pointsOf(card.salary_max);
  const money = (n) => fmtMoney(n, { digits: 0 });
  // a row of the queue says the range and no more: "al mes" is in the proposal above
  const [range, from, upTo] = row ? ["cola.row.pay", "cola.row.payMin", "cola.row.payMax"] : ["cola.fact.pay", "cola.fact.payMin", "cola.fact.payMax"];
  if (lo !== null && hi !== null && lo !== hi) return t(range, { from: money(lo), to: money(hi) });
  if (lo !== null) return t(from, { amount: money(lo) });
  if (hi !== null) return t(upTo, { amount: money(hi) });
  return "";
};

/** The five layers with what the card proves of each; a layer that is not really there is `missing`, never drawn as done. */
export function pathOf(card, { threshold = null } = {}) {
  const points = pointsOf(card.score);
  const score = points === null ? null : Math.round(points);
  const shot = !!card.has_screenshot;
  const seen = card.first_seen && Number.isFinite(Date.parse(card.first_seen)) ? fmtDate(card.first_seen, "datetime") : "";
  const source = clean(card.source, 24);
  return [
    { id: "aviso", state: "done", note: sane(seen ? t("cola.path.aviso", { source, at: seen }) : t("cola.path.aviso.plain", { source })).trim() },
    { id: "puntaje", state: score === null ? "missing" : "done", note: score === null ? "" : threshold == null ? t("cola.path.puntaje.any", { score }) : t("cola.path.puntaje", { score, threshold }) },
    { id: "cv", state: card.has_cv ? "done" : "missing", note: card.has_cv ? t("cola.path.cv.yes") : t("cola.path.cv.no") },
    { id: "respuestas", state: shot ? "done" : "missing", note: shot ? t("cola.path.respuestas.yes") : t("cola.path.respuestas.no") },
    { id: "compuerta", state: "here", note: shot ? t("cola.path.compuerta.yes") : t("cola.path.compuerta.no"), here: t("cola.path.here") },
  ];
}

/** The six bars of the score, from `GET /api/job` `score_detail` ({ factor: points }); null until it is known. */
export function factorsOf(detail) {
  const entry = detail && typeof detail === "object" ? detail : null;
  if (!entry || entry.status === "loading") return { state: "loading", bars: [] };
  if (entry.status === "error") return { state: "error", bars: [] };
  const bars = factorBars(entry.data && entry.data.score_detail);
  if (bars.length === 0) return { state: "none", bars: [] };
  return { state: "ok", bars: bars.map((b) => ({ ...b, label: has("cola.factor." + b.id) ? t("cola.factor." + b.id) : factorName(b.id) })) };
}

/** The mono line over the title: who, where it came from, where it is, what it pays, when it was prepared and how long it has waited. */
function factsOf(card, detail) {
  const where = [];
  const data = detail && detail.status === "ok" && detail.data ? detail.data : null;
  if (data && data.location) where.push(clean(data.location, 50));
  if (data && data.remote && !where.some((w) => /remot/i.test(w))) where.push(t("cola.fact.remote"));
  const prepared = card.updated_at && Number.isFinite(Date.parse(card.updated_at)) ? t("cola.fact.prepared", { at: fmtDate(card.updated_at, "datetime") }) : "";
  return [clean(card.company, 60), clean(card.source, 24), ...where, payOf(card), prepared, tn("cola.fact.waiting", whole(card.days_in_state))].filter(Boolean).map(sane).filter(Boolean);
}

/** `awaiting_approval.cards` best score first (stable), the order j and k walk. */
export function orderedProposals(snap) {
  const cards = ((snap.awaiting_approval || {}).cards || []).filter((c) => c && c.key);
  const score = (c) => pointsOf(c.score) ?? 0;
  return cards.map((c, i) => [c, i]).sort((a, b) => score(b[0]) - score(a[0]) || a[1] - b[1]).map(([c]) => c);
}

// ---- verbs

/** [{ id, key, label, primary, visible, permanent }]: the visible ones are buttons, the others work by key only. */
const verb = (id, label, o = {}) => ({ id, key: o.key || keyShown(id), label, primary: !!o.primary, visible: o.visible !== false, permanent: !!o.permanent, title: o.title || null });

function focusVerbs(engine) {
  return [
    verb("approve", t("cola.act.approve." + engine.approve), { primary: true, permanent: true }),
    verb("snooze", t("act.skip"), { title: t("act.skip.hint") }),
    verb("open", t("cola.act.open")),
    verb("link", t("act.viewPosting"), { visible: false }),
  ];
}

function rowOf(card, kind, engine, extra = {}) {
  const pay = payOf(card, true);
  const role = clean(card.title, 90);
  const meta = (extra.meta || []).filter(Boolean);
  return {
    id: "card:" + card.key,
    data: { card: card.key },
    kind: t("cola.kind." + kind),
    kindId: kind,
    company: clean(card.company, 60),
    text: [role, pay].filter(Boolean).join(" · "),
    meta: sane(meta.join(" · ")),
    open: card.key,
    link: safeHref(card.url),
    actions: extra.actions,
  };
}

function pendingRow(card, engine) {
  return rowOf(card, "pending", engine, {
    meta: [scoreText(card.score), clean(card.source, 24), card.has_screenshot ? t("cola.row.shot") : t("cola.row.noshot")],
    actions: [verb("approve", t("act.approve"), { primary: true, permanent: true }), verb("snooze", t("act.skip"), { visible: false }), verb("open", t("cola.act.open"), { visible: false }), verb("link", t("act.viewPosting"), { visible: false })],
  });
}

function approvedRow(card, engine) {
  return rowOf(card, "approved", engine, {
    meta: [scoreText(card.score), clean(card.source, 24), t("cola.row.queued." + engine.approve)],
    actions: [verb("open", t("act.open"), { primary: true }), verb("link", t("act.viewPosting"), { visible: false })],
  });
}

function blockedRow(card) {
  const why = reasonShort(card);
  const row = rowOf(card, "blocked", null, {
    meta: [scoreText(card.score), clean(card.source, 24), card.has_screenshot ? t("cola.row.shot") : t("cola.row.noshot"), t("cola.row.answers")],
    actions: [
      verb("answer", t("cola.act.answer"), { primary: true, key: ANSWER_KEY }),
      ...((card.actions || []).includes("resolve") ? [verb("resolve", t("act.resolve"), { visible: false, permanent: true })] : []),
      verb("open", t("cola.act.open"), { visible: false }),
    ],
  });
  if (why) row.text = clean([clean(card.title, 90), why].join(" · "), 160);
  return row;
}

/** The one row of what already went out in the last 24 hours, newest first, or null. */
function sentRow(snap, nowMs, cap) {
  const moves = ((snap.activity || {}).moves || [])
    .filter((x) => x && x.to === "submitted" && Date.parse(x.at) >= nowMs - DAY && Date.parse(x.at) <= nowMs + DAY)
    .sort((a, b) => Date.parse(b.at) - Date.parse(a.at));
  if (moves.length === 0) return null;
  const named = moves.slice(0, 3).map((x) => t("cola.row.sent", { company: clean(x.company, 40), at: fmtDate(x.at, "time") }));
  if (moves.length > 3) named.push(t("cola.queue.more", { n: moves.length - 3 }));
  const trust = snap.trust;
  return {
    id: "sent",
    data: { card: moves[0].key },
    kind: t("cola.kind.sent", { n: moves.length }),
    kindId: "sent",
    company: "",
    text: named.join(" · "),
    meta: trust && cap > 0 ? t("cola.row.cap", { sent: whole(trust.sent_24h), cap }) : "",
    open: moves[0].key,
    link: null,
    count: moves.length,
    actions: [verb("open", t("act.open"), { primary: true })],
  };
}

// ---- the funnel of the last days

const STEPS = [
  { id: "read", value: "seen", base: null },
  { id: "above", value: "above", base: "seen" },
  { id: "cv", value: "with_cv", base: "above" },
  { id: "form", value: "complete", base: "with_cv" },
  { id: "sent", value: "sent", base: "complete" },
];

/** Five figures, the band between them on a logarithmic scale, and the last one (what passed the gate) marked. */
export function funnelOf(pipeline) {
  const p = pipeline || {};
  const days = whole(p.days);
  const raw = Object.fromEntries(STEPS.map((s) => [s.id, whole(p[s.value])]));
  const figs = STEPS.map((s, i) => {
    const n = raw[s.id];
    const base = s.base ? raw[STEPS.find((x) => x.value === s.base).id] : 0;
    let sub = "";
    if (i === 0) sub = days > 0 ? t("cola.funnel.read.sub", { days }) : "";
    else if (base >= MIN_BASE) sub = t("cola.funnel." + s.id + ".pct", { pct: fmtPercent(n / base) });
    else if (base > 0) sub = t("cola.funnel." + s.id + ".n", { n, base });
    return { id: s.id, n, label: t("cola.funnel." + s.id), sub, last: i === STEPS.length - 1 };
  });
  const top = raw.read;
  // the band: half of its height at each figure, log(n + 1) over log(top + 1); a step with none is a closed band
  const log = figs.map((f) => (top > 0 ? Math.log(f.n + 1) / Math.log(top + 1) : 0));
  return { days, empty: top === 0, figs, log, title: t("cola.funnel.title", { days }), none: t("cola.funnel.none", { days }) };
}

// ---- trust: how many approvals are his and how many sends the cap holds

export function trustOf(trust) {
  if (!trust || typeof trust !== "object") return null;
  const total = whole(trust.first_n);
  const reviewed = whole(trust.reviewed);
  const cap = whole(trust.cap);
  const sent = whole(trust.sent_24h);
  const squares = Math.min(total, SQUARES_MAX);
  // reviewed/total of the squares drawn, rounded down; only `reviewed >= total` fills them all (13 of 20 is not "done" on 12 squares)
  const filled = reviewed >= total ? squares : Math.min(squares - 1, Math.floor((reviewed * squares) / total));
  return {
    reviewed, total, cap, sent, review: !!trust.review_phase, capAfter: whole(trust.cap_after),
    squares: Array.from({ length: squares }, (_, i) => i < filled),
    reviewLabel: t("cola.trust.reviewed", { n: reviewed, total }),
    reviewAria: t("cola.trust.reviewed.aria", { n: reviewed, total }),
    reviewSub: trust.review_phase ? t("cola.trust.reviewed.sub", { total }) : t("cola.trust.done.sub"),
    sentLabel: t("cola.trust.sent", { sent, cap }),
    sentAria: t("cola.trust.sent.aria", { sent, cap }),
    sentSub: t("cola.trust.sent.sub"),
    k: cap > 0 ? Math.max(0, Math.min(1, sent / cap)) : 0,
  };
}

// ---- the whole page

/**
 * colaView(model, { nowMs, focusKey, batchMin, detail }) ->
 *   { empty, eyebrow, engine, head, proposals, focus, batch, funnel, queue, trust, foot }
 * `focusKey` is the proposal on screen (the first by score when it is gone or null); `detail(key)` is what `GET /api/job?key=` said:
 * `{ status: "loading" | "ok" | "error", data }` or null. `batchMin` is the lote's threshold, a preference of the page.
 */
export function colaView(m, { nowMs, focusKey = null, batchMin = DEFAULT_BATCH_MIN, detail = () => null, intent = null } = {}) {
  const snap = m.snap;
  const night = nightFigures(snap);
  const proposals = orderedProposals(snap);
  const total = Math.max(night.awaiting, proposals.length);
  const at = Math.max(0, proposals.findIndex((c) => c.key === focusKey));
  const card = proposals[at] || null;
  const engine = engineOf(snap.controls, nowMs, intent);
  const min = pointsOf(batchMin) ?? DEFAULT_BATCH_MIN;
  const trust = trustOf(snap.trust);
  const picks = proposals.filter((c) => (c.actions || []).includes("approve") && (pointsOf(c.score) ?? -Infinity) >= min)
    .map((c) => ({ key: c.key, company: clean(c.company, 40), score: Math.round(pointsOf(c.score)) }));
  const rest = proposals.filter((c) => c !== card);
  const prepared = (snap.board || []).find((col) => col.state === "prepared");
  const approved = ((prepared && prepared.cards) || []).filter((c) => c.approved && !proposals.some((p) => p.key === c.key));
  const blocked = ((snap.needs_gonza || {}).cards || []).filter((c) => c.reason_kind === "missing_answers");
  const sent = sentRow(snap, nowMs, trust ? trust.cap : 0);
  const threshold = pointsOf((snap.pipeline || {}).threshold);
  const reviewedNext = trust ? trust.reviewed + 1 : 0;

  const detailOf = card ? detail(card.key) : null;
  const focusScore = card ? pointsOf(card.score) : null;
  const focus = card && {
    id: "card:" + card.key,
    key: card.key,
    data: { card: card.key },
    company: clean(card.company, 60),
    title: clean(card.title, 120),
    facts: factsOf(card, detailOf),
    score: focusScore === null ? t("common.na") : String(Math.round(focusScore)),
    scoreAria: focusScore === null ? "" : t("cola.score.aria", { score: Math.round(focusScore) }),
    factors: factorsOf(detailOf),
    factorsError: t("cola.factors.error"),
    factorsNone: t("cola.factors.none"),
    path: pathOf(card, { threshold }),
    why: {
      label: t("cola.why.label"),
      text: [
        t("cola.why.base"),
        trust && trust.review && reviewedNext < trust.total ? t("cola.why.count", { n: reviewedNext, total: trust.total }) : "",
        trust && trust.review && reviewedNext >= trust.total ? t("cola.why.last", { total: trust.total, cap: trust.capAfter }) : "",
        engine.approve === "recent" ? t("cola.why.decide") : t("cola.why.decideIdle"),
      ].filter(Boolean).join(" "),
    },
    open: card.key,
    link: safeHref(card.url) || (detailOf && detailOf.status === "ok" && detailOf.data ? safeHref(detailOf.data.apply_url) : null),
    hasShot: !!card.has_screenshot,
    shotKey: card.has_screenshot ? card.key : null,
    preparedAt: card.updated_at || null,
    actions: focusVerbs(engine),
    step: { at: at + 1, total: proposals.length, label: t("cola.act.step", { at: at + 1, total: proposals.length }), many: proposals.length > 1, prev: t("cola.act.prev"), next: t("cola.act.next") },
  };

  const pending = rest.map((c) => pendingRow(c, engine));
  const approvedRows = approved.map((c) => approvedRow(c, engine));
  const blockedRows = blocked.map(blockedRow);
  const groups = [
    { id: "pending", count: pending.length, title: pending.length ? tn("cola.queue.wait", pending.length) : "", rows: pending.slice(0, PENDING_SHOWN), more: Math.max(0, pending.length - PENDING_SHOWN) },
    { id: "approved", count: approvedRows.length, title: approvedRows.length ? tn("cola.queue.approved", approvedRows.length) : "", rows: approvedRows.slice(0, APPROVED_SHOWN), more: Math.max(0, approvedRows.length - APPROVED_SHOWN) },
    { id: "blocked", count: blockedRows.length, title: blockedRows.length ? tn("cola.queue.blocked", blockedRows.length) : "", rows: blockedRows.slice(0, BLOCKED_SHOWN), more: Math.max(0, blockedRows.length - BLOCKED_SHOWN) },
    { id: "sent", count: sent ? sent.count : 0, title: sent ? tn("cola.queue.sent", sent.count) : "", rows: sent ? [sent] : [], more: 0 },
  ];
  // one sentence, "La cola: 1 preparada espera tu tecla, 1 bloqueada por respuestas", not a string of facts joined by dots
  const counts = groups.filter((g) => g.count > 0).map((g) => g.title);
  const queueTitle = counts.length ? t("cola.queue.title") + ": " + counts.join(", ") : t("cola.queue.title");

  const stop = engine.stopped;
  return {
    empty: !card,
    eyebrow: t("cola.eyebrow", { ask: t(askOf("cola")), hora: fmtDate(nowMs, "time") }),
    engine: { ...engine, line: [engine.window, engine.stopped ? t("cola.run.stopped") : "", engine.run].filter(Boolean).join(" · ") },
    head: card
      ? { lead: t("cola.head.lead"), n: at + 1, of: t("cola.head.of", { total }), ask: t("cola.head.ask"), total }
      : { lead: t("cola.head.empty"), n: null, of: "", ask: "", total: 0, sub: (snap.controls || {}).last_run_at ? t("cola.head.emptyRun", { ago: ago(snap.controls.last_run_at, nowMs) }) : t("cola.head.emptySub") },
    proposals: proposals.map((c) => ({ key: c.key, score: pointsOf(c.score) ?? 0 })),
    focus,
    batch: {
      min, picks, count: picks.length, can: picks.length > 0,
      label: picks.length > 0 ? t("cola.act.batch", { min, n: picks.length }) : t("cola.act.batch.none", { min }),
      key: keyShown("batch"),
      ask: t("cola.batch.ask", { n: picks.length }),
      names: [...picks.slice(0, BATCH_NAMES).map((p) => p.company + " " + p.score), ...(picks.length > BATCH_NAMES ? [t("cola.batch.more", { n: picks.length - BATCH_NAMES })] : [])].join(" · "),
      note: trust && trust.review && trust.reviewed + picks.length >= trust.total && picks.length > 0 ? t("cola.batch.review", { total: trust.total }) : "",
      said: t("cola.batch.said", { n: picks.length }),
      none: t("cola.batch.none", { min }),
      confirm: t("cola.batch.confirm"),
      cancel: t("cola.batch.cancel"),
      enter: keyShown("open"),
      esc: KEYS.find((k) => k.id === "close").display,
    },
    funnel: funnelOf(snap.pipeline),
    queue: { title: queueTitle, groups, empty: groups.every((g) => g.count === 0), none: t("cola.queue.none"), count: groups.reduce((n, g) => n + g.count, 0) },
    trust,
    foot: {
      left: trust ? (trust.review ? t("cola.foot.review", { total: trust.total, n: trust.reviewed, cap: trust.capAfter }) : t("cola.foot.cap", { cap: trust.cap })) : "",
      stop: { on: stop, key: keyShown("engine"), label: stop ? t("cola.act.resume") : t("cola.act.stop"), state: stop ? t("cola.foot.stop.on") : t("cola.foot.stop.off") },
    },
  };
}
