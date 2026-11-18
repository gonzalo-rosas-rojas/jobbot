import test from "node:test";
import assert from "node:assert/strict";

// A page that records which listeners are put on it, in the order they come.
const fakeDocument = () => {
  const added = [];
  return { added, addEventListener: (type, fn, capture) => added.push({ type, fn, capture }) };
};

test("importing core/dom.js registers nothing; trackHeldKeys() does, and heldKey() follows the repeats", async () => {
  const page = fakeDocument();
  globalThis.document = page;
  const dom = await import("../src/core/dom.js");
  assert.equal(page.added.length, 0, "an import must not touch the page");
  assert.equal(dom.heldKey(), false);

  dom.trackHeldKeys(page);
  assert.deepEqual(page.added.map((a) => a.type), ["keydown", "keyup"]);
  assert.ok(page.added.every((a) => a.capture === true), "seen in the capture phase, before the click a repeat causes");

  const [keydown, keyup] = page.added.map((a) => a.fn);
  keydown({ repeat: false });
  assert.equal(dom.heldKey(), false, "the first press is not a repeat");
  keydown({ repeat: true });
  assert.equal(dom.heldKey(), true, "a repeat means the key is held");
  keyup();
  assert.equal(dom.heldKey(), false, "letting go ends it");
  delete globalThis.document;
});
