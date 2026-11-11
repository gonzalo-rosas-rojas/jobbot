// Builds assets/icons.svg, one <symbol id="i-<name>"> per icon of a fixed list, from the Lucide SVGs in node_modules
// (lucide-static, ISC; the Feather-derived icons are MIT, both texts in THIRD_PARTY_NOTICES.md). Only the shapes are
// copied: every element and attribute is checked against a short allow-list while parsing, so nothing executable or
// styled can ride in with an update (R8 §2 rule 8; R8a §5).
//
//   node tools/icons.mjs           writes assets/icons.svg
//   node tools/icons.mjs --check   fails if the file on disk is not what would be written
//
// How the page uses it is in VENDORED.md ("Icons"). The sprite has no xmlns attribute (inline HTML does not need it) and
// no http text anywhere, so guard.mjs has nothing to object to.

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { ICONS } from "../src/ui/icons.js";

const here = path.dirname(fileURLToPath(import.meta.url));
export const ROOT = path.join(here, "..");
export const SPRITE = path.join(ROOT, "assets", "icons.svg");
const ICON_DIR = path.join(ROOT, "node_modules", "lucide-static", "icons");

// What the panel will draw is the closed list of src/ui/icons.js: the page draws from this sprite and this tool builds it from
// the same names, so the two cannot drift. The name is Lucide's; the id in the page is `i-<name>`.
export { ICONS };

// A Lucide icon is a handful of these, with these attributes, and nothing else.
export const SHAPES = new Set(["path", "circle", "rect", "line", "polyline", "polygon", "ellipse"]);
export const SHAPE_ATTRS = new Set(["d", "cx", "cy", "r", "rx", "ry", "x", "y", "x1", "y1", "x2", "y2", "width", "height", "points"]);
// The look of every icon, on the <symbol>: outline, current colour, round ends. Size and colour come from the page's CSS.
const LOOK = 'viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"';

/** The shapes inside one Lucide file, re-written compactly. Throws on anything outside the allow-list. */
export function shapesOf(name, svg) {
  const body = svg.replace(/<!--[\s\S]*?-->/g, "");
  const open = /<svg\b[^>]*>/.exec(body);
  const close = body.lastIndexOf("</svg>");
  if (!open || close < 0) throw new Error(`${name}: not an svg file`);
  const inner = body.slice(open.index + open[0].length, close);
  const shapes = [];
  let rest = inner;
  const tag = /^\s*<([a-zA-Z0-9]+)((?:\s+[a-zA-Z:-]+="[^"<>]*")*)\s*\/>/;
  for (let m = tag.exec(rest); m; m = tag.exec(rest)) {
    if (!SHAPES.has(m[1])) throw new Error(`${name}: element <${m[1]}> is not allowed`);
    const attrs = [...m[2].matchAll(/([a-zA-Z:-]+)="([^"]*)"/g)].map((a) => [a[1], a[2].replace(/\s+/g, " ").trim()]);
    for (const [k, v] of attrs) {
      if (!SHAPE_ATTRS.has(k)) throw new Error(`${name}: attribute ${k} on <${m[1]}> is not allowed`);
      if (/[<>&"']|javascript|data:|url\(|http/i.test(v)) throw new Error(`${name}: attribute ${k} has a value that is not geometry`);
    }
    shapes.push(`<${m[1]} ${attrs.map(([k, v]) => `${k}="${v}"`).join(" ")}/>`);
    rest = rest.slice(m[0].length);
  }
  if (rest.trim() !== "") throw new Error(`${name}: unexpected markup: ${rest.trim().slice(0, 60)}`);
  if (shapes.length === 0) throw new Error(`${name}: no shapes`);
  return shapes;
}

/** The whole sprite as a string. */
export function buildSprite(read = (name) => readFileSync(path.join(ICON_DIR, name + ".svg"), "utf8")) {
  const out = ['<svg width="0" height="0" hidden aria-hidden="true">'];
  for (const name of ICONS) out.push(`<symbol id="i-${name}" ${LOOK}>${shapesOf(name, read(name)).join("")}</symbol>`);
  out.push("</svg>", "");
  return out.join("\n");
}

function main() {
  const sprite = buildSprite();
  if (process.argv.includes("--check")) {
    const disk = existsSync(SPRITE) ? readFileSync(SPRITE, "utf8").replace(/\r\n/g, "\n") : "";
    if (disk !== sprite) { console.error("assets/icons.svg is not what tools/icons.mjs writes; run it"); process.exit(1); }
    console.log(`ok: assets/icons.svg is current (${ICONS.length} icons, ${sprite.length} bytes)`);
    return;
  }
  writeFileSync(SPRITE, sprite);
  console.log(`wrote assets/icons.svg: ${ICONS.length} icons, ${sprite.length} bytes`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
