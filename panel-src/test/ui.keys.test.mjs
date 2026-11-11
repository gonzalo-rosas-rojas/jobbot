import test from "node:test";
import assert from "node:assert/strict";
import { KEYS, isSequence, keysFor, conflicts, installKeys } from "../src/ui/keys.js";
import { helpRows } from "../src/ui/help.js";
import { ignoreKey, typingIn, localKeys } from "../src/ui/keyfilter.js";
import { has, setLang } from "../src/i18n/index.js";

const STOPS = ["hoy", "procesos", "cola", "aprender", "sistema"];
const FIELD = { tagName: "INPUT", type: "text" };
const LOCAL_BOX = { tagName: "DIV", closest: (selector) => (selector === "[data-local-keys]" ? {} : null) };

function press(target, key, { where = {}, shift = false, ctrl = false, meta = false, alt = false, repeat = false } = {}) {
  const mods = new Set([shift && "Shift", ctrl && "Control", meta && "Meta", alt && "Alt"].filter(Boolean));
  const e = new Event("keydown", { cancelable: true });
  const props = { key, code: key.length === 1 ? (/\d/.test(key) ? "Digit" + key : "Key" + key.toUpperCase()) : key, repeat, isComposing: false, ctrlKey: ctrl, metaKey: meta, altKey: alt, shiftKey: shift, getModifierState: (m) => mods.has(m) };
  for (const [name, value] of Object.entries(props)) Object.defineProperty(e, name, { value });
  Object.defineProperty(e, "target", { value: where });
  target.dispatchEvent(e);
  return e;
}

function rig(opts = {}) {
  const win = new EventTarget();
  const calls = [];
  const all = Object.fromEntries(KEYS.filter((k) => k.scope !== "local").map((k) => [k.id, (key, event) => calls.push([k.id, key, event])]));
  const state = { stop: "hoy", locked: false, drawer: false };
  const detach = installKeys({ target: win, handlers: opts.handlers || all, stop: () => state.stop, locked: () => state.locked, drawerOpen: () => state.drawer, chordMs: 30 });
  return { win, calls, state, detach };
}

test("A19: the table has no conflicts, and in every stop a first press means one thing", () => {
  assert.deepEqual(conflicts(), []);
  for (const stop of STOPS) {
    const firsts = new Map();
    for (const entry of keysFor(stop).filter((k) => k.scope !== "local")) {
      for (const key of entry.keys) {
        const first = key.split(" ")[0].toLowerCase();
        assert.ok(!firsts.has(first) || firsts.get(first) === entry.id, `${stop}: "${first}" is both ${firsts.get(first)} and ${entry.id}`);
        firsts.set(first, entry.id);
      }
    }
  }
});

test("A19: no row or stop key redefines a global key", () => {
  const globals = KEYS.filter((k) => k.scope === "global").flatMap((k) => k.keys.map((x) => x.toLowerCase()));
  for (const entry of KEYS.filter((k) => k.scope === "row" || k.scope === "stop")) {
    for (const key of entry.keys) assert.ok(!globals.includes(key.split(" ")[0].toLowerCase()) || key.includes(" "), `${entry.id} (${key}) redefines a global`);
  }
});

test("conflicts() really finds a clash: two row keys in a stop, a row key that is a global, a chord whose first press is a single key", () => {
  const base = [{ id: "g", keys: ["t"], scope: "global" }];
  assert.deepEqual(conflicts(base), []);
  assert.equal(conflicts([...base, { id: "a", keys: ["x"], scope: "row", stops: ["hoy"] }, { id: "b", keys: ["x"], scope: "row", stops: ["hoy"] }]).length, 1);
  assert.deepEqual(conflicts([...base, { id: "a", keys: ["x"], scope: "row", stops: ["hoy"] }, { id: "b", keys: ["x"], scope: "row", stops: ["cola"] }]), [], "the same key in two stops is fine");
  assert.equal(conflicts([...base, { id: "a", keys: ["t"], scope: "row", stops: ["hoy"] }]).length >= 1, true);
  assert.equal(conflicts([{ id: "a", keys: ["f"], scope: "stop", stops: ["cola"] }, { id: "b", keys: ["f 1"], scope: "stop", stops: ["cola"] }]).length, 1);
  assert.deepEqual(conflicts([{ id: "a", keys: ["f 1", "f 2"], scope: "stop", stops: ["cola"] }, { id: "go", keys: ["1", "2"], scope: "global" }]), [], "a chord's second press may be a global digit");
});

test("the table is the one of spec 3.4", () => {
  const byId = Object.fromEntries(KEYS.map((k) => [k.id, k]));
  const want = {
    go: [["1", "2", "3", "4", "5"], "global"], prologue: [["0"], "global"], search: [["$mod+k"], "global"], help: [["[Shift]+?"], "global"],
    theme: [["t"], "global"], sound: [["m"], "global"], lang: [["l"], "global"], close: [["Escape"], "global"],
    move: [["j", "k"], "row"], open: [["Enter"], "row"], approve: [["a"], "row"], confirm: [["c"], "row"], resolve: [["u"], "row"], withdraw: [["x"], "row"],
    snooze: [["s"], "row"], register: [["r"], "row"], done: [["h"], "row"], newtask: [["n"], "stop"], link: [["o"], "row"],
    filter: [["f 1", "f 2", "f 3", "f 4"], "stop"], find: [["[Shift]+/"], "stop"], field: [["v"], "stop"], batch: [["Shift+A"], "stop"], engine: [["p"], "stop"], reveal: [["Enter", "Space"], "local"], grade: [["0", "1", "2", "3", "4", "5"], "local"],
  };
  assert.deepEqual(Object.keys(byId).sort(), Object.keys(want).sort());
  for (const [id, [keys, scope]] of Object.entries(want)) assert.deepEqual([byId[id].keys, byId[id].scope], [keys, scope], id);
  assert.deepEqual(byId.done.stops, ["hoy"]);
  assert.deepEqual(byId.approve.stops, ["hoy", "cola"]);
  assert.deepEqual(byId.batch.stops, ["cola"]);
  assert.deepEqual(byId.grade.stops, ["aprender"]);
  assert.deepEqual(byId.reveal.stops, ["aprender"]);
  // APRENDER (spec 5.4): j k and Enter walk and open the questions that block a form, u gives them back to the queue, o opens the posting; the
  // review inside its own box has Enter/Space to reveal and 0-5 to grade (a local key never reaches the page's handlers)
  assert.deepEqual(KEYS.filter((k) => (k.stops || []).includes("aprender")).map((k) => k.id), ["move", "open", "resolve", "link", "reveal", "grade"]);
  assert.ok(KEYS.every((k) => typeof k.display === "string" && k.display && has(k.label)), "every key shows something and says what it does");
});

test("keysFor: the prologue has only the global keys, a stop adds its own in table order, and `live` hides what is not wired yet", () => {
  const globals = KEYS.filter((k) => k.scope === "global").map((k) => k.id);
  assert.deepEqual(keysFor("prologo").map((k) => k.id), globals);
  assert.deepEqual(keysFor(null).map((k) => k.id), globals);
  const hoy = keysFor("hoy").map((k) => k.id);
  assert.deepEqual(hoy.slice(0, globals.length), globals);
  for (const id of ["move", "open", "approve", "confirm", "resolve", "withdraw", "snooze", "register", "done", "newtask", "link"]) assert.ok(hoy.includes(id), id);
  assert.ok(!hoy.includes("batch") && !hoy.includes("grade") && !hoy.includes("filter"));
  assert.ok(keysFor("aprender").some((k) => k.id === "grade"));
  assert.deepEqual(keysFor("hoy", { live: new Set(["done"]) }).map((k) => k.id), [...globals, "done"]);
});

test("the help is the table: every row has a display and a label in both languages", () => {
  for (const lang of ["es", "en"]) {
    setLang(lang);
    for (const stop of [null, ...STOPS]) {
      const rows = helpRows(stop);
      assert.ok(rows.length >= 8);
      const shown = rows.map((r) => r.display);
      assert.equal(new Set(shown).size, shown.length, `${stop}: a key is listed twice`);
      for (const row of rows) assert.ok(has(row.label), `${row.label} is missing`);
    }
  }
  setLang("es");
  assert.deepEqual(helpRows("hoy", new Set()).map((r) => r.id), KEYS.filter((k) => k.scope === "global").map((k) => k.id));
});

test("keyfilter: fields, local-key boxes, held keys (but j, k and the arrows) and browser chords stay quiet; Esc and Ctrl+K never do", () => {
  assert.equal(typingIn(FIELD), true);
  assert.equal(typingIn({ tagName: "INPUT", type: "checkbox" }), false);
  assert.equal(localKeys(LOCAL_BOX), true);
  assert.equal(localKeys({}), false);
  const ev = (key, extra = {}) => ({ key, target: {}, ...extra });
  assert.equal(ignoreKey(ev("j", { target: FIELD })), true);
  assert.equal(ignoreKey(ev("j", { target: LOCAL_BOX })), true);
  assert.equal(ignoreKey(ev("Escape", { target: FIELD })), false);
  assert.equal(ignoreKey(ev("k", { target: FIELD, ctrlKey: true })), false);
  for (const key of ["1", "5", "?", "Escape", "Enter", "g"]) assert.equal(ignoreKey(ev(key, { repeat: true })), true, `${key} held down fires once`);
  // Review B10: holding j, k or an arrow walks a long list; only the keys that move a cursor repeat, and only where the rest of the rules let them.
  for (const key of ["j", "k", "ArrowUp", "ArrowDown"]) {
    assert.equal(ignoreKey(ev(key, { repeat: true })), false, `${key} held down keeps moving`);
    assert.equal(ignoreKey(ev(key, { repeat: true, target: FIELD })), true, `${key} held down in a field is still the field's`);
    assert.equal(ignoreKey(ev(key, { repeat: true }), { locked: () => true }), true, `${key} held down under a dialog is still the dialog's`);
    assert.equal(ignoreKey(ev(key, { repeat: true, isComposing: true })), true, `${key} while composing is the input method's`);
  }
  assert.equal(ignoreKey(ev("k", { repeat: true, ctrlKey: true })), true, "Ctrl+K held down opens the palette once, not a hundred times");
  assert.equal(ignoreKey(ev("c", { ctrlKey: true })), true);
  assert.equal(ignoreKey(ev("j"), { locked: () => true }), true);
});

test("keyfilter: every kind of field is the field's, a dialog still hears Esc and Ctrl+K, and AltGr is not the palette", () => {
  for (const target of [{ tagName: "TEXTAREA" }, { tagName: "SELECT" }, { tagName: "DIV", isContentEditable: true }, { tagName: "INPUT", type: "search" }]) {
    assert.equal(typingIn(target), true, target.tagName);
    assert.equal(ignoreKey({ key: "3", target }), true, `a digit in ${target.tagName} is text`);
  }
  assert.equal(typingIn({ tagName: "INPUT", type: "range" }), false);
  assert.equal(typingIn(null), false);
  assert.equal(ignoreKey({ key: "j", target: { tagName: "INPUT", type: "checkbox" } }), false, "a checkbox takes no letters");
  assert.equal(ignoreKey({ key: "1", altKey: true, target: {} }), true, "Alt+digit is the browser's");
  assert.equal(ignoreKey({ key: "l", metaKey: true, target: {} }), true, "Cmd+L is the browser's");
  assert.equal(ignoreKey({ key: "k", ctrlKey: true, altKey: true, target: {} }), true, "AltGr combinations are not the palette");
  assert.equal(ignoreKey({ key: "K", metaKey: true, target: FIELD }), false, "Cmd+K opens the palette from a field");
  assert.equal(ignoreKey({ key: "Escape", target: {} }, { locked: () => true }), false, "Esc closes the dialog");
  assert.equal(ignoreKey({ key: "k", ctrlKey: true, target: {} }, { locked: () => true }), false, "Ctrl+K works under a dialog");
  assert.equal(ignoreKey({ key: "3", isComposing: true, target: {} }), true, "an input method is composing");
});

test("installKeys: a global key calls its handler with the key and prevents the default", () => {
  const { win, calls } = rig();
  const e = press(win, "3");
  assert.deepEqual(calls.map(([id, key]) => [id, key]), [["go", "3"]]);
  assert.equal(e.defaultPrevented, true);
  press(win, "t");
  assert.equal(calls.at(-1)[0], "theme");
  press(win, "Escape");
  assert.equal(calls.at(-1)[0], "close");
  press(win, "k", { ctrl: true });
  assert.equal(calls.at(-1)[0], "search");
});

test("installKeys: ? needs Shift on some keyboards and not on others, / likewise", () => {
  const { win, calls } = rig();
  press(win, "?", { shift: true });
  press(win, "?");
  assert.deepEqual(calls.map((c) => c[0]), ["help", "help"]);
  win.dispatchEvent(new Event("noop"));
});

test("installKeys: nothing fires in a field, in a local-key box, or on a repeat; Esc and Ctrl+K do", () => {
  const { win, calls } = rig();
  press(win, "1", { where: FIELD });
  press(win, "j", { where: LOCAL_BOX });
  press(win, "1", { repeat: true });
  assert.deepEqual(calls, []);
  press(win, "Escape", { where: FIELD });
  press(win, "k", { ctrl: true, where: LOCAL_BOX });
  assert.deepEqual(calls.map((c) => c[0]), ["close", "search"]);
});

test("installKeys: a dialog that owns the keyboard silences everything but Esc and Ctrl+K; an open drawer blocks 0-5 only", () => {
  const { win, calls, state } = rig();
  state.locked = true;
  press(win, "1"); press(win, "j");
  assert.deepEqual(calls, []);
  press(win, "Escape");
  assert.deepEqual(calls.map((c) => c[0]), ["close"]);
  state.locked = false; state.drawer = true; calls.length = 0;
  press(win, "2"); press(win, "0");
  assert.deepEqual(calls, [], "0-5 do not travel while the detail is open");
  press(win, "j"); press(win, "t");
  assert.deepEqual(calls.map((c) => c[0]), ["move", "theme"]);
});

test("installKeys: a row or stop key fires only in the stops where the table says it works", () => {
  const { win, calls, state } = rig();
  press(win, "h");
  assert.deepEqual(calls.map((c) => c[0]), ["done"], "h is HOY's");
  state.stop = "procesos"; calls.length = 0;
  const e = press(win, "h");
  assert.deepEqual(calls, []);
  assert.equal(e.defaultPrevented, false, "a key that means nothing here is left to the browser");
  press(win, "n"); press(win, "v");
  assert.deepEqual(calls.map((c) => c[0]), ["newtask", "field"]);
  state.stop = "cola"; calls.length = 0;
  press(win, "a"); press(win, "A", { shift: true });
  assert.deepEqual(calls.map((c) => c[0]), ["approve", "batch"], "Shift+A is the batch, a is one row");
  state.stop = null; calls.length = 0;
  press(win, "a"); press(win, "j");
  assert.deepEqual(calls, [], "the prologue has no rows");
});

test("installKeys: the chord f then 1 is a filter and never a trip to HOY, because sequences are bound first", () => {
  const { win, calls, state } = rig();
  state.stop = "procesos";
  press(win, "f"); press(win, "2");
  assert.deepEqual(calls.map(([id, key]) => [id, key]), [["filter", "f 2"]]);
  calls.length = 0;
  press(win, "2");
  assert.deepEqual(calls.map(([id, key]) => [id, key]), [["go", "2"]], "a lone 2 still travels");
});

test("installKeys: a verb the stop has not implemented is silent and is not prevented", () => {
  const { win, calls } = rig({ handlers: { go: () => {} } });
  const e = press(win, "a");
  assert.deepEqual(calls, []);
  assert.equal(e.defaultPrevented, false);
});

test("detach() takes the listener away", () => {
  const { win, calls, detach } = rig();
  detach();
  press(win, "1");
  assert.deepEqual(calls, []);
});

test("installKeys: Enter on a control (button, tab, link) is the control's own and is neither run nor prevented; on the page or a row it opens", () => {
  const { win, calls } = rig();
  const control = (tagName, token) => ({ tagName, closest: (selector) => (selector.includes(token) ? {} : null) });
  for (const where of [control("BUTTON", "button,"), control("DIV", "[role=tab]"), control("A", "a[href]"), control("SUMMARY", "summary")]) {
    const e = press(win, "Enter", { where });
    assert.equal(e.defaultPrevented, false, where.tagName + ": the browser keeps its click");
  }
  assert.deepEqual(calls, []);
  const e = press(win, "Enter");
  assert.deepEqual(calls.map((c) => c[0]), ["open"]);
  assert.equal(e.defaultPrevented, true);
});

test("installKeys: a chord that does not exist at this stop was never started, so its last press still does its own work", () => {
  const { win, calls, state } = rig();
  press(win, "f"); press(win, "2");
  assert.deepEqual(calls.map(([id, key]) => [id, key]), [["go", "2"]], "a stray f in HOY must not swallow the 2");
  state.stop = null; calls.length = 0;
  press(win, "f"); press(win, "0");
  assert.deepEqual(calls.map(([id, key]) => [id, key]), [["prologue", "0"]]);
  state.stop = "procesos"; state.drawer = true; calls.length = 0;
  press(win, "f"); press(win, "3");
  assert.deepEqual(calls.map(([id, key]) => [id, key]), [["filter", "f 3"]], "inside a stop that has the chord it is the chord, even with the detail open");
});
