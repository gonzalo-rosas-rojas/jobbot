// axisHours marks the hours of its own zone (a half-hour offset shows it; Chile and UTC cannot); routeOfHash opens every room of ROOM_IDS and the old #learn.
import test from "node:test";
import assert from "node:assert/strict";
import { setTimeZone, setLang } from "../src/i18n/index.js";
import { axisHours } from "../src/core/time.js";
import { ROOM_IDS, routeOfHash } from "../src/core/route.js";

const slotsFrom = (startIso, count) => Array.from({ length: count }, (_, i) => ({ at: new Date(Date.parse(startIso) + i * 1800000).toISOString() }));

test("axisHours: in a zone with a half-hour offset the marks fall on the hour of that zone", () => {
  setLang("es");
  setTimeZone("Asia/Kolkata");                         // UTC+5:30: 09:30 UTC is 15:00 there
  try {
    const marks = axisHours(slotsFrom("2026-10-06T09:30:00Z", 48), 200);
    assert.ok(marks.length > 0);
    assert.ok(marks.every((m) => m.hour % 3 === 0), "every mark is a multiple of three on the local clock: " + JSON.stringify(marks));
    assert.deepEqual([marks[0].hour, marks[0].percent], [18, 12.5], "slot 6 (18:00 there) is the first mark; 15:00 is the start and is left out");
  } finally {
    setTimeZone("UTC");
  }
});

test("routeOfHash opens every room of ROOM_IDS and the old #learn", () => {
  for (const id of ROOM_IDS) assert.deepEqual(routeOfHash("#" + id), { room: id, job: null }, id);
  assert.deepEqual(routeOfHash("#procesos/a%3Ab"), { room: "procesos", job: "a:b" });
  assert.deepEqual(routeOfHash("#learn"), { room: "aprender", job: null });
  assert.deepEqual(routeOfHash("#prologo"), { room: "prologo", job: null });
  assert.equal(routeOfHash("#nowhere"), null);
  assert.equal(routeOfHash("#hoyx"), null);
});
