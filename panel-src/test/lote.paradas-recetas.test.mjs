// Acceptance test of the encargo docs/encargos/lote-paradas-y-recetas.md: small fixes of the panel in one run. (1) askOf, the question of a stop
// as the table of stops names it (moments/shared/stops.js), refuses a stop it does not know with an error that names it, not a bare TypeError;
// (2) the shell's announcement of a stop reads the question from that table too: no module but the table builds the key stop.<id>.ask by hand;
// (3) the engine has no recipe that no cue plays (press, release, flip, glance and focus go), META describes exactly the recipes that stay, and
// the old names press and release still reach what they reached (the fallback of HOY and COLA).
// No browser: askOf called, the sources of src/ read, the tables of sound/ compared.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { askOf, STOPS_META } from "../src/moments/shared/stops.js";
import { RECIPES, META } from "../src/sound/recipes.js";
import { CUES, ALIASES, cueOf } from "../src/sound/cues.js";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");   // panel-src
const SRC = path.join(ROOT, "src");

// ---- (1) askOf names the stop it does not know

test("askOf gives the question key of each stop of the table, and refuses an unknown stop with an error that names it", () => {
  for (const stop of STOPS_META) assert.equal(askOf(stop.id), stop.ask, stop.id);
  for (const id of ["nope", "tower"]) {
    assert.throws(() => askOf(id), (e) => {
      assert.ok(e instanceof Error, `askOf("${id}") throws an Error`);
      assert.ok(e.message.includes(id), `the message names the stop it was asked for: "${e.message}"`);
      return true;
    });
  }
});

// ---- (2) the question key of a stop is written only in the table

function sources(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, e.name);
    if (e.isDirectory()) sources(full, out);
    else if (e.name.endsWith(".js")) out.push(full);
  }
  return out;
}

// a quoted "stop." that something is glued to, a template that starts `stop.${...}`, or a whole key stop.<id>.ask written out
const BY_HAND = [/["'`]stop\.["'`]/, /`stop\.\$\{/, /["'`]stop\.[a-z]+\.ask["'`]/];
const TABLE = path.join(SRC, "moments", "shared", "stops.js");
const WORDS = path.join(SRC, "i18n") + path.sep;

test("the question of a stop is named only by the table of stops: no module of src/ builds the key stop.<id>.ask by hand", () => {
  const hits = [];
  for (const file of sources(SRC)) {
    if (file === TABLE || file.startsWith(WORDS)) continue;
    fs.readFileSync(file, "utf8").split("\n").forEach((line, i) => {
      if (BY_HAND.some((re) => re.test(line))) hits.push(`${path.relative(ROOT, file).replace(/\\/g, "/")}:${i + 1}`);
    });
  }
  assert.deepEqual(hits, [], "read the question key from the table (askOf, or STOPS_META through moments/registry.js) instead of writing it again");
});

// ---- (3) the recipes of the engine are the ones the cues play

test("every recipe of recipes.js is the recipe of at least one cue, and META describes exactly those recipes", () => {
  const played = new Set(CUES.map((cue) => cue.recipe));
  const orphans = Object.keys(RECIPES).filter((name) => !played.has(name));
  assert.deepEqual(orphans, [], "a recipe no cue plays is dead weight in the page: delete it (with its META entry)");
  assert.deepEqual(Object.keys(META).sort(), Object.keys(RECIPES).sort(), "META has one entry per recipe, no more");
});

test("the old names still reach their cues: press is the second press (the fallback of HOY and COLA), release stays silent on purpose", () => {
  assert.equal(cueOf("press")?.id, "confirm");
  assert.equal(cueOf("release"), null);
  assert.deepEqual(Object.keys(ALIASES).sort(), ["ceremony", "press", "release", "tick", "toggle"]);
});
