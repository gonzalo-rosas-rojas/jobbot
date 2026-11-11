// Small functions that turn data into words, on top of the i18n dictionaries and formatters: no Spanish or English
// is written here. Ported from the old page; the wording moved to the dictionaries, the arithmetic is the same.

import { t, tn, has, fmtNumber, fmtPercent, fmtMoney, fmtDate, fmtRelative } from "../i18n/index.js";

export const pct = fmtPercent;
export const usd = fmtMoney;
export const ago = fmtRelative;
/** "7 Oct, 14:05" (the old page printed the short date and time of Santiago). */
export const when = (iso) => fmtDate(iso, "datetime");
/** "1 día" / "2 días": the plural of `key` (`key.one`, `key.other`) with the count. */
export const plural = (n, key, vars) => tn(key, n, vars);

/** A factor of the score, by the name the page knows; one it does not know shows as the server sent it (a factor added later still reads). */
export const factorName = (key) => (has("detail.factor." + key) ? t("detail.factor." + key) : String(key));
/** The points a factor gave, as a number in the language's own format; "" when the server sent anything that is not a number (never a made-up "0"). */
export const factorPoints = (value) => { const n = pointsOf(value); return n === null ? "" : fmtNumber(n, { maximumFractionDigits: 1 }); };

// What each factor can give: the weights of profile/criteria.toml (03 §1.2). The snapshot carries the points of each factor, not what it
// could have given. The order is the two columns of COLA's sheet: three on the left, three on the right.
export const FACTOR_WEIGHTS = [["fit", 30], ["compensation", 15], ["interview_style", 10], ["stability", 20], ["learning", 15], ["other", 10]];
const FACTOR_MAX = new Map(FACTOR_WEIGHTS);

/** The points of a factor as the number the server meant, or null when it sent anything else (text, a boolean, nothing): never a made-up zero. */
export function pointsOf(value) {
  const n = typeof value === "number" || (typeof value === "string" && value.trim() !== "") ? Number(value) : NaN;
  return Number.isFinite(n) ? n : null;
}

/**
 * The bars of a score breakdown ({ factor: points }) as data, the same for every place that draws one: the factors the page knows in the order of
 * FACTOR_WEIGHTS, then any other name the server added, sorted. A factor whose points are not a number is no bar. `k` is the length of its track,
 * 0 to 1: its points over what it can give (a factor the page does not know is measured against the largest one). Anything but an object gives [].
 */
export function factorBars(scoreDetail) {
  const raw = scoreDetail && typeof scoreDetail === "object" ? scoreDetail : {};
  const ids = [...FACTOR_WEIGHTS.map(([id]) => id).filter((id) => id in raw), ...Object.keys(raw).filter((id) => !FACTOR_MAX.has(id)).sort()];
  const top = Math.max(...FACTOR_MAX.values());
  return ids.flatMap((id) => {
    const points = pointsOf(raw[id]);
    if (points === null) return [];
    const max = FACTOR_MAX.get(id) || null;
    return [{ id, points, max, text: factorText(id, points), k: Math.max(0, Math.min(1, points / (max || top))) }];
  });
}

/** "27 / 30": the points of a factor over what it can give (just the points for a factor the page does not know); null when they are not a number. */
export function factorText(id, value) {
  const n = pointsOf(value);
  if (n === null) return null;
  const max = FACTOR_MAX.get(id);
  return max ? factorPoints(n) + " / " + factorPoints(max) : factorPoints(n);
}

/** The score of a row with its word, rounded ("puntaje 89"); "" when the card has none, so a row never shows a bare number or "null". */
export function scoreText(score) {
  const n = pointsOf(score);
  return n === null ? "" : t("common.score", { score: Math.round(n) });
}

export const daysSince = (iso, nowMs) => Math.max(0, Math.floor((nowMs - Date.parse(iso)) / 86400000));
export const norm = (text) => String(text || "").toLowerCase();
export const escapeRegExp = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
/** Up to three letters or digits of a name, for a small badge: "Alpha Test Co" gives "ALP". legacy quirk: no name gives "", not "?" (the fallback is stripped as a symbol). */
export const initials = (name) => String(name || "?").replace(/[^\p{L}\p{N}]/gu, "").slice(0, 3).toUpperCase();
export const lowerFirst = (text) => text.charAt(0).toLowerCase() + text.slice(1);
/** A phrase made into a sentence: first letter up, a full stop unless it already ends in one. */
export function asSentence(text) {
  const up = text.charAt(0).toUpperCase() + text.slice(1);
  return /[.!?»]$/.test(up) ? up : up + ".";
}

/**
 * The time left to a task, or how late it is, with a tone for the page to colour: "late", "soon" (under 48 h) or "".
 * No date is no countdown (null).
 */
export function countdown(task, nowMs) {
  if (!task.due_at) return null;
  const ms = Date.parse(task.due_at) - nowMs;
  if (task.overdue || ms < 0) {
    const hours = Math.floor(-ms / 3600000);
    const text = hours < 48 ? t("time.due.late.hours", { n: Math.max(hours, 1) }) : tn("time.due.late.days", Math.floor(hours / 24));
    return { text, tone: "late" };
  }
  const hours = Math.floor(ms / 3600000);
  if (hours < 1) return { text: t("time.due.lessHour"), tone: "soon" };
  if (hours < 48) return { text: t("time.due.hours", { n: hours }), tone: "soon" };
  return { text: tn("time.due.days", Math.floor(hours / 24)), tone: "" };
}

// ---- Names of things that come from the server as ids. An id the dictionary does not know is shown as it is,
// so a state added to the server later still reads as something.

const label = (prefix, id) => (has(prefix + id) ? t(prefix + id) : String(id));
export const stateLabel = (state) => label("state.", state);
export const stateName = (state) => label("state.plural.", state);
export const stageLabel = (state) => label("stage.", state);
// An answer he can record, said the way it happened ("Me citaron a entrevista"), and the question that confirms it. `from` is the
// state it leaves: a second interview (interview -> interview) has its own words.
const markKey = (prefix, mark, from) => (mark === from && has(prefix + mark + ".again") ? prefix + mark + ".again" : prefix + mark);
export const markPhrase = (mark, from) => { const k = markKey("mark.", mark, from); return has(k) ? t(k) : stateLabel(mark); };
export const markQuestion = (mark, from) => { const k = markKey("mark.ask.", mark, from); return has(k) ? t(k) : t("mark.ask.other", { state: stateLabel(mark) }); };
export const kindLabel = (kind) => label("kind.", kind);
export const turnLabel = (turn) => label("turn.", turn);
export const statusWord = (status) => label("status.", status);
export const actionLabel = (action) => label("act.", action);
export const actionHint = (action) => (has("act." + action + ".hint") ? t("act." + action + ".hint") : "");

/**
 * Why an application needs Gonza, in one sentence. The server sends it in Spanish with a `reason_kind`; the kinds are
 * a closed list, so the sentence is the dictionary's (and follows the language). An unknown kind shows what the
 * server said. No reason at all is null.
 */
export function reasonShort(card) {
  const kind = card.reason_kind;
  if (kind && has("reason.short." + kind)) return t("reason.short." + kind);
  return card.reason_short || null;
}

export function reasonGroupTitle(kind) {
  return label("reason.group.", kind);
}

// ---- The bot's run window and its spend, as the header says them

/**
 * When the window opens, from the server's own clock text ("2026-10-07 18:00", Santiago): the hour alone when it is
 * today there, otherwise the day and the hour ("7 Oct 18:00"). `todayServer` is today's date in that same clock.
 */
export function nextWindowLabel(next, todayServer) {
  const [day, hour] = String(next || "").split(" ");
  if (day === todayServer && hour) return hour;
  if (day && hour) return fmtDate(day + "T12:00:00Z", "day", "UTC") + " " + hour;
  return String(next || "");
}

export function windowLabel(controls, todayServer) {
  if (controls.window_open === true) return t("win.open");
  if (controls.window_open === false) {
    return controls.next_window ? t("win.closedOpens", { when: nextWindowLabel(controls.next_window, todayServer) }) : t("win.closed");
  }
  return t("win.unknown");
}

export function spendLabel(claude) {
  const spent = fmtMoney(claude.spent_24h);
  return claude.daily_budget != null ? t("spend.line", { spent, budget: fmtMoney(claude.daily_budget) }) : t("spend.noBudget", { spent });
}
