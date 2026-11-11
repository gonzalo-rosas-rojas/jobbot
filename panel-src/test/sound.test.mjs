// The sound budgets, measured. Builds the sound lab, serves it on port 8792 with the production CSP, drives it with
// tools/cdp.mjs, renders every recipe offline THROUGH the master chain and asserts the numbers of the brief; then
// checks the engine's product rules in the live page (no AudioContext before a gesture, budget, unsolicited rules,
// persistence). Chrome and the server are always closed.
import test, { before, after } from "node:test";
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { launch } from "../tools/cdp.mjs";
import { META, RECIPES } from "../src/sound/recipes.js";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const PORT = 8792;
const LAB = `http://127.0.0.1:${PORT}/dist/lab-sound.html`;
const SHOTS = path.join(root, "dist", "shots", "sound");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Counts every construction of a live AudioContext (OfflineAudioContext is a different constructor).
const COUNT_CONTEXTS = `window.__acCount = 0; const Real = window.AudioContext;
  if (Real) window.AudioContext = new Proxy(Real, { construct(t, a, nt) { window.__acCount++; return Reflect.construct(t, a, nt); } });`;

// What the tonal recipes are tuned to, for the first note; measured values must land within 2 %.
const NOMINAL = {
  "arrive:hoy": 587.33, "arrive:procesos": 493.88, "arrive:cola": 369.99, "arrive:aprender": 293.66, "arrive:sistema": 146.83,
  approve: 293.66, undo: 369.99, toggle: 440, "toggle:off": 493.88, turn: 880, late: 220, error: 110, ceremony: 293.66,
};
const ROOMS = ["hoy", "procesos", "cola", "aprender", "sistema"].map((r) => `arrive:${r}`);

let server;
let browser;
let page;
let report = [];

async function fresh(p = page) {
  await p.eval("try { localStorage.clear() } catch (e) {}");
  await p.goto(LAB);
  await p.waitFor("window.__ready === true", { timeout: 20000 });
}

async function waitServer() {
  for (let i = 0; i < 60; i++) {
    try {
      if ((await fetch(LAB)).ok) return;
    } catch {
      /* not up yet */
    }
    await sleep(100);
  }
  throw new Error("the lab server did not start on " + PORT);
}

before(async () => {
  execFileSync(process.execPath, ["build.mjs", "lab", "sound"], { cwd: root, stdio: "pipe" });
  server = spawn(process.execPath, ["serve.mjs", String(PORT)], { cwd: root, stdio: "ignore" });
  await waitServer();
  browser = await launch({ width: 1280, height: 1750 });
  page = await browser.newPage();
  await page.send("Page.addScriptToEvaluateOnNewDocument", { source: COUNT_CONTEXTS });
  await fresh();
});

after(async () => {
  await browser?.close();
  server?.kill();
});

const get = (all, name) => all.find((r) => r.name === name);

test("every recipe is audible, quiet, finite, clean at the end and inside its duration budget", async () => {
  const all = await page.eval("window.__renderAll({ master: 0.25 })");
  report = all;
  assert.equal(all.length, 16, "5 rooms + 9 other recipes + toggle off");
  for (const r of all) {
    assert.ok(r.peak > 0.02, `${r.name} is silent (peak ${r.peak})`);
    assert.ok(r.peak <= 0.5, `${r.name} is loud (peak ${r.peak})`);
    assert.equal(r.nan, 0, `${r.name} has NaN or Infinity`);
    assert.ok(r.tailPeak < 0.002, `${r.name} leaves a tail of ${r.tailPeak} after its declared end`);
    const limit = r.name === "ceremony" ? 0.9 : r.name.startsWith("arrive:") ? 0.42 : 0.2;
    assert.ok(r.duration <= limit, `${r.name} lasts ${r.duration} s, limit ${limit}`);
    assert.ok(r.lastAudibleMs / 1000 <= limit + 0.02, `${r.name} is still audible at ${r.lastAudibleMs} ms`);
  }
  assert.ok(get(all, "ceremony").duration >= 0.7, "the ceremony is at least 700 ms");
  for (const [name, hz] of Object.entries(NOMINAL)) {
    const r = get(all, name);
    assert.ok(Math.abs(r.fundamentalHz / hz - 1) < 0.02, `${name} measures ${r.fundamentalHz} Hz, tuned to ${hz}`);
  }
  console.log(
    "\nrecipe           ms   peak   rms    f0 Hz  centroid  >4k share  notes (Hz)\n" +
      all.map((r) => [r.name.padEnd(15), String(Math.round(r.duration * 1000)).padStart(4), r.peak.toFixed(3), r.rms.toFixed(3), String(Math.round(r.fundamentalHz)).padStart(6), String(Math.round(r.centroidHz)).padStart(8), r.hiFrac.toFixed(3).padStart(9), "  " + JSON.stringify(r.notesHz)].join("  ")).join("\n"),
  );
});

test("jitter is seeded: same seed same sound, other seeds differ, budgets hold for any seed", async () => {
  const a = await page.eval("window.__renderAll({ seed: 11 })");
  const b = await page.eval("window.__renderAll({ seed: 11 })");
  assert.deepEqual(a.map((r) => [r.duration, r.peak]), b.map((r) => [r.duration, r.peak]));
  const c = await page.eval("window.__renderAll({ seed: 12 })");
  assert.ok(a.some((r, i) => r.duration !== c[i].duration || r.fundamentalHz !== c[i].fundamentalHz), "a different seed changes something");
  for (let seed = 1; seed <= 8; seed++) {
    for (const r of await page.eval(`window.__renderAll({ seed: ${seed} })`)) {
      const limit = r.name === "ceremony" ? 0.9 : r.name.startsWith("arrive:") ? 0.42 : 0.2;
      assert.ok(r.duration <= limit, `${r.name} seed ${seed}: ${r.duration} s over ${limit}`);
      assert.ok(r.tailPeak < 0.002 && r.nan === 0 && r.peak > 0.02 && r.peak <= 0.5, `${r.name} seed ${seed}`);
    }
  }
});

test("the room motifs are distinct and ordered by the height of the floor (measured, not the table)", () => {
  const rooms = ROOMS.map((n) => get(report, n));
  for (let i = 1; i < rooms.length; i++) {
    assert.ok(rooms[i].fundamentalHz < rooms[i - 1].fundamentalHz, `${rooms[i].name} (${rooms[i].fundamentalHz}) is not below ${rooms[i - 1].name} (${rooms[i - 1].fundamentalHz})`);
  }
  const interval = (r) => (r.notesHz.length > 1 ? r.notesHz[1] / r.notesHz[0] : 0);
  for (let i = 0; i < rooms.length; i++) {
    for (let j = i + 1; j < rooms.length; j++) {
      const [a, b] = [rooms[i], rooms[j]];
      const differs =
        Math.abs(a.fundamentalHz / b.fundamentalHz - 1) > 0.05 || a.onsetsMs.length !== b.onsetsMs.length || Math.abs(interval(a) - interval(b)) > 0.05;
      assert.ok(differs, `${a.name} and ${b.name} are not distinct`);
    }
  }
  for (const r of rooms) {
    assert.ok(r.onsetsMs.length === (r.name === "arrive:sistema" ? 1 : 2), `${r.name} has ${r.onsetsMs.length} onsets`);
    assert.ok(r.duration <= 0.42);
  }
  console.log("\nroom        first Hz  onsets  interval  centroid Hz\n" + rooms.map((r) => `${r.name.padEnd(16)}${String(Math.round(r.fundamentalHz)).padStart(6)}${String(r.onsetsMs.length).padStart(7)}${interval(r) ? interval(r).toFixed(3).padStart(10) : "         -"}${String(Math.round(r.centroidHz)).padStart(10)}`).join("\n"));
});

test("soft by construction: no sustained energy above 4 kHz, only the click-like sounds have a share", () => {
  const clicks = new Set(["tick", "open", "close"]);
  for (const r of report) {
    assert.ok(r.hiFrac <= (clicks.has(r.name) ? 0.25 : 0.02), `${r.name}: ${r.hiFrac} of its power is above 4 kHz`);
  }
});

test("below 100 Hz there is almost nothing, except the hum of the machine room", () => {
  for (const r of report) assert.ok(r.loFrac <= (r.name === "arrive:sistema" ? 0.2 : 0.05), `${r.name}: ${r.loFrac} of its power is below 100 Hz`);
});

test("the recipes' nominal levels (used to find the quietest voice) match what is measured", () => {
  assert.deepEqual(Object.keys(META).sort(), Object.keys(RECIPES).sort());
  for (const r of report) {
    const level = META[r.name.split(":")[0]].level;
    assert.ok(Math.abs(r.peak / level - 1) < 0.2, `${r.name} measures ${r.peak.toFixed(3)} but META says ${level}`);
  }
});

test("the limiter holds: peak <= 0.5 with the master forced to 0.6, alone and with six sounds piled up", async () => {
  const loud = await page.eval("window.__renderAll({ master: 0.6 })");
  for (const r of loud) {
    assert.ok(r.peak <= 0.5, `${r.name} reaches ${r.peak} at master 0.6`);
    assert.equal(r.nan, 0);
    assert.ok(r.tailPeak < 0.004, `${r.name} tail ${r.tailPeak} at master 0.6`);
  }
  const loudest = Math.max(...loud.map((r) => r.peak));
  assert.ok(loudest <= 0.35, `the loudest recipe reaches ${loudest} at master 0.6: the ceiling is a seatbelt, recipes must not lean on it`);
  const six = await page.eval("window.__renderPileup(0.6)");
  const everything = await page.eval("window.__renderPileup(0.6, true, 3)");
  assert.ok(six <= 0.5 && six > 0.1, `six sounds at once peak at ${six}`);
  assert.ok(everything <= 0.5 && everything > 0.3, `every recipe three times over peaks at ${everything}`);
  console.log(`\nworst case at master 0.6: loudest single recipe ${loudest.toFixed(3)}, six at once ${six.toFixed(3)}, all 48 voices at once ${everything.toFixed(3)}`);
});

test("engine: nothing exists before a gesture, and a disabled engine drops everything", async () => {
  await fresh();
  const s = await page.eval(`(() => { const s = window.__sound; return {
    ctx: s.ctx, enabled: s.enabled, volume: s.volume, frozen: Object.isFrozen(s.names), names: s.names.length,
    play: s.play("approve"), preview: s.preview("approve"), count: window.__acCount }; })()`);
  assert.deepEqual(s, { ctx: null, enabled: false, volume: 0.25, frozen: true, names: 11, play: false, preview: false, count: 0 });
});

test("engine: a click enables it, then budget, duplicates, priority and the voice cap behave", async () => {
  await page.click("#enable");
  await page.waitFor("window.__sound.ctx && window.__sound.ctx.state === 'running'");
  assert.equal(await page.eval("window.__acCount"), 1, "exactly one AudioContext, created by the click");
  assert.equal(await page.eval("window.__sound.enabled"), true);
  assert.equal(await page.eval("window.__sound.recent().length"), 0, "enabling plays nothing");

  // hover is silent: move the mouse over the controls
  for (const [x, y] of [[200, 200], [400, 400], [600, 800]]) await page.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
  assert.equal(await page.eval("window.__sound.recent().length"), 0, "hover made a sound");

  // the arrival sound takes a room; anything else (the tower's floor included) is dropped
  assert.deepEqual(
    await page.eval(`(() => { const s = window.__sound; return [s.preview("arrive", { room: "cola" }), s.preview("arrive", { floor: 5 }), s.preview("arrive", { floor: 9 }), s.preview("arrive", {}), s.preview("nope"), s.preview("toString")]; })()`),
    [true, false, false, false, false, false],
  );
  await sleep(1100);

  // approve, the same again within 80 ms, a second distinct sound, a third distinct sound
  assert.deepEqual(await page.eval(`(() => { const s = window.__sound; return [s.play("approve"), s.play("approve"), s.play("tick"), s.play("open")]; })()`), [true, false, true, false]);

  // ctx.currentTime is read once per sound, also when it has to cut a weaker one to get its slot
  await sleep(1100);
  const reads = await page.eval(`(() => {
    const s = window.__sound; s.play("tick"); s.play("open");
    const d = Object.getOwnPropertyDescriptor(BaseAudioContext.prototype, "currentTime"); let n = 0;
    Object.defineProperty(BaseAudioContext.prototype, "currentTime", { configurable: true, get() { n++; return d.get.call(this); } });
    const ok = s.play("error");
    Object.defineProperty(BaseAudioContext.prototype, "currentTime", d);
    return [ok, n]; })()`);
  assert.deepEqual(reads, [true, 1]);

  // priority: clicks fill the two slots, an error still gets through and replaces the weakest
  await sleep(1100);
  const burst = await page.eval(`(() => { const s = window.__sound; return [s.play("tick"), s.play("open"), s.play("close"), s.play("error"), s.play("tick")]; })()`);
  assert.deepEqual(burst, [true, true, false, true, false]);
  assert.deepEqual(await page.eval("window.__sound.recent().filter((v) => !v.cut).map((v) => v.key).sort()"), ["error", "open"]);

  // the voice cap: six at once, the seventh quiet one is dropped, a louder one replaces the quietest
  await sleep(1100);
  const cap = await page.eval(`(() => { const s = window.__sound; const out = [];
    for (let i = 0; i < 7; i++) out.push(s.preview("tick"));
    out.push(s.preview("ceremony")); return [out, s.recent().filter((v) => v.cut).length]; })()`);
  assert.deepEqual(cap, [[true, true, true, true, true, true, false, true], 1]);

  // the stress button: seven sounds in about 200 ms, two survive, and they are the important ones
  await sleep(1100);
  await page.click("#stress");
  await sleep(600);
  const log = await page.eval("document.getElementById('log').textContent");
  assert.match(log, /stress: accepted tick, [a-z]+, approve, error; still audible: approve, error/);
});

test("engine: the unsolicited rules (busy, hidden, once per 5 minutes) and preview", async () => {
  await sleep(1100);
  const busy = await page.eval(`(() => { const s = window.__sound; s.setBusy(true);
    const r = [s.play("turn", { unsolicited: true }), s.play("late"), s.preview("turn"), s.play("tick")]; s.setBusy(false); return r; })()`);
  assert.deepEqual(busy, [false, false, true, true], "busy drops only unsolicited sounds; preview and his own actions play");

  await sleep(1100);
  const hidden = await page.eval(`(() => { const s = window.__sound;
    Object.defineProperty(document, "hidden", { configurable: true, get: () => true });
    const r = [s.play("late"), s.play("approve")];
    delete document.hidden; return r; })()`);
  assert.deepEqual(hidden, [false, true], "a hidden tab drops unsolicited sounds, not his own actions");

  await sleep(1100);
  assert.equal(await page.eval(`window.__sound.play("turn")`), true, "the drops above did not use up the 5-minute allowance");
  await sleep(1100);
  assert.equal(await page.eval(`window.__sound.play("turn")`), false, "a second turn within 5 minutes is dropped");
  assert.equal(await page.eval(`window.__sound.play("turn", { unsolicited: false })`), true, "a caller may say the sound was caused by his own action");
});

test("engine: volume clamps to 0..0.6, 0 is silent, and disabling suspends the context", async () => {
  await sleep(1100);
  const v = await page.eval(`(() => { const s = window.__sound; const out = [];
    s.setVolume(0.9); out.push(s.volume); s.setVolume(-1); out.push(s.volume, s.play("undo"), s.preview("undo"));
    s.setVolume(0.4); out.push(s.volume); return out; })()`);
  assert.deepEqual(v, [0.6, 0, false, false, 0.4]);
  assert.equal(await page.eval("localStorage.getItem('panel.volume')"), "0.4");
  assert.equal(await page.eval("window.__sound.toggle()"), false, "toggle returns the new state");
  assert.equal(await page.eval(`window.__sound.play("approve")`), false);
  await page.waitFor("window.__sound.ctx.state === 'suspended'");
  assert.equal(await page.eval("localStorage.getItem('panel.sound')"), "0");
  assert.equal(await page.eval("window.__sound.toggle()"), true);
  await page.waitFor("window.__sound.ctx.state === 'running'");
  assert.equal(await page.eval("window.__acCount"), 1, "turning it off and on again reuses the same context");
});

test("persistence: a saved 'on' is restored after a reload but the context waits for the next gesture", async () => {
  assert.equal(await page.eval("localStorage.getItem('panel.sound')"), "1");
  await page.goto(LAB);
  await page.waitFor("window.__ready === true", { timeout: 20000 });
  const s = await page.eval(`({ enabled: window.__sound.enabled, volume: window.__sound.volume, ctx: window.__sound.ctx, count: window.__acCount, play: window.__sound.play("approve") })`);
  assert.deepEqual(s, { enabled: true, volume: 0.4, ctx: null, count: 0, play: false });
  assert.equal(await page.eval("window.__sound.setEnabled(true)"), true, "the shell's first pointer or key event does this");
  await page.waitFor("window.__sound.ctx.state === 'running'");
  assert.equal(await page.eval("window.__acCount"), 1);
  assert.equal(await page.eval(`window.__sound.play("approve")`), true, "the first sound right after setEnabled(true) plays");
});

test("the lab buttons work: audition, the arrival tour, and the live analyser sees the sound", async () => {
  await sleep(1100);
  await page.click('[data-play="approve"]');
  assert.match(await page.eval("document.getElementById('log').textContent"), /preview approve\s+-> true/);
  await sleep(1100);
  await page.click("#tour");
  await sleep(3300);
  const log = await page.eval("document.getElementById('log').textContent");
  for (const room of ["hoy", "procesos", "cola", "aprender", "sistema"]) assert.match(log, new RegExp(`play\\s+arrive:${room}\\s+-> true`), `the tour dropped ${room}`);
  await page.media({ dark: true });
  await page.shot(path.join(SHOTS, "lab-tour-dark.png"));
  await page.media({ dark: false });
  await page.shot(path.join(SHOTS, "lab-tour-light.png"));
  await page.media({ dark: true });
  await page.click('[data-play="ceremony"]');
  await sleep(430);
  await page.shot(path.join(SHOTS, "lab-live-ceremony.png"));
});

test("no Web Audio, no storage: the engine is still there and harmless", async () => {
  const p = await browser.newPage();
  await p.send("Page.addScriptToEvaluateOnNewDocument", {
    source: `delete window.AudioContext; delete window.webkitAudioContext;
      Object.defineProperty(window, "localStorage", { get() { throw new Error("blocked"); } });`,
  });
  await p.goto(LAB);
  await p.waitFor("window.__ready === true", { timeout: 20000 });
  const r = await p.eval(`(() => { const s = window.__sound; return [s.enabled, s.setEnabled(true), s.toggle(), s.play("approve"), s.preview("tick"), s.setVolume(0.3), s.tap({}), s.ctx, s.recent().length]; })()`);
  assert.deepEqual(r, [false, false, false, false, false, 0.3, false, null, 0]);
  assert.deepEqual(p.problems(), []);
});

test("storage that throws does not stop the engine from working", async () => {
  const p = await browser.newPage();
  await p.send("Page.addScriptToEvaluateOnNewDocument", {
    source: `Object.defineProperty(window, "localStorage", { get() { throw new Error("blocked"); } });`,
  });
  await p.goto(LAB);
  await p.waitFor("window.__ready === true", { timeout: 20000 });
  assert.deepEqual(await p.eval(`(() => { const s = window.__sound; return [s.enabled, s.volume, s.setEnabled(true), s.setVolume(0.3)]; })()`), [false, 0.25, true, 0.3]);
  await p.waitFor("window.__sound.ctx.state === 'running'");
  assert.equal(await p.eval(`window.__sound.play("approve")`), true);
  assert.deepEqual(p.problems(), []);
});

test("no console errors and no CSP violations over the whole session", () => {
  assert.deepEqual(page.problems(), []);
});
