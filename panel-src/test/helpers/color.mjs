// test/helpers/color.mjs — the colour pairs of §4.1 read from tokens.css as written, with the same WCAG maths as the Rust test
// (crates/jobbot-panel/src/lib.rs). Shared by contrast.test.mjs and stage.sky.test.mjs.
import { readFileSync } from "node:fs";

const css = readFileSync(new URL("../../src/styles/tokens.css", import.meta.url), "utf8").replace(/\r\n/g, "\n");
const block = (opener) => {
  const at = css.indexOf(opener);
  if (at < 0) throw new Error(`tokens.css has no ${JSON.stringify(opener)}`);
  const body = css.slice(at + opener.length);
  return body.slice(0, body.indexOf("}"));
};
/** { "--text": "#e9eff7", ... } for the hex tokens of the block that opens with `opener`. */
export function tokensOf(opener) {
  const out = {};
  for (const m of block(opener).matchAll(/(--[\w-]+):\s*(#[0-9a-fA-F]{6})\s*;/g)) out[m[1]] = m[2];
  return out;
}
export const rgb = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
const lin = (c) => ((c /= 255) <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
export const luminance = ([r, g, b]) => 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
/** WCAG contrast of two colours given as "#rrggbb" or as [r, g, b]. */
export const contrast = (a, b) => {
  const [x, y] = [luminance(typeof a === "string" ? rgb(a) : a), luminance(typeof b === "string" ? rgb(b) : b)];
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
};
/** `fg` painted over `bg` at opacity `alpha`, as [r, g, b]. */
export const over = (fg, bg, alpha) => fg.map((v, i) => v * alpha + bg[i] * (1 - alpha));
