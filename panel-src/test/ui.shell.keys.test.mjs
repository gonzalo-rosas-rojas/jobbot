import test from "node:test";
import assert from "node:assert/strict";
import { createKeyHandlers, MOMENT_VERBS } from "../src/ui/shell-keys.js";
import { KEYS } from "../src/ui/keys.js";
import { MOMENTS } from "../src/moments/registry.js";
import hoy from "../src/moments/hoy.js";

// The collaborators are fakes that write what they were asked to a log, so the order of Esc and the owner of each verb can be read.
function rig({ stop = "hoy", drawerOpen = false, helpOpen = false, menuOpen = false, paletteOpen = false, sheetOpen = false } = {}) {
  const log = [];
  const state = { stop, drawerOpen, helpOpen, menuOpen, paletteOpen, sheetOpen, row: null, job: null };
  const cursor = { move: (n) => log.push(["cursor.move", n]), clear: () => log.push(["cursor.clear"]), current: () => state.row, jobOf: () => state.job };
  const drawer = { isOpen: () => state.drawerOpen, step: (n) => log.push(["drawer.step", n]), close: () => log.push(["drawer.close"]), open: (job, row) => log.push(["drawer.open", job, row]) };
  const help = { isOpen: () => state.helpOpen, close: () => log.push(["help.close"]), toggle: () => log.push(["help.toggle"]) };
  const menu = { isOpen: () => state.menuOpen, close: () => log.push(["menu.close"]) };
  const palette = { isOpen: () => state.paletteOpen, open: () => log.push(["palette.open"]), close: () => log.push(["palette.close"]) };
  const tools = { theme: () => log.push(["theme"]), sound: () => log.push(["sound"]), lang: () => log.push(["lang"]) };
  const sheet = { isOpen: () => state.sheetOpen, close: () => log.push(["sheet.close"]) };       // the sound's "Escuchar los sonidos"
  const handlers = createKeyHandlers({ stop: () => state.stop, cursor, drawer, help, menu, palette, tools, sheet, go: (id) => log.push(["go", id]), cancelJump: () => log.push(["cancelJump"]) });
  return { handlers, log, state };
}

test("1 to 5 travel to the five stops, 0 to the prologue, and the page keys reach their tools", () => {
  const { handlers, log } = rig();
  for (const n of ["1", "2", "3", "4", "5"]) handlers.go(n);
  handlers.prologue("0");
  for (const id of ["search", "help", "theme", "sound", "lang"]) handlers[id](id);
  assert.deepEqual(log, [["go", "hoy"], ["go", "procesos"], ["go", "cola"], ["go", "aprender"], ["go", "sistema"], ["go", "prologo"], ["palette.open"], ["help.toggle"], ["theme"], ["sound"], ["lang"]]);
});

test("Esc closes one layer per press, in the order help, palette, menu, drawer, cursor, and always stops a trip first", () => {
  const order = [];
  for (const [open, expect] of [
    [{ helpOpen: true, paletteOpen: true, menuOpen: true, drawerOpen: true }, "help.close"],
    [{ paletteOpen: true, menuOpen: true, drawerOpen: true }, "palette.close"],
    [{ menuOpen: true, drawerOpen: true }, "menu.close"],
    [{ drawerOpen: true }, "drawer.close"],
    [{}, "cursor.clear"],
  ]) {
    const { handlers, log } = rig(open);
    handlers.close("Escape");
    assert.deepEqual(log.map((l) => l[0]), ["cancelJump", expect], JSON.stringify(open));
    order.push(expect);
  }
  assert.deepEqual(order, ["help.close", "palette.close", "menu.close", "drawer.close", "cursor.clear"]);
});

test("Ctrl K opens the palette, and says nothing while the help or a menu has the keyboard: one layer at a time", () => {
  for (const open of [{ helpOpen: true }, { menuOpen: true }]) {
    const { handlers, log } = rig(open);
    handlers.search("$mod+k");
    assert.deepEqual(log, [], JSON.stringify(open));
  }
  const { handlers, log } = rig({ paletteOpen: true });
  handlers.search("$mod+k");
  assert.deepEqual(log, [["palette.open"]], "with the palette open the key brings the focus back to its field (the palette does that in open())");
});

test("a moment that takes a verb keeps it; one that does not leaves j, k and Enter to the page", () => {
  const { handlers, log, state } = rig({ stop: "procesos" });
  state.row = { dataset: {} };
  state.job = "k1";
  const original = MOMENTS.procesos.onKey;
  const asked = [];
  MOMENTS.procesos.onKey = (verb, row, event) => { asked.push([verb, row, event.key]); return verb === "filter"; };
  try {
    handlers.filter("f 2", { key: "2" });
    assert.deepEqual(asked, [["filter", state.row, "2"]], "the moment hears the verb, the row under the cursor and the event");
    assert.deepEqual(log, [], "it took the key: the page does nothing more");
    handlers.move("j", { key: "j" });
    handlers.open("Enter", { key: "Enter" });
    assert.deepEqual(log, [["cursor.move", 1], ["drawer.open", "k1", state.row]]);
  } finally { MOMENTS.procesos.onKey = original; }
});

test("with the drawer open j and k change the application, and a row verb still goes to the moment", () => {
  const { handlers, log, state } = rig({ stop: "hoy", drawerOpen: true });
  handlers.move("j", { key: "j" });
  handlers.move("k", { key: "k" });
  assert.deepEqual(log, [["drawer.step", 1], ["drawer.step", -1]]);
  let heard = null;
  const original = hoy.onKey;
  hoy.onKey = (verb) => { heard = verb; return true; };
  try { handlers.approve("a", { key: "a" }); } finally { hoy.onKey = original; }
  assert.equal(heard, "approve");
  assert.ok(state.drawerOpen);
});

test("Enter with no row under the cursor and no moment to take it does nothing", () => {
  const { handlers, log } = rig({ stop: "prologo" });
  handlers.open("Enter", { key: "Enter" });
  assert.deepEqual(log, []);
});

test("every verb of the key table that the page owns has a handler, so no listed key is dead; the drill's keys are the box's own", () => {
  const { handlers } = rig();
  for (const k of KEYS.filter((x) => x.scope !== "local")) assert.equal(typeof handlers[k.id], "function", `${k.id} has no handler`);
  for (const verb of MOMENT_VERBS) assert.equal(typeof handlers[verb], "function", verb);
});

test("the sound sheet is a layer of its own: Esc closes it before the others, and Ctrl K closes it and opens the palette", () => {
  const esc = rig({ sheetOpen: true, helpOpen: true, drawerOpen: true });
  esc.handlers.close("Escape");
  assert.deepEqual(esc.log, [["cancelJump"], ["sheet.close"]], "one layer per press, and this one is on top");
  const search = rig({ sheetOpen: true });
  search.handlers.search("$mod+k");
  assert.deepEqual(search.log, [["sheet.close"], ["palette.open"]], "the palette is not opened behind a dialog that stays");
  const none = rig();
  none.handlers.search("$mod+k");
  assert.deepEqual(none.log, [["palette.open"]], "with no sheet nothing changes");
  const without = createKeyHandlers({ stop: () => "hoy", cursor: { clear() {} }, drawer: { isOpen: () => false }, help: { isOpen: () => false }, menu: { isOpen: () => false }, palette: { isOpen: () => false, open() {} }, tools: {}, go() {}, cancelJump() {} });
  assert.doesNotThrow(() => { without.close("Escape"); without.search("$mod+k"); }, "a shell without the sheet (a lab) works as before");
});
