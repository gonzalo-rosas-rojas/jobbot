// The palette while the page learns something new (test/helpers/fake-dom.mjs). It asked for its entries only when it opened, so a palette opened
// before the first snapshot said "nothing matches" about applications that arrived a moment later, and kept saying it until it was closed. What
// it shows is now what the page knows: `refresh()` asks again, and draws again only when the rows it would show are other rows.
import test from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
const { createPalette } = await import("../src/ui/palette.js");
const { buildEntries, cardsOf } = await import("../src/ui/palette-items.js");
const { STOPS_META } = await import("../src/moments/registry.js");

const card = (key, company, title) => ({ key, company, title, state: "submitted" });
const snapOf = (cards) => ({ board: [{ state: "submitted", total: cards.length, cards }] });
const KESTREL = card("demo:kestrel", "Kestrel Labs", "Staff Backend Engineer");
const ALBA = card("demo:alba", "Alba Robotics", "Platform Engineer");
const noRun = () => {};

/** A palette over a snapshot the test can replace, as the shell gives it one. */
function rig(first = null) {
  dom.reset();
  const page = { snap: first, opened: [] };
  const palette = createPalette({
    root: dom.document.body, sound: { play() {} },
    source: () => ({
      entries: buildEntries({
        stops: STOPS_META, cards: cardsOf(page.snap), current: "hoy", state: { theme: "dark", lang: "es", sound: false, motion: "auto" },
        run: { go: noRun, job: (key) => page.opened.push(key), theme: noRun, lang: noRun, sound: noRun, motion: noRun, help: noRun },
      }),
      verbs: [],
    }),
  });
  const q = (selector) => dom.document.querySelector(selector);
  const type = (text) => { q("#palette-input").value = text; q("#palette-input").dispatchEvent({ type: "input", bubbles: true }); };
  const key = (name) => q("#palette-input").dispatchEvent({ type: "keydown", key: name, bubbles: true });
  const rows = () => dom.document.querySelectorAll('[role="option"]');
  const labels = () => rows().map((o) => o.querySelector(".palette-label").textContent);
  const active = () => rows().find((o) => o.getAttribute("aria-selected") === "true");
  return { page, palette, q, type, key, rows, labels, active };
}

test("opened before the first snapshot: the application that arrives is found without closing the palette", () => {
  const r = rig(null);
  r.palette.open();
  r.type("kestrel");
  assert.deepEqual(r.labels(), [], "nothing is known yet");
  assert.equal(r.q(".palette-empty").hidden, false);

  r.page.snap = snapOf([KESTREL]);
  r.palette.refresh();

  assert.deepEqual(r.labels(), ["Kestrel Labs"]);
  assert.equal(r.q(".palette-empty").hidden, true, "it no longer says that nothing matches");
  assert.equal(r.q("#palette-list").hidden, false);
  assert.equal(r.q("#palette-input").value, "kestrel", "what he typed is still there");
  assert.match(r.q('[role="status"]').textContent, /1 resultado/, "and the reader hears that there is a row now");
  r.key("Enter");
  assert.deepEqual(r.page.opened, ["demo:kestrel"], "Enter opens it");
});

test("the active row stays on the same application when the rows around it change", () => {
  const r = rig(snapOf([card("demo:eng-b", "Borealis", "Engineer"), card("demo:eng-c", "Cumulus", "Engineer")]));
  r.palette.open();
  r.type("engineer");
  r.key("ArrowDown");
  const chosen = r.active().querySelector(".palette-label").textContent;

  r.page.snap = snapOf([card("demo:eng-a", "Aurora", "Engineer"), card("demo:eng-b", "Borealis", "Engineer"), card("demo:eng-c", "Cumulus", "Engineer")]);
  r.palette.refresh();

  assert.equal(r.labels().length, 3);
  assert.equal(r.active().querySelector(".palette-label").textContent, chosen, "the row he was on, not the position it had");
  assert.equal(r.q("#palette-input").getAttribute("aria-activedescendant"), r.active().id);
});

test("the active row that is gone gives its place to the first", () => {
  const r = rig(snapOf([KESTREL, card("demo:kestrel-2", "Kestrel Works", "Engineer")]));
  r.palette.open();
  r.type("kestrel");
  r.key("ArrowDown");

  r.page.snap = snapOf([KESTREL]);
  r.palette.refresh();

  assert.deepEqual(r.labels(), ["Kestrel Labs"]);
  assert.equal(r.active(), r.rows()[0]);
});

test("news does not take the list from where he had scrolled it", () => {
  const r = rig(snapOf([KESTREL]));
  r.palette.open();
  r.type("kestrel");
  // a browser brings the active row into view when the rows are drawn; here that is the list back at its top
  Element.prototype.scrollIntoView = function scrollIntoView() { this.closest("#palette-list").scrollTop = 0; };
  try {
    r.q("#palette-list").scrollTop = 40;
    r.page.snap = snapOf([KESTREL, card("demo:kestrel-2", "Kestrel Works", "Engineer")]);
    r.palette.refresh();
    assert.equal(r.labels().length, 2, "the rows were drawn again");
    assert.equal(r.q("#palette-list").scrollTop, 40);
  } finally { delete Element.prototype.scrollIntoView; }
});

test("a snapshot that changes nothing he sees leaves the rows and the reader alone", () => {
  const r = rig(snapOf([KESTREL]));
  r.palette.open();
  r.type("kestrel");
  const row = r.rows()[0];
  const said = r.q('[role="status"]').firstChild;

  r.page.snap = snapOf([KESTREL, ALBA]);                    // one more application, but not one this query finds
  r.palette.refresh();

  assert.equal(r.rows()[0], row, "the same row, not a copy of it (the mouse over it, the reader on it)");
  assert.equal(r.q('[role="status"]').firstChild, said, "nothing is said again");
});

test("closed, a refresh does nothing; the next opening asks for itself", () => {
  const r = rig(null);
  r.palette.refresh();
  assert.equal(r.palette.isOpen(), false);
  r.page.snap = snapOf([KESTREL]);
  r.palette.open();
  r.type("kestrel");
  assert.deepEqual(r.labels(), ["Kestrel Labs"]);
});
