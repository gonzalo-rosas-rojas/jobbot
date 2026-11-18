// Clock helpers: the hour of a moment, its day, "now" on the dial, and the geometry of the 24-hour strip.
//
// Two clocks are in play. What the person sees follows the browser's time zone (or the one the tests pin with
// setTimeZone). But the bot's run window and the text of `controls.next_window` are written by the server in
// Santiago time, so comparisons against them use SERVER_TZ, whatever the browser says.

import { fmtDate, getTimeZone } from "../i18n/index.js";

export const SERVER_TZ = "America/Santiago";

export const SECOND_MS = 1000;
export const MINUTE_MS = 60 * SECOND_MS;
export const HOUR_MS = 60 * MINUTE_MS;
export const DAY_MS = 24 * HOUR_MS;
/** A task due within this many hours is "soon" (the countdown and the ticket both say so). */
export const SOON_HOURS = 48;

/** "14:05" (24 hours) in the active time zone; a value that is not a time comes back as given. */
export const hourOf = (iso) => fmtDate(iso, "time");

// Building an Intl formatter is the slow part, and a list of moves asks for dozens of times.
const made = new Map();
const FORMATTERS = {
  day: (tz) => new Intl.DateTimeFormat("sv-SE", { timeZone: tz || undefined }),
  clock: (tz) => new Intl.DateTimeFormat("en-GB", { timeZone: tz || undefined, hour: "2-digit", minute: "2-digit", hourCycle: "h23" }),
};
function format(kind, tz) {
  const make = FORMATTERS[kind];
  if (!make) throw new Error('time: unknown format "' + kind + '"'); // never quietly format something else
  const id = kind + "|" + (tz || "");
  if (!made.has(id)) made.set(id, make(tz));
  return made.get(id);
}

/** "2026-10-07" in the active time zone; a value that is not a date comes back as given. */
export function dayOf(iso) {
  try { return format("day", getTimeZone()).format(new Date(iso)); } catch (e) { return String(iso); }
}

/** Today's date as the server's clock writes it, for comparing with `controls.next_window`. */
export function serverDay(ms) {
  return format("day", SERVER_TZ).format(new Date(ms));
}

/** Hour and minute on the wall clock of `tz`; `instant` is an ISO date or milliseconds. */
function hourMinuteOf(instant, tz) {
  const out = {};
  for (const p of format("clock", tz).formatToParts(new Date(instant))) out[p.type] = p.value;
  return { hour: Number(out.hour), minute: Number(out.minute) };
}

/** Hours since midnight as a fraction (14.5 is 14:30), in the active zone: where the hand of a 24-hour dial points. */
export function clockHours(ms, tz = getTimeZone()) {
  const { hour, minute } = hourMinuteOf(ms, tz);
  return hour + minute / 60;
}

// ---- The strip of the last 24 hours: 48 half-hour slots, the last one holding "now"

/** `sum` of the counts from index `at` to the end (how many scores are at or above a threshold). */
export const sumFrom = (counts, at) => counts.slice(Math.max(0, at)).reduce((n, x) => n + x, 0);

const EVERY_HOURS = 3;                     // one label for each three hours of the strip
const NOW_CLEARANCE = 11;                  // percent of the axis around "now" that keeps its own label alone
const EDGE = 2;                            // percent at the start of the axis where no label fits

/**
 * The labelled hours of the strip: every third hour on the hour, by slot index and percent of the width, leaving out
 * the one at the very start and any that would sit on top of "now" (`nowPercent`). Hour and minute are read from the
 * same clock (the active zone), so a zone with a half-hour offset marks its own hours.
 */
export function axisHours(slots, nowPercent) {
  const out = [];
  slots.forEach((slot, i) => {
    let clock;
    try { clock = hourMinuteOf(slot.at, getTimeZone()); } catch (e) { return; } // a slot whose time cannot be read gets no label; the axis goes on
    if (clock.minute !== 0 || clock.hour % EVERY_HOURS !== 0) return;
    const at = (i / slots.length) * 100;
    if (Math.abs(at - nowPercent) < NOW_CLEARANCE || at < EDGE) return;
    out.push({ percent: at, hour: clock.hour });
  });
  return out;
}
