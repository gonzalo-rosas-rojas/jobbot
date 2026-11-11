import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";

// Keyboard focus (WCAG 2.4.7). base.css draws one ring for every :focus-visible (gold, an offset, a dark band under it). A stylesheet may
// take that ring away from an element only if (a) the element is a focus target nobody operates (a heading or a panel that receives focus
// from a script so the screen reader starts there; it is never in the Tab order), or (b) it is a text field that draws its own mark.
// This is a plain text scan of the sheets, listed the way build.mjs lists them (see css.guard.test.mjs): robust, not clever. A new
// `outline: none` fails here until its author lists it with the reason, which is the moment someone looks at it.
const ROOT = new URL("../", import.meta.url);
const OWN = ["tokens", "fonts", "base", "stage", "dial", "sheet", "actions", "moments", "menu"];
const files = () => [
  ...OWN.map((n) => `src/styles/${n}.css`).filter((p) => existsSync(new URL(p, ROOT))),
  ...readdirSync(new URL("src/styles/ui/", ROOT)).filter((n) => n.endsWith(".css") && !n.startsWith("_")).map((n) => `src/styles/ui/${n}`),
];
const code = (p) => readFileSync(new URL(p, ROOT), "utf8").replace(/\r\n/g, "\n").replace(/\/\*[\s\S]*?\*\//g, "");

/** [selector, declarations] of every innermost rule (an @media block gives its rules, not itself). */
const rulesOf = (css) => [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((m) => [m[1].trim(), m[2]]);

/** A declaration that leaves no outline: `outline: none | 0 | 0px`, `outline-style: none`, `outline-width: 0`, shorthand included. */
const removesOutline = (body) => {
  for (const d of body.split(";")) {
    const m = /^\s*(outline(?:-style|-width)?)\s*:\s*(.+?)\s*$/.exec(d);
    if (!m) continue;
    const first = m[2].split(/\s+/)[0];
    if (m[1] === "outline-style" ? first === "none" : m[1] === "outline-width" ? /^0(px)?$/.test(first) : first === "none" || /^0(px)?$/.test(first)) return true;
  }
  return false;
};

const NOT_OPERATED = "a focus target nobody operates: it takes focus from a script (tabindex -1) so the reading starts there";
const HEADING = "a heading that takes focus from a script when the moment changes (tabindex -1), so the reading starts there";
// Selector -> why it may have no ring. Everything listed is outside the Tab order; none of them is a button, a link or a field.
const NO_RING = new Map([
  [".hoy .mega", HEADING], [".cola .mega", HEADING], [".aprender .mega", HEADING], [".sistema .mega", HEADING],
  [".prologo .mega", HEADING], [".procesos .eyebrow", HEADING],
  [".loading-title", "the line of the loading state; it takes focus from a script (tabindex -1)"],
  [".pd-title", "a plain h2 of the drawer: it has no tabindex and is never focused (the declaration does nothing)"],
  [".detail", "the drawer panel (tabindex -1): focus moves to it when it opens; its own buttons carry the ring"],
  [".aprender .ap-box", NOT_OPERATED], [".aprender .ap-bigq", NOT_OPERATED], [".aprender .ap-note-line", NOT_OPERATED],
  [".aprender .ap-answer", NOT_OPERATED + " (it scrolls: whether a mark should show when it takes focus is the owner's call)"],
]);

// Text fields that hide the ring and draw their own mark. `ring` is the :focus-visible rule of the field: it keeps a transparent outline
// (Windows high contrast, forced-colors, repaints it in a system colour, whereas a box-shadow or a border colour is thrown away there,
// so without it the field would show no focus at all in that mode) and it must set box-shadow, because the global ring puts a dark band
// of 6 px there, which on the paper of Papel is a black frame around the field. `mark` is the rule that paints the mark.
const FIELDS = [
  { ring: ".palette-input:focus-visible", mark: ".palette-field:focus-within", paints: /box-shadow\s*:\s*inset[^;]*var\(--gold\)/ },
  { ring: ".procesos .find-input:focus-visible", mark: ".procesos .find-input:focus-visible", paints: /box-shadow\s*:\s*inset[^;]*var\(--ink-accent\)/ },
  { ring: ".aprender .ap-mine:focus-visible", mark: ".aprender .ap-mine:focus-visible", paints: /box-shadow\s*:\s*inset[^;]*var\(--ink-accent\)/ },
];

const all = files().flatMap((file) => rulesOf(code(file)).map(([selector, body]) => ({ file, selector, body })));
const selectorsOf = (rule) => rule.selector.split(",").map((s) => s.trim());

test("a stylesheet removes the outline only from elements nobody operates, and each one is listed with its reason", () => {
  assert.ok(all.length > 400, "the scan reads the sheets of the page");
  const loose = [];
  for (const rule of all) {
    if (!removesOutline(rule.body)) continue;
    for (const sel of selectorsOf(rule)) if (!NO_RING.has(sel)) loose.push(`${rule.file}: "${sel}" removes the outline; give it a :focus-visible mark (and keep \`outline: 2px solid transparent\`) or list it in NO_RING with the reason`);
  }
  assert.deepEqual(loose, []);
});

test("every element listed as having no ring still exists in a sheet (a stale entry hides the next removal)", () => {
  const removed = new Set(all.filter((r) => removesOutline(r.body)).flatMap(selectorsOf));
  const stale = [...NO_RING.keys()].filter((s) => !removed.has(s));
  assert.deepEqual(stale, []);
});

test("a text field that draws its own mark keeps a transparent outline, replaces the global dark band and paints the mark (WCAG 2.4.7)", () => {
  for (const { ring, mark, paints } of FIELDS) {
    const rings = all.filter((r) => selectorsOf(r).includes(ring));
    assert.ok(rings.length, `${ring} is styled`);
    assert.ok(rings.some((r) => /outline\s*:\s*2px solid transparent/.test(r.body)), `${ring} keeps \`outline: 2px solid transparent\` (forced-colors repaints it)`);
    assert.ok(rings.some((r) => /box-shadow\s*:/.test(r.body)), `${ring} sets box-shadow, or the dark band of the global ring stays around the field`);
    const drawn = all.filter((r) => selectorsOf(r).includes(mark));
    assert.ok(drawn.some((r) => paints.test(r.body)), `${mark} paints the mark of the field`);
  }
  // The palette is always Tinta (it takes back the night tokens), so its gold mark sits on the night glass; the two fields of the sheet
  // use --ink-accent: #704500 on the paper of Papel (test/contrast.test.mjs holds that pair at 7:1) and gold on the night.
});
