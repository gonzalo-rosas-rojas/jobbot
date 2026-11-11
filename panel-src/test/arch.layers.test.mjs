// The import direction of the page, as a test (AGENT-RULES.md, "The layers"; docs/adr/0004-the-moment-contract.md). It asks esbuild for the import graph of
// src/main.js (the metafile: no new dependency, about a third of a second) and judges every edge between two layers against RULES. The edges that exist today
// and should not are listed in DEBT. The test fails on a NEW wrong edge, and ALSO when a DEBT entry no longer exists, so the list can only shrink: pay a debt and
// you must delete its line. Only files that the bundle reaches are seen; a file that nothing imports is not judged here.
import test from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");

// layer -> the layers it may import. A file of src/moments/ may import another moment only through ./registry.js (the shell's door), see violation().
const RULES = {
  main: ["core", "i18n", "scene", "stage", "sound", "ui", "moments", "lib"],
  core: ["core", "i18n", "lib"],
  i18n: ["i18n"],
  scene: ["scene", "core", "lib"],
  stage: ["stage", "core", "scene"],
  sound: ["sound", "core"],
  ui: ["ui", "core", "i18n", "scene", "sound", "lib"],
  moments: ["moments", "core", "i18n", "ui", "scene", "lib"],
};

// "from -> to" at file level: the edges that go the wrong way today. Nobody adds to this list; it only loses lines as the code is moved to where it belongs.
const DEBT = new Set([
  // the dictionary index reads the stored language from the store, and the core reads words from the dictionary: a cycle between two layers
  "src/i18n/index.js -> src/core/store.js",
  // pieces that sit in the wrong layer: the scene reads a key filter of the ui, the sound reads the clock of the scene
  "src/scene/camera.js -> src/ui/keyfilter.js",
  "src/sound/player.js -> src/scene/clock.js",
  "src/sound/background.js -> src/scene/clock.js",
  // the palette of the ui ranks the applications with the search of PROCESOS
  "src/ui/palette-rank.js -> src/moments/procesos/search.js",
  // the shared kernel nobody named: HOY's actions.js / forms.js are used by PROCESOS (they belong in core/ or in a shared place; util.js moved to moments/shared/)
  "src/moments/procesos.js -> src/moments/hoy/actions.js", "src/moments/procesos.js -> src/moments/hoy/forms.js",
]);

const layerOf = (file) => {
  if (file.includes("node_modules")) return "lib";
  const parts = file.split("/");
  return parts[0] === "src" ? (parts[1].endsWith(".js") ? "main" : parts[1]) : "other";
};
const momentOf = (file) => { const p = file.split("/"); return p[1] === "moments" ? p[2].replace(/\.js$/, "") : null; };

/** True when `from` importing `to` (paths as the metafile writes them, "src/...") goes against the rules. */
function violation(from, to) {
  const a = layerOf(from);
  const b = layerOf(to);
  if (a === "ui" && to === "src/moments/registry.js") return false;      // the shell and its keys reach the moments only through the registry
  if (a in RULES && !RULES[a].includes(b)) return true;
  if (a === "moments" && b === "moments") {
    const ma = momentOf(from);
    const mb = momentOf(to);
    // between two moments only the registry may reach (it is the table of all of them), and moments/shared/ is the declared kernel any moment may use
    if (ma !== mb && ma !== "registry" && mb !== "shared") return true;
  }
  return false;
}

async function edges() {
  const r = await build({ entryPoints: [path.join(ROOT, "src/main.js")], absWorkingDir: ROOT, bundle: true, write: false, metafile: true, format: "iife", logLevel: "silent", define: { __DEFAULT_LANG__: '"es"', "process.env.NODE_ENV": '"production"' } });
  const out = [];
  for (const [from, info] of Object.entries(r.metafile.inputs)) for (const imp of info.imports) if (!imp.external) out.push([from, imp.path]);
  return out;
}

test("the layers of the page import in one direction (a ratchet: the debts can only shrink)", async () => {
  const found = new Set();
  for (const [from, to] of await edges()) if (violation(from, to)) found.add(`${from} -> ${to}`);
  const fresh = [...found].filter((e) => !DEBT.has(e));
  const gone = [...DEBT].filter((e) => !found.has(e));
  assert.deepEqual(fresh, [], "a new import crosses the layers the wrong way (AGENT-RULES.md, \"The layers\"): move the code to where the table puts it; do not add it to DEBT");
  assert.deepEqual(gone, [], "a debt was paid (or its file moved): delete these lines from DEBT");
});

test("the rule itself: what each layer may import", () => {
  const bad = [
    ["src/core/format.js", "src/ui/keys.js"],                           // the core knows no ui
    ["src/core/model.js", "src/moments/hoy/view.js"],
    ["src/scene/engine.js", "src/stage/sky.js"],
    ["src/stage/stage.js", "src/ui/hud.js"],
    ["src/sound/player.js", "src/ui/keys.js"],
    ["src/i18n/index.js", "src/core/store.js"],
    ["src/ui/shell.js", "src/moments/hoy.js"],                          // the ui reaches a moment only through the registry
    ["src/moments/cola/view.js", "src/moments/hoy/util.js"],            // a moment reaching into another
    ["src/moments/procesos.js", "src/moments/hoy/actions.js"],
    ["src/moments/hoy.js", "src/stage/sky.js"],
    ["src/moments/hoy.js", "src/sound/player.js"],
    ["src/main.js", "scripts/demo/demo.js"],                            // nothing outside src/ is part of the page
    ["src/core/dom.js", "src/other.js"],
  ];
  for (const [from, to] of bad) assert.equal(violation(from, to), true, `${from} -> ${to} should be refused`);
  const good = [
    ["src/main.js", "src/ui/shell.js"],
    ["src/core/dom.js", "src/i18n/index.js"],
    ["src/core/dom.js", "node_modules/tinykeys/dist/tinykeys.module.js"],
    ["src/stage/stage.js", "src/scene/poses.js"],
    ["src/ui/shell.js", "src/moments/registry.js"],                     // the door
    ["src/ui/shell.js", "src/scene/poses.js"],
    ["src/moments/registry.js", "src/moments/hoy.js"],                  // the registry is the table of all of them
    ["src/moments/hoy.js", "src/moments/hoy/view.js"],                  // a moment and its own parts
    ["src/moments/hoy/render.js", "src/moments/hoy/util.js"],
    ["src/moments/hoy/view.js", "src/moments/shared/armer.js"],         // the declared shared kernel
    ["src/moments/cola/view.js", "src/ui/keys.js"],
    ["src/moments/cola/view.js", "src/core/model.js"],
  ];
  for (const [from, to] of good) assert.equal(violation(from, to), false, `${from} -> ${to} should be allowed`);
});
