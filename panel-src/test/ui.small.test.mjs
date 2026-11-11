import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { routeOfHash, hashOf, restoreView, createView, useStorage, keep } from "../src/core/store.js";
import { createRouter } from "../src/ui/router.js";
import { TOAST_MS } from "../src/ui/toast.js";
import { STOP_IDS } from "../src/scene/poses.js";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (file) => readFileSync(path.join(root, file), "utf8").replace(/\r\n/g, "\n");

/** A window with only what the router touches, and a log of what it did to the history. */
function fakeWin(hash = "") {
  const log = [];
  const listeners = new Map();
  const win = {
    location: { hash, pathname: "/" },
    history: {
      pushState: (_s, _t, url) => { log.push(["push", url]); win.location.hash = url.slice(1); },
      replaceState: (_s, _t, url) => { log.push(["replace", url]); win.location.hash = url.slice(1); },
      back: () => log.push(["back"]),
    },
    addEventListener: (type, fn) => listeners.set(type, fn),
    removeEventListener: (type) => listeners.delete(type),
  };
  return { win, log, pop: (nextHash) => { win.location.hash = nextHash; listeners.get("popstate")?.(); }, listening: () => listeners.size };
}

test("an address names any of the six stops, the prologue included; the old #learn still opens Aprender", () => {
  for (const id of STOP_IDS) assert.deepEqual(routeOfHash("#" + id), { room: id, job: null }, id);
  assert.deepEqual(routeOfHash("#procesos/a%2Fb"), { room: "procesos", job: "a/b" });
  assert.deepEqual(routeOfHash("#learn"), { room: "aprender", job: null });
  assert.equal(routeOfHash("#nowhere"), null);
  assert.equal(hashOf("prologo"), "#prologo");
  useStorage({ local: new Map(), session: new Map() });
  keep.set("view", { room: "prologo", filter: "all", folds: {} });
  const view = createView();
  restoreView(view, { embedded: false, hash: "" });
  assert.equal(view.room, "hoy", "a saved view never reopens in the prologue: work mode opens on HOY");
  assert.deepEqual(restoreView(createView(), { hash: "#prologo" }), { room: "prologo", job: null }, "the address may");
  useStorage(null);
});

test("the router writes one history entry per jump and rewrites the current one for everything else", () => {
  const { win, log } = fakeWin("#hoy");
  const router = createRouter({ win });
  assert.equal(router.write("hoy"), false, "the same address writes nothing");
  assert.equal(router.write("procesos", null, { push: true }), true);
  assert.equal(router.write("cola", null, { push: false }), false);
  assert.equal(router.write("procesos", "fx:3", { push: true }), true);
  assert.equal(router.write("prologo", "ignored"), false, "the prologue has no application");
  assert.deepEqual(log, [["push", "/#procesos"], ["replace", "/#cola"], ["push", "/#procesos/fx%3A3"], ["replace", "/#prologo"]]);
  assert.deepEqual(router.read(), { stop: "prologo", job: null });
});

test("a panel inside another page never touches the history", () => {
  const { win, log } = fakeWin("");
  const router = createRouter({ embedded: true, win });
  assert.equal(router.write("cola", null, { push: true }), false);
  assert.deepEqual(log, []);
});

test("Back and Forward come out as { stop, job }; a stray anchor is not a place; the entry the drawer added is taken back without a trace", () => {
  const { win, pop, log, listening } = fakeWin("#hoy");
  const router = createRouter({ win });
  const seen = [];
  const off = router.listen((route) => seen.push(route));
  pop("#cola");
  pop("#procesos/fx%3A3");
  pop("#app");                                   // the skip link
  pop("");
  assert.deepEqual(seen, [{ stop: "cola", job: null }, { stop: "procesos", job: "fx:3" }, { stop: "hoy", job: null }]);
  router.back();
  assert.deepEqual(log.at(-1), ["back"]);
  pop("#procesos");                              // the pop that back() caused
  assert.equal(seen.length, 3, "our own pop is not reported");
  pop("#sistema");
  assert.deepEqual(seen.at(-1), { stop: "sistema", job: null });
  off();
  assert.equal(listening(), 0);
});

test("the toast lives four seconds", () => {
  assert.equal(TOAST_MS, 4000);
});

test("the tab never names the job: only title.js writes document.title, and only to put it back to Panel", () => {
  const files = (dir) => readdirSync(path.join(root, dir)).flatMap((n) => {
    const rel = path.join(dir, n);
    return statSync(path.join(root, rel)).isDirectory() ? files(rel) : rel.endsWith(".js") ? [rel] : [];
  });
  const writers = files("src").filter((f) => /document\.title\s*=[^=]/.test(read(f)));
  assert.deepEqual(writers.map((f) => f.replace(/\\/g, "/")), ["src/ui/title.js"]);
  assert.match(read("src/ui/title.js"), /document\.title = base/);
  assert.doesNotMatch(read("src/ui/title.js"), /"\("|\(\$\{/, "no count in the title text");
  assert.match(read("page.template.html"), /<title>Panel<\/title>/);
});

test("the cursor never calls scrollIntoView: that would move the page, and the camera belongs to the scene engine", () => {
  assert.doesNotMatch(read("src/ui/cursor.js").replace(/\/\/.*$/gm, ""), /scrollIntoView/);
});
