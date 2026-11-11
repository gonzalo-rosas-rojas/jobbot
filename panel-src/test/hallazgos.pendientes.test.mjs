// Acceptance of the encargo hallazgos-pendientes: COLA's engine switch is SISTEMA's (E1), the help sheet without a stop (E3), one function for the
// bars of a score (E5), a number from the server is read with pointsOf and nothing else (E6), and the approval cards carry no score stub (E7).
// No browser: the views are pure, the switch runs against a fake session, a few sources are read.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { setLang, setTimeZone, t } from "../src/i18n/index.js";
import { model, candidates } from "../src/core/model.js";
import { factorBars, pointsOf, FACTOR_WEIGHTS } from "../src/core/format.js";
import { colaView, factorsOf } from "../src/moments/cola/view.js";
import { createColaActions } from "../src/moments/cola/actions.js";
import { hoyView } from "../src/moments/hoy/view.js";
import { drawerView, barsOf } from "../src/moments/procesos/drawer-content.js";
import { scoreRows } from "../src/ui/detail/score.js";
import { rejectionsOf, learnOf } from "../src/moments/aprender/data.js";
import { aprenderView } from "../src/moments/aprender/view.js";
import { helpModel } from "../src/ui/help.js";
import { KEYS } from "../src/ui/keys.js";
import esCommon from "../src/i18n/es/common.js";
import enCommon from "../src/i18n/en/common.js";
import { snapshotOf } from "./helpers/demo-host.mjs";

const SRC = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "src");
const fresh = (lang = "es") => { setLang(lang); setTimeZone("America/Santiago"); };
const clone = (x) => JSON.parse(JSON.stringify(x));
const BAD = [true, "", "   ", [5000], {}, "n/a"];

// ---- E1: the engine's switch in COLA is the shared one

function rig() {
  const gates = [];
  const session = { act: (path_, body, opts = {}) => new Promise((resolve) => gates.push(async (r) => { if (opts.beforeRefresh) await opts.beforeRefresh(r); resolve(r); })) };
  const actions = createColaActions({ session, sound: { play() {} }, announce() {}, t: (k) => k, open() {}, go() {}, openUrl() {}, toast: null, onChange() {} });
  return { actions, answer: (r) => gates.shift()(r) };
}

test("E1: stopping the engine from COLA says STOP at once with the button pending, and a refusal gives the page back", async () => {
  fresh();
  const snap = await snapshotOf();
  snap.controls = { ...snap.controls, stopped: false, engine: "recent" };
  const m = model(snap);
  const nowMs = Date.parse(snap.generated_at);
  const { actions, answer } = rig();

  const before = colaView(m, { nowMs, intent: actions.engineState().intent });
  assert.equal(before.foot.stop.label, "Parar el motor");
  assert.doesNotMatch(before.engine.line, /STOP activo/);

  const pressed = actions.engine(!!before.engine.stopped);
  const mid = actions.engineState();
  assert.deepEqual(mid, { intent: true, pending: true }, "the request is out and the page already says STOP");
  assert.equal(actions.isPending("engine"), true, "the button is data-pending while the server has not answered");
  const during = colaView(m, { nowMs, intent: mid.intent });
  assert.equal(during.foot.stop.label, "Reanudar el motor");
  assert.match(during.engine.line, /STOP activo/);
  assert.equal(during.engine.stopped, true);

  await answer({ ok: false, status: 409, data: {} });
  assert.equal(await pressed, "fired");
  assert.deepEqual(actions.engineState(), { intent: null, pending: false });
  const after = colaView(m, { nowMs, intent: actions.engineState().intent });
  assert.equal(after.foot.stop.label, "Parar el motor");
  assert.doesNotMatch(after.engine.line, /STOP activo/);
});

test("E1: after a stop the server accepted, the next snapshot is the truth again", async () => {
  fresh();
  const snap = await snapshotOf();
  snap.controls = { ...snap.controls, stopped: false, engine: "recent" };
  const m = model(snap);
  const nowMs = Date.parse(snap.generated_at);
  const { actions, answer } = rig();
  const pressed = actions.engine(false);
  await answer({ ok: true, status: 200, data: {} });
  await pressed;
  assert.equal(actions.engineState().intent, true, "until a snapshot arrives the page keeps what he asked for");
  actions.settle();
  assert.equal(actions.engineState().intent, null);
  assert.equal(colaView(m, { nowMs, intent: actions.engineState().intent }).foot.stop.label, "Parar el motor");
});

test("E1: COLA hands the intent to its view and settles it on every snapshot, as SISTEMA does", () => {
  const cola = fs.readFileSync(path.join(SRC, "moments", "cola.js"), "utf8");
  assert.match(cola, /intent:\s*me\.actions\.engineState\(\)\.intent/);
  assert.match(cola, /S\.actions\.settle\(\)/);
});

// ---- E3: the help sheet without a stop

test("E3: helpModel without a stop, or with one the table does not have, has no question and only the keys of the whole page", () => {
  fresh();
  const globals = KEYS.filter((k) => k.scope === "global").map((k) => k.display);
  for (const stop of [undefined, null, "", "nowhere", "__proto__", 7]) {
    const sheet = helpModel(stop);
    assert.equal(sheet.ask, null, String(stop));
    assert.equal(sheet.groups.length, 1, String(stop));
    assert.deepEqual(sheet.groups[0].rows.map((r) => r.key), globals, String(stop));
    assert.ok(sheet.usage.length > 0);
  }
  assert.equal(helpModel("cola").ask, t("stop.cola.ask"), "a stop of the table still has its question");
});

// ---- E5: one function for the bars of a score

test("E5: factorBars goes in the order of the sheet, fills a factor by its points over its maximum and keeps what COLA kept", () => {
  fresh();
  const bars = factorBars({ other: 5, stability: 10, fit: 30, zeta: 3, interview_style: 10, learning: 15, compensation: 15, alfa: 60 });
  assert.deepEqual(bars.map((b) => b.id), ["fit", "compensation", "interview_style", "stability", "learning", "other", "alfa", "zeta"]);
  const by = Object.fromEntries(bars.map((b) => [b.id, b]));
  assert.equal(by.compensation.k, 1, "15 of 15 fills the track");
  assert.equal(by.fit.k, 1);
  assert.equal(by.stability.k, 0.5);
  assert.equal(by.other.k, 0.5);
  assert.equal(by.zeta.k, 0.1, "a factor the page does not know is measured against the largest weight (30)");
  assert.equal(by.alfa.k, 1, "more than it can give does not overflow");
  assert.equal(by.fit.max, 30);
  assert.equal(by.zeta.max, null);
  assert.equal(by.compensation.text, "15 / 15");
  for (const bad of BAD) assert.deepEqual(factorBars({ fit: bad }), [], JSON.stringify(bad));
  assert.deepEqual(factorBars({ fit: -4 }).map((b) => b.k), [0], "below zero is an empty track");
  assert.deepEqual(factorBars(null), []);
  assert.deepEqual(factorBars("fit"), []);
  assert.deepEqual(factorBars({ fit: "27" }).map((b) => b.points), [27], "a number in a string is that number");
});

test("E5: the same application reads the same in COLA, in the PROCESOS drawer and in the common drawer", () => {
  fresh();
  const detail = { other: 5, stability: 18, fit: 27, brand_new: 4, interview_style: 9, learning: 12, compensation: 15 };
  const cola = factorsOf({ status: "ok", data: { score_detail: detail } }).bars;
  const drawer = barsOf(detail);
  const common = scoreRows(detail);
  const want = ["fit", "compensation", "interview_style", "stability", "learning", "other", "brand_new"];
  for (const [what, rows] of [["COLA", cola], ["PROCESOS", drawer], ["common", common]]) {
    assert.deepEqual(rows.map((b) => b.id), want, what);
    assert.deepEqual(rows.map((b) => b.k), cola.map((b) => b.k), `${what}: the same lengths`);
    assert.deepEqual(rows.map((b) => b.text), cola.map((b) => b.text), `${what}: the same words`);
    assert.equal(rows.find((b) => b.id === "compensation").k, 1, `${what}: Sueldo 15 / 15 fills`);
  }
  assert.deepEqual(barsOf([1, 2]), [], "an array is no breakdown in the drawer");
  assert.deepEqual(scoreRows([1, 2]), []);
});

test("E5: only core/format.js computes with FACTOR_WEIGHTS", () => {
  const offenders = [];
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const file = path.join(dir, entry.name);
      if (entry.isDirectory()) { walk(file); continue; }
      if (!file.endsWith(".js") || file.endsWith(path.join("core", "format.js"))) continue;
      const text = fs.readFileSync(file, "utf8");
      for (const line of text.split("\n")) {
        if (!line.includes("FACTOR_WEIGHTS")) continue;
        if (/^\s*(import\s.*from|export\s*\{\s*FACTOR_WEIGHTS\s*\}\s*;)/.test(line) || /^\s*\/\//.test(line)) continue;
        offenders.push(path.relative(SRC, file) + ": " + line.trim());
      }
    }
  };
  walk(SRC);
  assert.deepEqual(offenders, []);
  assert.equal(FACTOR_WEIGHTS.length, 6);
});

// ---- E6: a number from the server is what pointsOf says, or nothing

test("E6: pointsOf is the one reading (a number, or a non-blank string that is one; all else is null)", () => {
  assert.equal(pointsOf(12), 12);
  assert.equal(pointsOf("12.5"), 12.5);
  for (const bad of [...BAD, null, undefined, NaN, Infinity]) assert.equal(pointsOf(bad), null, JSON.stringify(bad));
});

test("E6: no .js of the page keeps its own copy of the test", () => {
  const found = [];
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const file = path.join(dir, entry.name);
      if (entry.isDirectory()) { walk(file); continue; }
      if (!file.endsWith(".js") || file.endsWith(path.join("core", "format.js"))) continue;
      const text = fs.readFileSync(file, "utf8");
      if (text.includes("isFinite(Number(") || /const finite =/.test(text)) found.push(path.relative(SRC, file));
    }
  };
  walk(SRC);
  assert.deepEqual(found, []);
});

test("E6: COLA says what it says when the value is missing, for a salary, a score and the thresholds that arrive as true, blank or an array", async () => {
  fresh();
  const base = await snapshotOf();
  const nowMs = Date.parse(base.generated_at);
  const keyOf = (s) => s.awaiting_approval.cards[0].key;
  const missing = clone(base);
  const target = missing.awaiting_approval.cards[0];
  for (const f of ["salary_min", "salary_max", "score"]) delete target[f];
  delete missing.pipeline.threshold;
  const want = colaView(model(missing), { nowMs, focusKey: keyOf(missing) });
  for (const bad of BAD) {
    const odd = clone(missing);
    Object.assign(odd.awaiting_approval.cards[0], { salary_min: bad, salary_max: bad, score: bad });
    odd.pipeline.threshold = bad;
    const got = colaView(model(odd), { nowMs, focusKey: keyOf(odd), batchMin: bad });
    const label = JSON.stringify(bad);
    assert.deepEqual(got.focus.facts, want.focus.facts, `facts ${label}`);
    assert.equal(got.focus.score, want.focus.score, `score ${label}`);
    assert.equal(got.focus.scoreAria, want.focus.scoreAria, `aria ${label}`);
    assert.deepEqual(got.focus.path, want.focus.path, `path ${label}`);
    assert.equal(got.batch.min, colaView(model(missing), { nowMs }).batch.min, `batch threshold ${label}`);
    assert.equal(got.batch.picks.some((p) => p.key === keyOf(odd)), false, `a score that is no number is no pick ${label}`);
  }
});

test("E6: HOY reads the salary above, then the one below, and otherwise says what an offer without salary says", async () => {
  fresh();
  const snap = await snapshotOf();
  snap.today.tasks = [];
  snap.today.tasks_total = 0;
  const card = { ...clone(snap.awaiting_approval.cards[0]), key: "x:offer", state: "offer", company: "Acme", title: "Dev" };
  delete card.salary_min;
  delete card.salary_max;
  snap.board = snap.board.filter((col) => col.state !== "offer").concat([{ state: "offer", total: 1, cards: [card] }]);
  snap.awaiting_approval = { total: 0, cards: [] };
  const nowMs = Date.parse(snap.generated_at);
  const instrument = (extra) => {
    const s = clone(snap);
    Object.assign(s.board.find((col) => col.state === "offer").cards[0], extra);
    const v = hoyView(model(s), { nowMs });
    const row = [v.next, ...v.rows].find((r) => r && r.kindId === "offer");
    assert.ok(row, "the offer is a row of HOY");
    return row.instrument;
  };
  const none = instrument({});
  assert.doesNotMatch(none, /US\$/);
  for (const bad of BAD) {
    assert.equal(instrument({ salary_max: bad, salary_min: bad }), none, `both ${JSON.stringify(bad)}`);
    assert.match(instrument({ salary_max: bad, salary_min: 5000 }), /5\.000/, `max ${JSON.stringify(bad)} falls to the minimum`);
  }
  assert.match(instrument({ salary_max: 9000, salary_min: 5000 }), /9\.000/);
  assert.match(instrument({ salary_max: "9000" }), /9\.000/);
});

test("E6: both drawers do not invent a salary or a threshold out of true, a blank or an array", async () => {
  fresh();
  const snap = await snapshotOf();
  const card = snap.awaiting_approval.cards[0];
  const nowMs = Date.parse(snap.generated_at);
  const view = (extra, threshold) => {
    const s = clone(snap);
    s.pipeline.threshold = threshold;
    return drawerView({ location: "Remoto", ...extra }, card, model(s), nowMs);
  };
  const want = view({}, undefined);
  for (const bad of BAD) {
    const got = view({ salary_min: bad, salary_max: bad }, bad);
    assert.deepEqual(got.sub, want.sub, JSON.stringify(bad));
    assert.equal(got.score.threshold, null, JSON.stringify(bad));
    assert.doesNotMatch(got.sub.join(" "), /US\$/);
    const odd = view({ salary_min: bad, salary_max: 6000 }, 70);
    assert.match(odd.sub.join(" "), /6\.000/);
    assert.doesNotMatch(odd.sub.join(" "), /US\$\s?[015]\b/);
  }
});

test("E6: APRENDER does not count true, a blank or an array as a figure, a score or an average", async () => {
  fresh();
  const snap = await snapshotOf();
  const m = model(snap);
  const nowMs = Date.parse(snap.generated_at);
  const odd = clone(snap);
  odd.pipeline.complete = true;
  const figures = aprenderView(model(odd), { nowMs }).bank.figures;
  assert.equal(figures.find((f) => f.id === "complete").nodata, true);
  assert.equal(figures.find((f) => f.id === "complete").n, null);
  odd.pipeline.complete = " ";
  assert.equal(aprenderView(model(odd), { nowMs }).bank.figures.find((f) => f.id === "complete").nodata, true);
  odd.pipeline.complete = "12";
  assert.equal(aprenderView(model(odd), { nowMs }).bank.figures.find((f) => f.id === "complete").n, 12);

  const seenFigure = (seen) => {
    const s = clone(snap);
    s.learn = { total: 40, seen, due: 2, topics: [], weak_topics: [], gaps: [] };
    return aprenderView(model(s), { nowMs }).bank.figures.find((f) => f.id === "seen");
  };
  for (const bad of BAD) {
    assert.equal(seenFigure(bad).n, null, "seen " + JSON.stringify(bad));
    assert.equal(seenFigure(bad).nodata, true, "seen " + JSON.stringify(bad));
  }
  assert.equal(seenFigure("36").n, 36);
  assert.equal(seenFigure(0).n, 0);

  const rejected = (score) => ({ state: "rejected", total: 6, cards: Array.from({ length: 6 }, (_, i) => ({ key: "r" + i, source: "s", score })) });
  for (const bad of BAD) {
    const s = { board: [rejected(bad)] };
    const r = rejectionsOf(s);
    assert.equal(r.n, 0, JSON.stringify(bad));
    assert.deepEqual(r.rows, []);
  }
  assert.equal(rejectionsOf({ board: [rejected("70")] }).n, 6);

  const topics = (average) => learnOf({ learn: { topics: [{ topic: "sql", total: 3, seen: 1, due: 1 }], weak_topics: [{ topic: "sql", average }] } }).topics[0].average;
  for (const bad of BAD) assert.equal(topics(bad), null, JSON.stringify(bad));
  assert.equal(topics("2.5"), 2.5);
  assert.ok(m);
});

// ---- E7: the approval cards carry no score stub

test("E7: candidates() leaves big and sub null on approvals and stuck forms, and the stub key is gone from both dictionaries", async () => {
  fresh();
  const snap = await snapshotOf();
  const list = candidates(model(snap));
  const cards = list.filter((c) => c.kindId === "approve" || (c.key && c.kindId !== "offer" && c.kindId !== "followup"));
  assert.ok(list.some((c) => c.kindId === "approve"), "the demo has an approval");
  assert.ok(cards.length > 0);
  for (const c of cards) {
    assert.equal(c.big, null, c.id);
    assert.equal(c.sub, null, c.id);
  }
  for (const c of list.filter((x) => x.task)) assert.ok(c.big && c.sub, "a task keeps its stub: " + c.id);
  assert.equal("ticket.stub.score" in esCommon, false);
  assert.equal("ticket.stub.score" in enCommon, false);
});
