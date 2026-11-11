import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { tokensOf } from "./helpers/color.mjs";

// The head of the page template. The tint of the browser's own bar (Chrome on Android, installed windows, Safari) is the colour the top of the
// window really has: the night of the stage, in Tinta and in Papel (Papel repaints the sheet only, and the panel never follows the system
// scheme, so a light/dark pair keyed on prefers-color-scheme would tint the bar light over a dark page). The value is written once in
// tokens.css; this keeps the tag from drifting from it.
const template = readFileSync(new URL("../page.template.html", import.meta.url), "utf8").replace(/\r\n/g, "\n");
const head = template.slice(0, template.indexOf("</head>"));

test("the page has one theme-color and it is the night of the stage, --night-0 of tokens.css", () => {
  const tags = [...head.replace(/<!--[\s\S]*?-->/g, "").matchAll(/<meta\s+[^>]*name="theme-color"[^>]*>/g)].map((m) => m[0]);
  assert.equal(tags.length, 1, "exactly one <meta name=\"theme-color\"> (no media pair: the page does not follow the system scheme)");
  assert.doesNotMatch(tags[0], /\bmedia=/, "no prefers-color-scheme pair");
  const content = /content="(#[0-9a-fA-F]{6})"/.exec(tags[0]);
  assert.ok(content, "a #rrggbb content");
  const night = tokensOf(":root {\n  color-scheme: dark;")["--night-0"];
  assert.ok(night, "tokens.css has --night-0");
  assert.equal(content[1].toLowerCase(), night.toLowerCase());
});

test("Papel does not repaint the stage: no stage token in its block, so the bar tint is the same in both themes", () => {
  const papel = tokensOf(':root[data-theme="light"] {');
  for (const name of Object.keys(papel)) assert.match(name, /^--(sheet|ink)/, `${name} is not a sheet token`);
  assert.equal("--night-0" in papel, false);
});
