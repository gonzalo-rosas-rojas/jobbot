// The tab has ONE icon. The published demo declares its own <link rel="icon" href="favicon.svg"> in its markup; title.js must draw its counter
// icon (a blob) onto that link instead of adding a second one, and keep creating the link itself when the page has none (the real panel).
import test from "node:test";
import assert from "node:assert/strict";

/** A document with just what title.js touches: a head that holds links, a canvas that hands back a blob, and tokens it can read. */
function fakeDocument(links) {
  const head = { links, append(el) { links.push(el); } };
  const canvas = { getContext: () => new Proxy({}, { get: () => () => {}, set: () => true }), toBlob: (cb) => cb(new Blob(["x"])) };
  return {
    title: "",
    head,
    documentElement: {},
    createElement: (tag) => (tag === "canvas" ? canvas : { tag, rel: "", href: "" }),
    querySelector: (sel) => (sel === 'link[rel="icon"]' ? links.find((l) => l.rel === "icon") || null : null),
  };
}

async function drawOnce(doc) {
  const saved = { document: globalThis.document, getComputedStyle: globalThis.getComputedStyle };
  globalThis.document = doc;
  globalThis.getComputedStyle = () => ({ getPropertyValue: () => "#fff" });
  try {
    const { createTitle } = await import("../src/ui/title.js");
    createTitle().update(null);
    await new Promise((r) => setTimeout(r, 20));
  } finally { Object.assign(globalThis, saved); }
}

test("a link rel=icon already in the document receives the blob and no second link is added", async () => {
  const mine = { tag: "link", rel: "icon", type: "image/svg+xml", href: "favicon.svg" };
  const links = [mine];
  await drawOnce(fakeDocument(links));
  assert.equal(links.length, 1, "still one link");
  assert.ok(links[0] === mine && mine.href.startsWith("blob:"), `the existing link now points at the blob (${mine.href})`);
});

test("without a link rel=icon the title makes one, as before", async () => {
  const links = [];
  await drawOnce(fakeDocument(links));
  assert.equal(links.length, 1);
  assert.equal(links[0].rel, "icon");
  assert.ok(links[0].href.startsWith("blob:"));
});
