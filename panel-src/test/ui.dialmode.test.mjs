// The dial's mode: what the stage drew becomes "big", "docked" or "none" (scene/camera.js dialModeOf), and the HUD carries it as data-dial (ui/hud.js setDialMode):
// "none" takes the attribute away, and the same mode again writes nothing.
import test from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./helpers/fake-dom.mjs";

installFakeDom();
const { dialModeOf } = await import("../src/scene/camera.js");
const { setDialMode } = await import("../src/ui/hud.js");

test("dialModeOf: nothing drawn is none; a drawn dial is docked or big by its layout", () => {
  assert.equal(dialModeOf(null), "none");
  assert.equal(dialModeOf(undefined), "none");
  assert.equal(dialModeOf({ layout: { docked: true } }), "docked");
  assert.equal(dialModeOf({ layout: { docked: false } }), "big");
});

/** An element whose dataset counts the writes. */
function counted() {
  const store = {};
  const log = [];
  const dataset = new Proxy(store, {
    set(o, k, v) { log.push(["set", k, v]); o[k] = v; return true; },
    deleteProperty(o, k) { log.push(["delete", k]); delete o[k]; return true; },
  });
  return { el: { dataset }, store, log };
}

test("setDialMode: big and docked leave data-dial with that value, none removes it, and the same mode again writes nothing", () => {
  const { el, store, log } = counted();
  setDialMode(el, "big");
  assert.equal(store.dial, "big");
  setDialMode(el, "big");
  assert.equal(log.length, 1, "the same mode is not written again");
  setDialMode(el, "docked");
  assert.equal(store.dial, "docked");
  setDialMode(el, "none");
  assert.equal("dial" in store, false);
  assert.deepEqual(log.map((x) => x[0]), ["set", "set", "delete"]);
});
