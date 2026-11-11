// What the keys and the buttons of APRENDER do to the data (spec §3.4, §5.4, §9.3). It has no DOM: the session, the sound, the clock and
// the timers are handed in, so the whole of it (which request each verb makes, the two steps, the row that is dimmed while the server
// answers) runs in Node with a fake session.
//
// A row is the `row` of aprenderView: { id, data: { card }, keys[], open, link, actions[{ id, permanent }] }. `run(actionId, row)` says what
// happened: "armed" (first press of u), "fired" (the request went), "opened", or "ignored".
//   - `resolve` is the one verb that changes data: it gives the forms a question blocks back to the queue (`POST /api/resolve`, one `key` or
//     a list of `keys`, the server answers 409 if one is refused and the others stay done). It says "the answer is in my bank", which is a
//     claim the page cannot check, so it needs two deliberate presses like HOY's permanent verbs: the armed state lives HERE, not in a
//     button's text, which lets the page redraw under it without disarming anything;
//   - the row is dim from the first instant (R7 §2.9) and stays so until the snapshot that follows the 200; a refusal gives it back;
//   - there is no verb that saves an answer: the route (`POST /api/answers`) does not exist, and the page does not pretend it does.

import { heldKey } from "../../core/dom.js";
import { createArmer } from "../shared/armer.js";

/** The request of a verb on a row, or null when the row cannot take it: { path, body }. */
export function requestOf(actionId, row) {
  if (actionId !== "resolve") return null;
  const keys = (row && Array.isArray(row.keys) ? row.keys : []).filter(Boolean);
  if (keys.length === 0) return null;
  return { path: "/api/resolve", body: keys.length === 1 ? { key: keys[0] } : { keys } };
}

/**
 * createActions({ session, sound, announce, t, open, openUrl, onChange, now, timers, held }) ->
 *   { run(actionId, row), isArmed(rowId, actionId), armedOf(rowId), isPending(rowId), disarm(), destroy() }
 * `onChange(rowId)` is called whenever a row's pending or armed state changes: the render paints that row again.
 */
export function createActions({ session, sound, announce, t, open, openUrl, onChange = () => {}, now = Date.now, timers = { set: (fn, ms) => setTimeout(fn, ms), clear: (id) => clearTimeout(id) }, held = heldKey }) {
  const pending = new Set();
  const armer = createArmer({ announce, sound, t, onChange, now, timers, held });

  async function send(actionId, row) {
    const request = requestOf(actionId, row);
    if (!request) return "ignored";
    pending.add(row.id);
    onChange(row.id);
    try {
      await session.act(request.path, request.body, {
        beforeRefresh: (r) => {
          if (r.ok) {
            sound.play("undo");
          } else {
            pending.delete(row.id); // the server said no (a 409 carries its reason, the toast already has it): the row is his again
            onChange(row.id);
            sound.play("error");
          }
        },
      });
    } finally {
      pending.delete(row.id);
      onChange(row.id);
    }
    return "fired";
  }

  async function run(actionId, row) {
    if (!row || !Array.isArray(row.actions) || !row.actions.some((a) => a.id === actionId)) return "ignored";
    if (pending.has(row.id)) return "ignored";
    if (actionId === "open") {
      if (!row.open) return "ignored";
      open(row.open, row.id);
      return "opened";
    }
    if (actionId === "link") {
      if (!row.link) return "ignored";
      openUrl(row.link);
      return "opened";
    }
    if (actionId !== "resolve") return "ignored";
    if (row.actions.find((a) => a.id === actionId).permanent) {
      const pressed = armer.press(row.id, actionId);
      if (pressed !== "confirmed") return pressed;
    } else {
      armer.release(row.id);
    }
    return send(actionId, row);
  }

  return {
    run,
    isArmed: armer.isArmed,
    /** The verb that is armed on this row, or null: the row is drawn with it ("¿Seguro?" on that one button). */
    armedOf: armer.armedOf,
    isPending: (rowId) => pending.has(rowId),
    disarm: armer.disarm,
    destroy() { armer.disarm(); pending.clear(); },
  };
}
