import test from "node:test";
import assert from "node:assert/strict";
import { searchOf, partsOf, fold } from "../src/moments/procesos/search.js";

const items = [
  { key: "1", company: "Kestrel Labs", title: "Staff Backend Engineer" },
  { key: "2", company: "Ámbar Pay", title: "Platform Engineer" },
  { key: "3", company: "Nimbus Health", title: "AI Engineer" },
  { key: "4", company: "Omega (Test) [Co]", title: "Rust Developer" },
];
const hit = (found, key) => (found.get(key) || {}).ranges;

test("an empty query asks for nothing: no filter at all (null), not an empty list", () => {
  assert.equal(searchOf(items, ""), null);
  assert.equal(searchOf(items, "   "), null);
  assert.equal(searchOf(items, null), null);
});

test("it finds a company by a few letters, in any order, ignoring case and accents, and says which letters matched", () => {
  assert.deepEqual([...searchOf(items, "kes").keys()], ["1"]);
  assert.deepEqual([...searchOf(items, "LABS kestrel").keys()], ["1"], "out of order");
  assert.deepEqual([...searchOf(items, "ambar").keys()], ["2"], "Ámbar is found by ambar");
  assert.deepEqual([...searchOf(items, "engineer").keys()].sort(), ["1", "2", "3"], "the role counts too");
  const found = searchOf(items, "nimb");
  assert.deepEqual(found.get("3").parts.company.filter((p) => p.hit).map((p) => p.text.toLowerCase()).join(""), "nimb");
  assert.equal(found.get("3").parts.company.map((p) => p.text).join(""), "Nimbus Health", "the parts always spell the whole text");
});

test("no match is an empty map (the page says so), and a query made of regex characters is only text", () => {
  assert.equal(searchOf(items, "zzzqq").size, 0);
  for (const nasty of ["(", "[a-", "\\", ".*", "a|b", "$^", "((((", "?", "+++", "{1,", "<img onerror=x>"]) {
    assert.doesNotThrow(() => searchOf(items, nasty), nasty);
  }
  assert.deepEqual([...searchOf(items, "(test)").keys()], ["4"], "brackets in a name are letters of the name");
});

test("partsOf cuts a text at the ranges and clips a range that belongs to the other half", () => {
  assert.deepEqual(partsOf("Kestrel Labs", [0, 3]), [{ text: "Kes", hit: true }, { text: "trel Labs", hit: false }]);
  assert.deepEqual(partsOf("Kestrel Labs", [4, 7, 8, 10]), [{ text: "Kest", hit: false }, { text: "rel", hit: true }, { text: " ", hit: false }, { text: "La", hit: true }, { text: "bs", hit: false }]);
  assert.deepEqual(partsOf("abc", []), [{ text: "abc", hit: false }]);
  assert.deepEqual(partsOf("abc", undefined), [{ text: "abc", hit: false }]);
  // the company is the first part of "company title": a range that starts after it is the title's, and one that spans both is cut in two
  assert.deepEqual(partsOf("Kestrel", [5, 12], 0), [{ text: "Kestr", hit: false }, { text: "el", hit: true }]);
  assert.deepEqual(partsOf("Staff", [8, 11], 8), [{ text: "Sta", hit: true }, { text: "ff", hit: false }], "an offset moves the window to the second half");
  assert.deepEqual(partsOf("", [0, 3]), [], "nothing to say");
});

test("fold keeps the length of the text so that a range found in it fits the original", () => {
  for (const text of ["Ámbar", "Zürich Ñandú", "éclair", "naïve café", "日本語"]) assert.equal(fold(text).length, text.length, text);
  assert.equal(fold("Ámbar"), "Ambar");
});
