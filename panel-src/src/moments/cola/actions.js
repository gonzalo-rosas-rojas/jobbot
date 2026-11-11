// What the keys and the buttons of COLA do to the data (spec §3.4, §5.3, §9.3). It has no DOM: the session, the sound, the clock and the
// timers are handed in, so all of it (which request each verb makes, the two presses of what cannot be undone, the drawn confirmation of
// the lote, the row that is dim while the server answers, the way back of a skip) runs in Node with a fake session.
//
// A row is the `row` of colaView: { id, data: { card }, open, link, actions[{ id, permanent }] }. `run(actionId, row)` says what happened:
// "armed" (first press of a permanent verb), "fired" (the request went), "opened", "answering", "ignored" or "cancelled".
//   - a verb that changes the data goes through `session.act` with the row `pending` from the first instant (R7 §2.9: the row answers in
//     under 100 ms, the server in its own time); a 409, a 500 or no network gives the row back and the toast says why (session.act);
//   - `approve` cannot be undone (the server has no way back), so the first press arms it ("¿Seguro? Pulsa otra vez") and the second,
//     deliberate one (not a held key, not within 350 ms, within 5 s) does it; the armed state lives HERE, not in a button's text, which is
//     what lets the page be redrawn under it without disarming anything;
//   - `snooze` asks until when (`ask`, the little menu) and has a way back: the toast offers "Deshacer", which is `/api/unsnooze`;
//   - the lote is two presses by drawing: `batch.open()` shows the confirmation, `batch.confirm()` (↵) sends ONE request with every key,
//     `batch.cancel()` (Esc) drops it. It also closes by itself (`batch.close()`): after BATCH_MAX_MS, when a snapshot takes one of its picks away, when the
//     drawer opens or the page leaves COLA. The server answers one line per key and a 409 when any failed: the others stay done;
//   - `engine()` is the engine's switch, the one of SISTEMA (../shared/engine.js): stopping goes at once with a "Deshacer", resuming takes two presses.

import { ARM_MIN_MS, heldKey } from "../../core/dom.js";
import { createArmer } from "../shared/armer.js";
import { createEngineSwitch, engineRequest } from "../shared/engine.js";

export const DEFAULT_SNOOZE = "1d";
// How long a drawn lote waits for its yes. Longer than the 5 s of an armed verb (ADR 0006): the lote asks him to read a list of names first
// and the ADR gives it no limit of its own. After this it closes by itself, because it cannot be undone and nobody should confirm a question forgotten on screen.
export const BATCH_MAX_MS = 30000;
export const MAX_BATCH = 200;                       // the server takes at most this many keys in one request (crates/jobbot-panel/src/http.rs)
// The sound of each verb when the server says yes (the cues of src/sound/cues.js). The two presses have their own: "arm" and "confirm".
const CUE = { approve: "approve", batch: "approve", confirm: "sent", resolve: "undo", snooze: "snooze", unsnooze: "undo" };

/** The request of a verb on a row, or null when the row cannot take it: { path, body }. `extra` is what `ask` answered. */
export function requestOf(actionId, row, extra = {}) {
  const card = row.data && row.data.card;
  switch (actionId) {
    case "approve":
    case "resolve":
    case "confirm":
    case "unsnooze": return card ? { path: "/api/" + actionId, body: { key: card } } : null;
    case "snooze": return card ? { path: "/api/snooze", body: { key: card, until: extra.until || DEFAULT_SNOOZE } } : null;
    default: return null;
  }
}

// A control with an action of its own: Enter on one of these does what it does, never the lote's yes. A tabindex of -1 is only a place to put the focus.
const CONTROL = 'a[href], button, input, select, textarea, summary, [role="button"], [role="menuitem"], [role="link"], [data-action], [tabindex]:not([tabindex="-1"])';

/**
 * Whether Enter, pressed with the focus on `target`, is the drawn lote's yes: inside the lote (but not on its cancel button), or on no control at all
 * (`bare` are the nodes that mean "nowhere": the body, the root). Any other button, link or field keeps its Enter. Pure: the page hands in `inConfirm`.
 */
export function enterIsLots(target, { inConfirm = () => false, bare = [] } = {}) {
  if (!target || bare.includes(target)) return true;
  if (inConfirm(target)) return !(target.closest && target.closest('[data-batch="cancel"]'));
  return !(target.closest && target.closest(CONTROL));
}

/** The lote is one request: `{ keys }` (at most MAX_BATCH), or null when there is nothing to approve. */
export const batchRequest = (keys) => (keys.length ? { path: "/api/approve", body: { keys: keys.slice(0, MAX_BATCH) } } : null);

export { engineRequest };

/**
 * createColaActions({ session, sound, announce, t, open, go, openUrl, ask, toast, onChange, now, timers, held }) ->
 *   { run(actionId, row), engine(stopped), batch: { open(picks), confirm(), cancel(), close(), sync(cards), isOpen(), picks() },
 *     isArmed(rowId, actionId), armedOf(rowId), isPending(rowId), engineArmed(), disarm(), destroy() }
 * `onChange(id)` hears every row whose pending or armed state changed, and "batch" and "engine": the render paints that part again.
 */
export function createColaActions({ session, sound, announce, t, open, go, openUrl, ask = null, toast = null, onChange = () => {}, now = Date.now, timers = { set: (fn, ms) => setTimeout(fn, ms), clear: (id) => clearTimeout(id) }, held = heldKey }) {
  const pending = new Set();
  const armer = createArmer({ announce, sound, t, onChange, now, timers, held });   // rowId "engine" is the engine's switch
  const engineSwitch = createEngineSwitch({ armer, session, sound, announce, t, toast, onChange: () => onChange("engine") });
  let drawn = null;          // { picks, at, timer }: the lote waiting for its confirmation

  const say = (r) => (session.said ? session.said(r, "") : "");

  async function send(actionId, row, extra) {
    const request = requestOf(actionId, row, extra);
    if (!request) return "ignored";
    pending.add(row.id);
    onChange(row.id);
    try {
      await session.act(request.path, request.body, {
        onOk: (r) => {
          // a skip has a way back; the toast of the server's own words is replaced by the same words with "Deshacer"
          if (actionId === "snooze" && toast) {
            toast(say(r) || t("cola.undo.snooze"), true, { undo: () => session.act("/api/unsnooze", { key: row.data.card }, { beforeRefresh: (u) => sound.play(u.ok ? CUE.unsnooze : "error") }) });
          }
        },
        beforeRefresh: (r) => {
          if (r.ok) {
            sound.play(CUE[actionId] || "press");
          } else {
            pending.delete(row.id); // the server said no (a 409 carries its reason, the toast already has it): the row is his again
            onChange(row.id);
            sound.play("error");
          }
        },
      });
    } finally {
      pending.delete(row.id); // the snapshot after the 200 has already redrawn without the row, or with it: either way it is not waiting
      onChange(row.id);
    }
    return "fired";
  }

  async function run(actionId, row) {
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
      return "opened";
    }
    if (actionId === "answer") {
      go("aprender");
      return "answering";
    }
    const permanent = row.actions.find((a) => a.id === actionId).permanent;
    if (permanent) {
      const step = armer.press(row.id, actionId);
      if (step !== "confirmed") return step;
    } else {
      armer.release(row.id);
    }
    let extra = {};
    if (actionId === "snooze") {
      if (ask) {
        extra = await ask(actionId, row);
        if (extra == null) return "cancelled";
      }
    }
    return send(actionId, row, extra);
  }

  // ---- the lote: drawn, then confirmed
  function drop() {
    if (!drawn) return false;
    if (drawn.timer != null) timers.clear(drawn.timer);
    drawn = null;
    onChange("batch");
    return true;
  }

  const batch = {
    /** Shows the confirmation for these picks ([{ key, company, score }]); false when there is nothing to approve. */
    open(picks) {
      if (!picks || picks.length === 0) return false;
      armer.disarm();
      drawn = { picks: picks.slice(0, MAX_BATCH), at: now(), timer: null };
      drawn.timer = timers.set(() => { drawn.timer = null; batch.close(); }, BATCH_MAX_MS);
      sound.play("arm");
      onChange("batch");
      return true;
    },
    isOpen: () => drawn !== null,
    picks: () => (drawn ? drawn.picks.slice() : []),
    cancel: drop,
    /** The lote closes by itself (time ran out, the list changed, the page left COLA or opened the drawer): like cancel, and it says so. */
    close() {
      if (!drop()) return false;
      announce(t("cola.batch.closed"));
      return true;
    },
    /** A snapshot arrived: `cards` are the proposals now waiting. A pick that is gone, or no longer approvable, closes the whole lote. */
    sync(cards) {
      if (!drawn) return false;
      const ok = new Set((cards || []).filter((c) => (c.actions || []).includes("approve")).map((c) => c.key));
      return drawn.picks.every((p) => ok.has(p.key)) ? false : batch.close();
    },
    /** The second press. A held key or one inside 350 ms of the first is not it: "ignored", the confirmation stays. */
    async confirm() {
      if (!drawn) return "ignored";
      if (held() || now() - drawn.at < ARM_MIN_MS) return "ignored";
      const keys = drawn.picks.map((p) => p.key).filter((k) => !pending.has("card:" + k));
      const request = batchRequest(keys);
      drop();
      if (!request) return "ignored";
      sound.play("confirm");
      for (const key of request.body.keys) { pending.add("card:" + key); onChange("card:" + key); }
      try {
        await session.act(request.path, request.body, {
          beforeRefresh: (r) => {
            if (r.ok) sound.play(CUE.batch);
            else sound.play("error"); // some failed (409): the server did the rest, and the snapshot after it says which; every row is his again below
          },
        });
      } finally {
        for (const key of request.body.keys) { pending.delete("card:" + key); onChange("card:" + key); }
      }
      return "fired";
    },
  };

  return {
    run,
    engine: engineSwitch.press,
    /** { intent, pending } of the engine's switch: STOP as the page shows it before the snapshot, and whether the request is out. */
    engineState: engineSwitch.state,
    /** A snapshot arrived: after an answered request it is the truth, so the intent goes. */
    settle: engineSwitch.settle,
    batch,
    isArmed: armer.isArmed,
    /** The verb that is armed on this row, or null: the row is drawn with it ("¿Seguro?" on that one button). */
    armedOf: armer.armedOf,
    isPending: (rowId) => (rowId === "engine" ? engineSwitch.state().pending : pending.has(rowId)),
    engineArmed: () => armer.isArmed("engine"),
    disarm: armer.disarm,
    destroy() { armer.disarm(); drop(); pending.clear(); engineSwitch.reset(); },
  };
}
