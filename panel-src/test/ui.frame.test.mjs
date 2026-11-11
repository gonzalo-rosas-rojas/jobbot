import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { setLang, setTimeZone } from "../src/i18n/index.js";
import { ICONS, icon } from "../src/ui/icons.js";
import { ICONS as TOOL_ICONS } from "../tools/icons.mjs";
import { statusParts } from "../src/ui/hud.js";
import { countWords } from "../src/ui/stops.js";
import { STOPS } from "../src/stage/sky.js";
import { tokensOf, contrast, rgb } from "./helpers/color.mjs";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (file) => readFileSync(path.join(root, file), "utf8").replace(/\r\n/g, "\n");
const NOW = Date.parse("2026-10-07T12:00:00Z");                    // 09:00 in Santiago
const HOUR = 3600000;
const controls = (over = {}) => ({ controls: { stopped: false, window_open: false, next_window: "2026-10-07 18:00", last_run_at: new Date(NOW - 7 * HOUR).toISOString(), engine: "recent", ...over } });
const fresh = (lang = "es") => { setLang(lang); setTimeZone("America/Santiago"); };

test("the icon list is closed: one list for the code and the sprite tool, every name drawn in src is in it", () => {
  assert.equal(TOOL_ICONS, ICONS, "tools/icons.mjs reads the list of ui/icons.js");
  assert.equal(new Set(ICONS).size, ICONS.length);
  assert.throws(() => icon("sparkles"), /not one of the 33 icons/);
  const sprite = read("assets/icons.svg");
  for (const name of ICONS) assert.ok(sprite.includes(`<symbol id="i-${name}"`), name);
  const files = (dir) => readdirSync(path.join(root, dir)).flatMap((n) => {
    const rel = path.join(dir, n);
    return statSync(path.join(root, rel)).isDirectory() ? files(rel) : rel.endsWith(".js") ? [rel] : [];
  });
  for (const file of files("src")) {
    for (const m of read(file).matchAll(/\bicon\(\s*["']([a-z0-9-]+)["']/g)) assert.ok(ICONS.includes(m[1]), `${file} draws "${m[1]}", which is not in the closed list`);
  }
});

test("statusParts words the window and the run from the snapshot, and says nothing it does not know", () => {
  fresh();
  assert.deepEqual(statusParts(controls(), NOW), { window: "Cerrada, abre 18:00", run: "Corrida hace 7 h" });
  assert.deepEqual(statusParts(controls({ window_open: true, next_window: null }), NOW), { window: "Ventana abierta, cierra 09:00", run: "Corrida hace 7 h" });
  assert.equal(statusParts(controls({ stopped: true }), NOW).window, "STOP activo · Cerrada, abre 18:00");
  assert.equal(statusParts(controls({ window_open: null, next_window: null }), NOW).window, "Ventana sin datos");
  assert.equal(statusParts(controls({ next_window: null }), NOW).window, "Ventana cerrada");
  assert.match(statusParts(controls({ next_window: "2026-10-08 18:00" }), NOW).window, /^Cerrada, abre .+ 18:00$/, "another day says the day");
  // A15: the run is only ever worded from last_run_at; "sin corridas" only when the engine says it never ran
  assert.equal(statusParts(controls({ last_run_at: new Date(NOW - 41 * HOUR).toISOString(), engine: "stale" }), NOW).run, "Corrida hace 41 h");
  assert.equal(statusParts(controls({ last_run_at: null, engine: "never" }), NOW).run, "Sin corridas todavía");
  assert.equal(statusParts(controls({ last_run_at: null, engine: undefined }), NOW).run, "", "no engine word, no date: nothing");
  assert.deepEqual(statusParts(null, NOW), { window: "", run: "" });
  assert.deepEqual(statusParts({}, NOW), { window: "", run: "" });
  fresh("en");
  assert.deepEqual(statusParts(controls(), NOW), { window: "Closed, opens 18:00", run: "Last run 7 h ago" });
  assert.equal(statusParts(controls({ last_run_at: null, engine: "never" }), NOW).run, "No run yet");
  fresh();
});

test("countWords: the words a number is read as; a zero, an unknown stop and HOY say nothing", () => {
  fresh();
  assert.equal(countWords("procesos", { procesos: 74 }), "74 procesos vivos");
  assert.equal(countWords("procesos", { procesos: 1 }), "1 proceso vivo");
  assert.equal(countWords("cola", { cola: 2 }), "2 por aprobar");
  assert.equal(countWords("aprender", { aprender: 12 }), "12 repasos pendientes");
  assert.equal(countWords("aprender", { aprender: 1 }), "1 repaso pendiente");
  assert.deepEqual(["recent", "stale", "never"].map((s) => countWords("sistema", { sistema: s })), ["motor al día", "motor atrasado", "motor sin corridas"]);
  assert.deepEqual([countWords("procesos", { procesos: 0 }), countWords("cola", {}), countWords("hoy", { hoy: 5 }), countWords("sistema", {}), countWords("cola", null)], ["", "", "", "", ""]);
  fresh("en");
  assert.equal(countWords("procesos", { procesos: 74 }), "74 live processes");
  assert.equal(countWords("sistema", { sistema: "never" }), "engine has never run");
  fresh();
});

test("the HUD's text keeps 4.5:1 over the top of the sky at all ten stops, and the frame never uses --text-faint for words", () => {
  // --text-dim over the lightest top (13:00) is about 4.8:1: too thin a margin for --text-faint (about 3.6:1 there) to be words
  // (These are the nominal colours of the sky's top edge; what Chrome really paints behind the text is measured in ui.contrast.browser.test.mjs.)
  const t = tokensOf(":root {\n  color-scheme: dark;");
  for (const [, top] of STOPS.slice(0, 10)) {
    for (const token of ["--text", "--text-dim", "--gold"]) {
      const ratio = contrast(t[token], rgb(top));
      assert.ok(ratio >= 4.5, `${token} over ${top} is ${ratio.toFixed(2)}:1`);
    }
  }
  const css = read("src/styles/stage.css");
  for (const m of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    if (/^\s*\.(hud|stop|bar)/.test(m[1]) || /\.(hud|stop|bar)[-\w]*\s*$/.test(m[1].trim())) {
      assert.ok(!/(^|[;\s])color:\s*var\(--text-faint\)/.test(m[2]), `${m[1].trim()} paints words in --text-faint`);
    }
  }
});

test("stage.css: the stage is 100svh and sticky inside a clipped document, the HUD is --hud tall, flat mode starts at 760 px", () => {
  const css = read("src/styles/stage.css");
  const rule = (sel) => { const m = new RegExp("(?:^|\\n)" + sel.replace(/[.#]/g, "\\$&") + "\\s*\\{([^}]*)\\}").exec(css); assert.ok(m, `no rule for ${sel}`); return m[1]; };
  assert.match(rule(".doc"), /overflow:\s*clip/, "clip, never hidden (a hidden overflow would break sticky)");
  assert.doesNotMatch(rule(".stage"), /overflow:\s*hidden/);
  assert.match(rule(".stage"), /position:\s*sticky/);
  assert.match(rule(".stage"), /top:\s*0/);
  assert.match(rule(".stage"), /height:\s*100svh/);
  assert.match(rule(".stop-spacer"), /height:\s*100svh/);
  assert.match(rule(".hud"), /height:\s*var\(--hud\)/);
  assert.match(css, /@media \(max-width: 759px\)/);
  assert.equal(tokensOf(":root {\n  color-scheme: dark;")["--hud"], undefined, "--hud is a length, not a colour: the token test does not read it");
  assert.match(read("src/styles/tokens.css"), /--hud:\s*56px/);
});
