// Whether a click on a row (not on one of its buttons) opens something: the same test the verbs' `run("open")` makes (hoy/actions.js).
// HOY, COLA and APRENDER mark such a row `data-opens="true"`, and their CSS gives it the hand and an underline under the pointer: a row
// that opened on a click but looked like plain text was one of the things that made the page hard to read (2026-10-24).

/** True when the row view has a place to open and the verb to open it. */
export const opens = (view) => !!(view && view.open) && view.actions.some((a) => a.id === "open");
