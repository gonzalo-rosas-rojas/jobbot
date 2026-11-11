// Clock helpers: the hour of a moment, its day, "now" on the dial, and the geometry of the 24-hour strip.
//
// Two clocks are in play. What the person sees follows the browser's time zone (or the one the tests pin with
// setTimeZone). But the bot's run window and the text of `controls.next_window` are written by the server in
// Santiago time, so comparisons against them use SERVER_TZ, whatever the browser says.

import { fmtDate, getTimeZone } from "../i18n/index.js";

export const SERVER_TZ = "America/Santiago";

/** "14:05" (24 hours) in the active time zone; a value that is not a time comes back as given. */
export const hourOf = (iso) => fmtDate(iso, "time");

// Building an Intl formatter is the slow part, and a list of moves asks for dozens of times.
const made = new Map();
function format(kind, tz) {
  const id = kind + "|" + (tz || "");
  if (!made.has(id)) {
    made.set(id, kind === "day"
      ? new Intl.DateTimeFormat("sv-SE", { timeZone: tz || undefined })
      : new Intl.DateTimeFormat("en-GB", { timeZone: tz || undefined, hour: "2-digit", minute: "2-digit", hourCycle: "h23" }));
  }
  return made.get(id);
}

/** "2026-10-07" in the active time zone. */
export function dayOf(iso) {
  try { return format("day", getTimeZone()).format(new Date(iso)); } catch (e) { return String(iso); }
}

/** Today's date as the server's clock writes it, for comparing with `controls.next_window`. */
export function serverDay(ms) {
  return format("day", SERVER_TZ).format(new Date(ms));
}

function parts(iso, tz) {
  const out = {};
  for (const p of format("clock", tz).formatToParts(new Date(iso))) out[p.type] = p.value;
  return { hour: Number(out.hour), minute: Number(out.minute) };
}

/** The hour (0 to 23) of a moment in the active time zone; 0 when it is not a time. */
export function hourNumber(iso) {
  try { return parts(iso, getTimeZone()).hour; } catch (e) { return 0; }
}

/** Hours since midnight as a fraction (14.5 is 14:30), in the active zone: where the hand of a 24-hour dial points. */
export function clockHours(ms, tz = getTimeZone()) {
  const { hour, minute } = parts(ms, tz);
  return hour + minute / 60;
}

// ---- The strip of the last 24 hours: 48 half-hour slots, the last one holding "now"

/** `sum` of the counts from index `at` to the end (how many scores are at or above a threshold). */
export const sumFrom = (counts, at) => counts.slice(Math.max(0, at)).reduce((n, x) => n + x, 0);

/**
 * Puts every move of the strip at its place: `x` along `width` by time, and the first `row` (0 to 4) where it does not
 * touch the one before it by less than `gap`. Oldest first, so the rows fill from the left.
 */
export function placeMoves(moves, fromMs, spanMs, width, gap = 13) {
  const rowEnd = [];
  return moves.slice().sort((a, b) => Date.parse(a.at) - Date.parse(b.at)).map((move) => {
    const x = Math.max(0, Math.min(width, ((Date.parse(move.at) - fromMs) / spanMs) * width));
    let row = rowEnd.findIndex((end) => x - end >= gap);
    if (row < 0) row = rowEnd.length;
    row = Math.min(row, 4);
    rowEnd[row] = x;
    return { move, x, row };
  });
}

/**
 * The labelled hours of the strip: every third hour on the hour, by slot index and percent of the width, leaving out
 * the one at the very start and any that would sit on top of "now" (`nowPercent`).
 */
export function axisHours(slots, nowPercent) {
  const out = [];
  slots.forEach((slot, i) => {
    if (new Date(slot.at).getUTCMinutes() !== 0 || hourNumber(slot.at) % 3 !== 0) return;
    const at = (i / slots.length) * 100;
    if (Math.abs(at - nowPercent) < 11 || at < 2) return;
    out.push({ percent: at, hour: hourNumber(slot.at) });
  });
  return out;
}
