// Acceptance test of the encargo docs/encargos/lote-reloj-cajones-y-util.md: small fixes of the panel in one run. (1) The demo's "server" counts
// every time from the page's "now": ?clock= moves it to that hour of Santiago by the shorter way round and ?now= pins it, as src/main.js does,
// and no time is left on the real clock; (2) the two drawers give one name to each thing (the five layers are Capas, the moves are Historial,
// and neither is SISTEMA's word for the engine's steps) and write a factor's points over what it can give through one function, as COLA's
// sheet already did; (3) a score in a row of HOY or COLA says its word ("puntaje 89"), and SISTEMA's strip says what its bars count; (4) the
// text cleaner and the patch helpers of the moments live once, in moments/shared/util.js.
// No browser: the demo runs in a vm (helpers/demo-host.mjs), the views are pure, the dictionaries and a few sources are read.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath, pathToFileURL } from "node:url";
import { setLang, setTimeZone } from "../src/i18n/index.js";
import { model } from "../src/core/model.js";
import { horaOf } from "../src/scene/clock.js";
import { shortestDelta } from "../src/scene/poses.js";
import { hoyView } from "../src/moments/hoy/view.js";
import { colaView, factorsOf } from "../src/moments/cola/view.js";
import { barsOf } from "../src/moments/procesos/drawer-content.js";
import { sistemaView } from "../src/moments/sistema/view.js";
import * as commonDrawer from "../src/ui/detail/score.js";
import { snapshotOf, loadDemo, DEMO_SOURCE } from "./helpers/demo-host.mjs";
import { scenario } from "./helpers/sistema-scenarios.mjs";
import esCommon from "../src/i18n/es/common.js";
import enCommon from "../src/i18n/en/common.js";
import esProcesos from "../src/i18n/es/procesos.js";
import enProcesos from "../src/i18n/en/procesos.js";
import esSistema from "../src/i18n/es/sistema.js";
import enSistema from "../src/i18n/en/sistema.js";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");   // panel-src
const SRC = path.join(ROOT, "src");
const MIN = 60000, HOUR = 3600000, DAY = 86400000;
const fresh = (lang = "es") => { setLang(lang); setTimeZone("America/Santiago"); };
const source = (rel) => fs.readFileSync(path.join(SRC, rel), "utf8");

// ---- (1) the demo's clock is the page's

/** The page's own "now" for ?clock=<hour> (src/main.js): the real one moved to that hour of Santiago by the shorter way round. */
const pageNow = (ms, hour) => ms + (hour == null ? 0 : shortestDelta(horaOf(ms), hour) * HOUR);

test("?clock= moves the demo's snapshot to the page's hour of Santiago, whatever the machine's zone: generated_at, the last run, the next run and the night's strip count from it", async () => {
  const host = process.env.TZ;
  try {
    // the machine may be in Santiago, where its own hour and Santiago's are the same: the zones of the test make them differ
    for (const tz of [host, "UTC", "Asia/Tokyo"]) {
      if (tz === undefined) delete process.env.TZ; else process.env.TZ = tz;
      for (const clock of [9, 21]) {
        const before = Date.now();
        const snap = await snapshotOf({ search: "?clock=" + clock });
        const after = Date.now();
        const at = Date.parse(snap.generated_at);
        const where = `TZ=${tz || "(host)"} ?clock=${clock}`;
        assert.ok(at >= pageNow(before, clock) - 2 * MIN && at <= pageNow(after, clock) + 2 * MIN, `${where}: generated_at ${snap.generated_at} is not the page's now`);
        const near = (a, b, what) => assert.ok(Math.abs(a - b) < 5000, `${where}: ${what}`);
        near(at - Date.parse(snap.controls.last_run_at), 7 * HOUR, "the last run is seven hours before the snapshot");
        near(Date.parse(snap.controls.scheduled_task.next_run_at) - at, 11 * HOUR, "the next run is eleven hours after the snapshot");
        const to = Date.parse(snap.activity.to);
        assert.ok(to > at && to <= at + 30 * MIN, `${where}: the night's strip ends in the half hour of the snapshot (${snap.activity.to})`);
      }
    }
  } finally {
    if (host === undefined) delete process.env.TZ; else process.env.TZ = host;
  }
});

test("?now= pins the demo's snapshot to that instant, as it pins the page, and wins over ?clock=", async () => {
  for (const search of ["?now=2026-03-04T15:00:00.000Z", "?clock=9&now=2026-03-04T15:00:00.000Z", "?now=2026-03-04T15:00:00.000Z&clock=21"]) {
    const snap = await snapshotOf({ search });
    assert.equal(snap.generated_at, "2026-03-04T15:00:00.000Z", search);
    assert.equal(snap.controls.last_run_at, "2026-03-04T08:00:00.000Z", search);
    assert.equal(snap.controls.scheduled_task.next_run_at, "2026-03-05T02:00:00.000Z", search);
    assert.equal(snap.activity.to, "2026-03-04T15:30:00.000Z", search);
  }
});

test("?clock= reads the hours the page reads (9.5, 9,5, 09:30) and ignores what the page ignores (abc, 25, nothing)", async () => {
  for (const [raw, hour] of [["9.5", 9.5], ["9,5", 9.5], ["09:30", 9.5], ["21:30", 21.5], ["abc", null], ["25", null], ["", null]]) {
    const before = Date.now();
    const snap = await snapshotOf({ search: "?clock=" + encodeURIComponent(raw) });
    const at = Date.parse(snap.generated_at);
    assert.ok(Math.abs(at - pageNow(before, hour)) < 2 * MIN, `?clock=${raw}: generated_at ${snap.generated_at}, the page's now is ${new Date(pageNow(before, hour)).toISOString()}`);
  }
});

// The demo in a vm whose real clock is stopped at `fakeMs` (null: the real one). `published`: the page without nonces, the only one that
// follows the run window (busyAt).
function demoAt(search, fakeMs, { published = false, lang = "es" } = {}) {
  const win = {};
  const context = vm.createContext({
    window: win, URL, URLSearchParams, setTimeout, clearTimeout, console,
    location: { search, href: "http://127.0.0.1:47213/" + search },
    document: {
      currentScript: { nonce: published ? "" : "test" }, documentElement: { lang }, addEventListener() {}, head: { append() {} }, body: { prepend() {} },
      createElement: () => ({ getContext: () => ({}), setAttribute() {} }),
    },
    MutationObserver: class { observe() {} },
    localStorage: { getItem: () => null, setItem() {} },
  });
  if (fakeMs != null) {
    vm.runInContext(`(() => { const Real = Date; globalThis.Date = class extends Real { constructor(...a) { if (a.length) super(...a); else super(${fakeMs}); } static now() { return ${fakeMs}; } }; })();`, context);
  }
  vm.runInContext(DEMO_SOURCE, context, { filename: "demo.js" });
  return win.__DEMO__;
}

/** What the demo answers along a short story: the snapshot, a mark and its undo, the mark again, a snooze, the snapshot and the job after. */
async function story(demo) {
  const s0 = (await demo.api("/api/snapshot")).data;
  const card = s0.board.flatMap((c) => c.cards).find((c) => c.state === "submitted");
  const mark = await demo.api("/api/mark", { key: card.key, state: "replied" });
  const undo = await demo.api("/api/mark/undo", { key: card.key });
  await demo.api("/api/mark", { key: card.key, state: "replied" });
  const top = s0.awaiting_approval.cards[0];
  const snooze = top ? await demo.api("/api/snooze", { key: top.key, until: 2 }) : null;
  const s1 = (await demo.api("/api/snapshot")).data;
  const job = (await demo.api("/api/job?key=" + encodeURIComponent(card.key))).data;
  return JSON.parse(JSON.stringify({ s0, mark, undo, snooze, s1, job }));
}

// Every Date.now() and new Date() of the demo must go through the page's now: the undo window, the days in a state, a snooze, a week back,
// the next window, a weekday, what is late, the run window. A test cannot name them all, so it compares the demo told ?now=A on the real clock
// with the demo whose real clock IS A and has no parameter: anything left on the real clock makes the two differ.
test("no time of the demo is left on the real clock: ?now=A answers exactly what the demo answers when the real clock is A, published or not", async () => {
  // 12:00 and 20:40 in Santiago: the run window closed and open
  for (const A of ["2026-03-04T15:00:00.000Z", "2026-03-07T23:40:00.000Z"]) {
    for (const published of [false, true]) {
      const pinned = await story(demoAt("?now=" + A, null, { published }));
      assert.equal(pinned.undo.ok, true, `${A}: a mark is undone at once (${JSON.stringify(pinned.undo.data)})`);
      assert.deepEqual(pinned, await story(demoAt("", Date.parse(A), { published })), `${A}${published ? ", published" : ""}`);
    }
  }
  // and under ?clock= a mark made now can be undone now
  for (const clock of [9, 21]) {
    const demo = loadDemo({ search: "?clock=" + clock });
    const card = (await demo.api("/api/snapshot")).data.board.flatMap((c) => c.cards).find((c) => c.state === "submitted");
    await demo.api("/api/mark", { key: card.key, state: "replied" });
    const undo = await demo.api("/api/mark/undo", { key: card.key });
    assert.equal(undo.ok, true, `?clock=${clock}: the mark is undone at once (${JSON.stringify(undo.data)})`);
  }
});

const ISO = /^\d{4}-\d\d-\d\dT\d\d:\d\d(?::\d\d(?:\.\d{3})?)?Z$/;
const LOCAL = /^\d{4}-\d\d-\d\d \d\d:\d\d$/;
/** Where `b` is not `a` a week later: every instant moves seven days, every other value stays. */
function notAWeekLater(a, b, at = "", out = []) {
  if (typeof a === "string" && (ISO.test(a) || LOCAL.test(a))) {
    const ms = (x) => Date.parse(LOCAL.test(x) ? x.replace(" ", "T") + "Z" : x);
    if (typeof b !== "string" || ms(b) - ms(a) !== 7 * DAY) out.push(`${at}: ${a} -> ${b}`);
  } else if (a && typeof a === "object") {
    for (const k of new Set([...Object.keys(a), ...Object.keys(b || {})])) notAWeekLater(a[k], b && b[k], at + "." + k, out);
  } else if (a !== b) out.push(`${at}: ${a} vs ${b}`);
  return out;
}

test("?now= a week later moves every time of the snapshot and of a job a week, after a mark too, and changes nothing else", async () => {
  for (const lang of ["es", "en"]) {
    const runs = [];
    for (const iso of ["2026-03-04T15:00:00.000Z", "2026-03-11T15:00:00.000Z"]) {
      const demo = loadDemo({ search: "?now=" + iso, lang });
      const card = (await demo.api("/api/snapshot")).data.board.flatMap((c) => c.cards).find((c) => c.state === "submitted");
      const mark = await demo.api("/api/mark", { key: card.key, state: "replied" });
      assert.equal(mark.ok, true, JSON.stringify(mark.data));
      runs.push({ snap: (await demo.api("/api/snapshot")).data, job: (await demo.api("/api/job?key=" + encodeURIComponent(card.key))).data });
    }
    assert.deepEqual(notAWeekLater(runs[0], runs[1]), [], lang);
  }
});

// ---- (2) the two drawers: one name for each thing, one way to write the points

test("both drawers call the five layers Capas and the moves Historial, each through one key of common.js, and neither name is SISTEMA's steps", () => {
  assert.equal(esCommon["drawer.journey"], "Capas");
  assert.equal(esCommon["drawer.history"], "Historial");
  assert.equal(enCommon["drawer.journey"], "Layers");
  assert.equal(enCommon["drawer.history"], "History");
  for (const [lang, common, sistema] of [["es", esCommon, esSistema], ["en", enCommon, enSistema]]) {
    // one name per concept across the panel: the steps of the engine's run (SISTEMA) are another thing than the five layers of an application
    assert.notEqual(common["drawer.journey"], sistema["sistema.row.steps"], `${lang}: the drawers' layers and SISTEMA's steps are two concepts with two names`);
    assert.notEqual(common["drawer.history"], sistema["sistema.row.steps"], `${lang}: the drawers' history is not SISTEMA's steps either`);
  }
  for (const [lang, words] of [["es", esProcesos], ["en", enProcesos]]) {
    assert.deepEqual(["procesos.drawer.layers", "procesos.drawer.trail"].filter((k) => k in words), [], `${lang}: PROCESOS' drawer says them with drawer.journey and drawer.history, not with words of its own`);
  }
  // and in PROCESOS' drawer each heads its own thing: Capas over the plates of the five layers, Historial over the trail of moves
  const pd = source("moments/procesos/drawer-content.js");
  const layers = pd.indexOf('t("drawer.journey")'), plates = pd.indexOf('class: "plates"'), history = pd.indexOf('t("drawer.history")');
  assert.ok(layers >= 0 && history >= 0, "PROCESOS' drawer says drawer.journey and drawer.history");
  assert.ok(layers < plates && plates < history && history < pd.indexOf("v.trail", history), "PROCESOS' drawer: Capas heads the plates, Historial heads the trail");
});

const textOf = (rows) => Object.fromEntries(rows.map((r) => [r.id, r.text]));
const threeOf = (detail) => [["the PROCESOS drawer", barsOf(detail)], ["COLA's sheet", factorsOf({ status: "ok", data: { score_detail: detail } }).bars], ["the common drawer", commonDrawer.scoreRows(detail)]];

test("COLA, the PROCESOS drawer and the common drawer write a factor's points over what it can give with the same function, and draw what it gives", () => {
  assert.equal(typeof commonDrawer.scoreRows, "function", "ui/detail/score.js exports scoreRows(scoreDetail), the bars of the common drawer as data");
  for (const [lang, fit] of [["es", "27,5 / 30"], ["en", "27.5 / 30"]]) {
    fresh(lang);
    const want = { fit, compensation: "10 / 15", brand_new: "4" };
    for (const [what, rows] of threeOf({ fit: 27.5, compensation: 10, brand_new: 4 })) assert.deepEqual(textOf(rows), want, `${lang}: ${what}`);
  }
  fresh();
  // the data is only half of it: COLA's sheet draws the text of each bar instead of gluing points and maximum again, and the common drawer
  // draws the rows of scoreRows instead of its own loop
  const colaRender = source("moments/cola/render.js");
  assert.match(colaRender, /\bb\.text\b/, "cola/render.js draws b.text");
  assert.doesNotMatch(colaRender, /["'`] \/ ["'`]|\} \/ \$\{/, "cola/render.js writes no \" / \" of its own");
  const score = source("ui/detail/score.js");
  assert.ok(score.includes("export function scoreView"), "ui/detail/score.js still exports scoreView");
  assert.match(score.slice(score.indexOf("export function scoreView")), /\bscoreRows\(/, "scoreView draws scoreRows");
});

test("a factor whose points are not a number is a bar in none of the three (no raw text over the maximum, no made-up zero), and a number in a string is the same number", () => {
  assert.equal(typeof commonDrawer.scoreRows, "function", "ui/detail/score.js exports scoreRows(scoreDetail), the bars of the common drawer as data");
  for (const lang of ["es", "en"]) {
    fresh(lang);
    for (const bad of ["n/a", "x", NaN, {}, null, "", true, false]) {
      const want = { compensation: "10 / 15", brand_new: "4" };
      for (const [what, rows] of threeOf({ fit: bad, compensation: 10, brand_new: 4 })) assert.deepEqual(textOf(rows), want, `${lang}, fit ${typeof bad} ${JSON.stringify(String(bad))}: ${what}`);
    }
    const fit = { es: "27,5 / 30", en: "27.5 / 30" }[lang];
    for (const [what, rows] of threeOf({ fit: "27.5" })) assert.deepEqual(textOf(rows), { fit }, `${lang}, fit "27.5": ${what}`);
  }
  fresh();
});

// ---- (3) a number never stands alone

const SCORE_WORD = { es: "puntaje", en: "score" };

test("every score in the rows of HOY and COLA says its word (puntaje 89, score 89), in both languages", async () => {
  for (const lang of ["es", "en"]) {
    fresh(lang);
    const snap = await snapshotOf({ lang });
    const scores = new Map();
    for (const c of [...snap.board.flatMap((col) => col.cards), ...snap.awaiting_approval.cards, ...snap.needs_gonza.cards]) scores.set(c.key, c.score);
    const nowMs = Date.parse(snap.generated_at);
    const m = model(snap);
    const first = (line) => String(line || "").split(" · ")[0];

    const hoy = hoyView(m, { nowMs });
    const hoyRows = [hoy.next, ...hoy.rows].filter((r) => r && r.data && r.data.card && !["followup", "offer"].includes(r.kindId));
    assert.ok(hoyRows.length >= 2, `${lang}: the demo has rows of HOY with a score`);
    for (const r of hoyRows) assert.equal(first(r.instrument), `${SCORE_WORD[lang]} ${scores.get(r.data.card)}`, `${lang}: HOY ${r.data.card}`);

    const rows = colaView(m, { nowMs }).queue.groups.filter((g) => ["pending", "approved", "blocked"].includes(g.id)).flatMap((g) => g.rows);
    assert.ok(rows.length >= 2, `${lang}: the demo has rows of COLA with a score`);
    for (const r of rows) assert.equal(first(r.meta), `${SCORE_WORD[lang]} ${scores.get(r.data.card)}`, `${lang}: COLA ${r.data.card}`);
  }
  fresh();
});

// The demo's scores are whole numbers and none is missing; the server can send 88.6, 0 or no score at all. HOY and COLA write the score of
// a row with one function: rounded as COLA's rows always were, and nothing at all (not "null", not the word alone) when there is none.
test("a score with decimals, a zero or no score at all: HOY and COLA write the same text, and a row with no score keeps the rest of its line", async () => {
  for (const lang of ["es", "en"]) {
    fresh(lang);
    const snap = await snapshotOf({ lang });
    const base = snap.awaiting_approval.cards[0];
    const stuck = snap.needs_gonza.cards.find((c) => c.reason_kind === "missing_answers");
    assert.ok(base && stuck, `${lang}: the demo has a card to approve and one that misses answers`);
    const absent = { ...base, key: "t:absent", source: "greenhouse" };
    delete absent.score;
    // the best score is COLA's proposal (not a row); the others are rows of HOY and of COLA (no tasks, so HOY has room for all of them)
    snap.awaiting_approval.cards = [{ ...base, key: "t:top", score: 99 }, { ...base, key: "t:dec", score: 88.6, source: "greenhouse" },
      { ...base, key: "t:zero", score: 0, source: "greenhouse" }, { ...base, key: "t:none", score: null, source: "greenhouse" }, absent];
    snap.awaiting_approval.total = 5;
    snap.needs_gonza.cards = [{ ...stuck, key: "t:half", score: 71.5 }];
    snap.needs_gonza.total = 1;
    snap.today.tasks = [];
    snap.today.tasks_total = 0;
    const nowMs = Date.parse(snap.generated_at);
    const m = model(snap);
    const hoy = hoyView(m, { nowMs });
    const hoyLine = new Map([hoy.next, ...hoy.rows].filter((r) => r && r.data && r.data.card).map((r) => [r.data.card, String(r.instrument || "")]));
    const colaLine = new Map(colaView(m, { nowMs }).queue.groups.flatMap((g) => g.rows).filter((r) => r.data && r.data.card).map((r) => [r.data.card, String(r.meta || "")]));
    const word = SCORE_WORD[lang];
    const scoreOf = (line) => { const head = line.split(" · ")[0]; return head.startsWith(word + " ") ? head : ""; };
    for (const [key, want] of [["t:dec", `${word} 89`], ["t:half", `${word} 72`], ["t:zero", `${word} 0`], ["t:none", ""], ["t:absent", ""]]) {
      assert.ok(hoyLine.has(key) && colaLine.has(key), `${lang}: ${key} is a row of HOY and of COLA`);
      assert.equal(scoreOf(hoyLine.get(key)), want, `${lang}: HOY ${key} ("${hoyLine.get(key)}")`);
      assert.equal(scoreOf(colaLine.get(key)), want, `${lang}: COLA ${key} ("${colaLine.get(key)}")`);
    }
    for (const key of ["t:none", "t:absent"]) {
      for (const [view, line] of [["HOY", hoyLine.get(key)], ["COLA", colaLine.get(key)]]) {
        assert.ok(line.startsWith("greenhouse · "), `${lang}: ${view} ${key}, a row with no score keeps its source and what follows ("${line}")`);
        assert.doesNotMatch(line, new RegExp(`\\b(?:null|undefined|NaN|${word})\\b`, "i"), `${lang}: ${view} ${key}, a row with no score says nothing of a score ("${line}")`);
      }
    }
  }
  fresh();
});

test("SISTEMA's strip says what its bars count in its title, tonight and last night, in both languages", () => {
  for (const [lang, unit] of [["es", "por media hora"], ["en", "per half hour"]]) {
    fresh(lang);
    for (const name of ["night", "day"]) {
      const s = scenario(name);
      const title = sistemaView(model(s), { nowMs: Date.parse(s.generated_at) }).strip.title;
      assert.ok(title.includes(unit), `${lang}, ${name}: "${title}" does not say "${unit}"`);
    }
  }
  fresh();
});

// ---- (4) one copy of the helpers of the moments

function sources(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, e.name);
    if (e.isDirectory()) sources(full, out);
    else if (e.name.endsWith(".js")) out.push(full);
  }
  return out;
}
const DEFINES = /^export (?:function|const) (?:clean|safeHref|sync|setText|setData|setHidden|setTitle)\b/m;
const SHARED = path.join(SRC, "moments", "shared", "util.js");

// Two copies of a function do the same thing, so no call can tell them apart: the copy is a fact of the source, and the source is what this reads.
test("the text cleaner and the patch helpers are defined once, in moments/shared/util.js, and the cleaner still drops a bidi override", async () => {
  const where = sources(SRC).filter((f) => DEFINES.test(fs.readFileSync(f, "utf8"))).map((f) => path.relative(ROOT, f).replace(/\\/g, "/"));
  assert.deepEqual(where, ["src/moments/shared/util.js"], "one copy: move HOY's util.js to moments/shared/ and delete APRENDER's");
  assert.deepEqual(["hoy", "aprender"].filter((m) => fs.existsSync(path.join(SRC, "moments", m, "util.js"))), [], "moments/hoy/util.js and moments/aprender/util.js are gone");
  const util = await import(pathToFileURL(SHARED).href);
  assert.equal(util.clean("Acme\u202e Corp"), "Acme Corp");
  assert.equal(util.safeHref("javascript:alert(1)"), null);
});
