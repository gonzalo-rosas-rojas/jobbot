import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { useStorage } from "../src/core/store.js";
import { setLang, setTimeZone } from "../src/i18n/index.js";
import { holdVisit, sinceLastVisit, rememberVisit, lastVisit } from "../src/core/diff.js";

const rich = () => JSON.parse(readFileSync(new URL("./fixtures/snapshot.rich.json", import.meta.url), "utf8"));
const NOW = Date.parse("2026-10-07T15:00:00Z");
const HOUR = 3600000;
const shelf = () => { const m = new Map(); return { getItem: (k) => (m.has(k) ? m.get(k) : null), setItem: (k, v) => m.set(k, String(v)), removeItem: (k) => m.delete(k) }; };

test("a page hidden before its first snapshot does not erase the news of the time away", () => {
  useStorage({ local: shelf(), session: shelf() });
  setTimeZone("UTC");
  setLang("es");
  rememberVisit(NOW - 6 * HOUR);          // the visit before this one
  assert.equal(holdVisit(), NOW - 6 * HOUR, "boot holds it");
  rememberVisit(NOW);                     // the page is hidden (or unloads) before the first snapshot arrives
  assert.equal(lastVisit(), NOW, "the new visit is saved for the next time");
  assert.equal(holdVisit(), NOW - 6 * HOUR, "holding again keeps the first one");
  const news = sinceLastVisit(rich());
  assert.deepEqual(news.lines, ["Desde tu última visita: 3 novedades"], "the same three the 6 h before this page ask for");
  assert.equal(sinceLastVisit(rich(), 0).lines.length, 0, "an explicit moment still wins");
});
