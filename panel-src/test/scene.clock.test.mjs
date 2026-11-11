import test from "node:test";
import assert from "node:assert/strict";
import { WINDOW_START, WINDOW_END, MAX_GAP_MS, nightT, inWindow, horaOf, crossings, parseClock, createClock } from "../src/scene/clock.js";

test("nightT counts hours since 18:00 on a 24 h ring", () => {
  assert.equal(nightT(18), 0);
  assert.equal(nightT(9), 15);
  assert.equal(nightT(0), 6);
  assert.ok(Math.abs(nightT(17.99) - 23.99) < 1e-9);
  assert.equal(nightT(24), 6);
  assert.equal(nightT(-1), 5);
});

test("the bot window is 18:00 to 09:00", () => {
  assert.equal(WINDOW_START, 18);
  assert.equal(WINDOW_END, 9);
  for (const h of [18, 22, 0, 0.43, 5.5, 8.99]) assert.equal(inWindow(h), true, String(h));
  for (const h of [9, 12, 17.99]) assert.equal(inWindow(h), false, String(h));
});

test("horaOf reads the server zone, not the browser's", () => {
  assert.equal(horaOf(Date.UTC(2026, 9, 8, 12, 0), "America/Santiago"), 9); // UTC-3 in October
  assert.equal(horaOf(Date.UTC(2026, 9, 8, 21, 30), "America/Santiago"), 18.5);
  assert.equal(horaOf(Date.UTC(2026, 9, 8, 3, 15), "America/Santiago"), 0.25);
});

test("crossings: forward only, 09:00 and 18:00, once; midnight is no event; a gap over 6 h crosses nothing", () => {
  assert.deepEqual(crossings(8.99, 9.02), [9]);
  assert.deepEqual(crossings(17.9, 18.1), [18]);
  assert.deepEqual(crossings(8, 9), [9]);          // arriving exactly on the mark counts
  assert.deepEqual(crossings(9, 9.5), []);         // already past it
  assert.deepEqual(crossings(6, 11.9), [9]);       // a 5.9 h gap is still a walk, not a sleep
  assert.deepEqual(crossings(6, 13), []);          // 7 h: the laptop slept, the sky just jumps
  assert.deepEqual(crossings(23.9, 0.1), []);      // midnight is not a mark
  assert.deepEqual(crossings(10, 9), []);          // backwards (clock change) never crosses: the gap would be 23 h
  assert.deepEqual(crossings(8.5, 18.5), []);      // 10 h
  assert.deepEqual(crossings(8, 19, [9, 18]), []); // 11 h
  assert.deepEqual(crossings(14, 20, [9, 18]), [18]);
  assert.deepEqual(crossings(5, 9.5, [9, 5.5]), [5.5, 9]); // ordered by how soon they come
});

test("parseClock accepts hours, decimals, comma decimals and hh:mm, and refuses nonsense", () => {
  assert.equal(parseClock("?clock=9"), 9);
  assert.equal(parseClock("?x=1&clock=0.43"), 0.43);
  assert.equal(parseClock("?clock=18"), 18);
  assert.equal(parseClock("?clock=22"), 22);
  assert.equal(parseClock("?clock=9%3A30"), 9.5);
  assert.equal(parseClock("?clock=9,5"), 9.5);
  for (const bad of ["", "?clock=", "?clock=24", "?clock=-1", "?clock=abc", "?clock=%E0%A4%A", null, undefined]) assert.equal(parseClock(bad), null, String(bad));
});

function harness(startUtc, extra = {}) {
  let ms = startUtc;
  const calls = { minute: [], cross: [] };
  const ticks = [];
  const timers = { setInterval: (f, every) => { ticks.push(every); return 7; }, clearInterval: (id) => ticks.push("clear" + id) };
  const clock = createClock({ now: () => ms, tz: "America/Santiago", timers, onMinute: (h) => calls.minute.push(h), onCross: (m) => calls.cross.push(m), ...extra });
  return { clock, calls, ticks, advance: (n) => { ms += n; return clock.tick(); } };
}
const AT_0859 = Date.UTC(2026, 9, 8, 11, 59); // 08:59 in Santiago

test("a clock announces 09:00 once, when the page is open as it passes", () => {
  const { clock, calls, advance } = harness(AT_0859);
  assert.equal(clock.hora(), 8 + 59 / 60);
  advance(30000); assert.deepEqual(calls.cross, []);
  advance(31000); assert.deepEqual(calls.cross, [9]);
  advance(60000); advance(60000); assert.deepEqual(calls.cross, [9]);
  assert.equal(calls.minute.length, 3, "one onMinute per changed minute: 09:00, 09:01, 09:02");
});

test("after a sleep the sky jumps to the real hour and nothing is announced", () => {
  const { clock, calls, advance } = harness(AT_0859);
  advance(3 * 3600000 + MAX_GAP_MS); // slept three hours across 09:00: it is 12:04 now
  assert.deepEqual(calls.cross, []);
  assert.ok(Math.abs(clock.hora() - (12 + 4 / 60)) < 1e-9);
  assert.equal(calls.minute.length, 1);
});

test("a throttled background tab (one tick a minute) still announces the mark it crosses", () => {
  const { calls, advance } = harness(AT_0859);
  advance(60000); advance(60000);
  assert.deepEqual(calls.cross, [9]);
});

test("midnight passes without an event and without a negative hour", () => {
  const { clock, calls, advance } = harness(Date.UTC(2026, 9, 9, 2, 59, 30)); // 23:59:30
  advance(60000);
  assert.deepEqual(calls.cross, []);
  assert.ok(clock.hora() >= 0 && clock.hora() < 24);
});

test("an override (?clock=) fixes the hour, emits no crossings and no minute events", () => {
  const { clock, calls, advance } = harness(AT_0859, { override: 18 });
  assert.equal(clock.hora(), 18);
  advance(3600000); advance(3600000);
  assert.equal(clock.hora(), 18);
  assert.deepEqual(calls, { minute: [], cross: [] });
});

test("start and stop use the injected timers once", () => {
  const { clock, ticks } = harness(AT_0859);
  clock.start(); clock.start();
  clock.stop(); clock.stop();
  assert.deepEqual(ticks, [15000, "clear7"]);
});
