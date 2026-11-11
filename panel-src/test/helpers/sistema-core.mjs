// test/helpers/sistema-core.mjs: the snapshots SISTEMA is tested and photographed on, built from the three fixtures that are handed in (no file is
// read here, so the browser lab can use it too). They set what the Rust writes (crates/jobbot-panel/src/snapshot.rs, src/cli.rs `StepLog`) and the fixtures do
// not carry: `controls.last_run_result` and `controls.last_run_steps`, the three values of `engine`, a scheduled task that is null / absent /
// registered, an account that is ok or is not.
//
//   const SCENARIOS = scenariosOf({ rich, empty, heavy });     SCENARIOS.night() ...
//   night   rich, moved to 03:00 in Santiago: ran 56 minutes ago, ok, every step ran, the task runs at 03:04
//   stale   41 hours without a run, no scheduled task          never   the empty fixture: never ran, no account, no task, Windows never asked
//   stop    night, with STOP on                                 heavy   the heavy fixture moved to the same night, eight forms stopped in the last hours
//   day     rich at noon: the night that counts is the one that ended
// `rebase(snap, iso)` moves every timestamp of a snapshot so that `generated_at` is `iso`, and says again which half hours are inside the run
// window (the fixtures' `open` flags are right only for their own hour).

export const clone = (x) => JSON.parse(JSON.stringify(x));
export const HOUR = 3600000;
export const MIN = 60000;

const ISO = /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d+)?(Z|[+-]\d\d:\d\d)$/;
const stamp = (ms) => new Date(ms).toISOString().replace(".000Z", "Z");

function shift(value, delta) {
  if (typeof value === "string") return ISO.test(value) ? stamp(Date.parse(value) + delta) : value;
  if (Array.isArray(value)) return value.map((v) => shift(v, delta));
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, shift(v, delta)]));
  return value;
}

const santiagoHour = (ms) => Number(new Intl.DateTimeFormat("en-GB", { timeZone: "America/Santiago", hour: "2-digit", hourCycle: "h23" }).format(new Date(ms)));

/** Every timestamp of `snap` moved so that `generated_at` is `iso`; the run window (18:00 to 09:00 in Santiago) is worked out again per half hour. */
export function rebase(snap, iso) {
  const out = shift(snap, Date.parse(iso) - Date.parse(snap.generated_at));
  for (const slot of out.activity.slots) {
    const h = santiagoHour(Date.parse(slot.at));
    slot.open = h >= 18 || h < 9;
  }
  return out;
}

const at = (snap, ms) => stamp(Date.parse(snap.generated_at) + ms);

/** The record of a run that did everything (what `run-daily` writes when each step was due). */
export const STEPS_OK = () => ({
  discover: { ran: true, new: 3 },
  prepare: { ran: true, prepared: 2 },
  submit: { ran: true, submitted: 1, needs_gonza: 1 },
  dossiers: { ran: true, written: 2 },
});

/** The server's own words for each way a step does not run (`StepLog::skipped`, `RunEnd::skip_reason`). */
export const SKIPS = ["cooldown", "stop", "window_closed", "after_error", "error", "not_due"];
export const skipped = (why) => ({ ran: false, skipped: why });

/** `result` and `steps` take `undefined` to mean "the server wrote none" (the keys are left out), so they are read with `in`, not with defaults. */
export function withRun(snap, opts = {}) {
  const { agoMs = 56 * MIN, engine = "recent" } = opts;
  const s = clone(snap);
  s.controls.last_run_at = at(s, -agoMs);
  s.controls.engine = engine;
  const result = "result" in opts ? opts.result : "ok";
  const steps = "steps" in opts ? opts.steps : STEPS_OK();
  if (result === undefined) delete s.controls.last_run_result;
  else s.controls.last_run_result = result;
  if (steps === undefined) delete s.controls.last_run_steps;
  else s.controls.last_run_steps = steps;
  return s;
}

export function scenariosOf({ rich, empty, heavy }) {
  const SCENARIOS = {
    night: () => {
      const s = withRun(rebase(rich, "2026-10-08T06:00:00Z"));
      s.controls.window_open = true;
      s.controls.next_window = null;
      s.controls.scheduled_task = { exists: true, next_run_at: stamp(Date.parse(s.controls.last_run_at) + HOUR) };   // the task fires every hour
      s.controls.claude_account.checked_at = at(s, -9 * HOUR);
      return s;
    },
    stale: () => {
      const s = withRun(rebase(rich, "2026-10-08T06:00:00Z"), { agoMs: 41 * HOUR, result: "ok", engine: "stale" });
      s.controls.scheduled_task = { exists: false, next_run_at: null };
      return s;
    },
    never: () => rebase(empty, "2026-10-08T06:00:00Z"),
    stop: () => {
      const s = SCENARIOS.night();
      s.controls.stopped = true;
      return s;
    },
    heavy: () => {
      const s = withRun(rebase(heavy, "2026-10-08T06:00:00Z"));
      s.controls.window_open = true;
      s.controls.next_window = null;
      // eight forms that stopped for him in the last hours (the fixture dates them a day or more back)
      s.needs_gonza.cards.forEach((c, i) => { c.waiting_since = at(s, -(i + 1) * 45 * MIN); c.updated_at = c.waiting_since; });
      return s;
    },
    day: () => {
      const s = withRun(rebase(rich, "2026-10-08T15:00:00Z"), { agoMs: 7 * HOUR });
      s.controls.window_open = false;
      s.controls.next_window = "2026-10-08 18:00";
      return s;
    },
  };
  return SCENARIOS;
}
