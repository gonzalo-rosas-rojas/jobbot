// The hour of the stage (spec §2.2, §4.3). In work mode `hora` is the server's wall clock (core/time.js, SERVER_TZ) or the
// number given in ?clock=. This module only knows hours and the two marks the sky celebrates (09:00 and 18:00); it never
// touches the DOM and takes every timer from outside, so tests can drive it with a fake clock.

import { clockHours, SERVER_TZ } from "../core/time.js";

export const WINDOW_START = 18;
export const WINDOW_END = 9;
// Real time between two ticks beyond which the machine was asleep or the tab frozen: the sky jumps, nothing is announced.
// A hidden tab is throttled to one tick a minute, which stays well under this.
export const MAX_GAP_MS = 5 * 60 * 1000;
const MAX_WALK_H = 6;

/** Hours since 18:00 on a 24-hour ring, in [0, 24). The sky and the lit windows are functions of this. */
export const nightT = (hora) => (((hora - WINDOW_START) % 24) + 24) % 24;

/** True while the bot's window is open (18:00 to 09:00). */
export const inWindow = (hora) => hora >= WINDOW_START || hora < WINDOW_END;

/** Decimal hour (14.5 is 14:30) of a moment in the server's zone. */
export const horaOf = (ms, tz = SERVER_TZ) => clockHours(ms, tz);

/**
 * The marks passed going forward from `prev` to `next` (decimal hours), ordered by how soon they come. A gap of more than
 * six hours, or a step backwards (a clock change), crosses nothing; midnight is not a mark.
 */
export function crossings(prev, next, marks = [9, 18]) {
  const gap = (((next - prev) % 24) + 24) % 24;
  if (gap === 0 || gap > MAX_WALK_H) return [];
  return marks
    .map((mark) => ({ mark, d: (((mark - prev) % 24) + 24) % 24 }))
    .filter(({ d }) => d > 0 && d <= gap)
    .sort((a, b) => a.d - b.d)
    .map(({ mark }) => mark);
}

/** The value of ?clock= as a decimal hour in [0, 24): "9", "0.43", "9,5" and "9:30" work; anything else is null. */
export function parseClock(search) {
  const found = /[?&]clock=([^&#]*)/.exec(search || "");
  if (!found) return null;
  let raw;
  try { raw = decodeURIComponent(found[1]).trim(); } catch (e) { return null; }
  if (raw === "") return null;
  const hm = /^(\d{1,2}):(\d{2})$/.exec(raw);
  const n = hm ? Number(hm[1]) + Number(hm[2]) / 60 : Number(raw.replace(",", "."));
  return Number.isFinite(n) && n >= 0 && n < 24 ? n : null;
}

/**
 * `onMinute(hora)` when the minute changes, `onCross(mark)` once for each of 09:00 and 18:00 passed while the page was
 * watching. `override` pins the hour (and silences both). `tick()` is also called by the shell when the tab comes back.
 */
export function createClock({
  now = Date.now,
  tz = SERVER_TZ,
  override = null,
  timers = { setInterval: (f, ms) => setInterval(f, ms), clearInterval: (id) => clearInterval(id) },
  onMinute = () => {},
  onCross = () => {},
  everyMs = 15000,
} = {}) {
  const hora = () => (override != null ? override : horaOf(now(), tz));
  let last = hora();
  let lastMs = now();
  let lastMinute = Math.floor(last * 60);
  let handle = null;

  function tick() {
    const ms = now();
    const h = hora();
    if (override == null) {
      const elapsed = ms - lastMs;
      if (elapsed >= 0 && elapsed <= MAX_GAP_MS) for (const mark of crossings(last, h)) onCross(mark);
      const minute = Math.floor(h * 60);
      if (minute !== lastMinute) { lastMinute = minute; onMinute(h); }
    }
    lastMs = ms;
    last = h;
    return h;
  }

  return {
    hora,
    tick,
    start() { if (handle == null) handle = timers.setInterval(tick, everyMs); },
    stop() { if (handle != null) { timers.clearInterval(handle); handle = null; } },
  };
}
