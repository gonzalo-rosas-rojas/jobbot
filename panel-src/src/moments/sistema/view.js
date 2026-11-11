// SISTEMA as data (spec §5.5): everything the machine room says, worked out from the model, with no DOM. render.js only draws it. Pure: the
// same model and the same clock give the same view, which is how render.js knows nothing changed and leaves the nodes alone.
//
// It tells the truth about the engine and nothing else (A15, A16): the last run only from `controls.last_run_at`, the three states of
// `controls.engine`, how the run ended from `last_run_result`, what each step did from `last_run_steps`, the scheduled task only from
// `scheduled_task` (null is "sin dato" and is NEVER "no existe"; only `exists: false` says there is no task), the account only from
// `claude_account`. Every number comes from the model, every word from i18n, everything another program wrote goes through `clean`,
// and a value that is missing, odd or in the future never reaches the screen as NaN, null, undefined or "-3 h".
//
// The hours of this stop are the server's (America/Santiago, the clock of the run window): the night is 18:00 to 09:00 there.

import { t, tn, fmtDate, fmtMoney, fmtNumber, fmtPercent } from "../../i18n/index.js";
import { ago, windowLabel, reasonShort } from "../../core/format.js";
import { serverDay, SERVER_TZ } from "../../core/time.js";
import { horaOf, WINDOW_START, WINDOW_END } from "../../scene/clock.js";
import { KEYS } from "../../ui/keys.js";
import { clean } from "../shared/util.js";
import { askOf } from "../shared/stops.js";

const HOUR = 3600000;
const MIN = 60000;
/** The server calls the engine "stale" after this many hours without a run (crates/jobbot-panel/src/snapshot.rs, ENGINE_STALE_HOURS): keep in step. */
export const ENGINE_STALE_HOURS = 36;
export const STEP_IDS = ["discover", "prepare", "submit", "dossiers"];
export const ERRORS_SHOWN = 4;
export const SOURCES_SHOWN = 8;
/** A snapshot older than this (a poll is every 30 s) says how old it is (spec §9.4). */
export const OLD_MS = 10 * MIN;
const SKIP_REASONS = new Set(["cooldown", "stop", "window_closed", "after_error", "error", "not_due"]);
const ENGINES = new Set(["never", "stale", "recent"]);

// ---- small, careful readers: a missing or odd value is "no value", never a number made of it

/** A moment in ms, or null when the value is not an RFC 3339 text that means one. */
const momentOf = (x) => {
  if (typeof x !== "string" || x === "") return null;
  const n = Date.parse(x);
  return Number.isFinite(n) ? n : null;
};
const num = (x) => (typeof x === "number" && Number.isFinite(x) ? x : null);
const whole = (x) => { const n = num(x); return n == null ? 0 : Math.max(0, Math.trunc(n)); };
const isRecord = (x) => x !== null && typeof x === "object" && !Array.isArray(x);
const keyOf = (id) => { const e = KEYS.find((k) => k.id === id); return e ? e.display : ""; };
const hhmm = (ms) => fmtDate(ms, "time", SERVER_TZ);
const sameDay = (a, b) => serverDay(a) === serverDay(b);
/** "02:04" for a moment of today's date in the server's clock, "7 oct, 02:04" for another day. */
const whenOf = (ms, nowMs) => (sameDay(ms, nowMs) ? hhmm(ms) : fmtDate(ms, "datetime", SERVER_TZ));

// ---- the night

const SPAN_H = 24 - WINDOW_START + WINDOW_END;
const signedHours = (a, b) => ((a - b + 36) % 24) - 12;      // a - b on the 24-hour ring, in [-12, 12)

/**
 * The night that counts at `nowMs`: from 18:00 to 09:00 in the server's clock. From 18:00 on it is the one that has begun; from 09:00 to
 * 18:00 it is the one that ended ("anoche"). `tonight` says whether `nowMs` is inside it. A change of the clock inside the night moves an end
 * by an hour, which is corrected once.
 */
export function nightOf(nowMs) {
  const base = Math.floor(nowMs / MIN) * MIN;
  const hora = horaOf(base);
  let from = base - (hora >= WINDOW_START ? hora - WINDOW_START : hora + 24 - WINDOW_START) * HOUR;
  from -= signedHours(horaOf(from), WINDOW_START) * HOUR;
  let to = from + SPAN_H * HOUR;
  to -= signedHours(horaOf(to), WINDOW_END) * HOUR;
  return { from, to, tonight: nowMs >= from && nowMs <= to };
}

/** The strip: the hours of the night, the half hours with postings read, the closed ones, and the marks (last run, next run, now). */
function stripOf(snap, nowMs, lastMs, nextMs) {
  const night = nightOf(nowMs);
  const span = night.to - night.from;
  const at = (ms) => ((ms - night.from) / span) * 100;
  const inside = (ms) => ms != null && ms >= night.from && ms <= night.to;
  const a = isRecord(snap.activity) ? snap.activity : {};
  const slotMs = Math.max(1, num(a.slot_minutes) || 30) * MIN;
  const slots = (Array.isArray(a.slots) ? a.slots : []).filter(isRecord).map((s) => ({ ms: momentOf(s.at), seen: whole(s.seen), open: s.open })).filter((s) => s.ms != null && s.ms >= night.from && s.ms < night.to);
  const max = slots.reduce((m, s) => Math.max(m, s.seen), 0);
  const marks = [];
  if (inside(lastMs)) marks.push({ id: "last", pct: at(lastMs), label: t("sistema.strip.last", { hora: hhmm(lastMs) }) });
  if (inside(nextMs)) marks.push({ id: "next", pct: at(nextMs), label: t("sistema.strip.next", { hora: hhmm(nextMs) }) });
  if (night.tonight) marks.push({ id: "now", pct: at(nowMs), label: t("sistema.strip.now") });
  const from = hhmm(night.from);
  const to = hhmm(night.to);
  return {
    title: t(night.tonight ? "sistema.strip.title.tonight" : "sistema.strip.title.last", { from, to }),
    aria: t("sistema.strip.aria", { from, to }),
    hours: [0, 1, 2, 3, 4, 5].map((k) => { const ms = night.from + k * 3 * HOUR; return { pct: Math.min(100, Math.max(0, at(ms))), label: String(Math.round(horaOf(ms)) % 24).padStart(2, "0") }; }),
    bars: max > 0 ? slots.filter((s) => s.seen > 0).map((s) => ({ pct: at(s.ms), w: (slotMs / span) * 100, h: s.seen / max })) : [],
    busy: slots.filter((s) => s.open === false).map((s) => ({ pct: at(s.ms), w: (slotMs / span) * 100 })),
    marks,
    legend: t("sistema.strip.legend"),
    empty: marks.some((m) => m.id === "last") ? null : t("sistema.strip.empty"),
  };
}

// ---- the engine

function resultOf(c, hasRun) {
  const raw = typeof c.last_run_result === "string" ? c.last_run_result.trim() : "";
  if (raw === "ok") return { tone: "ok", text: t("sistema.result.ok"), detail: null };
  if (raw === "stopped" || raw === "window_closed") return { tone: "warn", text: t("sistema.result." + raw), detail: null };
  if (/^error:/.test(raw)) return { tone: "bad", text: t("sistema.result.error"), detail: clean(raw.slice(6), 160) || null };
  if (raw) return { tone: "none", text: clean(raw, 160), detail: null };
  return { tone: "none", text: t(hasRun ? "sistema.result.norecord" : "sistema.result.nodata"), detail: null };
}

/** The numbers a step carries, in the order its sentence uses them; null when the record has not got them. */
const COUNTS = { discover: ["new"], prepare: ["prepared"], submit: ["submitted", "needs_gonza"], dossiers: ["written"] };

function stepOf(id, entry) {
  const label = t("sistema.step." + id);
  if (!isRecord(entry)) return { id, label, tone: "none", text: t("sistema.step.nodata") };
  if (entry.ran === false) {
    const why = typeof entry.skipped === "string" ? entry.skipped.trim() : "";
    if (!why) return { id, label, tone: "none", text: t("sistema.step.nodata") };
    if (SKIP_REASONS.has(why)) return { id, label, tone: why === "error" ? "bad" : "skip", text: t("sistema.step.skip." + why) };
    return { id, label, tone: "skip", text: t("sistema.step.skip.other", { why: clean(why, 40) }) };
  }
  if (entry.ran !== true) return { id, label, tone: "none", text: t("sistema.step.nodata") };
  const counts = COUNTS[id].map((k) => num(entry[k]));
  let text = t("sistema.step.ran");
  if (counts.every((n) => n != null)) {
    const [n, hand] = counts.map((x) => Math.max(0, Math.trunc(x)));
    text = tn("sistema.step." + id + ".ran", n, { hand });
  }
  const code = num(entry.code);
  if (code != null && code !== 0) return { id, label, tone: "bad", text: text + " · " + t("sistema.step.code", { code: Math.trunc(code) }) };
  return { id, label, tone: "ran", text };
}

function taskOf(raw, nowMs) {
  if (!isRecord(raw) || typeof raw.exists !== "boolean") {
    return { state: "unknown", text: t("sistema.task.unknown"), hint: t("sistema.task.unknown.hint"), nextMs: null };
  }
  if (!raw.exists) return { state: "missing", text: t("sistema.task.missing"), hint: t("sistema.task.missing.hint"), nextMs: null };
  const nextMs = momentOf(raw.next_run_at);
  return {
    state: "registered",
    text: nextMs == null ? t("sistema.task.registered.nonext") : t("sistema.task.registered", { when: whenOf(nextMs, nowMs) }),
    hint: t("sistema.task.registered.hint"),
    nextMs,
  };
}

// ---- the account and the spend

/** `g***@example.com`: the server masks the address; a whole one that gets through anyway is masked here, and a text with no "@" is not shown. */
function maskEmail(x) {
  if (typeof x !== "string" || !x.includes("@")) return null;
  const at = x.lastIndexOf("@");
  const local = x.slice(0, at).trim();
  const domain = clean(x.slice(at + 1), 80);
  if (!local || !domain) return null;
  return (local.includes("***") ? clean(local, 40) : local.charAt(0) + "***") + "@" + domain;
}
const shortId = (x) => { const s = clean(x, 80); return s.length > 12 ? s.slice(0, 6) + "…" + s.slice(-4) : s; };

function accountOf(raw, nowMs) {
  if (!isRecord(raw)) {
    return { state: "none", text: t("sistema.account.none"), detail: null, who: null, folder: null, access: null, version: null, fix: t("sistema.account.fix.none"), anchor: null };
  }
  const ok = raw.ok === true;
  const text = (x, max) => (typeof x === "string" && x.trim() ? clean(x, max) : "");
  const checked = momentOf(raw.checked_at);
  const join = (parts) => parts.filter(Boolean).join(" · ") || null;
  return {
    state: ok ? "ok" : "bad",
    text: t(ok ? "sistema.account.ok" : "sistema.account.bad"),
    detail: ok ? null : text(raw.message, 200) || null,
    who: join([maskEmail(raw.email), text(raw.subscription, 24) ? t("sistema.account.plan", { plan: text(raw.subscription, 24) }) : ""]),
    folder: text(raw.config_dir, 120) || null,
    access: join([text(raw.auth_method, 24), text(raw.api_provider, 24), text(raw.org_id, 80) ? t("sistema.account.org", { org: shortId(raw.org_id) }) : ""]),
    version: join([text(raw.cli_version, 24) ? t("sistema.account.cli", { version: text(raw.cli_version, 24) }) : "", checked != null ? t("sistema.account.checked", { ago: ago(raw.checked_at, nowMs) }) : ""]),
    fix: ok ? null : t("sistema.account.fix.bad"),
    // whether anything pins the account down (expected_email and expected_org_id both set in the local profile): a false is a real warning, a
    // true is said quietly, and a field the server did not send says nothing
    anchor: raw.anchored === false ? { state: "loose", text: t("sistema.account.unanchored"), why: t("sistema.account.unanchored.why") }
      : raw.anchored === true ? { state: "ok", text: t("sistema.account.anchored") } : null,
  };
}

function spendOf(claude) {
  if (!isRecord(claude) || num(claude.spent_24h) == null) return null;
  const spent = Math.max(0, claude.spent_24h);
  const budget = num(claude.daily_budget) != null && claude.daily_budget > 0 ? claude.daily_budget : null;
  const stages = (Array.isArray(claude.by_stage) ? claude.by_stage : []).filter(isRecord)
    .filter((s) => typeof s.stage === "string" && s.stage.trim() && num(s.dollars) != null)
    .map((s) => ({ id: clean(s.stage, 24), text: tn("sistema.spend.stage", whole(s.calls), { stage: clean(s.stage, 24), dollars: fmtMoney(s.dollars) }) }));
  return {
    spent: fmtMoney(spent),
    budget: budget == null ? null : fmtMoney(budget),
    text: budget == null ? t("sistema.spend.nobudget", { spent: fmtMoney(spent) }) : t("sistema.spend.of", { spent: fmtMoney(spent), budget: fmtMoney(budget) }),
    ratio: budget == null ? null : Math.min(1, spent / budget),
    over: budget != null && spent > 0 && spent >= budget,
    overText: t("sistema.spend.over"),
    stages,
  };
}

// ---- the figures, the limits, the sources and the errors

function figuresOf(snap, task, nowMs) {
  const words = (value) => ({ value, words: true });
  let next;
  if (task.state === "registered" && task.nextMs != null) {
    next = { value: hhmm(task.nextMs), words: false, label: t(sameDay(task.nextMs, nowMs) ? "sistema.fig.next" : "sistema.fig.next.day", { day: fmtDate(task.nextMs, "day", SERVER_TZ) }) };
  } else {
    next = { ...words(t(task.state === "registered" ? "sistema.fig.scheduled" : task.state === "missing" ? "sistema.fig.notask" : "sistema.fig.nodata")), label: t("sistema.fig.next") };
  }
  const trust = isRecord(snap.trust) ? snap.trust : null;
  const sends = trust && num(trust.sent_24h) != null && num(trust.cap) != null
    ? { value: t("sistema.fig.of", { n: whole(trust.sent_24h), of: whole(trust.cap) }), words: false, label: t("sistema.fig.sends") }
    : { ...words(t("sistema.fig.nodata")), label: t("sistema.fig.sends") };
  // the Claude spend is not a figure: the account's row says it once, with its meter and its warning
  return [{ id: "next", ...next }, { id: "sends", ...sends }];
}

function limitsOf(snap, batchMin) {
  const trust = isRecord(snap.trust) ? snap.trust : null;
  const p = isRecord(snap.pipeline) ? snap.pipeline : {};
  const nodata = t("sistema.fig.nodata");
  let sends = { id: "sends", label: t("sistema.limit.sends"), value: nodata, sub: null };
  if (trust && num(trust.sent_24h) != null && num(trust.cap) != null) {
    const first = whole(trust.first_n);
    sends = {
      id: "sends", label: t("sistema.limit.sends"), value: t("sistema.fig.of", { n: whole(trust.sent_24h), of: whole(trust.cap) }),
      sub: trust.review_phase === true ? t("sistema.limit.sends.review", { reviewed: whole(trust.reviewed), first, after: whole(trust.cap_after) }) : t("sistema.limit.sends.after", { first }),
    };
  }
  const threshold = num(p.threshold);
  const batch = num(batchMin);
  // the score from which the engine may approve alone (controls.auto_approve_min): off when null, a whole 1..100 when on, and nothing the page
  // can say when the server did not send it or sent something else
  const c = isRecord(snap.controls) ? snap.controls : {};
  const minRaw = c.auto_approve_min;
  const autoOn = Number.isInteger(minRaw) && minRaw >= 1 && minRaw <= 100;
  const auto = { id: "auto", label: t("sistema.limit.auto"), value: minRaw === null ? t("sistema.limit.auto.off") : autoOn ? tn("sistema.limit.auto.from", minRaw) : nodata, sub: t("sistema.limit.auto.sub") };
  return {
    rows: [
      sends,
      { id: "score", label: t("sistema.limit.score"), value: threshold == null ? nodata : fmtNumber(threshold, { maximumFractionDigits: 1 }), sub: threshold == null ? null : t("sistema.limit.score.sub") },
      auto,
      { id: "batch", label: t("sistema.limit.batch"), value: batch == null ? nodata : fmtNumber(Math.trunc(batch)), sub: batch == null ? t("sistema.limit.batch.none") : t("sistema.limit.batch.set", { key: keyOf("batch") }) },
    ],
  };
}

function sourcesOf(snap, shown) {
  const raw = isRecord(snap.funnel) && isRecord(snap.funnel.by_source) ? snap.funnel.by_source : {};
  const all = Object.entries(raw).filter(([name, v]) => name.trim() && isRecord(v))
    .map(([name, v]) => ({ id: "src:" + name, name: clean(name, 28), n: whole(v.n), sent: whole(v.sent), answered: whole(v.answered) }))
    .filter((r) => r.name)
    .sort((a, b) => b.n - a.n || a.name.localeCompare(b.name));
  const top = all[0] ? all[0].n : 0;
  const rows = all.slice(0, shown).map((r) => ({ ...r, w: top > 0 ? r.n / top : 0 }));
  const m = isRecord(snap.metrics) ? snap.metrics : {};
  const sentWeek = whole(m.sent_week);
  let week = null;
  if (sentWeek > 0) {
    week = tn("sistema.sources.week", sentWeek, { a: whole(m.responses_week) });
    // a rate is only said when there are ten or more to take it from (spec §5.5): fewer is noise
    if (sentWeek >= 10 && num(m.response_rate) != null) week += " · " + fmtPercent(m.response_rate);
  }
  return {
    rows,
    week,
    note: rows.length ? t("sistema.sources.note") : null,
    empty: rows.length ? null : t("sistema.sources.empty"),
    moreLabel: all.length > rows.length ? t("sistema.more", { n: all.length - rows.length }) : null,
  };
}

function errorsOf(snap, nowMs, lastMs, runResult, shown) {
  const a = isRecord(snap.activity) ? snap.activity : {};
  const since = momentOf(a.from) ?? nowMs - 24 * HOUR;
  const cards = isRecord(snap.needs_gonza) && Array.isArray(snap.needs_gonza.cards) ? snap.needs_gonza.cards : [];
  const stuck = cards.filter(isRecord).map((card, i) => ({ card, i, at: momentOf(card.waiting_since) ?? momentOf(card.updated_at) }))
    .filter((x) => x.at != null && x.at >= since)
    .sort((x, y) => y.at - x.at || x.i - y.i)
    .map(({ card, at, i }) => {
      const co = clean(card.company, 50);
      const role = clean(card.title, 90);
      return {
        id: "err:" + (typeof card.key === "string" && card.key ? card.key : i), hour: hhmm(at),
        text: [co, role].filter(Boolean).join(" · ") || t("sistema.errors.unnamed"),
        why: reasonShort(card) || null, open: typeof card.key === "string" && card.key ? card.key : null,
      };
    });
  const rows = [];
  if (runResult.tone === "bad") rows.push({ id: "run", hour: lastMs != null ? hhmm(lastMs) : "", text: t("sistema.errors.run"), why: runResult.detail, open: null });
  rows.push(...stuck);
  return {
    total: rows.length,
    rows: rows.slice(0, shown),
    moreLabel: rows.length > shown ? t("sistema.more", { n: rows.length - shown }) : null,
    empty: rows.length ? null : t("sistema.errors.empty"),
    sources: t("sistema.errors.sources"),
  };
}

/**
 * sistemaView(model, { nowMs, intent, batchMin, show }) ->
 *   { engine, eyebrow, status, old, head: { lead, tail }, chip: { tone, text, stop }, figures[2], strip, run: { last, result, steps[4], window },
 *     task, stop, account, spend, limits, sources, errors, foot }
 * `intent` is what the person asked for before the server has answered (true: STOP on, false: STOP off, null: ask the snapshot): the page
 * shows it at once. `batchMin` is the batch threshold when something stores it; none does yet, so it is "sin dato" until one does.
 * `show` ({ sources, errors }) says how many boards and errors to list when the sheet has no room for all: the rest is "y N más".
 */
export function sistemaView(m, { nowMs, intent = null, batchMin = null, show = null } = {}) {
  const shownSources = show && Number.isInteger(show.sources) && show.sources >= 1 ? Math.min(show.sources, SOURCES_SHOWN) : SOURCES_SHOWN;
  const shownErrors = show && Number.isInteger(show.errors) && show.errors >= 1 ? Math.min(show.errors, ERRORS_SHOWN) : ERRORS_SHOWN;
  const snap = m.snap;
  const c = isRecord(snap.controls) ? snap.controls : {};
  const engine = ENGINES.has(c.engine) ? c.engine : "never";
  const lastMs = momentOf(c.last_run_at);
  const stopped = typeof intent === "boolean" ? intent : c.stopped === true;
  const task = taskOf(c.scheduled_task, nowMs);
  const result = resultOf(c, lastMs != null);
  const generated = momentOf(snap.generated_at);
  const head = engine === "never"
    ? { lead: t("sistema.head.never.lead"), tail: t("sistema.head.never.tail") }
    : { lead: t("sistema.head.run.lead"), tail: lastMs == null ? t("sistema.head.nodate.tail") : ago(c.last_run_at, nowMs) + "." };
  const windowText = windowLabel(c, serverDay(nowMs));
  const steps = isRecord(c.last_run_steps) ? c.last_run_steps : {};
  return {
    engine,
    eyebrow: t("sistema.eyebrow", { ask: t(askOf("sistema")), hora: hhmm(nowMs) }),
    status: stopped ? t("run.stopped") + " · " + windowText : windowText,
    old: generated != null && nowMs - generated >= OLD_MS ? t("sistema.old", { ago: ago(snap.generated_at, nowMs) }) : null,
    head,
    chip: { tone: engine, text: engine === "stale" ? t("sistema.chip.stale", { hours: ENGINE_STALE_HOURS }) : t("sistema.chip." + engine), stop: stopped },
    figures: figuresOf(snap, task, nowMs),
    strip: stripOf(snap, nowMs, lastMs, task.nextMs),
    run: {
      last: lastMs == null ? null : { at: lastMs, text: t("sistema.row.last.value", { when: whenOf(lastMs, nowMs), ago: ago(c.last_run_at, nowMs) }) },
      result,
      steps: STEP_IDS.map((id) => stepOf(id, steps[id])),
      window: windowText,
    },
    task: { state: task.state, text: task.text, hint: task.hint },
    stop: {
      on: stopped, action: stopped ? "resume" : "stop", key: keyOf("engine"),
      label: t(stopped ? "sistema.stop.act.resume" : "sistema.stop.act.stop"),
      text: t(stopped ? "sistema.stop.on" : "sistema.stop.off"),
      hint: t(stopped ? "sistema.stop.hint.resume" : "sistema.stop.hint.stop"),
    },
    account: accountOf(c.claude_account, nowMs),
    spend: spendOf(snap.claude),
    limits: limitsOf(snap, batchMin),
    sources: sourcesOf(snap, shownSources),
    errors: errorsOf(snap, nowMs, lastMs, result, shownErrors),
    foot: {
      title: t("sistema.foot.prefs"),
      keys: [["lang", "sistema.foot.lang"], ["theme", "sistema.foot.theme"], ["sound", "sistema.foot.sound"]].map(([id, label]) => ({ key: keyOf(id), label: t(label) })),
    },
  };
}
