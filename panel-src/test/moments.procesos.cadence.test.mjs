import test from "node:test";
import assert from "node:assert/strict";
import { CADENCE, cadenceOf, dayOf } from "../src/moments/procesos/cadence.js";

const card = (state, days) => ({ key: "k", state, days_in_state: days });

test("the constants are the rule of the spec: follow up at 8-10 and 15-17 days, give up at 45, the median answer is 6-7 days", () => {
  assert.deepEqual([CADENCE.ask, CADENCE.insist, CADENCE.close, CADENCE.median, CADENCE.decide], [[8, 10], [15, 17], 45, [6, 7], 3]);
  assert.ok(Object.isFrozen(CADENCE), "a table of constants nobody changes by accident");
});

test("a sent application walks the rule day by day: wait, ask (1st follow-up), wait, insist (2nd), wait for the end, give up", () => {
  const at = (day) => cadenceOf(card("submitted", day));
  assert.deepEqual([at(0).kind, at(0).step, at(0).daysTo], ["soon", 1, 8], "day 0: the first follow-up is 8 days away");
  assert.deepEqual([at(7).kind, at(7).daysTo], ["soon", 1]);
  for (const day of [8, 9, 10]) assert.deepEqual([at(day).kind, at(day).step, at(day).inWindow, at(day).daysTo], ["ask", 1, true, 0], `day ${day}`);
  assert.deepEqual([at(11).kind, at(11).step, at(11).daysTo], ["soon", 2, 4]);
  assert.deepEqual([at(14).kind, at(14).daysTo], ["soon", 1]);
  for (const day of [15, 16, 17]) assert.deepEqual([at(day).kind, at(day).step, at(day).inWindow], ["insist", 2, true], `day ${day}`);
  assert.deepEqual([at(18).kind, at(18).daysTo], ["closing", 27]);
  assert.deepEqual([at(44).kind, at(44).daysTo], ["closing", 1]);
  assert.deepEqual([at(45).kind, at(45).daysTo], ["lost", 0]);
  assert.equal(at(300).kind, "lost");
  assert.deepEqual(at(9).window, [8, 10]);
  assert.deepEqual(at(16).window, [15, 17]);
});

test("a company that answered is looked at on day 3 when nobody has a task on it; the other stages say where they are", () => {
  assert.equal(cadenceOf(card("replied", 0)).kind, "reply");
  assert.equal(cadenceOf(card("replied", 2)).kind, "reply");
  assert.equal(cadenceOf(card("replied", 3)).kind, "decide");
  assert.equal(cadenceOf(card("replied", 9), { hasTask: true }).kind, "task", "a task on it settles the question");
  assert.equal(cadenceOf(card("technical_test", 1)).kind, "test");
  assert.equal(cadenceOf(card("interview", 4)).kind, "interview");
  assert.equal(cadenceOf(card("offer", 0)).kind, "offer");
  assert.deepEqual([cadenceOf(card("ghosted", 20)).kind, cadenceOf(card("ghosted", 20)).daysTo], ["quiet", 25]);
  assert.equal(cadenceOf(card("ghosted", 60)).kind, "lost");
  for (const state of ["scored", "prepared", "needs_gonza"]) assert.equal(cadenceOf(card(state, 2)).kind, "pre", state);
});

test("a day that is missing, negative, fractional or not a number is a whole day of at least 0 (a broken snapshot never prints NaN)", () => {
  assert.equal(dayOf({ days_in_state: null }), 0);
  assert.equal(dayOf({}), 0);
  assert.equal(dayOf({ days_in_state: -3 }), 0);
  assert.equal(dayOf({ days_in_state: 9.7 }), 9);
  assert.equal(dayOf({ days_in_state: "12" }), 12);
  assert.equal(dayOf({ days_in_state: "abc" }), 0);
  for (const bad of [null, undefined, NaN, "x", -1]) {
    const c = cadenceOf({ state: "submitted", days_in_state: bad });
    assert.ok(Number.isInteger(c.day) && c.day === 0, String(bad));
  }
  assert.equal(cadenceOf({ state: "mystery", days_in_state: 5 }).kind, "pre", "a state the page does not know is still something");
});
