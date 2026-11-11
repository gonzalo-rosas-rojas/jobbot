// The background bus: what happens WITHOUT him, found by comparing one snapshot with the next, and said with ONE quiet sound.
//
//   a run of the bot ended        controls.last_run_at changed: bg.run when it ended well, bg.runfail when last_run_result says "error: ..."
//   new proposals to approve      awaiting_approval has a card it did not have
//   a company answered            an email that was not there (any kind), or an application that moved to replied or rejected
//   an interview, a test, an offer an interview_invite or technical_test email, or an application that moved to interview, technical_test or offer
//   STOP changed from outside     controls.stopped flipped without a change of his own in the last 90 s
//   the gate of 18:00 is near     17:30 passes while the page is open and proposals are waiting
//   a server error                a read failed after the page had data
//
// Rules, all tested in test/sound.background.test.mjs:
//   - NEVER for the first snapshot (nor for the page opening): that one is the baseline, and the sound is only for changes after it. A snapshot that
//     is more than 20 minutes after the previous one (a laptop that slept, a tab that was hidden for hours) is a baseline too: the news lines say it.
//   - what he did himself is not news: when a cue that changes data (cue.mutates) was asked for in the last 90 s, the changes he can cause (new
//     proposals, answers, STOP) are not sounded. A run of the bot and the gate are not his.
//   - a burst is one sound: all the events of one snapshot become the most important of them. The player then allows one background sound a
//     minute (an error excepted): a second event inside that minute is dropped, not postponed (a sound that comes late is a sound he cannot place).
//   - an error sounds at most once in five minutes, never before the first snapshot, and never for a wrong key (that one is a banner for good).
// Nothing here knows about audio: it asks the player (sound/player.js) for a cue, and the player says whether it may sound.

import { mailId } from "../core/diff.js";
import { crossings, horaOf } from "../scene/clock.js";
import { serverDay } from "../core/time.js";

export const STALE_MS = 20 * 60 * 1000;
const ERROR_GAP_MS = 5 * 60 * 1000;
const GATE_HOUR = 17.5;                          // the gate opens at 18:00; the reminder is half an hour before
const MAX_TICK_GAP_MS = 5 * 60 * 1000;           // beyond this between two looks at the clock the machine slept: nothing crossed
// Most important first: the one sound a burst gets.
export const PRIORITY = Object.freeze(["bg.interview", "bg.reply", "bg.runfail", "bg.stop", "bg.go", "bg.queue", "bg.run"]);
// What he can cause himself: sounded only when he did not just change something.
const HIS = new Set(["bg.interview", "bg.reply", "bg.stop", "bg.go", "bg.queue"]);
const MILESTONE_MAIL = new Set(["interview_invite", "technical_test"]);
const MILESTONE_STATE = new Set(["interview", "technical_test", "offer"]);
const ANSWER_STATE = new Set(["replied", "rejected"]);

const list = (x) => (Array.isArray(x) ? x : []);
const statesOf = (snap) => {
  const out = new Map();
  for (const col of list(snap && snap.board)) for (const c of list(col && col.cards)) out.set(c.key, c.state);
  return out;
};
const awaitingKeys = (snap) => new Set(list(snap && snap.awaiting_approval && snap.awaiting_approval.cards).map((c) => c.key));
const totalAwaiting = (snap) => Number(snap && snap.awaiting_approval && snap.awaiting_approval.total) || 0;

/** The cue ids of what changed between two raw snapshots, in the order of PRIORITY. Pure: it changes neither snapshot. */
export function detect(prev, next) {
  const found = new Set();
  if (!prev || !next) return [];

  const seen = new Set(list(prev.today && prev.today.emails).map(mailId));
  for (const mail of list(next.today && next.today.emails)) {
    if (seen.has(mailId(mail))) continue;
    found.add(MILESTONE_MAIL.has(mail.kind) ? "bg.interview" : "bg.reply");
  }
  const was = statesOf(prev);
  for (const [key, state] of statesOf(next)) {
    const before = was.get(key);
    if (!before || before === state) continue;
    if (MILESTONE_STATE.has(state)) found.add("bg.interview");
    else if (ANSWER_STATE.has(state)) found.add("bg.reply");
  }

  const had = awaitingKeys(prev);
  if ([...awaitingKeys(next)].some((key) => !had.has(key)) || totalAwaiting(next) > totalAwaiting(prev)) found.add("bg.queue");

  const a = prev.controls || {};
  const b = next.controls || {};
  if (b.last_run_at && b.last_run_at !== a.last_run_at) {
    const newer = !(Date.parse(a.last_run_at) > Date.parse(b.last_run_at));          // a run that is older than the one we knew is not a run
    const result = typeof b.last_run_result === "string" ? b.last_run_result.trim() : "";
    if (newer) {
      if (/^error/i.test(result)) found.add("bg.runfail");
      else if (result === "" || result === "ok") found.add("bg.run");
    }
  }
  if (a.stopped === false && b.stopped === true) found.add("bg.stop");
  if (a.stopped === true && b.stopped === false) found.add("bg.go");

  return PRIORITY.filter((id) => found.has(id));
}

/**
 * createBackground({ player, now, hora, serverDayOf }) -> { observe(snapshot), failed(error), tick(), reset() }
 *   observe  every snapshot the session accepts (the raw one)       -> the cue id it asked the player for, or null
 *   failed   every failed read, with the session's error            -> the same
 *   tick     every few seconds: the clock, for the gate of 18:00     -> the same
 * The player says whether the cue sounded (player.history()); what these return is what was asked.
 */
export function createBackground({ player, now = Date.now, hora = (ms) => horaOf(ms), dayOf = (ms) => serverDay(ms) }) {
  let last = null;                // the previous raw snapshot: the baseline of the next comparison
  let lastError = -Infinity;
  let lastHora = null;
  let lastTick = -Infinity;
  let gateDay = null;

  function ask(id) {
    if (!id) return null;
    player.play(id);
    return id;
  }

  function observe(snapshot) {
    const prev = last;
    last = snapshot;
    if (!prev || !snapshot) return null;                                             // the first snapshot is the baseline, never a sound
    const gap = Date.parse(snapshot.generated_at) - Date.parse(prev.generated_at);
    if (Number.isFinite(gap) && gap > STALE_MS) return null;
    const mine = player.ownRecently();
    const events = detect(prev, snapshot).filter((id) => !(mine && HIS.has(id)));
    return ask(events[0] || null);
  }

  function failed(error) {
    if (!last || (error && error.fatal)) return null;                               // before the first snapshot it is the page opening; a wrong key is a banner
    const t = now();
    if (t - lastError < ERROR_GAP_MS) return null;
    lastError = t;
    return ask("bg.error");
  }

  function tick() {
    const t = now();
    const h = hora(t);
    const before = lastHora;
    const elapsed = t - lastTick;
    lastHora = h;
    lastTick = t;
    if (before == null || elapsed < 0 || elapsed > MAX_TICK_GAP_MS) return null;
    if (crossings(before, h, [GATE_HOUR]).length === 0) return null;
    const day = dayOf(t);
    if (gateDay === day || totalAwaiting(last) <= 0) return null;
    gateDay = day;
    return ask("bg.gate");
  }

  return { observe, failed, tick, reset() { last = null; } };
}
