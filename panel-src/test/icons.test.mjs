// The icon sprite is generated from the Lucide package and may carry geometry only. These tests read the file that
// build steps will inline and refuse anything that could run, load or restyle.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { ICONS, SHAPES, SHAPE_ATTRS, SPRITE, buildSprite, shapesOf } from "../tools/icons.mjs";
import { guard } from "../guard.mjs";

const sprite = readFileSync(SPRITE, "utf8").replace(/\r\n/g, "\n");
const symbols = [...sprite.matchAll(/<symbol id="([^"]+)"([^>]*)>(.*?)<\/symbol>/g)].map((m) => ({ id: m[1], attrs: m[2], body: m[3], text: m[0] }));

test("the sprite on disk is what tools/icons.mjs writes", () => {
  assert.equal(sprite, buildSprite());
});

test("it holds exactly the listed icons, each as i-<name>, once", () => {
  assert.equal(new Set(ICONS).size, ICONS.length);
  assert.ok(ICONS.length >= 28 && ICONS.length <= 40, `${ICONS.length} icons`);
  assert.deepEqual(symbols.map((s) => s.id), ICONS.map((n) => `i-${n}`));
  for (const name of ICONS) assert.match(name, /^[a-z0-9]+(-[a-z0-9]+)*$/);
});

test("nothing in it can run, load or restyle", () => {
  for (const bad of ["<script", "<style", "<foreignObject", "<image", "<a ", "<use", "<link", "href", "xlink", "xmlns", "url(", "javascript:", "data:", "http", "style=", "class=", "@import"]) {
    assert.ok(!sprite.toLowerCase().includes(bad.toLowerCase()), `the sprite contains ${bad}`);
  }
  assert.ok(!/\son[a-z]+\s*=/i.test(sprite), "an on* handler");
  assert.deepEqual(guard(sprite, "", { strictPage: false }), []);
});

test("only symbols and the allowed shapes, with only the allowed attributes", () => {
  const tags = new Set([...sprite.matchAll(/<([a-zA-Z][a-zA-Z0-9]*)/g)].map((m) => m[1]));
  assert.deepEqual([...tags].sort(), ["svg", "symbol", ...[...SHAPES].filter((s) => tags.has(s))].sort());
  for (const s of symbols) {
    assert.match(s.attrs, /^ viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"$/, s.id);
    for (const m of s.body.matchAll(/ ([a-zA-Z:-]+)="/g)) assert.ok(SHAPE_ATTRS.has(m[1]), `${s.id} uses attribute ${m[1]}`);
  }
});

test("every symbol is under 2 KB, the whole sprite under 12 KB", () => {
  for (const s of symbols) assert.ok(s.text.length < 2048, `${s.id} is ${s.text.length} bytes`);
  assert.ok(sprite.length < 12 * 1024, `${sprite.length} bytes`);
});

test("the parser refuses what is not geometry", () => {
  const wrap = (inner) => `<!-- x --><svg class="lucide" viewBox="0 0 24 24">${inner}</svg>`;
  assert.deepEqual(shapesOf("ok", wrap('<path d="M0 0h1" />')), ['<path d="M0 0h1"/>']);
  assert.throws(() => shapesOf("a", wrap("<script>alert(1)</script>")), /unexpected markup|not allowed/);
  assert.throws(() => shapesOf("b", wrap('<path d="M0 0" onload="x()" />')), /attribute onload/);
  assert.throws(() => shapesOf("c", wrap('<foreignObject width="1" height="1" />')), /<foreignObject> is not allowed/);
  assert.throws(() => shapesOf("d", wrap('<path d="M0 0" style="fill:red" />')), /attribute style/);
  assert.throws(() => shapesOf("e", wrap('<path d="url(http://x)" />')), /not geometry/);
  assert.throws(() => shapesOf("f", wrap('<g><path d="M0 0" /></g>')), /unexpected markup/);
  assert.throws(() => shapesOf("g", wrap("")), /no shapes/);
  assert.throws(() => buildSprite(() => "<html></html>"), /not an svg file/);
});
