// The background bus (src/sound/background.js): what the comparison of two snapshots turns into one quiet sound, and everything that keeps it from
// being a nuisance: never at load, never for his own doing, one sound for a burst, and the errors and the gate of 18:00. Node only; the player is a
// recorder and the data are the fixtures with invented companies.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { detect, createBackground, PRIORITY, STALE_MS } from "../src/sound/background.js";
import { createPlayer, OWN_MS } from "../src/sound/player.js";
import { createPrefs } from "../src/sound/prefs.js";

const rich = JSON.parse(readFileSync(new URL("./fixtures/snapshot.rich.json", import.meta.url), "utf8"));
const MIN = 60000;
const clone = (x) => JSON.parse(JSON.stringify(x));
/** `change(snapshot)` edits a copy of the fixture; generated_at moves forward by `minutes`. */
const later = (base, change = () => {}, minutes = 0.5) => {
  const next = clone(base);
  next.generated_at = new Date(Date.parse(base.generated_at) + minutes * MIN).toISOString();
  change(next);
  return next;
};
const mail = (kind, subject = "Invented subject") => ({ received_at: "2026-10-07T16:00:00Z", from: "x@example.invalid", company: "Nu Test Co", job_key: null, kind, subject, summary: "-" });
const cardsOf = (snap) => snap.board.flatMap((c) => c.cards);

function rig() {
  const asked = [];
  const clock = { ms: Date.UTC(2026, 9, 9, 23, 0, 0), own: false, hour: 20 };
  const player = { play: (id) => { asked.push(id); return true; }, ownRecently: () => clock.own };
  const bg = createBackground({ player, now: () => clock.ms, hora: () => clock.hour, dayOf: (ms) => new Date(ms).toISOString().slice(0, 10) });
  return { asked, clock, bg };
}

test("the first snapshot is a baseline: it never sounds, and neither does the same data again", () => {
  const r = rig();
  assert.equal(r.bg.observe(rich), null);
  assert.equal(r.bg.observe(clone(rich)), null);
  assert.equal(r.bg.observe(later(rich)), null);
  assert.deepEqual(r.asked, []);
});

test("an email that was not there is a company answering; an invitation or a test is the milestone", () => {
  const r = rig();
  r.bg.observe(rich);
  assert.equal(r.bg.observe(later(rich, (s) => s.today.emails.unshift(mail("recruiter")))), "bg.reply");
  const a = later(rich, (s) => s.today.emails.unshift(mail("recruiter")));
  assert.equal(r.bg.observe(later(a, (s) => s.today.emails.unshift(mail("interview_invite", "Another subject")))), "bg.interview");
  const b = later(a, (s) => s.today.emails.unshift(mail("interview_invite", "Another subject")));
  assert.equal(r.bg.observe(later(b, (s) => s.today.emails.unshift(mail("technical_test", "A third one")))), "bg.interview");
  for (const kind of ["ack", "rejection", "action_required", "other"]) {
    const x = rig();
    x.bg.observe(rich);
    assert.equal(x.bg.observe(later(rich, (s) => s.today.emails.unshift(mail(kind)))), "bg.reply", kind);
  }
  assert.deepEqual(r.asked, ["bg.reply", "bg.interview", "bg.interview"]);
});

test("an application that moves to replied or rejected is an answer; to interview, technical test or offer, the milestone; a new card is not either", () => {
  const states = cardsOf(rich).map((c) => c.state);
  assert.ok(states.includes("submitted"), "the fixture has a sent application");
  const move = (to) => (s) => { cardsOf(s).find((c) => c.state === "submitted").state = to; };
  for (const [to, id] of [["replied", "bg.reply"], ["rejected", "bg.reply"], ["interview", "bg.interview"], ["technical_test", "bg.interview"], ["offer", "bg.interview"]]) {
    const r = rig();
    r.bg.observe(rich);
    assert.equal(r.bg.observe(later(rich, move(to))), id, to);
  }
  const r = rig();
  r.bg.observe(rich);
  assert.equal(r.bg.observe(later(rich, (s) => { s.board.find((c) => c.state === "scored").cards.push({ ...cardsOf(s)[0], key: "fx:new", state: "scored" }); })), null, "a posting that appears is not a company answering");
  assert.equal(r.bg.observe(later(rich, (s) => { cardsOf(s).find((c) => c.state === "submitted").state = "ghosted"; })), null, "going quiet is not an answer");
});

test("a proposal that was not waiting is new to approve: by its key, or by the total growing", () => {
  const r = rig();
  r.bg.observe(rich);
  const extra = { ...clone(rich.awaiting_approval.cards[0]), key: "fx:brand-new" };
  assert.equal(r.bg.observe(later(rich, (s) => { s.awaiting_approval.cards.push(extra); s.awaiting_approval.total += 1; })), "bg.queue");
  const r2 = rig();
  r2.bg.observe(rich);
  assert.equal(r2.bg.observe(later(rich, (s) => { s.awaiting_approval.total += 2; })), "bg.queue", "the server counts more than it lists");
  const r3 = rig();
  r3.bg.observe(later(rich, (s) => { s.awaiting_approval.cards.push(extra); s.awaiting_approval.total += 1; }));
  assert.equal(r3.bg.observe(later(rich)), null, "one fewer is not news");
});

test("a run of the bot: ended well, ended with an error, and the runs that did nothing", () => {
  const run = (change) => {
    const r = rig();
    r.bg.observe(rich);
    return r.bg.observe(later(rich, (s) => { s.controls.last_run_at = "2026-10-09T08:00:00Z"; change(s.controls); }));
  };
  assert.equal(run(() => {}), "bg.run", "no result in the snapshot (the server that has not learned to say it): a new time is a run");
  assert.equal(run((c) => { c.last_run_result = "ok"; }), "bg.run");
  assert.equal(run((c) => { c.last_run_result = "error: the engine could not open the page"; }), "bg.runfail");
  assert.equal(run((c) => { c.last_run_result = "stopped"; }), null, "STOP held it back: it did nothing");
  assert.equal(run((c) => { c.last_run_result = "window_closed"; }), null);
  const r = rig();
  r.bg.observe(rich);
  assert.equal(r.bg.observe(later(rich, (s) => { s.controls.last_run_at = "2026-10-01T08:00:00Z"; })), null, "an older time than the one we knew is not a run");
  assert.equal(r.bg.observe(later(rich, (s) => { s.controls.last_run_at = null; })), null);
});

test("STOP changed from outside: set and lifted", () => {
  const flip = (from, to) => {
    const r = rig();
    r.bg.observe(later(rich, (s) => { s.controls.stopped = from; }));
    return r.bg.observe(later(rich, (s) => { s.controls.stopped = to; }));
  };
  assert.equal(flip(false, true), "bg.stop");
  assert.equal(flip(true, false), "bg.go");
  assert.equal(flip(false, false), null);
  assert.equal(flip(undefined, true), null, "a snapshot that does not say is not a change");
});

test("a burst is one sound, the most important of what changed; the order is the priority", () => {
  const all = later(rich, (s) => {
    s.today.emails.unshift(mail("recruiter"));
    s.awaiting_approval.total += 1;
    s.controls.last_run_at = "2026-10-09T08:00:00Z";
    s.controls.stopped = true;
  });
  assert.deepEqual(detect(rich, all), ["bg.reply", "bg.stop", "bg.queue", "bg.run"]);
  const r = rig();
  r.bg.observe(rich);
  assert.equal(r.bg.observe(all), "bg.reply");
  assert.deepEqual(r.asked, ["bg.reply"], "one request to the player, not four");
  assert.deepEqual(detect(rich, later(rich, (s) => { s.today.emails.unshift(mail("interview_invite")); s.today.emails.unshift(mail("ack", "x")); })), ["bg.interview", "bg.reply"]);
  assert.deepEqual([...PRIORITY], ["bg.interview", "bg.reply", "bg.runfail", "bg.stop", "bg.go", "bg.queue", "bg.run"]);
  assert.equal(detect(null, rich).length + detect(rich, null).length, 0);
});

test("detect changes neither snapshot", () => {
  const a = clone(rich);
  const b = later(rich, (s) => { s.today.emails.unshift(mail("recruiter")); });
  const before = JSON.stringify([a, b]);
  detect(a, b);
  assert.equal(JSON.stringify([a, b]), before);
});

test("what he did himself is not news: the changes he can cause are silent for 90 seconds after his own action, a run and the gate are not his", () => {
  const r = rig();
  r.bg.observe(rich);
  r.clock.own = true;
  const next = later(rich, (s) => { s.today.emails.unshift(mail("recruiter")); s.awaiting_approval.total += 1; s.controls.stopped = true; });
  assert.equal(r.bg.observe(next), null);
  assert.equal(r.bg.observe(later(next, (s) => { s.controls.last_run_at = "2026-10-09T09:00:00Z"; })), "bg.run", "a run of the bot is nobody's doing but its own");
  r.clock.own = false;
  assert.equal(r.bg.observe(later(next, (s) => { s.controls.last_run_at = "2026-10-09T09:00:00Z"; s.controls.stopped = false; }, 2)), "bg.go");
  assert.equal(OWN_MS, 90000);
});

test("a snapshot far after the previous one is a new baseline: a laptop that slept does not ring for what it missed", () => {
  const r = rig();
  r.bg.observe(rich);
  assert.equal(r.bg.observe(later(rich, (s) => s.today.emails.unshift(mail("recruiter")), STALE_MS / MIN + 1)), null);
  const x = rig();
  x.bg.observe(rich);
  assert.equal(x.bg.observe(later(rich, (s) => s.today.emails.unshift(mail("recruiter")), STALE_MS / MIN - 1)), "bg.reply");
  // and the one after the baseline compares with it
  assert.equal(r.bg.observe(later(rich, (s) => s.today.emails.unshift(mail("recruiter")), STALE_MS / MIN + 2)), null, "same data as the baseline");
});

test("a failed read sounds once in five minutes, never before the first snapshot and never for a wrong key", () => {
  const r = rig();
  assert.equal(r.bg.failed({ kind: "network", fatal: false }), null, "the page was just opened and the server is down: that is a banner, not a sound");
  r.bg.observe(rich);
  assert.equal(r.bg.failed({ kind: "token", fatal: true }), null);
  assert.equal(r.bg.failed({ kind: "network", fatal: false }), "bg.error");
  r.clock.ms += 4 * MIN;
  assert.equal(r.bg.failed({ kind: "timeout", fatal: false }), null, "the backoff retries, the sound does not");
  r.clock.ms += 1 * MIN + 1;
  assert.equal(r.bg.failed({ kind: "server", fatal: false }), "bg.error");
  assert.deepEqual(r.asked, ["bg.error", "bg.error"]);
});

test("the gate of 18:00: 17:30 passes with the page open and proposals waiting, once a day; never at load, never after a sleep", () => {
  const r = rig();
  r.bg.observe(rich);
  assert.ok(rich.awaiting_approval.total > 0);
  const at = (hour, minutes = 0.33) => { r.clock.hour = hour; r.clock.ms += minutes * MIN; return r.bg.tick(); };
  assert.equal(at(17.4), null, "the first look at the clock only learns the hour: opening the page at 17:24 does not ring");
  assert.equal(at(17.45), null);
  assert.equal(at(17.49), null);
  assert.equal(at(17.51), "bg.gate", "17:30 went by");
  assert.equal(at(17.6), null);
  assert.equal(at(17.9), null);
  assert.deepEqual(r.asked, ["bg.gate"]);
  r.clock.ms += 24 * 60 * MIN;                                     // tomorrow
  assert.equal(at(17.49), null);
  assert.equal(at(17.51), "bg.gate", "and again the next day");

  const slept = rig();
  slept.bg.observe(rich);
  slept.clock.hour = 17.4;
  slept.bg.tick();
  slept.clock.hour = 17.6;
  slept.clock.ms += 6 * MIN;
  assert.equal(slept.bg.tick(), null, "six minutes between two looks: the machine slept, nothing was crossed in front of him");

  const empty = rig();
  empty.bg.observe(later(rich, (s) => { s.awaiting_approval.total = 0; s.awaiting_approval.cards = []; }));
  empty.clock.hour = 17.49;
  empty.bg.tick();
  empty.clock.hour = 17.51;
  empty.clock.ms += 20000;
  assert.equal(empty.bg.tick(), null, "nothing to approve, nothing to remind");
  assert.equal(rig().bg.tick(), null, "no snapshot yet");
});

test("with the real player: the first snapshot makes no sound and a later change makes one, and only on a free minute", () => {
  const calls = [];
  const engine = { enabled: true, volume: 0.25, ctx: {}, names: [], setEnabled() {}, setVolume() {}, setBusy() {}, tap() {}, recent: () => [], preview: () => true, play: (n, o) => { calls.push([n, o.gain]); return true; } };
  const box = new Map();
  const prefs = createPrefs({ store: { get: (k) => box.get(k) ?? null, set: (k, v) => box.set(k, v) } });
  const clock = { ms: Date.UTC(2026, 9, 9, 23, 0, 0) };
  const player = createPlayer({ engine, prefs, now: () => clock.ms, hora: () => 21, hidden: () => false });
  const bg = createBackground({ player, now: () => clock.ms, hora: () => 21 });
  bg.observe(rich);
  assert.deepEqual(calls, []);
  bg.observe(later(rich, (s) => s.today.emails.unshift(mail("recruiter"))));
  assert.deepEqual(calls, [["chord", 0.6]]);
  const second = later(rich, (s) => { s.today.emails.unshift(mail("recruiter")); s.awaiting_approval.total += 1; }, 1);
  bg.observe(second);
  assert.equal(calls.length, 1, "the queue arrived 20 s after the answer: dropped, not postponed");
  clock.ms += 2 * MIN;
  bg.observe(later(second, (s) => { s.awaiting_approval.total += 1; }, 1));
  assert.deepEqual(calls.map((c) => c[0]), ["chord", "turn"]);
});
