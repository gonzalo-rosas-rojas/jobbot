// keyShown (ui/keys.js): what the key of an action looks like on a button, written once for HOY and COLA. Where they differ is a named option.
import test from "node:test";
import assert from "node:assert/strict";
import { KEYS, keyShown } from "../src/ui/keys.js";

test("Enter is drawn as the return arrow and any other key as the help sheet shows it", () => {
  assert.equal(keyShown("open"), "↵");
  assert.equal(keyShown("snooze"), "s");
  assert.equal(keyShown("done"), "h");
  assert.equal(keyShown("engine"), "p");
});

test("the batch key is 'Shift A' unless the room asks for the shifted glyph (COLA does, HOY has no such key)", () => {
  assert.equal(keyShown("batch"), "Shift A");
  assert.equal(keyShown("batch", { shiftGlyph: true }), "⇧A");
  assert.equal(keyShown("open", { shiftGlyph: true }), "↵", "the option adds a glyph and takes none away");
});

test("every key of the table is shown, and an id that is not in the table says which room asked", () => {
  for (const k of KEYS) assert.equal(typeof keyShown(k.id), "string", k.id);
  assert.throws(() => keyShown("nope"), /keys: "nope" is not an action of the key table/);
  assert.throws(() => keyShown("nope", { room: "hoy" }), /^Error: hoy: "nope" is not an action/);
  assert.throws(() => keyShown("nope", { room: "cola" }), /^Error: cola: "nope" is not an action/);
});
