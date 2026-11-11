// The rhythm of a follow-up (spec §5.2): worked out in the browser from `state` and `days_in_state`, because the engine does not schedule
// follow-ups yet and the snapshot carries no `next_action` (§7, §9.1). A table of constants and one pure function; nothing here writes words
// (view.js turns the answer into a sentence of the language).
//
//   a sent application: day 8-10 ask how it goes (1st follow-up), day 15-17 insist (2nd), day 45 give it up for lost;
//   an answered one with no task on it is looked at on day 3 ("define the next step"); the median answer takes 6-7 days (R5 §2).

export const CADENCE = Object.freeze({
  ask: Object.freeze([8, 10]),
  insist: Object.freeze([15, 17]),
  close: 45,
  median: Object.freeze([6, 7]),
  decide: 3,
});

/** The days a card has spent in its stage as a whole number of at least 0; a missing or broken value is 0, never NaN. */
export function dayOf(card) {
  const n = Math.trunc(Number(card && card.days_in_state));
  return Number.isFinite(n) && n > 0 ? n : 0;
}

/**
 * cadenceOf(card, { hasTask }) -> { kind, day, step?, window?, inWindow, daysTo }
 *   kind  "soon" (a follow-up is coming: `step` 1 or 2, `daysTo` days away) · "ask" · "insist" (inside the window of step 1 or 2) · "closing" (waiting
 *         for day 45, `daysTo` away) · "lost" (45 days or more) · "reply" (answered less than 3 days ago) · "decide" (answered, nobody has a task) ·
 *         "task" (answered, a task is on it) · "test" · "interview" · "offer" · "quiet" (no news, `daysTo` the 45th day) · "pre" (not sent yet)
 */
export function cadenceOf(card, { hasTask = false } = {}) {
  const day = dayOf(card);
  const base = { day, inWindow: false, daysTo: 0 };
  const [ask0, ask1] = CADENCE.ask;
  const [insist0, insist1] = CADENCE.insist;
  switch (card && card.state) {
    case "submitted":
      if (day < ask0) return { ...base, kind: "soon", step: 1, window: CADENCE.ask, daysTo: ask0 - day };
      if (day <= ask1) return { ...base, kind: "ask", step: 1, window: CADENCE.ask, inWindow: true };
      if (day < insist0) return { ...base, kind: "soon", step: 2, window: CADENCE.insist, daysTo: insist0 - day };
      if (day <= insist1) return { ...base, kind: "insist", step: 2, window: CADENCE.insist, inWindow: true };
      if (day < CADENCE.close) return { ...base, kind: "closing", daysTo: CADENCE.close - day };
      return { ...base, kind: "lost" };
    case "replied":
      if (hasTask) return { ...base, kind: "task" };
      return { ...base, kind: day < CADENCE.decide ? "reply" : "decide" };
    case "technical_test": return { ...base, kind: "test" };
    case "interview": return { ...base, kind: "interview" };
    case "offer": return { ...base, kind: "offer" };
    case "ghosted":
      return day >= CADENCE.close ? { ...base, kind: "lost" } : { ...base, kind: "quiet", daysTo: CADENCE.close - day };
    default: return { ...base, kind: "pre" };
  }
}
