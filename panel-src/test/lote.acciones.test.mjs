// Acceptance test of the encargo docs/encargos/lote-acciones-que-se-entienden.md: what a press does, and what the screen says about it. (1) The engine's
// switch has one rule in COLA and in SISTEMA: stopping goes at once with "Deshacer" in its toast, resuming asks twice; (2) resolve, which the server
// cannot take back, asks twice wherever it is offered (HOY, COLA, APRENDER and the drawer); (3) the help says how the page is used before its keys, and
// a phone gets no keys, neither in the help nor over HOY's rows; (4) SISTEMA says the Claude spend once, and HOY's foot names STOP only when it is on.
// No browser: the actions get a fake session, a fake clock and fake timers; the views and the help's model are pure.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { setLang, setTimeZone, t } from "../src/i18n/index.js";
import { model, PERMANENT_ACTIONS } from "../src/core/model.js";
import { ARM_MIN_MS } from "../src/core/dom.js";
import { createColaActions } from "../src/moments/cola/actions.js";
import { createActions as sistemaActions } from "../src/moments/sistema/actions.js";
import { createActions as hoyActions } from "../src/moments/hoy/actions.js";
import { createActions as aprenderActions } from "../src/moments/aprender/actions.js";
import { hoyView } from "../src/moments/hoy/view.js";
import { colaView } from "../src/moments/cola/view.js";
import { aprenderView } from "../src/moments/aprender/view.js";
import { sistemaView } from "../src/moments/sistema/view.js";
import * as help from "../src/ui/help.js";
import { liveSet } from "../src/ui/wired.js";
import { snapshotOf } from "./helpers/demo-host.mjs";
import { scenario, SCENARIOS } from "./helpers/sistema-scenarios.mjs";

const fresh = (lang = "es") => { setLang(lang); setTimeZone("America/Santiago"); };
const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/snapshot.${name}.json`, import.meta.url), "utf8"));

// One fake for the four actions modules: a session whose `act` waits until the test answers, a clock that moves only when told, timers that never fire alone.
function rig(create) {
  const log = { requests: [], toasts: [] };
  let clock = 1000;
  const gates = [];
  let next = 1;
  const session = {
    said: () => "",
    act(path, body, opts = {}) {
      log.requests.push(path);
      return new Promise((resolve) => gates.push(async (r) => {
        if (r.ok && opts.onOk) opts.onOk(r);
        if (opts.beforeRefresh) await opts.beforeRefresh(r);
        resolve(r);
      }));
    },
  };
  const actions = create({
    session, sound: { play() {} }, announce() {}, t: (key) => key, open() {}, openUrl() {}, go() {}, onChange() {},
    toast: (text, ok, extra) => log.toasts.push({ text, ok, undo: extra && extra.undo }),
    now: () => clock, timers: { set: () => next++, clear() {} }, held: () => false,
  });
  return {
    actions, log,
    later: () => { clock += ARM_MIN_MS + 50; },
    // a press is never awaited before the test has looked at what it sent: a press that sends waits for an answer, and a wrong fix must fail, not hang
    settle: () => new Promise((r) => setImmediate(r)),
    answer: async (res = { ok: true, status: 200, data: {} }) => {
      const gate = gates.shift();
      if (!gate) return false;                                     // nothing was out: a trace says so instead of throwing
      await gate(res);
      await new Promise((r) => setImmediate(r));
      return true;
    },
  };
}
const REFUSED = { ok: false, status: 500, data: {} };
const colaPress = (r, stopped) => r.actions.engine(stopped);
const sistemaPress = (r, stopped) => r.actions.run({ on: stopped });
const SWITCHES = [["COLA", createColaActions, colaPress], ["SISTEMA", sistemaActions, sistemaPress]];

// ---- (1) the engine's switch: one rule in both stops

test("COLA's engine switch keeps SISTEMA's rule: one press stops the engine and its toast offers Deshacer; resuming asks twice", async () => {
  // what each stop answers, direction by direction, from the same presses
  for (const [name, create, press] of SWITCHES) {
    const stop = rig(create);
    const first = press(stop, false);
    assert.deepEqual(stop.log.requests, ["/api/stop"], `${name}: the first press on a running engine stops it`);
    await stop.answer();
    assert.equal(await first, "fired", name);
    assert.equal(stop.log.toasts.length, 1, `${name}: stopping leaves a toast`);
    assert.equal(stop.log.toasts[0].text, "sistema.stop.done", `${name}: with SISTEMA's sentence`);
    assert.equal(typeof stop.log.toasts[0].undo, "function", `${name}: and its Deshacer`);
    stop.log.toasts[0].undo();
    assert.deepEqual(stop.log.requests, ["/api/stop", "/api/resume"], `${name}: Deshacer is the way back`);

    const resume = rig(create);
    const arming = press(resume, true);
    await resume.settle();
    assert.deepEqual(resume.log.requests, [], `${name}: nothing is sent on the first press to resume`);
    assert.equal(await arming, "armed", `${name}: the first press on a stopped engine only arms`);
    resume.later();
    const second = press(resume, true);
    assert.deepEqual(resume.log.requests, ["/api/resume"], `${name}: the second deliberate press resumes`);
    await resume.answer();
    assert.equal(await second, "fired", name);
    assert.equal(resume.log.toasts.length, 1, `${name}: resuming leaves a toast too`);
    assert.equal(resume.log.toasts[0].text, "sistema.resume.done", `${name}: with SISTEMA's sentence`);
    assert.equal(typeof resume.log.toasts[0].undo, "function", `${name}: and its Deshacer`);
    resume.log.toasts[0].undo();
    assert.deepEqual(resume.log.requests, ["/api/resume", "/api/stop"], `${name}: whose way back is to stop again`);
  }
});

test("a stop or a resume the server refuses leaves no toast and no Deshacer, in COLA and in SISTEMA", async () => {
  for (const [name, create, press] of SWITCHES) {
    const stop = rig(create);
    const first = press(stop, false);
    assert.deepEqual(stop.log.requests, ["/api/stop"], `${name}: the first press on a running engine sends the stop`);
    await stop.answer(REFUSED);
    await first;
    assert.deepEqual(stop.log.toasts, [], `${name}: a refused stop says nothing was stopped: no toast, no Deshacer`);

    const resume = rig(create);
    press(resume, true);
    await resume.settle();
    resume.later();
    const second = press(resume, true);
    assert.deepEqual(resume.log.requests, ["/api/resume"], `${name}: the second press sends the resume`);
    await resume.answer(REFUSED);
    await second;
    assert.deepEqual(resume.log.toasts, [], `${name}: a refused resume leaves no toast either`);
  }
});

test("a second press while the switch's request is out is ignored and sends nothing more, in COLA and in SISTEMA", async () => {
  for (const [name, create, press] of SWITCHES) {
    const r = rig(create);
    const first = press(r, false);
    assert.deepEqual(r.log.requests, ["/api/stop"], `${name}: the first press sends the stop`);
    const again = press(r, false);
    await r.settle();
    assert.equal(r.log.requests.length, 1, `${name}: a second press before the answer sends nothing: ${r.log.requests.join(", ")}`);
    assert.equal(await again, "ignored", `${name}: it is ignored`);
    await r.answer();
    await first;
  }
});

// The same switch in two stops, compared by its traces: COLA's `p` and SISTEMA's `p` go through the same scripted presses, Deshacer clicks and
// server answers (yes, a refusal, or an answer that comes late, after other presses), and each step is written down as one line: what was sent,
// which toasts came (their sentence, and whether they offer Deshacer), whether the switch waits for the server or is armed, and what the press
// or the Deshacer returned once everything was answered. The two traces must be the same line for line, and SISTEMA's must be the one below:
// the rule SISTEMA already has on main, written out, so that neither stop can drift from it and both cannot drift together.
const SCRIPTS = {
  "stop, then its Deshacer": [["p", false], ["ok"], ["undo", 0], ["ok"]],
  "a p while the Deshacer is out, with either photo of the switch, and a second Deshacer": [["p", false], ["ok"], ["undo", 0], ["p", true], ["p", false], ["undo", 0], ["ok"]],
  "resume: armed, a press too soon, then the deliberate one": [["p", true], ["p", true], ["later"], ["p", true], ["ok"]],
  "a refused stop, then a stop that goes": [["p", false], ["no"], ["p", false], ["ok"]],
  "a double p while the stop is out": [["p", false], ["p", false], ["p", true], ["ok"]],
  "the Deshacer of the Deshacer": [["p", false], ["ok"], ["undo", 0], ["ok"], ["undo", 1], ["ok"]],
  "a refused Deshacer, then the same Deshacer again": [["p", false], ["ok"], ["undo", 0], ["no"], ["undo", 0], ["ok"]],
  "armed to resume, then a press on a running engine": [["p", true], ["p", false], ["ok"]],
  "a refused resume": [["p", true], ["later"], ["p", true], ["no"]],
};
const tick = () => new Promise((r) => setImmediate(r));
const shape = (v) => (typeof v === "string" ? v : v && typeof v === "object" ? "an object" : String(v));

async function switchTrace(stop, script) {
  const r = rig(stop === "COLA" ? createColaActions : sistemaActions);
  const press = stop === "COLA" ? (photo) => r.actions.engine(photo) : (photo) => r.actions.run({ on: photo });
  const waiting = stop === "COLA" ? () => r.actions.isPending("engine") : () => r.actions.state().pending;
  const armed = stop === "COLA" ? () => r.actions.engineArmed() : () => r.actions.state().armed;
  const lines = [];
  const returned = [];
  let sent = 0;
  let toasts = 0;
  for (const step of script) {
    let line = step.join(" ");
    let ret;
    if (step[0] === "p") ret = press(step[1]);
    else if (step[0] === "undo") {
      const toast = r.log.toasts[step[1]];
      ret = toast && typeof toast.undo === "function" ? toast.undo() : "no Deshacer to click";
    } else if (step[0] === "later") r.later();
    else if (!(await r.answer(step[0] === "ok" ? undefined : REFUSED))) line += " (nothing was out)";
    await tick();
    await tick();
    const newSent = r.log.requests.slice(sent);
    const newToasts = r.log.toasts.slice(toasts).map((x) => x.text + (typeof x.undo === "function" ? " + Deshacer" : ""));
    sent = r.log.requests.length;
    toasts = r.log.toasts.length;
    line += ` | sent [${newSent.join(", ")}] | toasts [${newToasts.join(", ")}]${waiting() ? " | waiting" : ""}${armed() ? " | armed" : ""}`;
    lines.push(line);
    returned.push(ret);
  }
  // what each press and each Deshacer returned, once every answer of the script is in (a promise still open says so)
  for (const [i, ret] of returned.entries()) {
    if (ret === undefined) continue;
    const value = ret && typeof ret.then === "function" ? await Promise.race([ret.then(shape), tick().then(tick).then(() => "still waiting")]) : shape(ret);
    lines[i] += ` | returns ${value}`;
  }
  return lines;
}

const SISTEMA_TRACES = {
  "stop, then its Deshacer": [
    "p false | sent [/api/stop] | toasts [] | waiting | returns fired",
    "ok | sent [] | toasts [sistema.stop.done + Deshacer]",
    "undo 0 | sent [/api/resume] | toasts [] | waiting | returns fired",
    "ok | sent [] | toasts [sistema.resume.done + Deshacer]",
  ],
  "a p while the Deshacer is out, with either photo of the switch, and a second Deshacer": [
    "p false | sent [/api/stop] | toasts [] | waiting | returns fired",
    "ok | sent [] | toasts [sistema.stop.done + Deshacer]",
    "undo 0 | sent [/api/resume] | toasts [] | waiting | returns fired",
    "p true | sent [] | toasts [] | waiting | returns ignored",
    "p false | sent [] | toasts [] | waiting | returns ignored",
    "undo 0 | sent [] | toasts [] | waiting | returns ignored",
    "ok | sent [] | toasts [sistema.resume.done + Deshacer]",
  ],
  "resume: armed, a press too soon, then the deliberate one": [
    "p true | sent [] | toasts [] | armed | returns armed",
    "p true | sent [] | toasts [] | armed | returns ignored",
    "later | sent [] | toasts [] | armed",
    "p true | sent [/api/resume] | toasts [] | waiting | returns fired",
    "ok | sent [] | toasts [sistema.resume.done + Deshacer]",
  ],
  "a refused stop, then a stop that goes": [
    "p false | sent [/api/stop] | toasts [] | waiting | returns fired",
    "no | sent [] | toasts []",
    "p false | sent [/api/stop] | toasts [] | waiting | returns fired",
    "ok | sent [] | toasts [sistema.stop.done + Deshacer]",
  ],
  "a double p while the stop is out": [
    "p false | sent [/api/stop] | toasts [] | waiting | returns fired",
    "p false | sent [] | toasts [] | waiting | returns ignored",
    "p true | sent [] | toasts [] | waiting | returns ignored",
    "ok | sent [] | toasts [sistema.stop.done + Deshacer]",
  ],
  "the Deshacer of the Deshacer": [
    "p false | sent [/api/stop] | toasts [] | waiting | returns fired",
    "ok | sent [] | toasts [sistema.stop.done + Deshacer]",
    "undo 0 | sent [/api/resume] | toasts [] | waiting | returns fired",
    "ok | sent [] | toasts [sistema.resume.done + Deshacer]",
    "undo 1 | sent [/api/stop] | toasts [] | waiting | returns fired",
    "ok | sent [] | toasts [sistema.stop.done + Deshacer]",
  ],
  "a refused Deshacer, then the same Deshacer again": [
    "p false | sent [/api/stop] | toasts [] | waiting | returns fired",
    "ok | sent [] | toasts [sistema.stop.done + Deshacer]",
    "undo 0 | sent [/api/resume] | toasts [] | waiting | returns fired",
    "no | sent [] | toasts []",
    "undo 0 | sent [/api/resume] | toasts [] | waiting | returns fired",
    "ok | sent [] | toasts [sistema.resume.done + Deshacer]",
  ],
  "armed to resume, then a press on a running engine": [
    "p true | sent [] | toasts [] | armed | returns armed",
    "p false | sent [/api/stop] | toasts [] | waiting | returns fired",
    "ok | sent [] | toasts [sistema.stop.done + Deshacer]",
  ],
  "a refused resume": [
    "p true | sent [] | toasts [] | armed | returns armed",
    "later | sent [] | toasts [] | armed",
    "p true | sent [/api/resume] | toasts [] | waiting | returns fired",
    "no | sent [] | toasts []",
  ],
};

test("COLA's p is SISTEMA's switch: the same presses, Deshacer clicks and answers give the same trace in both, and it is the rule SISTEMA has today", async () => {
  const sistema = {};
  for (const [name, script] of Object.entries(SCRIPTS)) sistema[name] = await switchTrace("SISTEMA", script);
  for (const [name, script] of Object.entries(SCRIPTS)) {
    assert.deepEqual(await switchTrace("COLA", script), sistema[name], `${name}: COLA's trace is SISTEMA's`);
  }
  assert.deepEqual(sistema, SISTEMA_TRACES, "SISTEMA's switch keeps the rule it has on main");
});

test("the switch's routes are written once: one module of src/ names /api/stop and /api/resume", () => {
  const SRC = new URL("../src/", import.meta.url);
  const files = readdirSync(SRC, { recursive: true }).map((f) => String(f).replace(/\\/g, "/")).filter((f) => f.endsWith(".js") && !f.startsWith("i18n/"));
  for (const route of ["/api/stop", "/api/resume"]) {
    const naming = files.filter((f) => readFileSync(new URL(f, SRC), "utf8").includes(route));
    assert.equal(naming.length, 1, `one module names ${route}, and the two stops take it from there: ${naming.join(", ")}`);
  }
});

// ---- (2) resolve asks twice everywhere

test("resolve, which the server cannot take back, asks twice wherever it is offered: HOY, COLA, APRENDER and the drawer", async () => {
  fresh("es");
  const snap = await snapshotOf({ lang: "es" });
  const nowMs = Date.parse(snap.generated_at);
  const m = model(snap);
  const offers = (r) => r && Array.isArray(r.actions) && r.actions.some((a) => a.id === "resolve");
  const hoy = hoyView(m, { nowMs });
  const cola = colaView(m, { nowMs });
  const aprender = aprenderView(m, { nowMs });
  const rows = [
    ["HOY", hoyActions, [hoy.next, ...hoy.rows].find(offers)],
    ["COLA", createColaActions, cola.queue.groups.flatMap((g) => g.rows).find(offers)],
    ["APRENDER", aprenderActions, aprender.missing.rows.find(offers)],
  ];
  for (const [name, create, row] of rows) {
    assert.ok(row, `${name}: the demo has a row that offers resolve`);
    const r = rig(create);
    const arming = r.actions.run("resolve", row);
    await r.settle();
    assert.deepEqual(r.log.requests, [], `${name}: the first press sends nothing`);
    assert.equal(await arming, "armed", `${name}: it arms`);
    r.later();
    const second = r.actions.run("resolve", row);
    assert.deepEqual(r.log.requests, ["/api/resolve"], `${name}: the second deliberate press sends it`);
    await r.answer();
    await second;
  }
  // the drawer of an application draws its server verbs with twoStep when they are in this list (ui/detail.js)
  assert.ok(PERMANENT_ACTIONS.includes("resolve"), "the drawer's verbs that ask twice include resolve");
});

// The same two presses in three stops, compared by their traces, like the switch: each stop's first resolve row goes through the same presses
// (a second one too soon, the deliberate one, one while the server is answering, a refusal) and the three traces must be APRENDER's, which
// asks twice on main.
const RESOLVE_SCRIPTS = {
  "armed, a press too soon, then the deliberate one": [["p"], ["p"], ["later"], ["p"], ["ok"]],
  "a press while the server answers": [["p"], ["later"], ["p"], ["p"], ["ok"]],
  "refused, then asked twice again": [["p"], ["later"], ["p"], ["no"], ["p"], ["later"], ["p"], ["ok"]],
};
const RESOLVE_TRACE = {
  "armed, a press too soon, then the deliberate one": [
    "p | sent [] | returns armed",
    "p | sent [] | returns ignored",
    "later | sent []",
    "p | sent [/api/resolve] | returns fired",
    "ok | sent []",
  ],
  "a press while the server answers": [
    "p | sent [] | returns armed",
    "later | sent []",
    "p | sent [/api/resolve] | returns fired",
    "p | sent [] | returns ignored",
    "ok | sent []",
  ],
  "refused, then asked twice again": [
    "p | sent [] | returns armed",
    "later | sent []",
    "p | sent [/api/resolve] | returns fired",
    "no | sent []",
    "p | sent [] | returns armed",
    "later | sent []",
    "p | sent [/api/resolve] | returns fired",
    "ok | sent []",
  ],
};

async function resolveTrace(create, row, script) {
  const r = rig(create);
  const lines = [];
  const returned = [];
  let sent = 0;
  for (const step of script) {
    let line = step[0];
    let ret;
    if (step[0] === "p") ret = r.actions.run("resolve", row);
    else if (step[0] === "later") r.later();
    else if (!(await r.answer(step[0] === "ok" ? undefined : REFUSED))) line += " (nothing was out)";
    await tick();
    await tick();
    line += ` | sent [${r.log.requests.slice(sent).join(", ")}]`;
    sent = r.log.requests.length;
    lines.push(line);
    returned.push(ret);
  }
  for (const [i, ret] of returned.entries()) {
    if (ret === undefined) continue;
    const value = ret && typeof ret.then === "function" ? await Promise.race([ret.then(shape), tick().then(tick).then(() => "still waiting")]) : shape(ret);
    lines[i] += ` | returns ${value}`;
  }
  return lines;
}

test("resolve answers the same presses the same way in HOY, COLA and APRENDER: the trace APRENDER has today", async () => {
  fresh("es");
  const snap = await snapshotOf({ lang: "es" });
  const nowMs = Date.parse(snap.generated_at);
  const m = model(snap);
  const offers = (x) => x && Array.isArray(x.actions) && x.actions.some((a) => a.id === "resolve");
  const hoy = hoyView(m, { nowMs });
  const stops = [
    ["APRENDER", aprenderActions, aprenderView(m, { nowMs }).missing.rows.find(offers)],
    ["HOY", hoyActions, [hoy.next, ...hoy.rows].find(offers)],
    ["COLA", createColaActions, colaView(m, { nowMs }).queue.groups.flatMap((g) => g.rows).find(offers)],
  ];
  const traces = {};
  for (const [name, script] of Object.entries(RESOLVE_SCRIPTS)) {
    traces[name] = {};
    for (const [stop, create, row] of stops) {
      assert.ok(row, `${stop}: the demo has a row that offers resolve`);
      traces[name][stop] = await resolveTrace(create, row, script);
    }
  }
  for (const [name, byStop] of Object.entries(traces)) {
    for (const stop of ["HOY", "COLA"]) assert.deepEqual(byStop[stop], byStop.APRENDER, `${name}: ${stop}'s trace is APRENDER's`);
  }
  assert.deepEqual(Object.fromEntries(Object.entries(traces).map(([k, v]) => [k, v.APRENDER])), RESOLVE_TRACE, "APRENDER keeps the two presses it has on main");
  fresh();
});

test("HOY asks twice for what PERMANENT_ACTIONS lists, the drawer's own list, and keeps no copy of it", async () => {
  fresh("es");
  const snap = await snapshotOf({ lang: "es" });
  const nowMs = Date.parse(snap.generated_at);
  const m = model(snap);
  // a verb added to the shared list must ask twice in HOY too: the view is loaded again (a fresh module) after the list grew
  PERMANENT_ACTIONS.push("snooze");
  try {
    const { hoyView: grown } = await import(`../src/moments/hoy/view.js?permanent=${Date.now()}`);
    const v = grown(m, { nowMs });
    const snoozes = [v.next, ...v.rows].filter(Boolean).flatMap((r) => (r.actions || []).filter((a) => a.id === "snooze"));
    assert.ok(snoozes.length > 0, "the demo's HOY offers snooze somewhere");
    assert.deepEqual(snoozes.filter((a) => !a.permanent).length, 0, "HOY marks every snooze as asking twice once PERMANENT_ACTIONS lists it");
  } finally {
    PERMANENT_ACTIONS.pop();
  }
  fresh();
});

// ---- (3) the help teaches the use, and a phone has no keys

const ROW = { es: /\bfila\b/i, en: /\brow\b/i };
const KEY_WORDS = { es: /\b(?:teclas?|atajos?|letras?)\b/i, en: /\b(?:keys?|shortcuts?|letters?)\b/i };
const CLICK = /\bclic\b|\bclick/i;
const TITLE = { es: /Cómo se usa/, en: /How it works/ };
const TOUCH = { es: /\btoque\b/i, en: /\btap\b/i };
const KEY_NAME = /^[\w-]+(?:\.[\w-]+)+$/;                          // what t() gives back for a key it does not have

test("the help says how the page is used before its keys, in both languages; on a phone it says it by touch and lists no key", () => {
  assert.equal(typeof help.helpModel, "function", "ui/help.js exports helpModel(stopId, { live, flat }), the sheet as data");
  for (const lang of ["es", "en"]) {
    fresh(lang);
    const desk = help.helpModel("hoy", { live: liveSet("hoy") });
    assert.equal(desk.title, t("keys.title"), `${lang}: the sheet's title is keys.title`);
    assert.match(desk.title, TITLE[lang], `${lang}: and it says how the page is used: "${desk.title}"`);
    assert.equal(desk.usage.length, 3, `${lang}: three lines of use`);
    assert.match(desk.usage[0], ROW[lang], `${lang}: the first one is about the row: "${desk.usage[0]}"`);
    assert.match(desk.usage[0], CLICK, `${lang}: and what a click on it does: "${desk.usage[0]}"`);
    for (const line of desk.usage) assert.doesNotMatch(line, KEY_NAME, `${lang}: a line of use is words, not a key: "${line}"`);
    assert.ok(desk.groups.length > 0 && desk.groups.every((g) => g.rows.length > 0), `${lang}: then the keys, by group`);
    assert.equal(desk.ask, t("stop.hoy.ask"), `${lang}: the sheet says the stop's question`);
    assert.equal(help.helpModel("cola", { live: liveSet("cola") }).ask, t("stop.cola.ask"), `${lang}: each stop its own`);
    // the keys are still the table of keys.js for that stop, filtered by what the stop answers, the page first and the stop after
    for (const id of ["hoy", "procesos", "cola", "aprender", "sistema"]) {
      const sheet = help.helpModel(id, { live: liveSet(id) });
      const table = help.helpRows(id, liveSet(id));
      const listed = sheet.groups.map((g) => g.rows.map((r) => r.key));
      const wanted = [table.filter((r) => r.scope === "global"), table.filter((r) => r.scope !== "global")].filter((g) => g.length).map((g) => g.map((r) => r.display));
      assert.deepEqual(listed, wanted, `${lang}, ${id}: the keys are helpRows(${id}, liveSet(${id}))`);
    }

    const phone = help.helpModel("hoy", { live: liveSet("hoy"), flat: true });
    assert.equal(phone.title, desk.title, `${lang}: a phone's sheet has the same title`);
    assert.deepEqual(phone.groups, [], `${lang}: a phone gets no keys`);
    assert.equal(phone.usage.length, 2, `${lang}: two lines of use: the row and the buttons, no line about keys`);
    assert.match(phone.usage[0], ROW[lang], `${lang}: by the row`);
    assert.match(phone.usage[1], TOUCH[lang], `${lang}: and what cannot be undone asks for a second touch: "${phone.usage[1]}"`);
    for (const line of phone.usage) {
      assert.doesNotMatch(line, KEY_NAME, `${lang}: "${line}"`);
      assert.doesNotMatch(line, KEY_WORDS[lang], `${lang}: a phone's help names no key: "${line}"`);
      assert.doesNotMatch(line, CLICK, `${lang}: nor a click: "${line}"`);
    }
  }
  fresh();
});

test("HOY's hint over the rows names no key on a phone and says what a click does on a desk, in both languages", () => {
  for (const [lang, name] of ["es", "en"].flatMap((l) => ["rich", "empty", "heavy"].map((n) => [l, n]))) {
    fresh(lang);
    const snap = fixture(name);
    const m = model(snap);
    const nowMs = Date.parse(snap.generated_at);
    const phone = hoyView(m, { nowMs, flat: true }).restHint;
    const desk = hoyView(m, { nowMs }).restHint;
    for (const [where, hint] of [["phone", phone], ["desk", desk]]) {
      assert.equal(typeof hint, "string", `${lang}, ${name}, ${where}: the view says the hint (restHint)`);
      assert.doesNotMatch(hint, KEY_NAME, `${lang}, ${where}: "${hint}"`);
      assert.match(hint, ROW[lang], `${lang}, ${where}: it is about the row: "${hint}"`);
    }
    assert.doesNotMatch(phone, KEY_WORDS[lang], `${lang}: no key on a phone: "${phone}"`);
    assert.doesNotMatch(phone, CLICK, `${lang}: and no click: "${phone}"`);
    assert.match(desk, CLICK, `${lang}: a desk is told what a click does: "${desk}"`);
  }
  fresh();
});

test("the hint's words are chosen in one place, HOY's view: no other module of src/ names hoy.rest.hint", () => {
  // the render paints view.restHint; a render that picked the key itself would draw the desk's hint on a phone
  const SRC = new URL("../src/", import.meta.url);
  const files = readdirSync(SRC, { recursive: true }).map((f) => String(f).replace(/\\/g, "/")).filter((f) => f.endsWith(".js") && !f.startsWith("i18n/"));
  const naming = files.filter((f) => readFileSync(new URL(f, SRC), "utf8").includes("hoy.rest.hint"));
  assert.deepEqual(naming, ["moments/hoy/view.js"], `only HOY's view names the hint's keys: ${naming.join(", ")}`);
});

// ---- (4) once per screen

test("SISTEMA says the Claude spend once: in the account's row, with its meter, and not again as a figure", () => {
  for (const lang of ["es", "en"]) {
    fresh(lang);
    let withSpend = 0;
    for (const name of Object.keys(SCENARIOS)) {
      const s = scenario(name);
      const v = sistemaView(model(s), { nowMs: Date.parse(s.generated_at) });
      assert.deepEqual(v.figures.filter((f) => f.id === "spend"), [], `${lang}, ${name}: no figure is the spend`);
      if (!(v.spend && v.spend.spent)) continue;
      withSpend++;
      assert.ok(v.spend.text.includes(v.spend.spent), `${lang}, ${name}: the account's row says it: "${v.spend.text}"`);
      const said = v.figures.filter((f) => String(f.value).includes(v.spend.spent) || String(f.label).includes(v.spend.spent));
      assert.deepEqual(said, [], `${lang}, ${name}: and no figure says it again`);
    }
    assert.ok(withSpend >= 2, `${lang}: at least two scenarios have a spend (${withSpend})`);
  }
  fresh();
});

test("HOY's foot names STOP only when it is on: off is the normal state and says nothing", () => {
  for (const [lang, name] of ["es", "en"].flatMap((l) => ["rich", "empty", "heavy"].map((n) => [l, n]))) {
    fresh(lang);
    const snap = fixture(name);
    snap.controls = { ...(snap.controls || {}), stopped: false };
    const off = hoyView(model(snap), { nowMs: Date.parse(snap.generated_at) }).foot;
    assert.ok(off.length > 0, `${lang}: the foot still says the window and the run`);
    assert.deepEqual(off.filter((p) => /\bSTOP\b/.test(p)), [], `${lang}, ${name}: STOP off is not said: ${off.join(" · ")}`);
    snap.controls.stopped = true;
    const on = hoyView(model(snap), { nowMs: Date.parse(snap.generated_at) }).foot;
    assert.ok(on.includes(t("hoy.foot.stopOn")), `${lang}, ${name}: STOP on is said: ${on.join(" · ")}`);
    // a switch the snapshot does not report is not on either
    for (const [what, edit] of [["stopped: null", (s) => { s.controls.stopped = null; }], ["no controls", (s) => { delete s.controls; }]]) {
      const s = fixture(name);
      s.controls = s.controls || {};
      edit(s);
      const foot = hoyView(model(s), { nowMs: Date.parse(s.generated_at) }).foot;
      assert.deepEqual(foot.filter((p) => /\bSTOP\b/.test(p)), [], `${lang}, ${what}: STOP is not said: ${foot.join(" · ")}`);
    }
  }
  fresh();
});
