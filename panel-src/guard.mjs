// The rules the generated page must satisfy. They mirror the Rust tests in crates/jobbot-panel/src/lib.rs (page_tests) and the budgets of
// docs/superpowers/specs/2026-10-08-el-turno-continuado-design.md (A5, A6, A11). build.mjs refuses to write a page that
// breaks any of them, and test/guard.test.mjs checks the same function against hand-made good and bad pages.

import { gzipSync } from "node:zlib";
import { createHash } from "node:crypto";

// What crates/jobbot-panel/src/lib.rs forbids anywhere in the page. Keep in step with that file.
export const FORBIDDEN = [
  "innerHTML", "outerHTML", "insertAdjacentHTML", "document.write", "eval(", "new Function", 'setAttribute("style"', ' style="',
  "javascript:", 'src="http', 'href="http', "url(http", "@import", "<link", "<iframe",
  "splitText", "url(data:", 'src="data:',
  // Ways to reach out, or to turn text into code or markup, that nothing in this panel has a reason for (review M5). The CSP of the real
  // page makes each of them inert; these strings are the second line there and, in the demo published as a static file, the first, next to
  // the meta CSP that build.mjs writes into it. A bare word is enough: no library of the bundle uses any of them. `fetch` is the one network
  // call the page does make, to its own origin (core/session.js), so it is judged by its pattern below. DOMParser is not here on purpose:
  // it parses into an inert document and is not a sink.
  "sendBeacon", "WebSocket", "EventSource", "XMLHttpRequest", "importScripts", "SharedWorker", "new Worker(", "srcdoc", "setHTMLUnsafe",
  "parseHTMLUnsafe", "new Image(", "import(",
];

// What a string cannot say: calls with a certain kind of argument. [pattern, what it is].
export const FORBIDDEN_PATTERNS = [
  [/\bfetch\(\s*["'`](?:https?:)?\/\//, "fetch of an absolute address"],
  [/\bset(?:Timeout|Interval)\(\s*["'`]/, "setTimeout or setInterval with a string (it is eval)"],
  [/\bcreateElement(?:NS)?\(\s*(?:[^,)]*,\s*)?["'](?:iframe|frame|script|object|embed)["']/, "an element that loads or runs something (iframe, frame, script, object, embed)"],
];

// The two blocks of tokens.css: Tinta (the default, and what `auto` means) and Papel (the sheet only). No third theme.
export const THEME_OPENERS = [":root {\n  color-scheme: dark;", ':root[data-theme="light"] {'];
export const JS_BUDGET = 480 * 1024;       // the length of the MINIFIED bundle (A6); tools/budgets.mjs says in which area it grows
// The whole page, sprite included, gzipped (A6). 110 KB was the cap for HOY and a prologue with the story inside; the five real moments, the
// drawer and the palette measured 149.2 KB at the close of F0, so decision D-20 (spec 14, taken without Gonza, reversible) set it to that
// rounded up to 150 plus 10 KB of room. The page is served from 127.0.0.1, where this weighs nothing; the cap that must stay tight is the
// publishable demo's (F4), which is built with its own budget when it comes. Raised to 200 KB for the sound of the actions and of the background
// (33 cues, the player, the background detector, the sheet "Escuchar los sonidos" and their words in two languages: about 13 KB gzipped, measured at
// 169.9 KB), the second turn of D-20; the cap is still the defence against growth nobody decided.
export const HTML_GZ_BUDGET = 200 * 1024;
export const FONT_FILES = ["barlow-condensed-700.woff2", "geist-mono-400.woff2", "instrument-serif-400italic.woff2", "inter-400.woff2", "inter-600.woff2"];
export const SCALES = [[12, 13, 15, 22, 34, 64, 120], [13, 15, 16, 26, 40, 80, 150]];
// Nothing imports three any more (the tower is archived and test/archive.guard.test.mjs keeps it so): a page whose bundle includes it is refused.
export const ENFORCE_NO_THREE = true;

const SIZE_OK = new Set(SCALES.flat());
const count = (text, needle) => text.split(needle).length - 1;
const styleOf = (page) => (/<style nonce="__NONCE__">([\s\S]*?)<\/style>/.exec(page) || [, ""])[1];

/** `font-size` values and `font:` shorthands the closed scale of §4.2 does not allow. */
export function typeProblems(css) {
  const out = [];
  for (const m of css.matchAll(/(?<![-\w])font-size\s*:\s*([^;}]+)/g)) {
    const v = m[1].trim();
    const px = /^(\d+(?:\.\d+)?)px$/.exec(v);
    const ok = /^var\(--fs-[1-7]\)$/.test(v) || v === "inherit" || v === "1em" || v === "100%" || (px && SIZE_OK.has(Number(px[1])));
    if (!ok) out.push(`font-size ${v} is outside the closed scale`);
  }
  for (const m of css.matchAll(/(?<![-\w])font\s*:\s*([^;}]+)/g)) {
    if (m[1].trim() !== "inherit") out.push(`font shorthand "${m[1].trim()}": use font-family, font-size and font-weight`);
  }
  const declared = [...css.matchAll(/--fs-(\d)\s*:\s*(\d+)px/g)].map((m) => Number(m[2]));
  if (declared.length && JSON.stringify(declared) !== JSON.stringify(SCALES.flat())) out.push(`--fs-1..7 are ${declared.join(",")}, expected ${SCALES.flat().join(",")}`);
  return out;
}

/** Every `url(` of the page points at `__ASSETS__/<name>` or at an in-page `#fragment`, with single, double or no quotes (the Rust test every_url_in_the_page_is_an_own_asset_or_a_fragment). */
export function urlProblems(page) {
  const out = [];
  for (const m of page.matchAll(/url\(/g)) {
    const rest = page.slice(m.index + 4).replace(/^["' ]+/, "");
    if (!rest.startsWith("__ASSETS__/") && !rest.startsWith("#")) out.push(`url( must point at __ASSETS__/<name> or #fragment, found ${JSON.stringify(page.slice(m.index, m.index + 40))}`);
  }
  return out;
}

/** `__ASSETS__` only as url("__ASSETS__/<name>") (quotes may be gone after minifying), and the five fonts named. */
export function assetProblems(page) {
  const out = [];
  for (const m of page.matchAll(/__ASSETS__/g)) {
    const before = page.slice(Math.max(0, m.index - 6), m.index);
    const after = page.slice(m.index + 10, m.index + 60);
    if (!/url\(["']?$/.test(before) || !/^\/[a-z0-9._-]+\.(woff2|svg)["']?\s*\)/.test(after)) {
      out.push('__ASSETS__ used outside url("__ASSETS__/<name>")');
      break;
    }
  }
  for (const f of FONT_FILES) if (!page.includes(`__ASSETS__/${f}`)) out.push(`font not referenced: ${f}`);
  return out;
}

// What the build or the server fills in; none of it may reach a file that a static host serves as it is.
const PUBLISHED_MARKERS = ["__ASSETS__", "__NONCE__", "__STYLE__", "__ICONS__", "__SCRIPT__", "__DEFAULT_LANG__"];

/** The inline <script> and <style> as a browser splits them: text inside one is not a tag, so the scan jumps to its close. `body` is null when it never closes. */
function inlineElements(page) {
  const out = [];
  const open = /<(script|style)(?=[\s>/])[^>]*>/gi;
  for (let m; (m = open.exec(page)); ) {
    const tag = m[1].toLowerCase();
    const close = new RegExp(`</${tag}(?=[\\s>/])`, "gi");
    close.lastIndex = open.lastIndex;
    const end = close.exec(page);
    out.push({ tag, open: m[0], body: end ? page.slice(open.lastIndex, end.index) : null });
    if (!end) break;
    open.lastIndex = end.index;
  }
  return out;
}

/**
 * Judges a published demo from the file alone (no browser): no marker or nonce left, exactly one meta Content-Security-Policy, every inline
 * script and style named in it by the SHA-256 of its body as it is now (a problem says that hash), and nothing in script-src that is not the
 * hash of a script. style-src may name more hashes than there are inline styles: the demo's runtime style has one.
 */
export function publishedProblems(page) {
  const problems = [];
  for (const marker of PUBLISHED_MARKERS) if (page.includes(marker)) problems.push(`${marker} is left in the page`);
  const metas = [...page.matchAll(/<meta\b[^>]*\bhttp-equiv\s*=\s*["']?content-security-policy\b[^>]*>/gi)];
  if (metas.length !== 1) problems.push(`expected exactly 1 meta Content-Security-Policy, found ${metas.length}`);
  const content = metas.length === 1 ? /\bcontent\s*=\s*(?:"([^"]*)"|'([^']*)')/i.exec(metas[0][0]) : null;
  if (metas.length === 1 && !content) problems.push("the meta Content-Security-Policy has no content");
  const policy = new Map();
  for (const directive of (content ? content[1] ?? content[2] : "").split(";")) {
    const [name, ...tokens] = directive.trim().split(/\s+/);
    if (name && !policy.has(name.toLowerCase())) policy.set(name.toLowerCase(), tokens);
  }
  const elements = inlineElements(page);
  const named = { script: policy.get("script-src") || [], style: policy.get("style-src") || [] };
  const seen = { script: 0, style: 0 };
  const hashes = { script: new Set(), style: new Set() };
  for (const el of elements) {
    const n = ++seen[el.tag];
    if (/\snonce\b/i.test(el.open)) problems.push(`<${el.tag}> #${n} has a nonce attribute`);
    if (el.body === null) { problems.push(`<${el.tag}> #${n} is never closed`); continue; }
    const hash = `'sha256-${createHash("sha256").update(el.body, "utf8").digest("base64")}'`;
    hashes[el.tag].add(hash);
    if (!named[el.tag].includes(hash)) problems.push(`<${el.tag}> #${n} is not named in ${el.tag}-src; its SHA-256 now is ${hash.slice(1, -1)}`);
  }
  for (const token of named.script) if (!hashes.script.has(token)) problems.push(`script-src names ${token}, which is not the hash of a script`);
  return problems;
}

/**
 * Returns a list of problems (empty = fine). `strictPage` is the real page; labs only get the safety checks.
 * `inputs` are the files esbuild bundled (metafile); `forbidThree` refuses any of them under node_modules/three.
 */
export function guard(page, js, { strictPage = true, budget = JS_BUDGET, inputs = [], forbidThree = ENFORCE_NO_THREE } = {}) {
  const problems = [];
  for (const needle of FORBIDDEN) {
    if (strictPage || needle !== "<link") if (page.includes(needle)) problems.push(`contains ${JSON.stringify(needle)}`);
  }
  for (const [re, what] of FORBIDDEN_PATTERNS) if (re.test(page)) problems.push(`contains ${what}`);
  if (js.includes("</script") || js.includes("<script")) problems.push("the script contains a script tag");
  if (page.includes("\r")) problems.push("carriage returns in the page");
  if (!strictPage) return problems;

  const scripts = count(page, "<script");
  const styles = count(page, "<style");
  if (scripts !== count(page, '<script nonce="__NONCE__"')) problems.push("a <script> tag has no nonce");
  if (styles !== count(page, '<style nonce="__NONCE__"')) problems.push("a <style> tag has no nonce");
  if (scripts !== 2) problems.push(`expected 2 script tags, found ${scripts}`);
  if (styles !== 1) problems.push(`expected 1 style tag, found ${styles}`);
  if (count(page, "__NONCE__") !== scripts + styles) problems.push("__NONCE__ appears outside the tags");
  for (const opener of THEME_OPENERS) {
    if (!page.includes(opener)) problems.push(`theme block missing: ${JSON.stringify(opener)}`);
  }
  const title = (page.split("<title>")[1] || "").split("</title>")[0].toLowerCase();
  for (const word of ["jobbot", "empleo", "postula", "job", "trabajo"]) {
    if (title.includes(word)) problems.push(`the title says ${JSON.stringify(word)}`);
  }
  const css = styleOf(page);
  problems.push(...typeProblems(css), ...assetProblems(page), ...urlProblems(page));
  for (const hex of ["#000", "#fff"]) {
    if (new RegExp(`${hex}(?:${hex.slice(1)})?(?![0-9a-fA-F])`, "i").test(css)) problems.push(`the style contains ${JSON.stringify(hex)}`);
  }
  if (forbidThree && inputs.some((p) => /node_modules[\\/]three[\\/]/.test(p))) problems.push('the bundle includes "three"');
  if (js.length > budget) problems.push(`script is ${(js.length / 1024).toFixed(1)} KB, over the ${(budget / 1024).toFixed(0)} KB budget`);
  const gz = gzipSync(page).length;
  if (gz > HTML_GZ_BUDGET) problems.push(`page is ${(gz / 1024).toFixed(1)} KB gzipped, over the ${HTML_GZ_BUDGET / 1024} KB budget (gzip)`);
  return problems;
}
