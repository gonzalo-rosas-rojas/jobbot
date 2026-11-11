import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { tokensOf, contrast } from "./helpers/color.mjs";

const css = readFileSync(new URL("../src/styles/tokens.css", import.meta.url), "utf8").replace(/\r\n/g, "\n");

const PAIRS = [
  ["--text", "--night-1", 7], ["--text", "--night-2", 7],
  ["--text-dim", "--night-1", 4.5], ["--text-dim", "--night-2", 4.5],
  ["--text-faint", "--night-0", 4.5], ["--text-faint", "--night-1", 4.5], ["--text-faint", "--night-2", 4.5],
  ["--gold", "--night-1", 7], ["--gold", "--night-2", 7], ["--mint", "--night-1", 4.5],
  ["--paper-ink", "--paper", 7], ["--paper-dim", "--paper", 4.5], ["--gold-ink", "--paper", 7],
];

test("every colour pair of §4.1 meets its ratio", () => {
  const t = tokensOf(":root {\n  color-scheme: dark;");
  for (const [fg, bg, min] of PAIRS) {
    assert.ok(t[fg] && t[bg], `missing ${fg} or ${bg}`);
    assert.ok(contrast(t[fg], t[bg]) >= min, `${fg} on ${bg} is ${contrast(t[fg], t[bg]).toFixed(2)}:1, below ${min}:1`);
  }
});

test("the palette has no #000 and no #fff anywhere in the styles", () => {
  for (const name of ["tokens", "fonts", "base"]) {
    const text = readFileSync(new URL(`../src/styles/${name}.css`, import.meta.url), "utf8");
    assert.doesNotMatch(text, /#(000|fff)(000|fff)?\b/i, `${name}.css has #000 or #fff`);
  }
});

test("Papel changes only the sheet: its block defines sheet tokens and no sky or text constants", () => {
  const papel = tokensOf(':root[data-theme="light"] {');
  for (const name of Object.keys(papel)) assert.match(name, /^--(sheet|ink)/, `${name} escapes the sheet`);
});

test("the two type scales of §4.2 are the closed sets", () => {
  const sizes = [...css.matchAll(/--fs-(\d):\s*(\d+)px/g)].map((m) => Number(m[2]));
  assert.deepEqual(sizes, [12, 13, 15, 22, 34, 64, 120, 13, 15, 16, 26, 40, 80, 150]);
});

test("M8: the focus ring is two-tone, so it is visible on Papel (gold alone is 1.45:1 there) and on Tinta (WCAG 1.4.11, 3:1)", () => {
  const base = readFileSync(new URL("../src/styles/base.css", import.meta.url), "utf8").replace(/\r\n/g, "\n");
  const rule = /(?:^|\n):focus-visible\s*\{([^}]*)\}/.exec(base);
  assert.ok(rule, "base.css has a :focus-visible rule");
  const outline = /outline:\s*(\d+)px solid var\((--[\w-]+)\)/.exec(rule[1]);
  const shadow = /box-shadow:\s*0 0 0 (\d+)px var\((--[\w-]+)\)/.exec(rule[1]);
  assert.ok(outline && shadow, "an outline and a box-shadow that completes it");
  assert.ok(Number(shadow[1]) > Number(outline[1]) + 2, "the shadow reaches past the outline (and its 2 px offset), so it is the outer tone");
  const t = tokensOf(":root {\n  color-scheme: dark;");
  const [inner, outer] = [t[outline[2]], t[shadow[2]]];
  assert.ok(inner && outer, `the two tones are hex tokens (${outline[2]}, ${shadow[2]})`);
  assert.ok(contrast(inner, outer) >= 3, "the two tones keep 3:1 between them");
  // The ground the outer tone meets: the paper of Papel, and the night glass of Tinta (--night-1 is the sheet's and the sky's base).
  for (const [name, ground] of [["Papel", t["--paper"]], ["Tinta", t["--night-1"]]]) {
    const seen = Math.max(contrast(inner, ground), contrast(outer, ground));
    assert.ok(seen >= 3, `on ${name} the ring is ${seen.toFixed(2)}:1 against its ground`);
  }
  assert.ok(contrast(inner, t["--paper"]) < 3, "(the test knows why: gold alone would not be enough on Papel)");
});
