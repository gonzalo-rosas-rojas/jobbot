// The words of the sound vocabulary: every cue has a name and a line saying when it sounds, in both languages, and every group and bus has a title.
import test from "node:test";
import assert from "node:assert/strict";
import { CUES, GROUPS, BUSES } from "../src/sound/cues.js";
import esSound from "../src/i18n/es/sound.js";
import enSound from "../src/i18n/en/sound.js";

test("every cue has a name and a line saying when it sounds, in both languages, and every group has a title", () => {
  for (const [lang, dict] of [["es", esSound], ["en", enSound]]) {
    for (const cue of CUES) {
      for (const part of ["name", "when"]) {
        const text = dict[`sound.cue.${cue.id}.${part}`];
        assert.ok(typeof text === "string" && text.trim().length > 2, `${lang}: sound.cue.${cue.id}.${part}`);
      }
    }
    for (const group of Object.values(GROUPS).flat()) assert.ok(dict[`sound.group.${group}`], `${lang}: sound.group.${group}`);
    for (const bus of Object.keys(BUSES)) assert.ok(dict[`sound.bus.${bus}`], `${lang}: sound.bus.${bus}`);
  }
});
