// What the keys and the buttons of HOY do to the data (spec §3.4, §5.1, §9.3). It has no DOM: the session, the sound, the clock and the
// timers are handed in, so the whole of it (which request each verb makes, the two steps of what cannot be undone, the row that is
// dimmed while the server answers) runs in Node with a fake session.
//
// A row is the `row` of hoyView: { id, data: { card } | { task }, open, link, actions[{ id, permanent }] }. `run(actionId, row)` says
// what happened: "armed" (first press of a permanent verb), "fired" (the request went), "opened", "ignored" or "cancelled".
//   - a verb that changes the data goes through `session.act` with the row `pending` from the first instant (R7 §2.9: the row answers
//     in under 100 ms, the server in its own time); a failure gives the row back, a success keeps it dim until the snapshot that
//     follows the 200 removes it;
//   - `approve`, `confirm` and `done` cannot be undone, so the first press arms them ("¿Seguro? Pulsa otra vez") and the second,
//     deliberate one (not a held key, not within 350 ms, within 5 s) does them; the armed state lives HERE, not in a button's text,
//     which is what lets the page redraw under it without disarming anything;
//   - `snooze`, `withdraw` and `register` need one more answer (until when, which reason, which note): `ask(id, row)` supplies it
//     (T19 draws the little menus) and null means he cancelled. Without `ask`, only `snooze` works, until tomorrow.
//   - those three have a way back, so the menu's pick acts at once and the toast offers "Deshacer" (with `toast`): a skip is
//     `/api/unsnooze`, an answer or a withdrawal is `/api/mark/undo` (the server takes back the last mark for 10 minutes;
//     2026-10-24, a pick in the wrong place had no way back from HOY or PROCESOS).

import { heldKey } from "../../core/dom.js";
import { createArmer, REAL_TIMERS } from "../shared/armer.js";
import { createSender } from "../shared/send.js";

export const DEFAULT_SNOOZE = "1d";
const ASKS = new Set(["snooze", "withdraw", "register"]);
// The sound of each verb when the server says yes (the cues of src/sound/cues.js). The two presses have their own: "arm" and "confirm".
const CUE = { done: "done", approve: "approve", confirm: "sent", resolve: "undo", send_anyway: "undo", snooze: "snooze", unsnooze: "undo", withdraw: "withdraw", register: "register" };
// The way back of a verb, and the sentence its toast falls back to when the server's answer has none.
const UNDO = {
  snooze: { path: "/api/unsnooze", words: "hoy.undo.snooze" },
  withdraw: { path: "/api/mark/undo", words: "hoy.undo.withdraw" },
  register: { path: "/api/mark/undo", words: "hoy.undo.register" },
};
const undoOf = (actionId, row) => {
  const back = UNDO[actionId];
  return back ? { path: back.path, body: { key: row.data.card }, words: back.words } : null;
};

const UNSNOOZE_ROW_PREFIX = "snz:";
/** The row HOY hands to `run` to take a snoozed card back: the strip of snoozed cards has no row of its own, only the card's key. */
export const unsnoozeRowOf = (key) => ({ id: UNSNOOZE_ROW_PREFIX + key, data: { card: key }, open: null, link: null, actions: [{ id: "unsnooze", permanent: false }] });

/** The request of a verb on a row, or null when the row cannot take it: { path, body }. `extra` is what `ask` answered. */
export function requestOf(actionId, row, extra = {}) {
  const card = row.data && row.data.card;
  const task = row.data && row.data.task;
  switch (actionId) {
    case "done": return task != null ? { path: "/api/task/done", body: { id: Number(task) } } : null;
    case "approve":
    case "confirm":
    case "resolve":
    case "send_anyway":
    case "unsnooze": return card ? { path: "/api/" + actionId, body: { key: card } } : null;
    case "snooze": return card ? { path: "/api/snooze", body: { key: card, until: extra.until || DEFAULT_SNOOZE } } : null;
    case "withdraw": return card ? { path: "/api/mark", body: { key: card, state: "withdrawn", note: extra.note || null } } : null;
    case "register": return card && extra.state ? { path: "/api/mark", body: { key: card, state: extra.state, note: extra.note || null } } : null;
    default: return null;
  }
}

/**
 * createActions({ session, sound, announce, t, open, openUrl, ask, toast, afterLink, onChange, now, timers, held }) ->
 *   { run(actionId, row, { confirmed }), isArmed(rowId, actionId), armedOf(rowId), isPending(rowId), disarm(), destroy() }
 * `onChange(rowId)` is called whenever a row's pending or armed state changes: the render paints that row again.
 * `toast(text, ok, { undo })` is the shell's toast; without it nothing offers "Deshacer".
 */
export function createActions({ session, sound, announce, t, open, openUrl, ask = null, toast = null, afterLink = null, onChange = () => {}, now = Date.now, timers = REAL_TIMERS, held = heldKey }) {
  const pending = new Set();
  const armer = createArmer({ announce, sound, t, onChange, now, timers, held });
  // the toast of the server's own words is replaced by the same words with "Deshacer" (shared/send.js)
  const send = createSender({ session, sound, pending, onChange, requestOf, cue: CUE, undoOf, toast, t });

  async function run(actionId, row, { confirmed = false } = {}) {
    if (!row || !row.actions.some((a) => a.id === actionId)) return "ignored";
    if (pending.has(row.id)) return "ignored";
    if (actionId === "open") {
      if (!row.open) return "ignored";
      open(row.open, row.id);
      return "opened";
    }
    if (actionId === "link") {
      if (!row.link) return "ignored";
      openUrl(row.link);
      if (afterLink) afterLink(row);
      return "opened";
    }
    // `confirmed`: the page already asked him (the "did you do it?" line), so that question is the second press
    const permanent = row.actions.find((a) => a.id === actionId).permanent && !confirmed;
    if (permanent) {
      const pressed = armer.press(row.id, actionId);
      if (pressed !== "confirmed") return pressed;
    } else {
      armer.release(row.id);
    }
    let extra = {};
    if (ASKS.has(actionId)) {
      if (ask) {
        extra = await ask(actionId, row);
        if (extra == null) return "cancelled";
      } else if (actionId !== "snooze") {
        return "ignored";
      }
    }
    return send(actionId, row, extra);
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
