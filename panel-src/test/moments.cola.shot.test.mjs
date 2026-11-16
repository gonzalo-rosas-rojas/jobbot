// COLA's capture of the dry run (moments/cola/shot.js) over the fake DOM: what the plane says for no proposal, a proposal with no capture, a capture that
// loads, one that fails and one that is no longer wanted when it arrives.
import test from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
const { setLang, setTimeZone, t } = await import("../src/i18n/index.js");
const { createShot } = await import("../src/moments/cola/shot.js");

const flush = () => new Promise((resolve) => setImmediate(resolve));

function rig(blobOf) {
  setLang("es");
  setTimeZone("America/Santiago");
  dom.reset();
  const asked = [];
  const session = { blob: (path) => { asked.push(path); return blobOf(path); } };
  const shot = createShot({ session });
  dom.document.body.append(shot.el);
  return { shot, asked, state: () => shot.el.dataset.state, note: () => shot.el.querySelector(".plane-note").textContent };
}

test("no proposal on screen hides the plane; a proposal with no capture says so and asks nothing", () => {
  const r = rig(() => null);
  r.shot.show(null);
  assert.equal(r.shot.el.hidden, true);
  assert.equal(r.state(), "off");
  r.shot.show({ key: null, preparedAt: null });
  assert.equal(r.shot.el.hidden, false);
  assert.equal(r.state(), "none");
  assert.equal(r.note(), t("cola.shot.none"));
  assert.deepEqual(r.asked, []);
  r.shot.destroy();
});

test("a capture that loads is asked for once, shown as a picture, and kept while it is the one on screen", async () => {
  const r = rig(() => new Blob(["x"]));
  r.shot.show({ key: "fx:1", preparedAt: "2026-10-07T06:00:00Z" });
  assert.equal(r.state(), "loading");
  await flush();
  assert.equal(r.state(), "ok");
  assert.equal(r.shot.el.querySelectorAll("img").length, 1);
  r.shot.show({ key: "fx:1", preparedAt: "2026-10-07T06:00:00Z" });
  assert.deepEqual(r.asked, ["/api/shot?key=fx%3A1"], "the same proposal is not fetched again");
  r.shot.destroy();
});

test("a capture that fails is the error state with its retry, and the cause does not escape", async () => {
  const r = rig(() => Promise.reject(new Error("net")));
  r.shot.show({ key: "fx:1", preparedAt: null });
  await flush();
  assert.equal(r.state(), "error");
  assert.equal(r.note(), t("cola.shot.error"));
  assert.equal(r.shot.el.querySelector(".plane-retry").hidden, false);
  r.shot.destroy();
});

test("a capture that arrives for a proposal that is no longer on screen is dropped", async () => {
  let release;
  const r = rig(() => new Promise((resolve) => { release = resolve; }));
  r.shot.show({ key: "fx:1", preparedAt: null });
  r.shot.show({ key: null, preparedAt: null });
  release(new Blob(["x"]));
  await flush();
  assert.equal(r.state(), "none");
  assert.equal(r.shot.el.querySelectorAll("img").length, 0);
  r.shot.destroy();
});
