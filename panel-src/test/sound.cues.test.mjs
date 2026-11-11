// The sound vocabulary (src/sound/cues.js): the table is coherent, every cue is synthesised and measured through the same master chain the
// ears get (peak, clicks, tail, duration, spectrum), the background bus is quieter than the actions bus, and no two cues are the same sound.
// The data checks run in Node; the measurements render every cue offline in Chrome (the sound lab, tools/cdp.mjs), which starts with
// --mute-audio and never plays anything: an OfflineAudioContext has no speakers.
import test, { before, after } from "node:test";
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { launch } from "../tools/cdp.mjs";
import { CUES, CUE_BY_ID, BUSES, GROUPS, ALIASES, cueOf } from "../src/sound/cues.js";
import { RECIPES, META } from "../src/sound/recipes.js";
import { GESTURES, GESTURE_META } from "../src/sound/gestures.js";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const PORT = 8793;
const LAB = `http://127.0.0.1:${PORT}/dist/lab-sound.html`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ---- the table ------------------------------------------------------------------------------------------------------------------------

test("the table is coherent: unique ids, real buses and groups, a recipe that exists, a duration budget by class", () => {
  const ids = CUES.map((c) => c.id);
  assert.equal(new Set(ids).size, ids.length, "an id is repeated");
  assert.ok(CUES.length >= 30 && CUES.length <= 35, `${CUES.length} cues: the sheet gives each one a key from 1-9 and a-z`);
  for (const cue of CUES) {
    assert.ok(Object.hasOwn(BUSES, cue.bus), `${cue.id}: bus ${cue.bus}`);
    assert.ok(GROUPS[cue.bus].includes(cue.group), `${cue.id}: group ${cue.group} is not one of the ${cue.bus} groups`);
    assert.ok(Object.hasOwn(RECIPES, cue.recipe) || Object.hasOwn(GESTURES, cue.recipe), `${cue.id}: no recipe called ${cue.recipe}`);
    assert.ok(Number.isFinite(cue.max) && cue.max > 0, `${cue.id} has no duration budget`);
    if (cue.bus === "background") assert.ok(cue.max < 0.9 + 1e-9, `${cue.id}: a background sound is under 900 ms`);
    else assert.ok(cue.max <= (cue.id.startsWith("arrive.") ? 0.42 : 0.35), `${cue.id}: a micro-interaction is under 350 ms (arrivals keep the engine's 420)`);
    assert.ok(Object.isFrozen(cue), `${cue.id} is not frozen`);
  }
  for (const [bus, groups] of Object.entries(GROUPS)) {
    for (const g of groups) assert.ok(CUES.some((c) => c.bus === bus && c.group === g), `the ${bus} group ${g} has no cue`);
  }
  assert.deepEqual(new Set(Object.keys(GESTURES)), new Set(Object.keys(GESTURE_META)), "every gesture has its priority and level");
  for (const name of Object.keys(GESTURES)) assert.ok(!Object.hasOwn(RECIPES, name) && !Object.hasOwn(META, name), `${name} would shadow a recipe of the engine`);
});

test("the background bus is quieter than the actions bus, limited to a sound a minute, with the error as its only exception", () => {
  assert.ok(BUSES.background.gain < BUSES.actions.gain);
  assert.equal(BUSES.background.minGapMs, 60000);
  assert.deepEqual([...BUSES.background.exempt], ["bg.error"]);
  assert.equal(BUSES.actions.minGapMs, undefined, "his own actions have no rate limit but the engine's budget");
  for (const cue of CUES.filter((c) => c.bus === "background")) assert.ok(cue.id.startsWith("bg."), cue.id);
  for (const cue of CUES.filter((c) => c.bus === "actions")) assert.ok(!cue.id.startsWith("bg."), cue.id);
});

test("a call means one cue: rooms for arrive, the old names as aliases, and anything else is no sound", () => {
  assert.equal(cueOf("arrive", { room: "cola" }).id, "arrive.cola");
  assert.equal(cueOf("arrive", { floor: 5 }), null, "the tower's floors are not rooms any more");
  assert.equal(cueOf("arrive", {}), null);
  assert.equal(cueOf("arrive", { room: "prologo" }), null, "the prologue has no motif");
  assert.equal(cueOf("tick").id, "step");
  assert.equal(cueOf("press").id, "confirm");
  assert.equal(cueOf("toggle").id, "switch");
  assert.equal(cueOf("release"), null, "release is silent on purpose: the result's own cue follows");
  assert.equal(cueOf("approve").id, "approve");
  assert.equal(cueOf("bg.reply").id, "bg.reply");
  for (const bad of ["nope", "toString", "__proto__", "constructor", "", undefined, null, 5, "turn", "late", "glance"]) assert.equal(cueOf(bad), null, String(bad));
  assert.deepEqual(Object.keys(ALIASES).sort(), ["ceremony", "press", "release", "tick", "toggle"]);
});

// ---- the measurements -----------------------------------------------------------------------------------------------------------------

let server;
let browser;
let page;
let all = [];

before(async () => {
  execFileSync(process.execPath, ["build.mjs", "lab", "sound"], { cwd: root, stdio: "pipe" });
  server = spawn(process.execPath, ["serve.mjs", String(PORT)], { cwd: root, stdio: "ignore" });
  for (let i = 0; i < 60; i++) {
    try { if ((await fetch(LAB)).ok) break; } catch { /* not up yet */ }
    await sleep(100);
  }
  browser = await launch({ width: 1280, height: 900 });
  page = await browser.newPage();
  await page.goto(LAB);
  await page.waitFor("window.__ready === true", { timeout: 30000 });
  all = await page.eval("window.__renderCues({ master: 0.25 })");
});

after(async () => {
  await browser?.close();
  server?.kill();
});

const byId = (id) => all.find((r) => r.name === `cue:${id}`);

test("every cue is audible, quiet, finite, clean at both ends, inside its duration budget, and soft", () => {
  assert.deepEqual(CUES.filter((c) => c.alt).map((c) => c.id), ["switch", "theme", "lang"]);
  assert.equal(all.length, CUES.length + 3, "every cue, plus the other direction of switch, theme and lang");
  const lines = [];
  for (const r of all) {
    const id = r.name.slice(4);
    const cue = CUE_BY_ID[r.cue];
    const quiet = cue.bus === "background" ? 0.12 : 0.2;
    assert.ok(r.peak > 0.02, `${id} is silent (peak ${r.peak})`);
    assert.ok(r.peak < 0.5, `${id} reaches ${r.peak}`);
    assert.ok(r.peak <= quiet, `${id} (${cue.bus}) peaks at ${r.peak}, over ${quiet}: too loud for a sound that is a hint`);
    assert.equal(r.nan, 0, `${id} has NaN or Infinity`);
    assert.ok(r.tailPeak < 0.002, `${id} leaves a tail of ${r.tailPeak} after its declared end`);
    assert.ok(r.duration <= cue.max, `${id} lasts ${r.duration} s, budget ${cue.max}`);
    assert.ok(r.lastAudibleMs / 1000 <= cue.max + 0.02, `${id} is still audible at ${r.lastAudibleMs} ms`);
    assert.ok(r.lastSample < 0.001, `${id} ends on ${r.lastSample}, not on silence`);
    if (!cue.click) {
      assert.ok(r.headPeak <= 0.35 * r.peak, `${id}: the first millisecond already reaches ${r.headPeak} of a ${r.peak} peak (a click)`);
      assert.ok(r.stepRel <= 0.45, `${id}: a jump of ${r.stepRel} of the peak between two samples (a click)`);
    }
    assert.ok(r.hiFrac <= (cue.click ? 0.25 : 0.02), `${id}: ${r.hiFrac} of its power is above 4 kHz`);
    assert.ok(r.loFrac <= (id === "arrive.sistema" ? 0.2 : 0.05), `${id}: ${r.loFrac} of its power is below 100 Hz`);
    lines.push([id.padEnd(16), cue.bus.slice(0, 3), String(Math.round(r.duration * 1000)).padStart(4), r.peak.toFixed(3), String(Math.round(r.fundamentalHz)).padStart(5), String(r.onsetsMs.length).padStart(2), String(Math.round(r.centroidHz)).padStart(6), r.hiFrac.toFixed(3), JSON.stringify(r.notesHz)].join("  "));
  }
  console.log("\ncue               bus   ms   peak    f0  on  centroid  >4k    notes (Hz)\n" + lines.join("\n"));
});

test("the background bus is quieter than the actions bus, cue by cue where they share a sound and on average everywhere", () => {
  const mean = (xs) => xs.reduce((a, b) => a + b, 0) / xs.length;
  const peakOf = (bus) => all.filter((r) => CUE_BY_ID[r.cue].bus === bus).map((r) => r.peak);
  assert.ok(mean(peakOf("background")) < 0.8 * mean(peakOf("actions")), `background ${mean(peakOf("background")).toFixed(3)} vs actions ${mean(peakOf("actions")).toFixed(3)}`);
  for (const [bg, own] of [["bg.stop", "stop"], ["bg.go", "resume"]]) {
    assert.ok(byId(bg).peak < 0.75 * byId(own).peak, `${bg} (${byId(bg).peak}) is not quieter than ${own} (${byId(own).peak})`);
    assert.ok(Math.abs(byId(bg).fundamentalHz / byId(own).fundamentalHz - 1) < 0.02, `${bg} is the same sound as ${own}: the same association`);
  }
});

test("the gestures say what they mean: rising goes forward, falling goes back, STOP is low and RESUME is clear", () => {
  const notes = (id) => byId(id).notesHz;
  const rises = (id) => notes(id).length >= 2 && notes(id).every((hz, i, a) => i === 0 || hz > a[i - 1] * 0.99);
  const falls = (id) => notes(id).length >= 2 && notes(id).every((hz, i, a) => i === 0 || hz < a[i - 1] * 1.01);
  for (const id of ["approve", "sent", "register", "taskadd", "resume"]) assert.ok(id === "resume" || rises(id), `${id} should rise: ${notes(id)}`);
  for (const id of ["snooze", "undo", "withdraw"]) assert.ok(falls(id), `${id} should fall: ${notes(id)}`);
  assert.ok(byId("stop").fundamentalHz < 260, `STOP is a low tone, it measures ${byId("stop").fundamentalHz} Hz`);
  assert.ok(byId("stop").centroidHz < byId("resume").centroidHz, "RESUME is clearer (brighter) than STOP");
  assert.ok(byId("resume").fundamentalHz > byId("stop").fundamentalHz, "RESUME is higher than STOP");
  assert.ok(byId("switch").notesHz[1] > byId("switch").notesHz[0] && byId("switch:alt").notesHz[1] < byId("switch:alt").notesHz[0], "a switch goes up for on and down for off");
  assert.ok(byId("theme:alt").notesHz[1] > byId("theme:alt").notesHz[0] && byId("theme").notesHz[1] < byId("theme").notesHz[0], "the theme goes up toward Papel and down toward Tinta");
  assert.ok(byId("lang").notesHz[1] > byId("lang").notesHz[0] && byId("lang:alt").notesHz[1] < byId("lang:alt").notesHz[0], "the language goes up toward English and down toward Spanish");
  assert.equal(byId("confirm").onsetsMs.length >= 2, true, "the second press is a double tap");
  assert.equal(byId("arm").onsetsMs.length, 1, "the first press is a single note");
  assert.ok(byId("bg.reply").duration > byId("approve").duration, "what happens by itself is slower than what he does");
});

test("the nominal levels of the gestures (used to find the quietest voice) match what is measured", () => {
  const lines = [];
  for (const [name, meta] of Object.entries(GESTURE_META)) {
    const cue = CUES.find((c) => c.recipe === name);
    const r = byId(cue.id);
    const gain = cue.gain ?? BUSES[cue.bus].gain;
    assert.ok(Math.abs(r.peak / gain / meta.level - 1) < 0.2, `${name} measures ${(r.peak / gain).toFixed(3)} at full level but GESTURE_META says ${meta.level}`);
    lines.push(`${name}: ${(r.peak / gain).toFixed(3)}`);
  }
});

test("no two cues are the same sound, except the background twins of STOP and RESUME that keep the same association", () => {
  const twins = new Set(["bg.stop|stop", "bg.go|resume", "bg.error|error"]);
  const rows = all.filter((r) => !/:alt$/.test(r.name));
  const problems = [];
  for (let i = 0; i < rows.length; i++) {
    for (let j = i + 1; j < rows.length; j++) {
      const [a, b] = [rows[i], rows[j]];
      const key = [a.cue, b.cue].sort().join("|");
      const twin = twins.has([a.cue, b.cue].reverse().join("|")) || twins.has(key) || twins.has(`${b.cue}|${a.cue}`);
      const alike = Math.abs(a.fundamentalHz / b.fundamentalHz - 1) < 0.04 && a.onsetsMs.length === b.onsetsMs.length && Math.abs(a.duration - b.duration) < 0.03 && Math.abs(a.centroidHz / b.centroidHz - 1) < 0.1;
      if (alike && !twin) problems.push(`${a.cue} and ${b.cue}`);
    }
  }
  assert.deepEqual(problems, []);
});

test("the budgets hold for any seed of the jitter", async () => {
  for (let seed = 1; seed <= 6; seed++) {
    for (const r of await page.eval(`window.__renderCues({ master: 0.25, seed: ${seed} })`)) {
      const cue = CUE_BY_ID[r.cue];
      assert.ok(r.duration <= cue.max && r.tailPeak < 0.002 && r.nan === 0 && r.peak > 0.02 && r.peak < 0.5 && r.lastSample < 0.001, `${r.name} seed ${seed}: ${JSON.stringify({ d: r.duration, p: r.peak, t: r.tailPeak })}`);
    }
  }
});

test("at the highest volume (0.6) nothing passes 0.5, and the whole vocabulary at once stays under the ceiling", async () => {
  const loud = await page.eval("window.__renderCues({ master: 0.6 })");
  for (const r of loud) assert.ok(r.peak <= 0.5 && r.nan === 0, `${r.name} reaches ${r.peak} at master 0.6`);
});

test("the engine accepts the level a cue asks for, and the lab ran without a console error", async () => {
  await page.click("#enable");
  await page.waitFor("window.__sound.ctx && window.__sound.ctx.state === 'running'");
  const r = await page.eval(`(() => { const s = window.__sound; return [s.preview("stop", { gain: 0.4 }), s.preview("resume", { gain: 0 }), s.preview("nope", { gain: 1 })]; })()`);
  assert.deepEqual(r, [true, true, false], "a gain of 0 is silent but valid; an unknown recipe is still dropped");
  assert.deepEqual(page.problems(), []);
});
