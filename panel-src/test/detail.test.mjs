// Acceptance checks for the drawer's content (package P4c, ui/detail.js and ui/detail/*): the answer at the top, the
// Recorrido, the evidence below it and the actions. Pure parts (the answer's sentence rules, the dictionary) run in Node; the
// rest drives the real built page with the demo's fictional data in a private headless Chrome (tools/cdp.mjs): build the demo
// to its own file, serve it on port 8806 (ONLY that port), assert, and always close Chrome and the server. The demo's data is
// patched BEFORE it boots (see PATCH below) to give some applications a capture, hostile text, a refusal or missing fields.
// Screenshots land in dist/shots/detail/.
//
// What of FEATURES section 9 this covers (the numbers are the rows of that table): 8 picture and note survive redraws, 9 a
// refused mark keeps the note, 10 the picture and a recorded answer, 11 Escape, 12 no stray null, 30 an approval shows
// "Aprobada" before the list closes, 35 the path and where the buttons sit (the buttons are in the actions row under the
// answer, see the report); 5, 6 and 7 as they apply to the drawer (permanent actions are two-step, a double click only
// arms). Rows 31 to 34 (the night strip, the route, the threshold, trust) are Hoy and Cola, not the drawer.
import test, { before, after } from "node:test";
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { mkdirSync, readFileSync, readdirSync } from "node:fs";
import { launch } from "../tools/cdp.mjs";
import { model } from "../src/core/model.js";
import { asSentence } from "../src/core/format.js";
import { setLang, t, tn } from "../src/i18n/index.js";
import { answerOf, tasksOf } from "../src/ui/detail/now.js";
import { FACTOR_WEIGHTS } from "../src/moments/cola/view.js";
import esDetail from "../src/i18n/es/detail.js";
import enDetail from "../src/i18n/en/detail.js";
import esCommon from "../src/i18n/es/common.js";
import enCommon from "../src/i18n/en/common.js";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const shots = path.join(root, "dist", "shots", "detail");
const PORT = 8806;
const ORIGIN = `http://127.0.0.1:${PORT}`;
const PAGE = "dist/demo-detail.html";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const report = {};

let server;
let browser;

before(async () => {
  mkdirSync(shots, { recursive: true });
  report.build = execFileSync(process.execPath, ["build.mjs", "demo", "--entry", "lab/f0-drawer.entry.js", "--out", PAGE], { cwd: root, encoding: "utf8" }).trim();
  server = spawn(process.execPath, ["serve.mjs", String(PORT)], { cwd: root, stdio: "ignore" });
  for (let i = 0; i < 60; i++) {
    try {
      if ((await fetch(`${ORIGIN}/${PAGE}`)).ok) break;
    } catch {
      await sleep(100);
    }
  }
  browser = await launch({ width: 1440, height: 900 });
});

after(async () => {
  console.log("report:", JSON.stringify(report, null, 1));
  await browser?.close();
  server?.kill();
});

// ---- the patched mock ------------------------------------------------------------------------------------------------

// Runs before the demo script: it wraps window.__DEMO__ (the fictional server), the capture request and the blob URLs, and
// records every POST in window.__calls. `__CFG__` is replaced with a JSON object (see `open` below):
//   shots [keys]        cards that have a dry-run capture (the demo has none)     shotFail [keys]  captures that answer 404
//   shotDelay ms        how long the capture takes                                refuse {path: text}  that POST answers 409 with that text
//   jobDelay ms         how long /api/job takes                                   jobFail [keys]   /api/job answers 500 (jobFailText "" = no text)
//   hostile key         that application, its task, its mail and its detail carry text that looks like markup
//   sparse key          that application's detail has nothing optional (no place, pay, history, text...)
//   extraTask key       that application gets a second open task
const PATCH = String.raw`(function () {
  var CFG = __CFG__;
  var HOSTILE = '<img src=x onerror="window.pwned=1"> <b>x</b>';
  var BAD_LINK = "javascript:window.pwned=1";
  window.__calls = [];
  window.__shots = [];
  window.__urls = { created: [], revoked: [] };
  var real = null;
  function each(s, fn) {
    s.board.forEach(function (col) { col.cards.forEach(fn); });
    s.awaiting_approval.cards.forEach(fn);
    s.needs_gonza.cards.forEach(fn);
  }
  function card(c) {
    if ((CFG.shots || []).indexOf(c.key) >= 0) c.has_screenshot = true;
    if (CFG.hostile && c.key === CFG.hostile) {
      c.company = HOSTILE; c.title = HOSTILE; c.note = HOSTILE; c.source = HOSTILE; c.url = BAD_LINK;
      c.reason = HOSTILE + "\n" + HOSTILE; c.reason_short = HOSTILE;
    }
  }
  function snapshot(s) {
    each(s, card);
    if (CFG.hostile) {
      s.today.tasks.forEach(function (x) { if (x.job_key === CFG.hostile) { x.text = HOSTILE; x.link = BAD_LINK; } });
      s.today.emails.forEach(function (m) { if (m.job_key === CFG.hostile) { m.subject = HOSTILE; m.summary = HOSTILE; m.from = HOSTILE; } });
    }
    if (CFG.extraTask) {
      s.today.tasks.push({ id: 900, text: "Segunda tarea de prueba", job_key: CFG.extraTask, due_at: new Date(Date.now() + 3 * 864e5).toISOString(), link: "https://example.com/t", overdue: false });
      s.today.tasks_total += 1;
    }
  }
  function detail(d) {
    card(d.card);
    if (CFG.hostile && d.card.key === CFG.hostile) {
      d.location = HOSTILE; d.apply_url = BAD_LINK; d.dossier = HOSTILE; d.description = HOSTILE;
      d.events.forEach(function (e) { e.note = HOSTILE; });
      d.score_detail = { "<i>x</i>": 7, fit: 20, compensation: 9 };
      d.gaps = [{ topic: HOSTILE, seen: false }];
    }
    if (CFG.sparse && d.card.key === CFG.sparse) {
      d.location = ""; d.remote = false; d.salary_min = null; d.salary_max = null; d.events = []; d.score_detail = {}; d.gaps = [];
      d.description = ""; d.dossier = null; d.first_seen = null; d.apply_url = ""; d.card.url = ""; d.card.note = null; d.card.reason = null;
    }
  }
  function wait(ms, value) { return new Promise(function (resolve) { setTimeout(function () { resolve(value); }, ms); }); }
  var wrapped = {
    get startedAt() { return real && real.startedAt; },
    api: function (path, body) {
      var p = path.split("?")[0];
      if (body !== undefined) window.__calls.push({ path: p, body: body });
      if (body !== undefined && CFG.refuse && CFG.refuse[p]) return Promise.resolve({ ok: false, status: 409, data: { ok: false, message: CFG.refuse[p] } });
      var key = decodeURIComponent((path.split("key=")[1] || "").split("&")[0]);
      if (p === "/api/job" && (CFG.jobFail || []).indexOf(key) >= 0) {
        var data = CFG.jobFailText === "" ? null : { ok: false, message: CFG.jobFailText || "No se pudo abrir esa postulación (prueba)." };
        return wait(CFG.jobDelay || 0, { ok: false, status: 500, data: data });
      }
      return real.api(path, body).then(function (r) {
        if (r.ok && p === "/api/snapshot") snapshot(r.data);
        if (r.ok && p === "/api/job") detail(r.data);
        return p === "/api/job" && CFG.jobDelay ? wait(CFG.jobDelay, r) : r;
      });
    },
  };
  Object.defineProperty(window, "__DEMO__", { configurable: true, get: function () { return wrapped; }, set: function (v) { real = v; } });

  // the capture: a fake form drawn on a canvas, served the way /api/shot would (and the token it came with is recorded)
  function png() {
    var c = document.createElement("canvas");
    c.width = 640; c.height = 1100;
    var g = c.getContext("2d");
    g.fillStyle = "#ffffff"; g.fillRect(0, 0, 640, 1100);
    g.fillStyle = "#2063b0"; g.fillRect(0, 0, 640, 90);
    g.fillStyle = "#cbd2da";
    for (var i = 0; i < 9; i++) { g.fillRect(40, 140 + i * 110, 180, 16); g.fillRect(40, 168 + i * 110, 560, 46); }
    g.fillStyle = "#fbc024"; g.fillRect(40, 168 + 4 * 110, 560, 46);
    return new Promise(function (resolve) { c.toBlob(resolve, "image/png"); });
  }
  var realFetch = window.fetch.bind(window);
  window.fetch = function (url, init) {
    var u = String(url);
    if (u.indexOf("/api/shot") !== 0) return realFetch(url, init);
    var key = decodeURIComponent((u.split("key=")[1] || "").split("&")[0]);
    window.__shots.push({ key: key, token: init && init.headers ? init.headers["X-Panel-Token"] : null });
    if ((CFG.shotFail || []).indexOf(key) >= 0) return wait(CFG.shotDelay || 0, new Response("no", { status: 404 }));
    return wait(CFG.shotDelay || 0).then(png).then(function (blob) { return new Response(blob, { status: 200, headers: { "content-type": "image/png" } }); });
  };
  var create = URL.createObjectURL.bind(URL);
  var revoke = URL.revokeObjectURL.bind(URL);
  URL.createObjectURL = function (b) { var u = create(b); window.__urls.created.push(u); return u; };
  URL.revokeObjectURL = function (u) { window.__urls.revoked.push(u); return revoke(u); };
})();`;

const patch = (cfg = {}) => PATCH.replace("__CFG__", JSON.stringify(cfg));

// ---- helpers ---------------------------------------------------------------------------------------------------

// The demo's own "demo data" badge writes a <style> element without the page's nonce, which the real CSP refuses; that
// one pair of messages is the demo's, not the drawer's, and is the only thing filtered out.
const demoBadge = (p) => /Applying inline style violates/.test(p.text) || /^CSP violation: style-src-elem inline\s*$/.test(p.text);
const problems = (page) => page.problems().filter((p) => !demoBadge(p));

const VK = { ArrowDown: 40, ArrowUp: 38, ArrowLeft: 37, ArrowRight: 39, Home: 36, End: 35, Escape: 27, Enter: 13, Tab: 9 };

/** A real key press through the browser's input pipeline (so it counts as a user gesture). */
async function press(page, key) {
  const printable = key.length === 1;
  const vk = VK[key] ?? (printable ? key.toUpperCase().charCodeAt(0) : 0);
  const code = printable ? (/\d/.test(key) ? "Digit" + key : /[a-z]/i.test(key) ? "Key" + key.toUpperCase() : key) : key;
  await page.send("Input.dispatchKeyEvent", {
    type: printable || key === "Enter" ? "keyDown" : "rawKeyDown", key, code, windowsVirtualKeyCode: vk, text: printable ? key : key === "Enter" ? "\r" : undefined,
  });
  await page.send("Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode: vk });
}

/** Opens the demo with `cfg` patched in. `query` is the address's query; `t=` gives the session a token to send. */
async function open(cfg = {}, { query = "debug=1&world=off&t=tok123", width, height, dark, reduced, hash = "", b = browser } = {}) {
  const page = await b.newPage();
  // not `mobile`: that emulation widens the layout viewport to fit whatever overflows (another room's), so a phone would
  // look as wide as its widest part and `innerWidth` could never show an overflow
  if (width) await page.viewport(width, height);
  if (dark !== undefined || reduced !== undefined) await page.media({ dark, reducedMotion: reduced });
  await page.send("Page.addScriptToEvaluateOnNewDocument", { source: patch(cfg) });
  await page.send("Storage.clearDataForOrigin", { origin: ORIGIN, storageTypes: "local_storage,session_storage" });
  await page.goto(`${ORIGIN}/${PAGE}?${query}${hash}`);
  await page.waitFor("window.__app && __app.rooms.mounted() && __app.model()", { timeout: 15000 });
  await page.eval("window.__snd = []; (() => { const p = __app.sound.play.bind(__app.sound); __app.sound.play = (n, o) => { __snd.push(n); return p(n, o); }; })()");
  return page;
}

const D = `document.getElementById("detail")`;
const q = (sel) => `document.querySelector(${JSON.stringify(sel)})`;
const calls = (page, p) => page.eval(`window.__calls.filter((c) => c.path === ${JSON.stringify(p)})`);
const exists = (page, sel) => page.eval(`!!${q(sel)}`);
const count = (page, sel) => page.eval(`document.querySelectorAll(${JSON.stringify(sel)}).length`);
const text = (page, sel) => page.eval(`(${q(sel)} || {}).textContent`);
const body = (page) => page.eval(`${D}.innerText`);
const click = (page, sel) => page.eval(`${q(sel)}.click()`);
const sounds = (page) => page.eval("window.__snd");

/** Opens an application's drawer through the shell, and waits until its content is on screen. */
async function openJob(page, key) {
  await page.eval(`__app.drawer.open(${JSON.stringify(key)})`);
  await page.waitFor(`${D}.classList.contains("open") && !!${D}.querySelector('[data-job="${key}"]')`, { timeout: 8000 });
}

async function closeDrawer(page) {
  await page.eval("__app.drawer.close()");
  await page.waitFor(`!${D}.classList.contains("open")`);
}

/** A permanent button: the first click arms it, the second (after the arming time) does it. */
async function twoStep(page, sel) {
  await click(page, sel);
  await sleep(420);
  await click(page, sel);
}

/** Types into the note field the way a person does: the field has the focus and an input event says what is in it. */
const typeNote = (page, value) => page.eval(`(() => { const i = document.getElementById("mark-note"); i.focus(); i.value = ${JSON.stringify(value)}; i.dispatchEvent(new Event("input", { bubbles: true })); })()`);

const toast = (page) => page.eval(`document.getElementById("toast").classList.contains("show") ? document.getElementById("toast").textContent : ""`);
const STRAY = /\bnull\b|\bundefined\b|\[object|\bNaN\b|⟦/;

// ---- pure parts: the answer's sentence ------------------------------------------------------------------------

const NOW = Date.parse("2026-10-07T15:00:00Z");
const HOUR = 3600000;
const DAY = 24 * HOUR;
const iso = (ms) => new Date(ms).toISOString();

const mk = (o = {}) => ({
  key: "k:1", company: "Acme", title: "Dev", score: 80, state: "submitted", source: "lever", url: "https://acme.example/1", reason: null, reason_kind: null,
  reason_short: null, note: null, approved: false, has_screenshot: false, has_cv: true, has_dossier: false, updated_at: iso(NOW - 3 * DAY - HOUR), actions: [], marks: [], ...o,
});
const aTask = (o = {}) => ({ id: 7, text: "Entregar la prueba", job_key: "k:1", due_at: iso(NOW + 30 * HOUR), link: null, overdue: false, ...o });

function world(cards, tasks = []) {
  const states = [...new Set(cards.map((c) => c.state))];
  return model({
    generated_at: iso(NOW),
    board: states.map((state) => { const list = cards.filter((c) => c.state === state); return { state, label: state, total: list.length, cards: list }; }),
    today: { tasks_total: tasks.length, tasks, emails_total: 0, emails: [] },
  });
}
const answer = (card, tasks = []) => answerOf(card, world([card], tasks));

test("answer: what is not sent yet says whose turn it is at the layer where it stopped", () => {
  setLang("es");
  let a = answer(mk({ state: "prepared", actions: ["approve"] }));
  assert.equal(a.tone, "you");
  assert.equal(a.sentence, t("detail.now.approve"));
  assert.equal(a.sub, t("ticket.why.approve"));
  assert.equal(a.turn.key, "mine");
  a = answer(mk({ state: "prepared", approved: true }));
  assert.equal(a.tone, "robot");
  assert.equal(a.sentence, t("detail.now.robot.approved"));
  a = answer(mk({ state: "prepared" }));
  assert.equal(a.sentence, t("detail.now.robot.dryrun"));
  assert.equal(a.sub, t("detail.now.robot.dryrun.sub"));
  a = answer(mk({ state: "scored" }));
  assert.equal(a.sentence, t("detail.now.robot.scored"));
  assert.equal(a.turn.cls, "robot");
  // a stuck form: the dictionary's sentence for the kind of help, and what the button does under it
  a = answer(mk({ state: "needs_gonza", reason_kind: "missing_answers", reason_short: "texto del servidor", actions: ["resolve"] }));
  assert.equal(a.tone, "you");
  assert.equal(a.sentence, asSentence(t("reason.short.missing_answers")));
  assert.equal(a.sub, t("act.resolve.hint"));
  a = answer(mk({ state: "needs_gonza", reason_kind: "handoff", actions: ["confirm"] }));
  assert.equal(a.sub, t("act.confirm.hint"));
});

test("answer: a sent application follows the rules of the Procesos rows (the turn, the next task, the days without news)", () => {
  setLang("es");
  // the company's turn: waiting, and after ten days without news, late
  let a = answer(mk({ updated_at: iso(NOW - 12 * DAY - HOUR) }));
  assert.equal(a.tone, "late");
  assert.equal(a.sentence, "Sin noticias hace 12 días");
  assert.equal(a.sentence, tn("detail.now.stale", 12));
  assert.equal(a.turn.key, "wait");
  a = answer(mk());
  assert.equal(a.tone, "calm");
  assert.equal(a.sentence, t("detail.now.wait"));
  assert.equal(a.sub, t("detail.now.last", { when: tn("time.ago.days", 3) }));
  // his turn: a task is on it
  const task = aTask();
  a = answer(mk({ state: "technical_test" }), [task]);
  assert.equal(a.tone, "you");
  assert.equal(a.sentence, "Prueba técnica: la empresa espera tu entrega");
  assert.equal(a.task, task);
  assert.equal(a.turn.cls, "you");
  a = answer(mk({ state: "technical_test" }), [aTask({ due_at: iso(NOW - 2 * HOUR), overdue: true })]);
  assert.equal(a.tone, "late");
  assert.equal(a.late, true);
  assert.equal(a.turn.cls, "late");
  a = answer(mk({ state: "submitted" }), [task]);
  assert.equal(a.sentence, t("detail.now.mine.other"));
  // nobody has a task: set the next step
  a = answer(mk({ state: "interview" }));
  assert.equal(a.turn.key, "check");
  assert.equal(a.sentence, t("detail.now.check.interview"));
  assert.equal(a.sub, t("detail.now.check.sub"));
  assert.equal(answer(mk({ state: "replied" })).sentence, t("detail.now.check.replied"));
  assert.equal(answer(mk({ state: "ghosted", updated_at: iso(NOW - 21 * DAY) })).sentence, t("detail.now.check.ghosted"));
  // an offer is always his turn, with or without a task
  a = answer(mk({ state: "offer" }));
  assert.equal(a.sentence, t("detail.now.mine.offer"));
  assert.equal(a.sub, t("ticket.why.offer"));
  a = answer(mk({ state: "offer" }), [task]);
  assert.equal(a.sub, null);
});

test("answer: closed applications, a task named by company, no model, and English", () => {
  setLang("es");
  assert.equal(answer(mk({ state: "accepted" })).tone, "ok");
  assert.equal(answer(mk({ state: "rejected" })).sentence, t("detail.now.rejected"));
  assert.equal(answer(mk({ state: "withdrawn" })).sentence, t("detail.now.withdrawn"));
  assert.equal(answer(mk({ state: "withdrawn" })).turn, null);
  // a task that has no key but names the company belongs to it; one that names another company or has another key does not
  const named = aTask({ job_key: null, text: "Escribir a Acme sobre la prueba" });
  const other = aTask({ id: 8, job_key: "k:9", text: "Algo de otra empresa" });
  const card = mk({ state: "replied" });
  const m = world([card, mk({ key: "k:9", company: "Zeta" })], [named, other]);
  assert.deepEqual(tasksOf(card, m).map((x) => x.id), [7]);
  assert.equal(answerOf(card, m).sentence, t("detail.now.mine.replied"));
  // without a model (the drawer opened before the first picture) it still says something and does not throw
  const alone = answerOf(mk({ updated_at: iso(Date.now() - 2 * DAY) }), null);
  assert.equal(alone.sentence, t("detail.now.wait"));
  assert.deepEqual(alone.tasks, []);
  setLang("en");
  assert.equal(answer(mk({ state: "prepared", actions: ["approve"] })).sentence, "Waiting for your approval");
  assert.equal(answer(mk({ updated_at: iso(NOW - 12 * DAY - HOUR) })).sentence, "No news for 12 days");
  setLang("es");
});

test("the dictionary: both languages say the same things, nothing is defined twice and every key the code asks for exists", () => {
  const vars = (s) => Array.from(new Set(Array.from(s.matchAll(/\{([A-Za-z_]\w*)\}/g), (m) => m[1]))).sort();
  assert.deepEqual(Object.keys(esDetail).sort(), Object.keys(enDetail).sort());
  for (const key of Object.keys(esDetail)) {
    assert.match(key, /^detail\.[a-z_]+(\.[a-z_]+)*$/, key);
    assert.deepEqual(vars(esDetail[key]), vars(enDetail[key]), `variables of ${key}`);
    assert.ok(esDetail[key].trim() && enDetail[key].trim(), `${key} is empty`);
    assert.ok(!(key in esCommon) && !(key in enCommon), `${key} is also in common`);
  }
  // every literal key that the drawer's modules pass to t() or tn()
  const es = { ...esCommon, ...esDetail };
  const en = { ...enCommon, ...enDetail };
  const files = ["src/ui/detail.js", ...readdirSync(path.join(root, "src/ui/detail")).map((f) => "src/ui/detail/" + f)];
  const keys = new Set();
  for (const file of files) for (const m of readFileSync(path.join(root, file), "utf8").matchAll(/\bt[n]?\(\s*"([a-z][a-z_]*(?:\.[a-zA-Z_]+)+)"/g)) keys.add(m[1]);
  assert.ok(keys.size > 40, "found " + keys.size + " keys");
  for (const key of keys) {
    const plural = Object.hasOwn(es, key + ".one") || Object.hasOwn(es, key + ".other");
    assert.ok(Object.hasOwn(es, key) || plural, `es has no ${key}`);
    assert.ok(Object.hasOwn(en, key) || plural, `en has no ${key}`);
  }
  // the keys built from a name: one for each state or kind the code can ask for
  const families = {
    "detail.now.mine.": ["replied", "technical_test", "interview", "offer", "other"],
    "detail.now.check.": ["replied", "technical_test", "interview", "ghosted", "other"],
    "detail.now.robot.": ["scored", "dryrun", "approved", "scored.sub", "dryrun.sub", "approved.sub"],
    "detail.now.": ["accepted", "rejected", "withdrawn"],
    "detail.factor.": ["fit", "compensation", "stability", "learning", "interview_style", "other"],
  };
  for (const [prefix, names] of Object.entries(families)) for (const n of names) assert.ok(esDetail[prefix + n] && enDetail[prefix + n], prefix + n);
});

// ---- screenshots, to look at -------------------------------------------------------------------------------------

async function look(name, { cases, dark, width = 1440, height = 900, lang, scroll = 0 }) {
  const cfg = { shots: ["demo:15", "demo:16"], shotDelay: 100 };
  const page = await open(cfg, { dark, width, height, query: "debug=1&world=off&t=tok123" + (lang ? "&lang=" + lang : "") });
  for (const [label, key] of cases) {
    await openJob(page, key);
    await page.waitFor(`!${D}.querySelector("#shot-slot") || !!${D}.querySelector("#shot-slot img")`, { timeout: 5000 });
    if (scroll) await page.eval(`${D}.scrollTop = ${scroll}`);
    await sleep(450);
    await page.shot(path.join(shots, `${label}-${name}.png`));
    await closeDrawer(page);
  }
  assert.deepEqual(problems(page), []);
}

const SITUATIONS = [
  ["approve", "demo:15"], ["handoff", "demo:12"], ["interview", "demo:4"], ["test", "demo:3"], ["stale", "demo:1"],
  ["offer", "demo:9"], ["ghosted", "demo:8"], ["rejected", "demo:23"], ["scored", "demo:20"], ["withdrawn", "demo:26"],
];

// The pictures for the art director. A short set by default (the suite must stay quick); DETAIL_LOOK=all takes every one.
const ALL = process.env.DETAIL_LOOK === "all";
const some = (names) => SITUATIONS.filter(([name]) => ALL || names.includes(name));

test("look: the drawer for each situation, light desktop", async () => {
  await look("light-desktop", { cases: some(["approve", "handoff", "interview", "stale"]), dark: false });
});

test("look: dark desktop in English, and the phone in both themes", async () => {
  await look("dark-desktop-en", { cases: some(["approve", "test"]), dark: true, lang: "en" });
  await look("light-phone", { cases: some(["approve", "interview"]), dark: false, width: 390, height: 844 });
  await look("dark-phone", { cases: some(["approve"]), dark: true, width: 390, height: 844 });
  if (ALL) {
    await look("light-phone-scrolled", { cases: some([]), dark: false, width: 390, height: 844, scroll: 1100 });
    await look("light-desktop-scrolled", { cases: some([]), dark: false, scroll: 900 });
  }
});

// ---- the answer, the header and the Recorrido, in the page ------------------------------------------------------

test("header: title, state, score, place, pay, links; the answer sits under it with the actions row after", async () => {
  const page = await open({});
  await openJob(page, "demo:4");
  assert.equal(await text(page, "#detail h2"), "Nimbus Health — AI Engineer");
  assert.equal(await text(page, "#detail .d-meta .chip"), t("state.interview"));
  const meta = await text(page, "#detail .d-meta");
  assert.ok(meta.includes(t("drawer.score", { score: 90 })) && meta.includes("Remoto, América Latina"), meta);
  assert.ok(!meta.includes(t("drawer.remote")), "the word 'remoto' is not said twice");
  const salary = await text(page, "#detail .d-salary");
  assert.match(salary, /^Sueldo publicado: US\$5\.200 a US\$6\.800 al mes$/);
  assert.equal(await text(page, "#detail .d-links a"), t("act.viewPosting"));
  assert.equal(await page.eval(`${q("#detail .d-links a")}.getAttribute("target")`), "_blank");
  assert.match(await page.eval(`${q("#detail .d-links a")}.rel`), /noopener/);
  // no salary in the data: it says so
  await closeDrawer(page);
  await openJob(page, "demo:2");
  assert.equal(await text(page, "#detail .d-salary"), t("drawer.salary.range", { range: t("drawer.salary.between", { from: "US$4.500", to: "US$5.500" }) }));
  await closeDrawer(page);
  await openJob(page, "demo:5");
  assert.equal(await text(page, "#detail .d-salary"), t("drawer.salary.none"));
  // the order in the DOM is the order of reading: header, answer, actions, Recorrido
  await closeDrawer(page);
  await openJob(page, "demo:4");
  const order = await page.eval(`Array.from(${D}.querySelector(".d-body").children).map((e) => e.className.split(" ")[0] + (e.id ? "#" + e.id : ""))`);
  assert.deepEqual(order.slice(0, 4), ["d-head", "d-now#d-now", "d-actions", "d-sec#journey"]);
  assert.equal(await count(page, "#detail h2"), 1, "one h2: the title");
  assert.deepEqual(problems(page), []);
});

test("answer in the page: one sentence per situation, in the dictionary's words, with the tone and the chip for whose turn", async () => {
  const page = await open({ shots: ["demo:15"] });
  const cases = [
    ["demo:15", "you", t("detail.now.approve")], // prepared, a capture waits for him
    ["demo:12", "you", asSentence(t("reason.short.handoff"))], // a form he must send himself
    ["demo:4", "you", t("detail.now.mine.interview")], // interview with a task
    ["demo:3", "you", t("detail.now.mine.technical_test")],
    ["demo:2", "late", t("detail.now.mine.replied")], // its task is overdue
    ["demo:1", "late", tn("detail.now.stale", 12)], // twelve days without news
    ["demo:5", "calm", t("detail.now.wait")],
    ["demo:8", "you", t("detail.now.check.ghosted")],
    ["demo:9", "you", t("detail.now.mine.offer")],
    ["demo:17", "robot", t("detail.now.robot.dryrun")], // prepared without a capture
    ["demo:20", "robot", t("detail.now.robot.scored")],
    ["demo:23", "calm", t("detail.now.rejected")],
    ["demo:26", "calm", t("detail.now.withdrawn")],
  ];
  for (const [key, tone, sentence] of cases) {
    await openJob(page, key);
    assert.equal(await page.eval(`${q("#d-now")}.dataset.tone`), tone, key);
    assert.equal(await text(page, "#d-now .d-sentence"), sentence, key);
    // the tone is never alone: there is a word for whose turn it is, or the sentence says it is over
    const chip = await text(page, "#d-now .d-turn .chip");
    if (key !== "demo:23" && key !== "demo:26") assert.ok(chip && chip.length > 2, key + " has a chip");
    await closeDrawer(page);
  }
  assert.deepEqual(problems(page), []);
});

test("Recorrido (row 35): the five layers, where it stopped, what is missing; sent ones add the five stops", async () => {
  const page = await open({ shots: ["demo:15"] });
  // prepared and waiting for him: four done, the gate is his
  await openJob(page, "demo:15");
  assert.equal(await count(page, "#journey li.j-done"), 4);
  assert.equal(await count(page, "#journey li[data-layer]"), 5);
  assert.equal(await page.eval(`Array.from(document.querySelectorAll("#journey li[data-layer]")).map((e) => e.dataset.layer).join()`), "aviso,puntaje,cv,respuestas,compuerta");
  assert.equal(await exists(page, '#journey li[data-layer="compuerta"].j-you[aria-current="step"]'), true);
  assert.equal(await count(page, '#journey [aria-current="step"]'), 1);
  assert.match(await text(page, '#journey li[data-layer="compuerta"]'), new RegExp(t("layer.compuerta") + ".*" + t("status.you")));
  assert.match(await text(page, '#journey li[data-layer="puntaje"] .d-ldetail'), /^89 de 100, tu umbral es 60$/);
  assert.match(await text(page, '#journey li[data-layer="aviso"] .d-ldetail'), /^Guardado el \d+ \w+\.? desde greenhouse$/);
  assert.equal(await exists(page, "#journey .j-after"), false, "not sent: no stops after");
  assert.equal(await count(page, "#detail [data-action=approve]"), 1, "the approve button is there once");
  assert.equal(await exists(page, "#detail .d-actions [data-action=approve]"), true, "in the actions row, which stays in view");
  await closeDrawer(page);
  // sent, in the interview stage: all five done, then the five stops with the current one marked
  await openJob(page, "demo:4");
  assert.equal(await count(page, "#journey li.j-done"), 5);
  assert.equal(await exists(page, "#journey .j-after"), true);
  assert.equal(await text(page, "#journey .d-after"), t("drawer.afterSend"));
  assert.equal(await count(page, "#journey .d-steps li"), 5);
  assert.equal(await text(page, "#journey .d-steps li.here .d-slabel"), t("stage.interview"));
  assert.equal(await count(page, "#journey .d-steps li.done"), 3);
  assert.equal(await page.eval(`${q("#journey .d-steps")}.getAttribute("aria-label")`), t("detail.stage.aria", { name: t("stage.interview") }));
  await closeDrawer(page);
  // a form that stopped: the layer is his, and the reason is said once
  await openJob(page, "demo:13");
  assert.equal(await exists(page, '#journey li[data-layer="respuestas"].j-you'), true);
  assert.equal(await count(page, "#journey li.j-done"), 3);
  assert.equal(await count(page, "#journey li.j-todo"), 1);
  assert.equal(await exists(page, '#journey li[data-layer="respuestas"] .d-ldetail'), false, "the answer above already says it");
  await closeDrawer(page);
  // jobbot's turn: scored, the CV is next
  await openJob(page, "demo:20");
  assert.equal(await exists(page, '#journey li[data-layer="cv"].j-robot'), true);
  assert.equal(await count(page, "#journey li.j-todo"), 2);
  assert.equal(await count(page, "#detail .d-actions"), 0, "nothing to press: jobbot is on it");
  await closeDrawer(page);
  // a withdrawn one has no path: only the buttons it has (none)
  await openJob(page, "demo:26");
  assert.equal(await exists(page, "#journey"), false);
  assert.equal(await count(page, "#detail .d-actions"), 0);
  assert.deepEqual(problems(page), []);
});

// ---- actions ---------------------------------------------------------------------------------------------------

test("approve (rows 5, 6, 30): a double click only arms it, the second click acts, 'Aprobada' shows before the drawer reads again", async () => {
  const page = await open({ shots: ["demo:15"] }, { query: "debug=1&world=off&t=tok123" });
  await openJob(page, "demo:15");
  await page.waitFor(`!!${q("#shot-slot img")}`);
  const button = "#detail [data-action=approve]";
  assert.equal(await text(page, button), t("act.approve"));
  // two clicks in the same instant (a double click by habit) arm it and nothing more
  await page.eval(`(() => { const b = ${q(button)}; b.click(); b.click(); })()`);
  await sleep(450);
  assert.equal(await text(page, button), t("act.sure"));
  assert.equal(await page.eval(`${q(button)}.hasAttribute("data-armed")`), true, "the armed state is visible");
  assert.deepEqual(await calls(page, "/api/approve"), [], "nothing was sent");
  await click(page, button);
  await page.waitFor(`!!${q(button + ".sent")}`, { timeout: 4000 });
  assert.equal(await text(page, button), t("act.approved"), "the button says it is done before the list closes over it");
  await page.waitFor(`!${q("#detail [data-action=approve]")}`);
  assert.deepEqual(await calls(page, "/api/approve"), [{ path: "/api/approve", body: { key: "demo:15" } }]);
  assert.equal(await toast(page), "Aprobada: la próxima corrida la envía.", "the server's own words");
  // the drawer was read again: the gate is now jobbot's, the answer changed and says so, the focus stayed inside
  assert.equal(await exists(page, '#journey li[data-layer="compuerta"].j-robot'), true);
  assert.equal(await text(page, "#d-now .d-sentence"), t("detail.now.robot.approved"));
  assert.equal(await page.eval(`${q("#d-now")}.classList.contains("fresh")`), true, "what changed is marked");
  assert.equal(await page.eval(`${D}.contains(document.activeElement)`), true, "the focus did not fall out of the drawer");
  assert.equal(await count(page, "#detail .d-actions"), 0);
  const heard = (await sounds(page)).filter((n) => ["arm", "confirm", "approve", "error", "undo"].includes(n));
  assert.deepEqual(heard, ["arm", "confirm", "approve"], "the first press, the second press, then what it did");
  assert.equal(await page.eval("window.__shots.length"), 1, "the capture was not fetched again");
  assert.deepEqual(problems(page), []);
});

test("confirm is two-step and shows 'Anotada'; resolve is two-step too (the server cannot take it back) and plays undo", async () => {
  const page = await open({});
  await openJob(page, "demo:12");
  assert.equal(await text(page, "#detail [data-action=confirm]"), t("act.confirm"));
  await twoStep(page, "#detail [data-action=confirm]");
  await page.waitFor(`!!${q("#detail [data-action=confirm].sent")}`, { timeout: 4000 });
  assert.equal(await text(page, "#detail [data-action=confirm]"), t("act.noted"));
  await page.waitFor(`!${q("#detail [data-action=confirm]")}`);
  assert.deepEqual(await calls(page, "/api/confirm"), [{ path: "/api/confirm", body: { key: "demo:12" } }]);
  assert.equal(await text(page, "#detail .d-meta .chip"), t("state.submitted"));
  assert.equal(await exists(page, "#journey .j-after"), true, "it went out: the five stops now");
  assert.equal(await exists(page, "#detail [data-jump=mark]"), true, "and he can record the answer when it comes");
  await closeDrawer(page);
  // resolve: the first click only arms it (no undo exists for it on the server), the second does it
  await openJob(page, "demo:13");
  await click(page, "#detail [data-action=resolve]");
  await sleep(150);
  assert.deepEqual(await calls(page, "/api/resolve"), [], "the first click sends nothing");
  await sleep(270);
  await click(page, "#detail [data-action=resolve]");
  await page.waitFor(`!${q("#detail [data-action=resolve]")}`, { timeout: 5000 });
  assert.deepEqual(await calls(page, "/api/resolve"), [{ path: "/api/resolve", body: { key: "demo:13" } }]);
  assert.ok((await sounds(page)).includes("undo"));
  assert.equal(await text(page, "#detail .d-meta .chip"), t("state.prepared"));
  assert.equal(await text(page, "#d-now .d-sentence"), t("detail.now.robot.dryrun"));
  assert.deepEqual(problems(page), []);
});

test("a refused action says so in the server's words, plays error, and the drawer is read again with its button back", async () => {
  const page = await open({ shots: ["demo:15"], refuse: { "/api/approve": "No se pudo aprobar: la captura cambió (prueba)." } });
  await openJob(page, "demo:15");
  await twoStep(page, "#detail [data-action=approve]");
  await page.waitFor(`document.getElementById("toast").textContent.includes("la captura cambió")`);
  assert.equal(await toast(page), "No se pudo aprobar: la captura cambió (prueba).");
  assert.ok((await sounds(page)).includes("error"));
  assert.ok(!(await sounds(page)).includes("approve"));
  await sleep(300);
  assert.equal(await exists(page, "#detail [data-action=approve]"), true, "still there to try again");
  assert.equal(await page.eval(`${q("#detail [data-action=approve]")}.disabled`), false);
  assert.equal(await exists(page, "#detail [data-action=approve].sent"), false);
  assert.deepEqual(problems(page), []);
});

test("the next task is done from the drawer: two-step, and the answer changes; other tasks are listed with their own button", async () => {
  const page = await open({ extraTask: "demo:3" });
  await openJob(page, "demo:3");
  // the task that is next sits with the answer, with its button in the actions row
  assert.equal(await text(page, "#d-now .d-task-text"), "Entregar la prueba técnica de Cobalto Systems");
  assert.equal(await exists(page, "#detail .d-actions [data-done]"), true);
  assert.equal(await text(page, "#detail .d-actions [data-done]"), t("act.done"));
  // the others are in a section of their own, with the link only because it is https
  assert.equal(await text(page, "#d-tasks-h"), t("detail.tasks.other"));
  assert.equal(await count(page, "#detail .d-items [data-dtask]"), 1);
  assert.equal(await text(page, "#detail .d-items [data-dtask] span"), "Segunda tarea de prueba");
  assert.equal(await text(page, "#detail .d-items [data-dtask] a"), t("act.openLink"));
  assert.equal(await page.eval(`${q("#detail .d-items [data-dtask] a")}.getAttribute("href")`), "https://example.com/t");
  // a refused one (the mock does not know task 900) says so and stays; the drawer is read again (new nodes) before he goes on
  await page.eval(`window.__old = ${q("#detail .d-items [data-done]")}`);
  await twoStep(page, "#detail .d-items [data-done]");
  await page.waitFor(`document.getElementById("toast").textContent.includes("ya estaba cerrada")`);
  await page.waitFor(`${q("#detail .d-items [data-done]")} && ${q("#detail .d-items [data-done]")} !== window.__old`);
  assert.equal(await page.eval(`${q("#detail .d-items [data-done]")}.disabled`), false);
  // the next one: two clicks
  await click(page, "#detail .d-actions [data-done]");
  await sleep(100);
  assert.equal(await text(page, "#detail .d-actions [data-done]"), t("act.sure"), "armed");
  assert.deepEqual((await calls(page, "/api/task/done")).filter((c) => c.body.id === 102), []);
  await sleep(350);
  await click(page, "#detail .d-actions [data-done]");
  await page.waitFor(`window.__calls.some((c) => c.path === "/api/task/done" && c.body.id === 102)`);
  assert.equal(typeof (await calls(page, "/api/task/done")).at(-1).body.id, "number", "the id goes as a number");
  await page.waitFor(`!${q('#detail [data-dtask="102"]')}`);
  // its task is gone: the other one is the next now, and the section of other tasks has nothing left to list
  assert.equal(await text(page, "#d-now .d-task-text"), "Segunda tarea de prueba");
  assert.equal(await exists(page, "#d-tasks-h"), false);
  assert.equal(await text(page, "#d-now .d-sentence"), t("detail.now.mine.technical_test"));
  assert.equal(await page.eval(`${D}.contains(document.activeElement)`), true);
  assert.deepEqual(problems(page), []);
});

// ---- the screenshot -----------------------------------------------------------------------------------------------

test("the capture (rows 8, 10): fetched with the token, 'Cargando…' first, kept across redraws, revoked when it is not needed", async () => {
  const page = await open({ shots: ["demo:15", "demo:16"], shotDelay: 500 });
  await openJob(page, "demo:15");
  assert.equal(await text(page, "#shot-slot"), t("drawer.shot.loading"));
  await page.waitFor(`!!${q("#shot-slot img")}`, { timeout: 5000 });
  assert.deepEqual(await page.eval("window.__shots"), [{ key: "demo:15", token: "tok123" }], "one request, with the token");
  const ok = await page.eval(`(() => { const i = ${q("#shot-slot img")}; return i.complete && i.naturalWidth > 0 && i.src.startsWith("blob:") && i.alt; })()`);
  assert.equal(ok, t("drawer.shot.alt"));
  const src = await page.eval(`${q("#shot-slot img")}.src`);
  // a snapshot refresh does not touch the drawer: the same nodes
  await page.eval(`window.__keep = ${q("#shot-slot img")}`);
  for (let i = 0; i < 3; i++) await page.eval("__app.session.refresh({ force: true })");
  assert.equal(await page.eval(`window.__keep === ${q("#shot-slot img")}`), true, "the picture node was not redrawn");
  // a redraw of the drawer (it is read again after an action; the language changes) shows the same picture without asking
  await page.eval("__app.shell.drawer.rerender()");
  await page.eval("__app.drawer.reload()");
  assert.equal(await page.eval(`${q("#shot-slot img")}.src`), src);
  assert.equal(await page.eval("window.__shots.length"), 1);
  // the zoom is a toggle on the same picture
  await click(page, "#shot-slot button");
  assert.equal(await page.eval(`${q("#shot-slot button")}.getAttribute("aria-pressed")`), "true");
  assert.equal(await exists(page, "#shot-slot .d-shotbox.zoom"), true);
  assert.equal(await text(page, "#shot-slot button"), t("detail.shot.fit"));
  // another application: the first picture's URL is revoked, the second one is fetched
  await page.eval(`__app.drawer.open("demo:16")`);
  await page.waitFor(`!!${D}.querySelector('[data-job="demo:16"]')`);
  assert.ok((await page.eval("window.__urls.revoked")).includes(src), "the old blob URL was revoked");
  await page.waitFor(`!!${q("#shot-slot img")} && ${q("#shot-slot img")}.src !== ${JSON.stringify(src)}`, { timeout: 5000 });
  // closing revokes it too, once the content is gone
  const src2 = await page.eval(`${q("#shot-slot img")}.src`);
  await closeDrawer(page);
  await page.waitFor(`window.__urls.revoked.includes(${JSON.stringify(src2)})`, { timeout: 3000 });
  assert.equal(await page.eval(`${D}.children.length`), 0);
  // a request that comes back after the drawer was closed is dropped and never turned into a URL
  const created = await page.eval("window.__urls.created.length");
  await page.eval(`__app.drawer.open("demo:15")`);
  await page.waitFor(`!!${D}.querySelector('[data-job="demo:15"]')`);
  await closeDrawer(page);
  await sleep(1300);
  assert.equal(await page.eval("window.__urls.created.length"), created, "no URL was made for a picture nobody wanted");
  // every URL a capture made has been revoked (the one left over, if any, is the tab icon's)
  const live = await page.eval("window.__urls.created.filter((u) => !window.__urls.revoked.includes(u))");
  assert.ok(live.length <= 1, "unrevoked: " + live.join());
  assert.deepEqual(problems(page), []);
});

test("a capture that fails says so and can be tried again; the rest of the drawer is whole; without a capture it says so where one is expected", async () => {
  const page = await open({ shots: ["demo:15", "demo:16"], shotFail: ["demo:16"] });
  await openJob(page, "demo:16");
  await page.waitFor(`${q("#shot-slot")}.textContent.includes(${JSON.stringify(t("detail.shot.failed"))})`);
  assert.equal(await exists(page, "#journey"), true);
  assert.equal(await exists(page, "#detail [data-action=approve]"), true, "the buttons are there");
  assert.equal(await count(page, "#shot-slot button"), 1);
  await click(page, "#shot-slot button");
  await page.waitFor(`window.__shots.length === 2`);
  await page.waitFor(`${q("#shot-slot")}.textContent.includes(${JSON.stringify(t("detail.shot.failed"))})`);
  await closeDrawer(page);
  // prepared with no capture: the line; a sent one: nothing about a capture
  await openJob(page, "demo:17");
  assert.equal(await text(page, ".d-noshot"), t("drawer.shot.none"));
  assert.equal(await exists(page, "#shot-slot"), false);
  await closeDrawer(page);
  await openJob(page, "demo:4");
  assert.equal(await exists(page, ".d-noshot"), false);
  assert.deepEqual(problems(page), []);
});

// ---- "Registrar una respuesta" ----------------------------------------------------------------------------------

test("marks (rows 8, 10): the note survives redraws, closing and room changes; a mark goes with it; terminal marks are two-step", async () => {
  const page = await open({});
  await openJob(page, "demo:4");
  assert.equal(await page.eval(`${q("#mark-note")}.maxLength`), 200);
  assert.equal(await page.eval(`${q("#mark-note")}.labels.length`), 1, "the field has its label");
  assert.equal(await page.eval(`Array.from(document.querySelectorAll("#detail [data-mark]")).map((b) => b.dataset.mark).join()`), "offer,rejected,withdrawn");
  await typeNote(page, "Llamar el jueves a las 16:00");
  // redraws: a snapshot, the language, the drawer read again: the text stays and so does the focus
  await page.eval(`window.__keep = ${q("#mark-note")}`);
  for (let i = 0; i < 2; i++) await page.eval("__app.session.refresh({ force: true })");
  assert.equal(await page.eval(`window.__keep === ${q("#mark-note")}`), true, "a refresh did not redraw the drawer");
  await page.eval("__app.shell.drawer.rerender()");
  assert.equal(await page.eval(`${q("#mark-note")}.value`), "Llamar el jueves a las 16:00");
  await page.eval("__app.drawer.reload()");
  await page.waitFor(`${q("#mark-note")}.value === "Llamar el jueves a las 16:00" && document.activeElement.id === "mark-note"`);
  // Escape closes it; the note is there when he opens it again (and after a room change)
  await press(page, "Escape");
  await page.waitFor(`!${D}.classList.contains("open")`);
  await openJob(page, "demo:4");
  assert.equal(await page.eval(`${q("#mark-note")}.value`), "Llamar el jueves a las 16:00");
  await page.eval(`__app.go("procesos")`);
  await page.waitFor(`!${D}.classList.contains("open") && __app.rooms.mounted() === "procesos"`);
  await openJob(page, "demo:4");
  assert.equal(await page.eval(`${q("#mark-note")}.value`), "Llamar el jueves a las 16:00");
  // another application starts with its own, empty, note
  await closeDrawer(page);
  await openJob(page, "demo:3");
  assert.equal(await page.eval(`${q("#mark-note")}.value`), "");
  await closeDrawer(page);
  await openJob(page, "demo:4");
  // the answers are said the way they happened, and picking one changes nothing: it asks (2026-10-24, three clicks took one to "oferta")
  assert.deepEqual(await page.eval(`Array.from(document.querySelectorAll("#detail [data-mark]")).map((b) => b.textContent)`), [t("mark.offer"), t("mark.rejected"), t("mark.withdrawn")]);
  const company = (await text(page, "#detail h2")).split(" — ")[0];
  await click(page, "#detail [data-mark=offer]");
  assert.deepEqual(await calls(page, "/api/mark"), [], "a pick only asks");
  assert.equal(await text(page, "#mark-question"), t("mark.ask.offer"));
  assert.equal(await text(page, "#mark-ask .d-change"), t("detail.mark.change", { company, from: t("state.interview"), to: t("state.offer") }));
  assert.equal(await page.eval(`${q("#detail [data-mark=offer]")}.getAttribute("aria-pressed")`), "true");
  assert.equal(await page.eval("document.activeElement.id"), "mark-yes", "the question takes the focus to its yes");
  // the second half of a double click lands on nothing: the yes waits for a deliberate press
  await click(page, "#mark-yes");
  assert.deepEqual(await calls(page, "/api/mark"), [], "a double click never answers the question");
  // "No, volver" puts the question away and the focus back on the answer
  await click(page, "#mark-no");
  assert.equal(await page.eval(`${q("#mark-ask")}.hidden`), true);
  assert.equal(await page.eval("document.activeElement.dataset.mark"), "offer");
  // an answer that closes the application says so; another pick takes its place
  await click(page, "#detail [data-mark=rejected]");
  assert.equal(await text(page, "#mark-ask .d-change"), t("detail.mark.change.closes", { company, from: t("state.interview"), to: t("state.rejected") }));
  await click(page, "#detail [data-mark=offer]");
  assert.equal(await text(page, "#mark-question"), t("mark.ask.offer"));
  assert.equal(await page.eval(`Array.from(document.querySelectorAll("#detail [data-mark][aria-pressed=true]")).length`), 1);
  await sleep(420);
  // the yes records it, with the note, and the section turns into a receipt: the answers do not come back under the pointer
  await click(page, "#mark-yes");
  await page.waitFor(`window.__calls.some((c) => c.path === "/api/mark")`);
  assert.deepEqual(await calls(page, "/api/mark"), [{ path: "/api/mark", body: { key: "demo:4", state: "offer", note: "Llamar el jueves a las 16:00" } }]);
  await page.waitFor(`${q("#detail .d-meta .chip")}.textContent === ${JSON.stringify(t("state.offer"))} && !!${q("#mark-done")}`);
  assert.equal(await text(page, "#mark-done"), t("detail.mark.done", { company, from: t("state.interview"), to: t("state.offer") }));
  assert.equal(await page.eval("document.activeElement.id"), "mark-done", "the focus lands on what changed");
  assert.equal(await count(page, "#detail [data-mark]"), 0, "no answer to click by accident after one went through");
  assert.equal(await exists(page, "#mark-undo"), true, "the way back is there while the server allows it");
  assert.match(await body(page), /hasta las \d\d:\d\d/);
  assert.equal(await text(page, "#d-now .d-sentence"), t("detail.now.mine.offer"));
  assert.ok((await body(page)).includes("Nota: Llamar el jueves a las 16:00"), "and it is the note of the application now");
  // "Registrar otra respuesta" brings the answers of the new state back, the note used up, the last change still undoable above them
  await click(page, "#mark-again");
  assert.deepEqual(await page.eval(`Array.from(document.querySelectorAll("#detail [data-mark]")).map((b) => b.dataset.mark).join()`), "accepted,rejected,withdrawn");
  assert.equal(await page.eval("document.activeElement.dataset.mark"), "accepted");
  assert.equal(await page.eval(`${q("#mark-note")}.value`), "", "the note was used up");
  assert.equal(await exists(page, "#detail .d-last #mark-undo"), true);
  // Deshacer takes it back: the state, the note and the answers it had
  await click(page, "#mark-undo");
  await page.waitFor(`window.__calls.some((c) => c.path === "/api/mark/undo")`);
  assert.deepEqual(await calls(page, "/api/mark/undo"), [{ path: "/api/mark/undo", body: { key: "demo:4" } }]);
  await page.waitFor(`${q("#detail .d-meta .chip")}.textContent === ${JSON.stringify(t("state.interview"))}`);
  assert.equal(await page.eval(`Array.from(document.querySelectorAll("#detail [data-mark]")).map((b) => b.dataset.mark).join()`), "offer,rejected,withdrawn");
  assert.equal(await exists(page, "#mark-undo"), false, "nothing left to undo");
  assert.ok((await sounds(page)).includes("undo"));
  // the terminal one: no note (a null, not an empty string); the receipt stays, with its way back and no other answer to record
  await click(page, "#detail [data-mark=withdrawn]");
  await sleep(420);
  await click(page, "#mark-yes");
  await page.waitFor(`window.__calls.filter((c) => c.path === "/api/mark").length === 2`);
  assert.deepEqual((await calls(page, "/api/mark"))[1].body, { key: "demo:4", state: "withdrawn", note: null });
  await page.waitFor(`${q("#detail .d-meta .chip")}.textContent === ${JSON.stringify(t("state.withdrawn"))} && !!${q("#mark-done")}`);
  assert.equal(await exists(page, "#mark-again"), false, "nothing more to record");
  assert.equal(await exists(page, "#mark-undo"), true);
  assert.equal(await exists(page, "#journey"), false, "a closed one has no path");
  assert.equal(await page.eval(`${D}.contains(document.activeElement)`), true);
  assert.deepEqual(problems(page), []);
});

test("a refused mark (row 9) keeps the note, says why in the server's words and clears the message when he types", async () => {
  const page = await open({});
  await openJob(page, "demo:3");
  // the application moved on while the drawer was open: the mark he is about to press is no longer allowed
  await page.eval(`__app.session.api("/api/mark", { key: "demo:3", state: "interview", note: null })`);
  await typeNote(page, "mi nota");
  assert.equal(await exists(page, "#mark-refusal:not([hidden])"), false);
  await click(page, "#detail [data-mark=interview]");
  await sleep(420);
  await click(page, "#mark-yes");
  await page.waitFor(`${q("#mark-refusal")} && !${q("#mark-refusal")}.hidden`);
  const refusal = await text(page, "#mark-refusal");
  assert.equal(refusal, "No se pudo marcar: ya no se puede pasar a ese estado.");
  assert.equal(await toast(page), refusal);
  assert.equal(await page.eval(`${q("#mark-note")}.value`), "mi nota", "the note is still there");
  // the drawer was read again: it shows what is allowed now
  assert.equal(await page.eval(`Array.from(document.querySelectorAll("#detail [data-mark]")).map((b) => b.dataset.mark).join()`), "offer,rejected,withdrawn");
  assert.equal(await text(page, "#detail .d-meta .chip"), t("state.interview"));
  assert.ok((await sounds(page)).includes("error"));
  assert.equal(await exists(page, "#detail .d-last #mark-undo"), true, "the mark made elsewhere a moment ago can be taken back from here");
  // typing again clears the refusal
  await typeNote(page, "mi nota 2");
  assert.equal(await page.eval(`${q("#mark-refusal")}.hidden`), true);
  assert.deepEqual(problems(page), []);
});

// ---- the evidence ----------------------------------------------------------------------------------------------

test("history, score, gaps, dossier, description, emails: what the data says, newest first, with the folds kept", async () => {
  const page = await open({});
  await openJob(page, "demo:3");
  // the history: newest first
  const events = await page.eval(`Array.from(document.querySelectorAll("#detail .d-timeline .d-ev")).map((e) => ({ first: e.classList.contains("first"), what: e.querySelector(".d-evwhat").textContent, at: e.querySelector("time").getAttribute("datetime") }))`);
  assert.equal(events.length, 5);
  assert.equal(events[0].first, true);
  assert.equal(events[0].what, t("drawer.move", { from: t("state.replied"), to: t("state.technical_test") }));
  assert.equal(events.at(-1).what, t("state.scored"));
  assert.ok(events.every((e, i) => i === 0 || Date.parse(events[i - 1].at) >= Date.parse(e.at)));
  // the score: one bar per factor (the demo names them with the server's ids, so the page translates them) with its points over what it can
  // give, as PROCESOS and COLA write them, and the total; the points of the factors add up to the score of the card
  const bars = await page.eval(`Array.from(document.querySelectorAll("#detail .d-bar")).map((r) => r.children[0].textContent + "=" + r.children[2].textContent)`);
  const factors = await page.eval(`__app.session.api("/api/job?key=demo:3").then((r) => r.data.score_detail)`);
  assert.deepEqual(Object.keys(factors), ["fit", "compensation", "interview_style", "stability", "learning", "other"]);
  assert.equal(Object.values(factors).reduce((x, y) => x + y, 0), 86, "the factors add up to the score");
  const weight = Object.fromEntries(FACTOR_WEIGHTS);
  assert.deepEqual(bars, [...Object.entries(factors).map(([id, points]) => t("detail.factor." + id) + "=" + points + " / " + weight[id]), "Total=86"]);
  assert.ok(await page.eval(`Array.from(document.querySelectorAll("#detail .d-bar .meter i")).every((i) => i.style.getPropertyValue("--w") !== "")`));
  // the emails of this application, as text
  assert.equal(await count(page, "#detail [data-email]"), 1);
  assert.match(await text(page, "#detail [data-email]"), new RegExp(t("kind.technical_test") + ".*Prueba técnica: servicio de colas"));
  // the gaps, and the way to review them
  assert.equal(await text(page, "#d-gaps-h"), t("drawer.gaps"));
  assert.equal(await text(page, "#detail .d-list li"), t("drawer.gap.todo", { topic: "Colas y backpressure" }));
  // no dossier yet: it says when it is written
  assert.equal(await exists(page, "[data-fold='d:dossier']"), false);
  assert.ok((await body(page)).includes(t("drawer.dossier.none")));
  // the description is a fold that stays as he left it, with the focus on its summary
  const summary = "#detail details[data-fold='d:description'] > summary";
  assert.equal(await page.eval(`${q("#detail details[data-fold='d:description']")}.open`), false);
  await page.eval(`${q(summary)}.focus()`);
  await press(page, "Enter");
  await page.waitFor(`${q("#detail details[data-fold='d:description']")}.open`);
  assert.ok((await text(page, "#detail details[data-fold='d:description'] pre")).includes("Buscamos una persona"));
  await page.eval("__app.drawer.reload()");
  await page.waitFor(`${q("#detail details[data-fold='d:description']")}.open && document.activeElement === ${q(summary)}`);
  await page.eval("__app.shell.drawer.rerender()");
  assert.equal(await page.eval(`${q("#detail details[data-fold='d:description']")}.open`), true, "still open after a redraw");
  await closeDrawer(page);
  // an application with a dossier shows it open
  await openJob(page, "demo:4");
  assert.equal(await page.eval(`${q("#detail details[data-fold='d:dossier']")}.open`), true);
  assert.ok((await text(page, "#detail details[data-fold='d:dossier'] pre")).includes("Dos entrevistadores técnicos"));
  // to review, in Aprender: a button that takes him there
  await closeDrawer(page);
  await openJob(page, "demo:3");
  await click(page, "#detail [aria-labelledby='d-gaps-h'] button.act");
  await page.waitFor(`__app.rooms.mounted() === "aprender" && !${D}.classList.contains("open")`);
  assert.deepEqual(problems(page), []);
});

test("rows are the server's words as text: hostile titles, notes, reasons, tasks, mails, dossier and links stay inert (row 1, 12)", async () => {
  for (const key of ["demo:3", "demo:12"]) {
    const page = await open({ hostile: key, shots: ["demo:15"] });
    await openJob(page, key);
    // every place the text can appear
    const html = await page.eval(`${D}.innerHTML`);
    assert.ok(!/<img[^>]*src="?x/.test(html), "no image was made from the text");
    assert.equal(await page.eval("window.pwned"), undefined);
    assert.equal(await count(page, "#detail img"), 0);
    assert.equal(await count(page, "#detail b"), 0);
    assert.equal(await count(page, "#detail a[href^='javascript' i]"), 0);
    assert.equal(await count(page, "#detail a[href]:not([href^='http' i])"), 0);
    const shown = await body(page);
    assert.ok(shown.includes('<img src=x onerror="window.pwned=1">'), "it is shown as it is");
    assert.equal(await text(page, "#detail h2"), '<img src=x onerror="window.pwned=1"> <b>x</b> — <img src=x onerror="window.pwned=1"> <b>x</b>');
    // the posting and the form have unsafe addresses: no links, and no empty "Ver el aviso"
    assert.equal(await exists(page, "#detail .d-links"), false);
    if (key === "demo:3") {
      assert.ok(shown.includes("<i>x</i>"), "a factor name from the server");
      assert.equal(await count(page, "#detail [data-email]"), 1);
      assert.equal(await count(page, "#detail [data-dtask]"), 1);
      assert.equal(await count(page, "#detail [data-dtask] a, #detail .d-actions a"), 0, "the task's javascript: link is dropped");
      assert.equal(await count(page, "#detail .d-ev .d-evnote"), 5);
    } else {
      assert.equal(await page.eval(`${q("#detail details[data-fold='d:reason']")}.open`), true, "a stuck form shows its reason");
      assert.ok((await text(page, "#detail details[data-fold='d:reason'] pre")).includes("<img"));
    }
    assert.deepEqual(problems(page), []);
  }
});

test("row 12: nothing in the drawer says null, undefined, [object Object] or NaN, for all 26 applications and for a bare one", async () => {
  const page = await open({ sparse: "demo:3" });
  const keys = Array.from({ length: 26 }, (_, i) => "demo:" + (i + 1));
  for (const key of keys) {
    await openJob(page, key);
    const shown = await body(page);
    assert.doesNotMatch(shown, STRAY, key);
    assert.ok(shown.length > 200, key + " has content");
    await closeDrawer(page);
  }
  // the bare one: no place, no pay, no history, no score detail, no text, no links
  await openJob(page, "demo:3");
  const bare = await body(page);
  assert.doesNotMatch(bare, STRAY);
  assert.ok(bare.includes(t("drawer.salary.none")) && bare.includes(t("drawer.history.none")) && bare.includes(t("drawer.score.none")));
  assert.equal(await exists(page, "#detail .d-links"), false, "no address, no link");
  assert.deepEqual(problems(page), []);
});

// ---- container behaviours that the content depends on -------------------------------------------------------------

test("row 11 and stepping: Escape closes; j and k step to the next application, which starts at its top with nothing of the last one", async () => {
  const page = await open({ shots: ["demo:4"] });
  await page.eval(`(() => {
    document.querySelectorAll("#app [data-card]").forEach((e) => e.remove());
    for (const k of ["demo:4", "demo:3", "demo:15"]) { const b = document.createElement("button"); b.dataset.card = k; b.textContent = k; document.getElementById("app").append(b); }
  })()`);
  await openJob(page, "demo:4");
  await page.waitFor(`!!${q("#shot-slot img")}`);
  const picture = await page.eval(`${q("#shot-slot img")}.src`);
  assert.equal(await text(page, "#detail .seq .num"), "1 de 3");
  await page.eval(`${D}.scrollTop = 500`);
  assert.ok(await page.eval(`${D}.scrollTop > 0`), "the drawer was scrolled");
  await typeNote(page, "no debe pasar a la otra");
  await page.eval("document.activeElement.blur()");
  await press(page, "j");
  await page.waitFor(`!!${D}.querySelector('[data-job="demo:3"]') && ${D}.scrollTop === 0`);
  assert.equal(await text(page, "#detail .seq .num"), "2 de 3");
  assert.equal(await page.eval(`${q("#mark-note")}.value`), "", "this application has its own note");
  assert.ok((await page.eval("window.__urls.revoked")).includes(picture), "the first one's picture was let go");
  await press(page, "k");
  await page.waitFor(`!!${D}.querySelector('[data-job="demo:4"]')`);
  assert.equal(await page.eval(`${q("#mark-note")}.value`), "no debe pasar a la otra", "and its own note is back");
  await press(page, "Escape");
  await page.waitFor(`!${D}.classList.contains("open")`);
  assert.deepEqual(problems(page), []);
});

test("a failed /api/job on the first opening leaves the drawer closed, and on a drawer already open leaves it as it was, with the toast: the server's words, or the old ones when it sends none", async () => {
  const page = await open({ jobFail: ["demo:5"] });
  await page.eval(`__app.drawer.open("demo:5")`);
  await page.waitFor(`document.getElementById("toast").classList.contains("show")`);
  assert.equal(await toast(page), "No se pudo abrir esa postulación (prueba).");
  assert.equal(await page.eval(`${D}.classList.contains("open")`), false);
  const plain = await open({ jobFail: ["demo:5"], jobFailText: "" });
  await openJob(plain, "demo:4");
  await plain.eval(`__app.drawer.open("demo:5")`);
  await plain.waitFor(`document.getElementById("toast").classList.contains("show")`);
  assert.equal(await toast(plain), t("err.job"));
  assert.equal(t("err.job"), "No se pudo abrir la postulación. Reintenta en un momento; si sigue igual, reinicia jobbot panel.");
  assert.equal(await plain.eval(`${D}.classList.contains("open") && __app.drawer.key`), "demo:4", "the one on screen stays (J2-7)");
  assert.deepEqual(problems(page), []);
  assert.deepEqual(problems(plain), []);
});

test("the actions row stays in reach: sticky, under the bar and above the bottom edge, on a phone and on a desktop", async () => {
  for (const [width, height] of [[390, 844], [1440, 900]]) {
    const page = await open({ shots: ["demo:15"] }, { width, height });
    await openJob(page, "demo:15");
    await page.waitFor(`!!${q("#shot-slot img")}`);
    const rect = () => page.eval(`(() => { const r = ${q("#detail .d-actions")}.getBoundingClientRect(); const bar = ${q("#detail .detail-bar")}.getBoundingClientRect(); return { top: r.top, bottom: r.bottom, barBottom: bar.bottom, h: innerHeight, pos: getComputedStyle(${q("#detail .d-actions")}).position }; })()`);
    let r = await rect();
    assert.equal(r.pos, "sticky");
    assert.ok(r.bottom <= r.h && r.top >= r.barBottom - 1, `in view at the top (${JSON.stringify(r)})`);
    for (const to of [400, 900, 1500]) {
      await page.eval(`${D}.scrollTop = ${to}`);
      await sleep(80);
      r = await rect();
      assert.ok(r.top >= r.barBottom - 1 && r.bottom <= r.h, `still in view at ${to} (${JSON.stringify(r)})`);
    }
    const target = await page.eval(`(() => { const b = ${q("#detail [data-action=approve]")}.getBoundingClientRect(); return { w: b.width, h: b.height }; })()`);
    assert.ok(target.h >= 28 && target.w >= 28, "the panel's one action button: a target of 28 px or more on a desk, as in every stop (WCAG 2.5.8 asks 24)");
    if (width < 600) assert.ok(target.h >= 44, "and 44 on a phone");
    assert.equal(await page.eval(`document.documentElement.scrollWidth <= innerWidth && ${D}.scrollWidth <= ${D}.clientWidth`), true, "no sideways scroll");
    assert.deepEqual(problems(page), []);
  }
});

test("phone widths: no sideways scroll even with long hostile text, at 360 and 390", async () => {
  for (const width of [360, 390]) {
    const page = await open({ hostile: "demo:3", shots: ["demo:15"] }, { width, height: 780 });
    for (const key of ["demo:3", "demo:15", "demo:12", "demo:4"]) {
      await openJob(page, key);
      await sleep(120);
      assert.equal(await page.eval(`${D}.scrollWidth <= ${D}.clientWidth && document.documentElement.scrollWidth <= innerWidth`), true, `${key} at ${width}`);
      await closeDrawer(page);
    }
  }
});

// ---- language, motion, focus, cost -------------------------------------------------------------------------------

test("language: every word follows the language, none is left in the other one, and the drawer is read in English too", async () => {
  const page = await open({ shots: ["demo:15"] }, { query: "debug=1&world=off&t=tok123&lang=es" });
  await openJob(page, "demo:15");
  await page.waitFor(`!!${q("#shot-slot img")}`);
  // the section titles are small mono labels that the page upper-cases, so the words are compared without regard to case
  const es = (await body(page)).toLowerCase();
  const has = (shown, word) => shown.includes(word.toLowerCase());
  assert.ok(has(es, esDetail["detail.now.approve"]) && has(es, esCommon["drawer.journey"]) && has(es, esCommon["act.approve"]));
  assert.doesNotMatch(es, STRAY);
  // the language key, with the body focused (the note field would swallow it)
  await page.eval(`document.activeElement.blur(); document.body.focus()`);
  await press(page, "l");
  await page.waitFor(`document.documentElement.lang === "en"`);
  const en = (await body(page)).toLowerCase();
  assert.ok(has(en, enDetail["detail.now.approve"]) && has(en, enCommon["drawer.journey"]) && has(en, enCommon["act.approve"]));
  assert.ok(!has(en, esDetail["detail.now.approve"]) && !has(en, esCommon["drawer.journey"]) && !has(en, esCommon["act.approve"]), "nothing is left in Spanish");
  assert.doesNotMatch(en, STRAY);
  assert.equal(await page.eval("window.__shots.length"), 1, "the language did not fetch the picture again");
  assert.match(await text(page, '#journey li[data-layer="puntaje"] .d-ldetail'), /^89 out of 100, your threshold is 60$/);
  // every situation, in English
  for (const key of ["demo:12", "demo:4", "demo:1", "demo:20", "demo:23"]) {
    await closeDrawer(page);
    await openJob(page, key);
    const shown = await body(page);
    assert.doesNotMatch(shown, STRAY, key);
    for (const k of Object.keys(esDetail)) if (esDetail[k].length > 20 && esDetail[k] !== enDetail[k] && !/\{/.test(esDetail[k])) assert.ok(!shown.includes(esDetail[k]), `${key} shows Spanish: ${k}`);
  }
  assert.deepEqual(problems(page), []);
});

test("calm motion (?motion=calm): no 'Aprobada' pause, no flash on what changed, and the drawer still works", async () => {
  const page = await open({ shots: ["demo:15"] }, { query: "debug=1&world=off&t=tok123&motion=calm" });
  await openJob(page, "demo:15");
  await page.eval(`window.__sent = false; new MutationObserver(() => { if (document.querySelector("#detail .act.sent")) window.__sent = true; }).observe(${D}, { subtree: true, attributes: true, childList: true })`);
  await twoStep(page, "#detail [data-action=approve]");
  await page.waitFor(`!${q("#detail [data-action=approve]")}`, { timeout: 5000 });
  assert.equal(await page.eval("window.__sent"), false);
  assert.equal(await page.eval(`getComputedStyle(${q("#d-now")}).animationName`), "none");
  assert.equal(await text(page, "#d-now .d-sentence"), t("detail.now.robot.approved"));
  assert.deepEqual(problems(page), []);
});

test("keyboard: the buttons are reachable in reading order and a permanent one is done with two Enter presses", async () => {
  const page = await open({});
  await openJob(page, "demo:12");
  await page.eval(`${q("#act-confirm")}.focus()`);
  assert.equal(await page.eval("document.activeElement.id"), "act-confirm");
  await press(page, "Enter");
  await page.waitFor(`${q("#act-confirm")}.textContent === ${JSON.stringify(t("act.sure"))}`);
  await sleep(420);
  await press(page, "Enter");
  await page.waitFor(`window.__calls.some((c) => c.path === "/api/confirm")`);
  await page.waitFor(`!${q("#detail #act-confirm")}`);
  assert.equal(await page.eval(`${D}.contains(document.activeElement)`), true);
  // the focus order follows the reading order: the row, then the layers' links, the note, the marks
  const order = await page.eval(`(() => { const tabbable = Array.from(${D}.querySelectorAll("a[href], button, input, summary")).filter((e) => !e.disabled); return tabbable.map((e) => e.id || e.dataset.mark || e.tagName.toLowerCase()); })()`);
  assert.ok(order.indexOf("close-detail") < order.indexOf("d-jump"), order.join());
  assert.ok(order.indexOf("d-jump") < order.indexOf("mark-note") && order.indexOf("mark-note") < order.indexOf("mark-replied"), order.join());
  assert.deepEqual(problems(page), []);
});

test("a held Enter never is the second press: the auto-repeat of a key held down does not confirm a permanent action", async () => {
  const page = await open({});
  await openJob(page, "demo:12");
  await page.eval(`${q("#act-confirm")}.focus()`);
  await press(page, "Enter"); // the first press arms the button
  await page.waitFor(`${q("#act-confirm")}.textContent === ${JSON.stringify(t("act.sure"))}`);
  await sleep(450); // longer than the pause that tells a double click from a deliberate one
  // what a held key does: keydown after keydown with autoRepeat on, and no keyup in between
  for (let i = 0; i < 4; i++) {
    await page.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13, text: "\r", autoRepeat: true });
    await sleep(40);
  }
  await page.send("Input.dispatchKeyEvent", { type: "keyUp", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 });
  assert.equal(await page.eval(`window.__calls.some((c) => c.path === "/api/confirm")`), false, "a held Enter confirmed it");
  assert.equal(await page.eval(`${q("#act-confirm")}.textContent`), t("act.sure"), "it is still waiting for the deliberate press");
  // a fresh press after letting go is the deliberate one
  await press(page, "Enter");
  await page.waitFor(`window.__calls.some((c) => c.path === "/api/confirm")`);
  assert.deepEqual(problems(page), []);
});

test("cost: a redraw is cheap, leaves no nodes, URLs or listeners behind, and a refresh never redraws the drawer", async () => {
  const page = await open({ shots: ["demo:15"] });
  await openJob(page, "demo:15");
  await page.waitFor(`!!${q("#shot-slot img")}`);
  const nodes = await page.eval(`${D}.querySelectorAll("*").length`);
  const urls = await page.eval("window.__urls.created.length");
  const ms = await page.eval(`(() => { const d = __app.shell.drawer; const t0 = performance.now(); for (let i = 0; i < 100; i++) d.rerender(); return (performance.now() - t0) / 100; })()`);
  assert.equal(await page.eval(`${D}.querySelectorAll("*").length`), nodes, "the same number of nodes after 100 redraws");
  assert.equal(await page.eval("window.__urls.created.length"), urls, "no new blob URL");
  assert.equal(await page.eval("window.__shots.length"), 1);
  report.redrawMs = Math.round(ms * 100) / 100;
  assert.ok(ms < 12, "a redraw of the drawer takes " + ms + " ms");
  // a refresh redraws the room behind and never the drawer
  await page.eval(`window.__keep = ${q("#d-now")}`);
  await page.eval("__app.session.refresh({ force: true })");
  await sleep(150);
  assert.equal(await page.eval(`window.__keep === ${q("#d-now")}`), true);
  assert.deepEqual(problems(page), []);
});

test("with the world on and sound: a whole visit to the drawer (open, approve, mark, step, close) raises no console error and no CSP violation", async () => {
  const page = await open({ shots: ["demo:15", "demo:16"] }, { query: "debug=1&t=tok123" });
  await openJob(page, "demo:15");
  await page.waitFor(`!!${q("#shot-slot img")}`);
  await twoStep(page, "#detail [data-action=approve]");
  await page.waitFor(`!${q("#detail [data-action=approve]")}`, { timeout: 5000 });
  await closeDrawer(page);
  await openJob(page, "demo:4");
  await typeNote(page, "con el mundo encendido");
  await click(page, "#detail [data-mark=offer]");
  await sleep(420);
  await click(page, "#mark-yes");
  await page.waitFor(`${q("#detail .d-meta .chip")}.textContent === ${JSON.stringify(t("state.offer"))}`);
  await click(page, "#mark-undo");
  await page.waitFor(`${q("#detail .d-meta .chip")}.textContent === ${JSON.stringify(t("state.interview"))}`);
  await page.eval(`__app.drawer.open("demo:12")`);
  await page.waitFor(`!!${D}.querySelector('[data-job="demo:12"]')`);
  await closeDrawer(page);
  await sleep(500);
  assert.deepEqual(problems(page), []);
});
