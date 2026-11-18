// A two-step button of the drawer arms with the sound and the announcement of the drawing it belongs to: drawing the drawer again
// (another application, a new language) must not hand an older button the sound or the live region of the newest drawing.
import test from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./helpers/fake-dom.mjs";

installFakeDom();
const { h } = await import("../src/core/dom.js");
const { t } = await import("../src/i18n/index.js");
const { armed } = await import("../src/ui/detail/act.js");

function drawing() {
  const log = { sounds: [], said: [] };
  return { log, voices: { sound: { play: (cue) => log.sounds.push(cue) }, announce: (text) => log.said.push(text) } };
}

test("the first press of an older drawing's button sounds and announces on that drawing, not on the newest one", () => {
  const older = drawing();
  const newest = drawing();
  const press = armed("Approve", () => {}, older.voices);
  armed("Reject", () => {}, newest.voices); // a later drawing exists
  press({ currentTarget: h("button", {}, "Approve") });
  assert.deepEqual(older.log.sounds, ["arm"]);
  assert.deepEqual(older.log.said, [t("act.sure")]);
  assert.deepEqual(newest.log.sounds, []);
  assert.deepEqual(newest.log.said, []);
});
