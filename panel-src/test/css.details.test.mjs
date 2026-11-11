import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { tokensOf, contrast } from "./helpers/color.mjs";

// The small things you notice when you use the panel and not when you look at it (selection, caret, figures that do not dance, even lines, scrollbars,
// a pressed key cap). They are CSS only, so they are held by reading the sheets, the way css.guard.test.mjs and css.focus.test.mjs do.
const ROOT = new URL("../", import.meta.url);
const read = (p) => readFileSync(new URL(p, ROOT), "utf8").replace(/\r\n/g, "\n");
const code = (p) => read(p).replace(/\/\*[\s\S]*?\*\//g, "");
const rulesOf = (css) => [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((m) => [m[1].trim(), m[2]]);
const bodyOf = (file, selector) => rulesOf(code(file)).filter(([s]) => s.split(",").map((x) => x.trim()).includes(selector)).map(([, b]) => b).join(";");
const tokens = tokensOf(":root {\n  color-scheme: dark;");
const tokenOf = (value) => { const m = /^var\((--[\w-]+)\)$/.exec(value.trim()); assert.ok(m && tokens[m[1]], `${value} is a hex token of tokens.css`); return tokens[m[1]]; };

test("the selection is gold on the night, and ink-of-the-accent on the paper of Papel; each pair keeps 7:1 and shows against its ground", () => {
  const rules = rulesOf(code("src/styles/base.css")).filter(([s]) => s.includes("::selection"));
  const night = rules.find(([s]) => s === "::selection");
  const papel = rules.find(([s]) => s.includes('[data-theme="light"]'));
  assert.ok(night && papel, "base.css has the default selection and the one of Papel");
  const pair = ([, body]) => [tokenOf(/background\s*:\s*([^;]+)/.exec(body)[1]), tokenOf(/(?:^|;)\s*color\s*:\s*([^;]+)/.exec(body)[1])];
  const [nightBg, nightFg] = pair(night);
  const [papelBg, papelFg] = pair(papel);
  assert.ok(contrast(nightFg, nightBg) >= 7, `Tinta: text on the selection is ${contrast(nightFg, nightBg).toFixed(2)}:1`);
  assert.ok(contrast(papelFg, papelBg) >= 7, `Papel: text on the selection is ${contrast(papelFg, papelBg).toFixed(2)}:1`);
  assert.ok(contrast(nightBg, tokens["--night-1"]) >= 3, "Tinta: the highlight stands out from the glass");
  assert.ok(contrast(papelBg, tokens["--paper"]) >= 3, "Papel: the highlight stands out from the paper (gold alone would be 1.45:1 there)");
  assert.ok(contrast(tokens["--gold"], tokens["--paper"]) < 3, "(the test knows why Papel has its own pair)");
  // The paper pair is only for what is on the paper: the prologue keeps the night, and so do the dark pieces (they are outside the sheet).
  assert.match(papel[0], /\.sheet:not\(\[data-moment="prologo"\]\)/);
  assert.match(papel[0], /\.detail/);
});

test("the caret and the native controls take the accent of their surface, so the dark pieces (which re-pin it to gold) get gold and the paper gets its ink", () => {
  const css = code("src/styles/base.css");
  assert.match(css, /(?:^|\n)input, textarea, select, progress \{ accent-color: var\(--ink-accent\); \}/);
  assert.match(css, /(?:^|\n)input, textarea \{ caret-color: var\(--ink-accent\); \}/);
  assert.ok(contrast(tokens["--gold"], tokens["--night-1"]) >= 3 && contrast(tokens["--gold-ink"], tokens["--paper"]) >= 3, "the caret is seen on both grounds");
});

test("the figures that change in place keep a fixed width (tabular-nums): the existing ones stay and the data rows of SISTEMA and the counter of COLA join", () => {
  const WANT = [
    ["src/styles/base.css", ".num"], ["src/styles/ui/detail.css", ".d-score"], ["src/styles/dial.css", ".dial-time"],
    ["src/styles/ui/sistema.css", ".sistema .row-v"], ["src/styles/ui/sistema.css", ".sistema .row-s"], ["src/styles/ui/cola.css", ".cola .mega .of"],
  ];
  for (const [file, selector] of WANT) assert.match(bodyOf(file, selector), /font-variant-numeric\s*:\s*tabular-nums/, `${selector} in ${file}`);
});

test("headlines balance their lines and paragraphs avoid a widow; the HUD status keeps its own balance", () => {
  const base = code("src/styles/base.css");
  assert.match(base, /(?:^|\n)\.mega, h2, h3, \.ap-bigq, \.loading-title \{ text-wrap: balance; \}/);
  assert.match(base, /(?:^|\n)p \{ text-wrap: pretty; \}/);
  assert.match(bodyOf("src/styles/stage.css", ".hud-status"), /text-wrap\s*:\s*balance/);
});

test("scrollbars take the colours of their surface, and PROCESOS keeps hiding its own (it draws a track, a cue and fades of its own)", () => {
  const base = code("src/styles/base.css");
  assert.match(base, /html, \.palette, \.menu, \.help, \.sound-sheet, \.sheet\[data-moment="prologo"\] \{ scrollbar-color: var\(--text-dim\) transparent; \}/);
  assert.match(base, /\.sheet, \.detail \{ scrollbar-color: var\(--ink-dim\) transparent; \}/);
  assert.doesNotMatch(base, /scrollbar-width/, "no width here: it does not inherit and a thin bar changes how wide every list is");
  assert.match(bodyOf("src/styles/ui/procesos.css", ".procesos .scroller"), /scrollbar-width\s*:\s*none/);
  assert.match(code("src/styles/ui/procesos.css"), /\.procesos \.scroller::-webkit-scrollbar \{ display: none; \}/);
  assert.ok(contrast(tokens["--text-dim"], tokens["--night-1"]) >= 3 && contrast(tokens["--paper-dim"], tokens["--paper"]) >= 3, "the thumb is seen on the night and on the paper");
});

test("a key cap shows a pressed state while its button is held, and it does not animate", () => {
  const rule = rulesOf(code("src/styles/base.css")).find(([s]) => s.includes("button:active kbd"));
  assert.ok(rule, "base.css has the pressed state of a key cap");
  assert.match(rule[1], /transform\s*:\s*translateY\(1px\)/);
  assert.match(rule[1], /background\s*:\s*color-mix\(in srgb, currentColor \d+%, transparent\)/);
  assert.doesNotMatch(rule[1], /transition|animation/);
});

test("calm motion is the one it was: the base rule that cuts every animation and transition is still there, word for word", () => {
  assert.match(code("src/styles/base.css"), /@media \(prefers-reduced-motion: reduce\) \{ \*, \*::before, \*::after \{ animation-duration: 0\.01ms !important; transition-duration: 0\.01ms !important; \} \}/);
});
