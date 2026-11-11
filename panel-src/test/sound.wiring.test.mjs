// The wiring of the vocabulary, read from the source: every sound the page asks for by name is a cue of src/sound/cues.js (or an old name that
// is mapped), and every cue is asked for by something, so that the sheet "Escuchar los sonidos" never lists a sound that cannot happen.
import test from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { CUES, ALIASES, cueOf } from "../src/sound/cues.js";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const TABLE = path.join(root, "src", "sound", "cues.js");

function files(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const full = path.join(dir, name);
    if (statSync(full).isDirectory()) files(full, out);
    else if (name.endsWith(".js")) out.push(full);
  }
  return out;
}

test("every sound the page asks for by name is a cue, and every cue is asked for by something", () => {
  const strip = (f) => readFileSync(f, "utf8").replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "");
  const sources = files(path.join(root, "src")).filter((f) => !f.includes(`${path.sep}i18n${path.sep}`));
  const text = sources.map(strip).join("\n");
  // The table names every cue by definition: a use is a quoted id anywhere EXCEPT the file that defines the table.
  const outside = sources.filter((f) => path.resolve(f) !== TABLE).map(strip).join("\n");
  const asked = new Set();
  for (const m of text.matchAll(/\b(?:sound|player|ctx\.sound|me\.ctx\.sound)\.(?:play|preview)\(\s*(?:"([^"]+)"|'([^']+)')/g)) asked.add(m[1] ?? m[2]);
  assert.ok(asked.size > 5, "the scan found the calls");
  for (const name of asked) assert.ok(Object.hasOwn(ALIASES, name) || cueOf(name, { room: "hoy" }), `play("${name}") is not a cue or an alias`);
  // the cue maps of the moments ({ approve: "approve", ... }) and the background detector name cues too: any quoted cue id counts
  const quoted = new Set([...outside.matchAll(/"([a-z][\w.]*)"/g)].map((m) => m[1]));
  for (const cue of CUES) {
    const used = quoted.has(cue.id) || (cue.id.startsWith("arrive.") && quoted.has("arrive"));
    assert.ok(used, `nothing in src/ outside cues.js plays "${cue.id}": a sound that never sounds should not be in the sheet`);
  }
});

