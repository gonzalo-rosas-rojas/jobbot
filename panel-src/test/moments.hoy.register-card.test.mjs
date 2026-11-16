// HOY mounted over the fake DOM: the form of "registrar" (r) reads the card HOY drew the row from. When the same key is in a column of the board and among the
// ones that wait for him, the row is drawn from the last (the index's rule), so the choices must come from that one, not from the board's.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
const { setLang, setTimeZone } = await import("../src/i18n/index.js");
const { model } = await import("../src/core/model.js");
const { hoyView } = await import("../src/moments/hoy/view.js");
const { registerChoices } = await import("../src/moments/hoy/forms.js");
const hoy = (await import("../src/moments/hoy.js")).default;

const base = JSON.parse(readFileSync(new URL("./fixtures/snapshot.rich.json", import.meta.url), "utf8"));
const nowMs = Date.parse(base.generated_at);

test("the choices of 'registrar' come from the card the row was drawn from, not from the board's copy of the same key", async () => {
  setLang("es");
  setTimeZone("America/Santiago");
  dom.reset();
  const snap = structuredClone(base);
  const waiting = snap.needs_gonza.cards[0];
  waiting.marks = ["replied", "withdrawn"];
  const column = snap.board.find((col) => col.state === "prepared");
  column.cards = [{ ...waiting, state: "prepared", marks: ["rejected", "withdrawn"] }, ...column.cards];
  const m = model(snap);
  const rowId = [hoyView(m, { nowMs }).next, ...hoyView(m, { nowMs }).rows].filter(Boolean).find((r) => r.data.card === waiting.key).id;

  let menuOpened = null;
  const host = dom.document.createElement("div");
  dom.document.body.append(host);
  hoy.mount(host, {
    session: { act: async () => ({ ok: true }), fresh: new Set() },
    sound: { play() {} }, announce() {}, toast: null, open() {}, go() {}, reduced: () => true, now: () => nowMs, lastVisit: () => 0,
    menu: { open: (options) => { menuOpened = options; options.onClose("esc"); } },
  });
  hoy.update(m, { session: { fresh: new Set() }, now: () => nowMs, lastVisit: () => 0 });
  assert.equal(hoy.onKey("register", { dataset: { row: rowId } }, null), true);
  await new Promise((resolve) => setImmediate(resolve));
  hoy.unmount();

  assert.ok(menuOpened, "the menu of answers opened");
  const fromRow = registerChoices(waiting).map((c) => c.label);
  const fromBoard = registerChoices(column.cards[0]).map((c) => c.label);
  assert.notDeepEqual(fromRow, fromBoard, "the two copies would offer different answers");
  assert.deepEqual(menuOpened.items.map((i) => i.label), fromRow);
});
