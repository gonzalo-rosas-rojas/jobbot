// The three snapshot fixtures are what test/fixtures/make.mjs writes. They are compared as data: the files have also been edited by hand, so
// their layout is not the generator's, and a contract field added to them by hand (the `undo` of a card was one) must reach the generator too,
// or the next person who runs it loses that field without being told.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { patch, heavy } from "./fixtures/make.mjs";

const read = (name) => JSON.parse(readFileSync(new URL("./fixtures/" + name, import.meta.url), "utf8"));

test("running the generator again changes no data in the two hand-made fixtures", () => {
  assert.deepStrictEqual(patch(read("snapshot.empty.json"), { empty: true }), read("snapshot.empty.json"));
  assert.deepStrictEqual(patch(read("snapshot.rich.json"), { empty: false }), read("snapshot.rich.json"));
});

test("the heavy fixture is the one the generator makes from the rich one", () => {
  assert.deepStrictEqual(heavy(patch(read("snapshot.rich.json"), { empty: false })), read("snapshot.heavy.json"));
});
