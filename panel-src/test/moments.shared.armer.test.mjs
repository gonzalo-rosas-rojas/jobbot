// createArmer: the two-press rule of what cannot be undone (docs/adr/0006-two-presses-for-what-cannot-be-undone.md), tested once with a fake clock and fake timers.
// The four moments that use it (HOY, COLA, APRENDER, SISTEMA) have their own tests of which request each verb makes; this file is about the press.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createArmer } from "../src/moments/shared/armer.js";
import { ARM_MIN_MS, ARM_MAX_MS } from "../src/core/dom.js";

const SRC = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "src");

function rig({ held = false } = {}) {
  const state = { now: 10_000, held, said: [], cues: [], changes: [], scheduled: [], cleared: [] };
  const timers = {
    set(fn, ms) { const id = state.scheduled.length + 1; state.scheduled.push({ id, fn, ms }); return id; },
    clear(id) { state.cleared.push(id); },
  };
  const armer = createArmer({
    announce: (text) => state.said.push(text),
    sound: { play: (name) => state.cues.push(name) },
    t: (key) => "<" + key + ">",
    onChange: (ref) => state.changes.push(ref),
    now: () => state.now,
    timers,
    held: () => state.held,
  });
  /** Fires a scheduled expiry the way a timer would, unless it was cleared first. */
  const expire = (id) => { if (!state.cleared.includes(id)) state.scheduled.find((s) => s.id === id).fn(); };
  return { armer, state, expire };
}

test("the first press arms: it says so, plays the arm cue, tells the render and sets the expiry", () => {
  const { armer, state } = rig();
  assert.equal(armer.press("row-1", "approve"), "armed");
  assert.deepEqual(state.said, ["<act.sure>"]);
  assert.deepEqual(state.cues, ["arm"]);
  assert.deepEqual(state.changes, ["row-1"]);
  assert.equal(state.scheduled.length, 1);
  assert.equal(state.scheduled[0].ms, ARM_MAX_MS);
  assert.equal(armer.isArmed("row-1"), true);
  assert.equal(armer.isArmed("row-1", "approve"), true);
  assert.equal(armer.isArmed("row-1", "confirm"), false);
  assert.equal(armer.isArmed("row-2"), false);
  assert.equal(armer.armedOf("row-1"), "approve");
  assert.equal(armer.armedOf("row-2"), null);
  assert.equal(armer.any, true);
});

test("a second press in the same instant (a double click) is one press: ignored, still armed, nothing sounds", () => {
  const { armer, state } = rig();
  armer.press("r", "approve");
  state.cues.length = 0; state.changes.length = 0;
  for (const wait of [0, 1, ARM_MIN_MS - 1]) {
    state.now = 10_000 + wait;
    assert.equal(armer.press("r", "approve"), "ignored", `after ${wait} ms`);
  }
  assert.equal(armer.armedOf("r"), "approve");
  assert.deepEqual(state.cues, []);
  assert.deepEqual(state.changes, []);
  assert.deepEqual(state.cleared, []);
});

test("a held key is never the second press, however late it comes", () => {
  const { armer, state } = rig();
  armer.press("r", "approve");
  state.now += 2000;
  state.held = true;
  assert.equal(armer.press("r", "approve"), "ignored");
  assert.equal(armer.armedOf("r"), "approve");
  assert.deepEqual(state.cues, ["arm"]);
  state.held = false;                                  // the key was let go and pressed again on purpose
  assert.equal(armer.press("r", "approve"), "confirmed");
});

test("a deliberate second press confirms: the cue, the disarm, the render told, the expiry cleared", () => {
  const { armer, state } = rig();
  armer.press("r", "approve");
  state.now += ARM_MIN_MS;                             // exactly the minimum counts
  state.changes.length = 0;
  assert.equal(armer.press("r", "approve"), "confirmed");
  assert.deepEqual(state.cues, ["arm", "confirm"]);
  assert.deepEqual(state.changes, ["r"]);
  assert.deepEqual(state.cleared, [1]);
  assert.equal(armer.armedOf("r"), null);
  assert.equal(armer.any, false);
  assert.equal(armer.press("r", "approve"), "armed", "the next press starts again");
});

test("an armed press expires after ARM_MAX_MS and then the next press arms again instead of confirming", () => {
  const { armer, state, expire } = rig();
  armer.press("r", "approve");
  state.changes.length = 0;
  expire(1);
  assert.equal(armer.any, false);
  assert.deepEqual(state.changes, ["r"], "the render is told so it can drop the question");
  state.now += ARM_MAX_MS + 1;
  assert.equal(armer.press("r", "approve"), "armed");
  assert.equal(state.cues.filter((c) => c === "confirm").length, 0);
});

test("another verb on the same row, or the same verb on another row, re-arms and drops the first", () => {
  const { armer, state } = rig();
  armer.press("r1", "approve");
  state.changes.length = 0;
  assert.equal(armer.press("r1", "confirm"), "armed");
  assert.deepEqual(state.changes, ["r1", "r1"], "the old arming is dropped, then the new one");
  assert.equal(armer.armedOf("r1"), "confirm");
  state.changes.length = 0;
  assert.equal(armer.press("r2", "confirm"), "armed");
  assert.deepEqual(state.changes, ["r1", "r2"]);
  assert.equal(armer.armedOf("r1"), null);
  assert.equal(armer.armedOf("r2"), "confirm");
  assert.deepEqual(state.cleared, [1, 2], "each replaced arming cleared its own expiry");
  assert.deepEqual(state.cues, ["arm", "arm", "arm"], "no confirm in the whole sequence");
});

test("release drops the arming of that target only", () => {
  const { armer, state } = rig();
  armer.press("r1", "approve");
  state.changes.length = 0;
  armer.release("r2");
  assert.equal(armer.armedOf("r1"), "approve");
  assert.deepEqual(state.changes, []);
  armer.release("r1");
  assert.equal(armer.any, false);
  assert.deepEqual(state.changes, ["r1"]);
  assert.deepEqual(state.cleared, [1]);
});

test("disarm tells the render once and is harmless when nothing is armed", () => {
  const { armer, state } = rig();
  armer.disarm();
  assert.deepEqual(state.changes, []);
  armer.press("r", "approve");
  state.changes.length = 0;
  armer.disarm();
  armer.disarm();
  assert.deepEqual(state.changes, ["r"]);
  assert.deepEqual(state.cleared, [1]);
  assert.equal(armer.any, false);
});

test("drop clears the expiry without telling anyone (the moment is going away)", () => {
  const { armer, state } = rig();
  armer.drop();
  assert.deepEqual(state.cleared, []);
  armer.press("r", "approve");
  state.changes.length = 0;
  armer.drop();
  assert.deepEqual(state.changes, []);
  assert.deepEqual(state.cleared, [1]);
  assert.equal(armer.any, false);
  assert.equal(armer.armedOf("r"), null);
});

test("the defaults (the real clock and timers) arm and drop without leaving a timer behind", () => {
  const said = [];
  const armer = createArmer({ announce: (x) => said.push(x), sound: { play() {} }, t: (k) => k });
  assert.equal(armer.press("r", "x"), "armed");
  assert.equal(armer.press("r", "x"), "ignored", "the same instant, or a key held by a real keyboard");
  armer.drop();
  assert.deepEqual(said, ["act.sure"]);
});

// ---- nobody writes a new copy: the timing of the rule lives in the armer, in the drawn confirmations (COLA's lote, the drawer's
// "Registrar una respuesta"), and in the old twoStep that a later pass will retire
function sources(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, e.name);
    if (e.isDirectory()) sources(full, out); else if (e.name.endsWith(".js")) out.push(path.relative(SRC, full).split(path.sep).join("/"));
  }
  return out;
}
const users = (pattern) => sources(SRC).filter((f) => pattern.test(fs.readFileSync(path.join(SRC, f), "utf8"))).sort();

test("the two-press timing is written in the known places only", () => {
  assert.deepEqual(users(/\bARM_MAX_MS\b/), ["core/dom.js", "moments/shared/armer.js", "ui/detail/act.js"]);
  assert.deepEqual(users(/\bARM_MIN_MS\b/), ["core/dom.js", "moments/cola/actions.js", "moments/shared/armer.js", "ui/detail/mark.js"], "COLA's lote and the drawer's answers are confirmed by a drawn question with its own minimum");
  assert.deepEqual(users(/\btwoStep\s*[(,}]/), ["core/dom.js", "moments/procesos/drawer-content.js", "ui/detail/act.js"], "twoStep is the other copy of the rule, still to be moved onto the armer");
});

test("the four moments take their arming from the armer", () => {
  for (const f of ["hoy", "cola", "aprender", "sistema"]) {
    const text = fs.readFileSync(path.join(SRC, "moments", f, "actions.js"), "utf8");
    assert.match(text, /createArmer\(/, f);
    assert.doesNotMatch(text, /\blet armed\b/, `${f} keeps no armed state of its own`);
  }
});
