// The acceptance test of the encargo docs/encargos/boton-de-accion-unico.md: one action button for the whole panel. ui/act.js builds it (actButton) and the
// drawer's links (outLink); every module that draws verbs calls it and builds no other button by hand; src/styles/actions.css is the only sheet that gives
// the button its shape (size, weight, height, decoration, border, background), and no other sheet, rule or custom property redraws it, in the sources or
// in the built page; the five copies of that shape in the stops' sheets and the drawer's own `.btn` are gone. Armed, a button keeps its key and its verb
// (twoStep of core/dom.js writes into .act-label) and its gold wins over any stop's colour; pressed gold is only for the drawer's answers (.d-marks).
// ui.act.browser.test.mjs compares the computed look on the page, which a test without a browser cannot see.
import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(path.join(ROOT, p), "utf8").replace(/\r\n/g, "\n");
const exists = (p) => existsSync(path.join(ROOT, p));
const walk = (dir, ext) => readdirSync(path.join(ROOT, dir), { withFileTypes: true })
  .flatMap((e) => (e.isDirectory() ? walk(`${dir}/${e.name}`, ext) : e.name.endsWith(ext) ? [`${dir}/${e.name}`] : []));
const MODULES = () => [...walk("src/ui", ".js"), ...walk("src/moments", ".js")];

// ---- reading CSS: rules (inside @media too), their selectors, the compound a selector styles, and what a body declares

const uncomment = (css) => css.replace(/\/\*[\s\S]*?\*\//g, "");
const rulesOf = (css) => [...uncomment(css).matchAll(/([^{}]+)\{([^{}]*)\}/g)]
  .map((m) => ({ selectors: m[1].trim().split(",").map((s) => s.trim()).filter(Boolean), body: m[2] }))
  .filter((r) => !r.selectors[0].startsWith("@"));
const subjectOf = (selector) => selector.split(/\s*[\s>+~]\s*/).filter(Boolean).pop() || "";
const hasClass = (compound, name) => new RegExp(`\\.${name}(?![\\w-])`).test(compound);
const decls = (body) => body.split(";").map((d) => { const i = d.indexOf(":"); return [d.slice(0, i).trim().toLowerCase(), d.slice(i + 1).trim()]; }).filter(([p]) => p);

// What redraws a button: size, height, line, spacing of the type, decoration, border, background, shadow, and any custom property (a variable is how a
// shape comes back through actions.css itself). Only styles/actions.css may write these where an .act, its label or a bare button is styled; a stop may
// still place a button (a margin, a width, an order) or colour it.
const BANNED = (p) => /^(display|font|font-size|font-family|min-height|height|line-height|letter-spacing|box-shadow|text-underline-offset)$/.test(p)
  || /^(border|background|text-decoration)/.test(p) || p.startsWith("--");
// What a module hands to actButton names that button too: the ids (literal, or a prefix glued to a variable) and the data-* attributes of its calls.
function buttonNames() {
  const ids = new Set(), prefixes = new Set(), attrs = new Set();
  for (const f of [...walk("src/ui", ".js"), ...walk("src/moments", ".js")]) {
    for (const call of readFileSync(path.join(ROOT, f), "utf8").matchAll(/actButton\(\{([\s\S]*?)\}\)/g)) {
      for (const m of call[1].matchAll(/\bid:\s*["']([\w-]+)["'](\s*\+)?/g)) (m[2] ? prefixes : ids).add(m[1]);
      for (const m of call[1].matchAll(/["'](data-[\w-]+)["']\s*:/g)) attrs.add(m[1]);
    }
  }
  return { ids, prefixes, attrs };
}
let NAMES = null;
const names = () => (NAMES ??= buttonNames());
const ancestors = (s) => s.split(/\s*[\s>+~]\s*/).filter(Boolean).slice(0, -1);
// a selector reaches the button when it names .act or .act-label anywhere; styles a bare button (or every element) inside something; names, in the
// element it styles, an id or (on its own) a data-* attribute that a module hands to actButton (#seq-prev, [data-action…]); or styles a bare child of the drawer's
// bars (.detail-bar > :last-child, .d-actions > *)
const reaches = (s) => {
  const subject = subjectOf(s);
  if (hasClass(s, "act") || hasClass(s, "act-label")) return true;
  if (/^(button|\*|\[class)/.test(subject) && subject !== s.trim()) return true;
  const { ids, prefixes, attrs } = names();
  for (const m of subject.matchAll(/#([\w-]+)/g)) if (ids.has(m[1]) || [...prefixes].some((x) => m[1].startsWith(x))) return true;
  // an attribute alone names the element (with a class or a tag next to it, the class or the tag says what it is: .dot[data-engine] is no button)
  if (/^\[/.test(subject) && !/\.[\w-]/.test(subject)) for (const m of subject.matchAll(/\[\s*(data-[\w-]+)/g)) if (attrs.has(m[1])) return true;
  return /^(:|\*|\[)/.test(subject) && ancestors(s).some((c) => hasClass(c, "detail-bar") || hasClass(c, "d-actions"));
};
function redraws(rule, skip = () => false) {
  const sels = rule.selectors.filter((s) => reaches(s) && !skip(s));
  const bad = decls(rule.body).map(([p]) => p).filter((p) => BANNED(p) || (p === "font-weight" && sels.some((s) => hasClass(subjectOf(s), "act"))));
  return sels.length && bad.length ? `${sels.join(", ")} { ${bad.join(", ")} }` : null;
}

function buildPageStyle(tag) {
  mkdirSync(path.join(ROOT, "dist"), { recursive: true });
  const entry = `dist/t32-${tag}-${process.pid}.js`;
  const out = `dist/t32-${tag}-${process.pid}.html`;
  writeFileSync(path.join(ROOT, entry), 'document.body.dataset.ok = "1";');
  try {
    execFileSync(process.execPath, ["build.mjs", "--entry", entry, "--out", out], { cwd: ROOT, encoding: "utf8", stdio: "pipe" });
    const style = /<style[^>]*>([\s\S]*?)<\/style>/.exec(read(out));
    assert.ok(style, "the page has its <style>");
    return style[1];
  } finally {
    rmSync(path.join(ROOT, entry), { force: true });
    rmSync(path.join(ROOT, out), { force: true });
  }
}

// ---- a DOM just big enough for core/dom.js's h(): actButton builds through it (AGENT-RULES.md: core/dom.js is the only place that builds DOM)

class FakeNode {}
class FakeText extends FakeNode { constructor(text) { super(); this.nodeType = 3; this.data = String(text); } get textContent() { return this.data; } set textContent(v) { this.data = String(v); } }
class FakeElement extends FakeNode {
  constructor(tag) { super(); this.nodeType = 1; this.tagName = tag.toUpperCase(); this.attrs = new Map(); this.childNodes = []; this.dataset = {}; this.listeners = {}; this.style = { setProperty() {} }; }
  get className() { return this.attrs.get("class") || ""; }
  set className(v) { this.attrs.set("class", String(v)); }
  setAttribute(name, value) { this.attrs.set(name, String(value)); if (name.startsWith("data-")) this.dataset[name.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase())] = String(value); }
  getAttribute(name) { return this.attrs.has(name) ? this.attrs.get(name) : null; }
  hasAttribute(name) { return this.attrs.has(name); }
  removeAttribute(name) { this.attrs.delete(name); }
  addEventListener(type, fn) { this.listeners[type] = fn; }
  append(...kids) { for (const k of kids) this.childNodes.push(k instanceof FakeNode ? k : new FakeText(k)); }
  get children() { return this.childNodes.filter((k) => k instanceof FakeElement); }
  get firstChild() { return this.childNodes[0] || null; }
  get lastChild() { return this.childNodes[this.childNodes.length - 1] || null; }
  get textContent() { return this.childNodes.map((k) => k.textContent).join(""); }
  set textContent(v) { this.childNodes = [new FakeText(v)]; }
  querySelector(sel) {
    const m = /^\.([\w-]+)$/.exec(sel);
    if (!m) throw new Error(`the fake DOM looks up one class only: ${sel}`);
    for (const k of this.children) {
      if (k.className.split(/\s+/).includes(m[1])) return k;
      const deeper = k.querySelector(sel);
      if (deeper) return deeper;
    }
    return null;
  }
}
const fakeDocument = { createElement: (tag) => new FakeElement(tag), createElementNS: (ns, tag) => new FakeElement(tag), createTextNode: (text) => new FakeText(text), addEventListener() {} };

async function loadAct() {
  globalThis.Node ??= FakeNode;
  globalThis.document ??= fakeDocument;
  return exists("src/ui/act.js") ? import(pathToFileURL(path.join(ROOT, "src/ui/act.js")).href) : {};
}

const clsClasses = () => {
  const cls = new Set();
  for (const f of MODULES()) for (const m of read(f).matchAll(/actButton\(\{[^}]*\bcls:\s*["']([\w-]+)["']/g)) cls.add(m[1]);
  return cls;
};

// ---- 1

test("ui/act.js exports actButton, the one action button: a <button type=button class=act>, a mono key when there is one (empty too), the verb in .act-label, and every other attribute as given", async () => {
  const { actButton } = await loadAct();
  assert.equal(typeof actButton, "function", "src/ui/act.js does not export actButton");
  const b = actButton({ key: "a", label: "Aprobar", primary: true, "data-action": "approve", onclick: () => {} });
  assert.equal(b.tagName, "BUTTON");
  assert.equal(b.getAttribute("type"), "button");
  assert.deepEqual(b.className.split(/\s+/).sort(), ["act", "primary"]);
  assert.equal(b.getAttribute("data-action"), "approve");
  assert.equal(typeof b.listeners.click, "function", "an onclick is a listener");
  assert.deepEqual(["key", "label", "primary", "cls"].filter((x) => b.hasAttribute(x) || actButton({ key: "", cls: "x" }).hasAttribute(x)), [], "key, label, primary and cls are actButton's own: not attributes of the button");
  const [k, label] = b.children;
  assert.ok(k && k.tagName === "KBD" && k.className === "k" && k.textContent === "a", "the key comes first, as <kbd class=k>");
  assert.ok(label && label.tagName === "SPAN" && label.className === "act-label" && label.textContent === "Aprobar", "then the verb, as <span class=act-label>");
  const plain = actButton({ label: "Cerrar" });
  assert.equal(plain.className, "act", "a button that is not the next verb is plain .act");
  assert.deepEqual(plain.children.map((c) => c.tagName + "." + c.className), ["SPAN.act-label"], "no key: no <kbd>, only the verb");
  // the ones the stops fill later: an empty key and an empty verb, with a class of their own
  const batch = actButton({ key: "", cls: "batch-btn", "data-action": "batch" });
  assert.deepEqual(batch.className.split(/\s+/).sort(), ["act", "batch-btn"], "cls is added to act");
  assert.deepEqual(batch.children.map((c) => c.tagName + "." + c.className + "=" + c.textContent), ["KBD.k=", "SPAN.act-label="], "an empty key is still a <kbd>");
  const seq = actButton({ label: "‹", primary: false, id: "seq-prev", title: "t", "aria-label": "Anterior" });
  assert.equal(seq.className, "act");
  assert.deepEqual([seq.getAttribute("id"), seq.getAttribute("title"), seq.getAttribute("aria-label")], ["seq-prev", "t", "Anterior"], "id, title and aria-* go through");
});

// ---- 2

test("ui/act.js exports outLink, a link with the exit icon after its words, and the drawer's modules draw no bare link", async () => {
  const { outLink } = await loadAct();
  assert.equal(typeof outLink, "function", "src/ui/act.js does not export outLink");
  const a = outLink("https://example.test/job", "Ver el aviso");
  assert.equal(a.tagName, "A");
  assert.equal(a.textContent, "Ver el aviso", "the link reads as its words");
  const last = a.childNodes[a.childNodes.length - 1];
  assert.ok(last.tagName === "SVG" && last.children[0]?.getAttribute("href") === "#i-external-link", "the exit icon comes last");
  const svgIn = (n) => (n.children || []).some((c) => c.tagName === "SVG" || svgIn(c));
  for (const url of ["", null, "javascript:alert(1)"]) {
    const none = outLink(url, "Ver el aviso");
    assert.ok(none.tagName !== "A" && !svgIn(none), `outLink(${JSON.stringify(url)}) is no link, and carries no exit icon (${none.tagName})`);
  }
  const bare = [];
  for (const f of ["src/ui/detail.js", ...walk("src/ui/detail", ".js"), "src/ui/drawer.js"]) {
    read(f).split("\n").forEach((l, i) => { if (/(?<![\w.$])link\(|h\(\s*["'`]a["'`]/.test(l)) bare.push(`${f}:${i + 1}`); });
  }
  assert.deepEqual(bare, [], "the drawer's links are outLink");
});

// ---- 3

// Per module: the <button>s built by hand that are not verbs and stay. COLA: the steps k and j, the two keys of the batch's confirmation and the retry of the
// factors; APRENDER: "more", a topic and a grade; HOY: "more". Every other button of these modules is an actButton.
const OTHER_BUTTONS = {
  "src/moments/hoy/rows.js": 0, "src/moments/hoy/prompt.js": 0, "src/moments/hoy/render.js": 1,
  "src/moments/aprender/render.js": 2, "src/moments/aprender/drillui.js": 1, "src/moments/cola/render.js": 5,
  "src/moments/sistema/render.js": 0, "src/moments/procesos/drawer-content.js": 0,
  "src/ui/detail.js": 0, "src/ui/detail/mark.js": 0, "src/ui/detail/shot.js": 0, "src/ui/drawer.js": 0,
};
test("every module that draws verbs imports and calls actButton, and builds by hand only the buttons that are not verbs", () => {
  const wrong = [];
  for (const [f, n] of Object.entries(OTHER_BUTTONS)) {
    const src = read(f);
    const imports = /import\s*\{[^}]*\bactButton\b[^}]*\}\s*from\s*["'][^"']*\/act\.js["']/.test(src);
    const calls = /\bactButton\(/.test(src);
    const hand = (src.match(/\bh\(\s*["'`]button["'`]/g) || []).length;
    if (!imports || !calls || hand !== n) wrong.push(`${f}: imports ${imports}, calls ${calls}, ${hand} other buttons (expected ${n})`);
  }
  assert.deepEqual(wrong, []);
});

// Every string literal of a line ("…", '…', `…`, with the ${…} of a template taken out), split into its space-separated words: a class string
// is caught by any of its tokens, not only the first ("turn-open act", `x ${y} act`).
const tokensOf = (line) => [...line.matchAll(/(["'`])((?:\\.|(?!\1)[^\\\n])*)\1/g)].flatMap((m) => m[2].replace(/\$\{[^}]*\}/g, " ").split(/\s+/).filter(Boolean));

// ---- 4

test("no module of src/moments or src/ui writes the classes act or act-label in any quote, as any word of a class string (template strings too): only ui/act.js does", () => {
  const hand = [];
  for (const file of MODULES()) {
    if (file === "src/ui/act.js") continue;
    read(file).split("\n").forEach((line, i) => { if (tokensOf(line).some((w) => w === "act" || w === "act-label")) hand.push(`${file}:${i + 1}`); });
  }
  assert.deepEqual(hand, [], "these lines build an action button (or its label) by hand: use actButton from ui/act.js");
});

// ---- 5

test("the drawer has no .btn of its own: no sheet styles a .btn and no module of src/ui or src/moments gives a button that class", () => {
  const sheets = [];
  for (const file of walk("src/styles", ".css")) {
    for (const rule of rulesOf(read(file))) for (const s of rule.selectors) if (hasClass(s, "btn")) sheets.push(`${file}: ${s}`);
  }
  assert.deepEqual(sheets, [], "a .btn rule is still in the styles");
  const code = [];
  for (const file of MODULES()) read(file).split("\n").forEach((line, i) => { if (tokensOf(line).includes("btn")) code.push(`${file}:${i + 1}`); });
  assert.deepEqual(code, [], "a module still writes the class btn");
});

// ---- 6

test("only styles/actions.css draws the button: no other sheet (an _-prefixed one too) redraws .act, .act-label or a bare button, longhands and custom properties included", () => {
  const files = walk("src/styles", ".css");
  const elsewhere = [];
  for (const f of files.filter((x) => x !== "src/styles/actions.css")) for (const r of rulesOf(read(f))) { const o = redraws(r); if (o) elsewhere.push(`${f}: ${o}`); }
  assert.deepEqual(elsewhere, [], "only styles/actions.css draws the shape of .act");
  assert.ok(exists("src/styles/actions.css"), "src/styles/actions.css does not exist");
  const display = rulesOf(read("src/styles/actions.css")).filter((r) => r.selectors.some((s) => hasClass(subjectOf(s), "act")) && decls(r.body).some(([p]) => p === "display"));
  assert.equal(display.length, 1, `actions.css gives .act its display once: ${JSON.stringify(display)}`);
});

// ---- 7

test("the page's one style carries styles/actions.css: the built page gives .act its display once, inline-flex, and no rule of the page redraws the button", () => {
  const css = buildPageStyle("act");
  const display = rulesOf(css).filter((r) => r.selectors.some((s) => hasClass(subjectOf(s), "act")) && decls(r.body).some(([p]) => p === "display"));
  assert.equal(display.length, 1, `the page's style sets the display of .act ${display.length} times: ${display.map((r) => r.selectors.join(",")).join(" | ")}`);
  assert.match(display[0].body, /display:\s*inline-flex/);
  // actions.css's own rules all start with .act; anything else in the page that reaches the button and redraws it came from somewhere else
  const page = rulesOf(css).map((r) => redraws(r, (s) => /^\.act(?![\w-])/.test(s))).filter(Boolean);
  assert.deepEqual(page, [], "a rule of the page redraws the button");
});

// ---- 8

// The stops look as they did: these are the values the five copies shared (13 px, 28 px, grey with the next verb in ink and its key gold, gold when armed,
// the verb underlined only under the mouse), with the weight of the verb moved to the whole button and the key back at 400.
const DESK = {
  ".act": { display: "inline-flex", "min-height": "28px", color: "var(--ink-dim)", "font-size": "var(--fs-2)", "font-weight": "600", "text-decoration": "none" },
  ".act .k": { "font-weight": "400" },
  ".act.primary": { color: "var(--ink)" },
  ".act.primary .k": { color: "var(--ink-accent)", "border-color": "var(--ink-accent)" },
  ".act:hover .act-label": { "text-decoration": "underline", "text-underline-offset": "3px" },
};
test("actions.css: the stops' values, as tokens; no underline at rest; 44 px and no keys on a phone; and the drawer's links stay underlined, their exit icon apart from the words", () => {
  assert.ok(exists("src/styles/actions.css"), "src/styles/actions.css does not exist");
  const css = uncomment(read("src/styles/actions.css"));
  const desk = rulesOf(css.replace(/@media[^{]*\{(?:[^{}]*\{[^{}]*\})*[^{}]*\}/g, ""));
  const off = [];
  for (const [sel, want] of Object.entries(DESK)) {
    const got = Object.fromEntries(desk.filter((r) => r.selectors.includes(sel)).flatMap((r) => decls(r.body)));
    for (const [p, v] of Object.entries(want)) if (got[p] !== v) off.push(`${sel} { ${p}: ${got[p] ?? "nothing"} } (the stops had ${v})`);
  }
  assert.deepEqual(off, [], "actions.css draws the button the stops had");
  const tokens = new Set([...read("src/styles/tokens.css").matchAll(/(--[\w-]+)\s*:/g)].map((m) => m[1]));
  const bad = [];
  for (const r of rulesOf(css)) for (const [p, v] of decls(r.body)) {
    for (const m of v.matchAll(/var\(\s*(--[\w-]+)/g)) if (!tokens.has(m[1])) bad.push(`${r.selectors} ${p}: ${v} (not a token of tokens.css)`);
    if (/^text-decoration/.test(p) && !/none/.test(v) && !r.selectors.every((s) => /:hover|:focus/.test(s))) bad.push(`${r.selectors} ${p}: ${v} at rest`);
  }
  assert.deepEqual(bad, []);
  const touch = /@media\s*\(\s*max-width:\s*759px\s*\)\s*\{((?:[^{}]*\{[^{}]*\})*)[^{}]*\}/.exec(css);
  assert.ok(touch, "actions.css has its touch block (max-width: 759px)");
  const rules = rulesOf(touch[1]);
  assert.ok(rules.some((r) => r.selectors.includes(".act") && /min-height:\s*44px/.test(r.body)), "44 px on a phone (review B7)");
  assert.ok(rules.some((r) => r.selectors.includes(".act .k") && /display:\s*none/.test(r.body)), "no keys on a phone");
  const links = rulesOf(read("src/styles/ui/detail.css")).filter((r) => r.selectors.some((s) => subjectOf(s) === "a"));
  assert.ok(links.some((r) => /text-decoration:\s*underline/.test(r.body)), "the drawer's links are underlined");
  assert.ok(!links.some((r) => /text-decoration(-line)?:\s*none/.test(r.body)), "and nothing takes it away");
  // outLink puts the icon right after the words, with no text between: a margin on the icon of a link keeps them apart
  const air = ["src/styles/actions.css", "src/styles/ui/detail.css"].flatMap((f) => rulesOf(read(f))).some((r) =>
    r.selectors.some((sel) => subjectOf(sel) === ".icon" && sel.split(/\s*[\s>+~]\s*/).slice(0, -1).some((c) => /^a(?![\w-])/.test(c)))
    && decls(r.body).some(([p, v]) => /^margin-(left|inline-start|inline)$/.test(p) && parseFloat(v) > 0));
  assert.ok(air, "the exit icon of a link stands apart from its words: a margin-left on `a .icon` (actions.css or ui/detail.css)");
});

// ---- 9

test("the classes a caller hands to actButton (cls) are the button too: no sheet redraws them, except a desk-only height", () => {
  const cls = clsClasses();
  assert.ok(cls.size >= 4, `the stops hand their own classes to actButton (ap-start, ap-reveal, batch-btn, stop-btn): ${[...cls].join(", ") || "none"}`);
  const bad = [];
  for (const f of walk("src/styles", ".css").filter((x) => x !== "src/styles/actions.css")) {
    const css = uncomment(read(f)).replace(/@media\s*\(\s*min-width:\s*760px\s*\)\s*\{(?:[^{}]*\{[^{}]*\})*[^{}]*\}/g, "");
    for (const r of rulesOf(css)) for (const s of r.selectors) if ([...cls].some((c) => hasClass(subjectOf(s), c))) {
      const b = decls(r.body).map(([p]) => p).filter((p) => BANNED(p));
      if (b.length) bad.push(`${f}: ${s} { ${b.join(", ")} }`);
    }
  }
  assert.deepEqual(bad, [], "a phone gets the 44 px of actions.css for every action button");
});

// ---- 10

test("twoStep (core/dom.js) arms an action button in its verb: the key and the .act-label stay, and the verb comes back on the timeout and after the second press", async (ctx) => {
  const { actButton } = await loadAct();
  assert.equal(typeof actButton, "function", "src/ui/act.js does not export actButton");
  ctx.mock.timers.enable({ apis: ["setTimeout", "Date"], now: 1_000_000 });
  const { h, twoStep, ARM_MIN_MS, ARM_MAX_MS } = await import(pathToFileURL(path.join(ROOT, "src/core/dom.js")).href);
  const { t } = await import(pathToFileURL(path.join(ROOT, "src/i18n/index.js")).href);
  const sure = t("act.sure");
  const shape = (b) => b.childNodes.map((c) => (c.tagName ? `${c.tagName}.${c.className}=${c.textContent}` : `#text=${c.textContent}`));
  let ran = 0;
  const b = actButton({ key: "c", label: "Ya la envié", "data-act": "confirm" });
  const press = twoStep("Ya la envié", () => { ran += 1; });
  press({ currentTarget: b });
  assert.deepEqual(shape(b), ["KBD.k=c", `SPAN.act-label=${sure}`], "armed, the button keeps its key and its label, and the label says so");
  ctx.mock.timers.tick(ARM_MAX_MS);
  assert.deepEqual(shape(b), ["KBD.k=c", "SPAN.act-label=Ya la envié"], "the timeout gives the verb back, in its label");
  press({ currentTarget: b });
  ctx.mock.timers.tick(ARM_MIN_MS + 50);
  press({ currentTarget: b });
  assert.equal(ran, 1, "the second, deliberate press runs it");
  assert.deepEqual(shape(b), ["KBD.k=c", "SPAN.act-label=Ya la envié"], "and the verb is back, in its label");
  // a button with no .act-label: the words are its whole text, as before
  const plain = h("button", { type: "button" }, "Hecha");
  const pressPlain = twoStep("Hecha", () => {});
  pressPlain({ currentTarget: plain });
  assert.equal(plain.textContent, sure);
  ctx.mock.timers.tick(ARM_MAX_MS);
  assert.equal(plain.textContent, "Hecha");
});

// ---- 11

test("the armed gold wins over any stop's colour: actions.css writes it !important, and no other sheet paints an action button !important", () => {
  assert.ok(exists("src/styles/actions.css"), "src/styles/actions.css does not exist");
  const own = rulesOf(read("src/styles/actions.css"));
  const gold = (sel, prop) => own.some((r) => r.selectors.includes(sel) && decls(r.body).some(([p, v]) => p === prop && /^var\(--ink-accent\)\s*!important$/.test(v)));
  const armedSel = own.flatMap((r) => r.selectors).find((x) => /^\.act\[data-armed\][^ ]*$/.test(x));
  const missing = [];
  if (!gold(".act.armed", "color")) missing.push(".act.armed { color: var(--ink-accent) !important }");
  if (!armedSel || !gold(armedSel, "color")) missing.push(".act[data-armed…] { color: var(--ink-accent) !important }");
  for (const prop of ["color", "border-color"]) {
    if (!gold(".act.armed .k", prop)) missing.push(`.act.armed .k { ${prop}: var(--ink-accent) !important }`);
    if (!armedSel || !gold(armedSel + " .k", prop)) missing.push(`.act[data-armed…] .k { ${prop}: var(--ink-accent) !important }`);
  }
  assert.deepEqual(missing, [], "actions.css: armed is gold over anything a stop writes");
  const cls = [...clsClasses()];
  const loud = [];
  for (const f of walk("src/styles", ".css").filter((x) => x !== "src/styles/actions.css")) {
    for (const r of rulesOf(read(f))) {
      const sels = r.selectors.filter((x) => reaches(x) || cls.some((c) => hasClass(x, c)));
      const paint = decls(r.body).filter(([p, v]) => /!important/.test(v) && /^(color|background|border|opacity|filter|-webkit-text-fill-color)/.test(p)).map(([p]) => p);
      if (sels.length && paint.length) loud.push(`${f}: ${sels.join(", ")} { ${paint.join(", ")} }`);
    }
  }
  assert.deepEqual(loud, [], "only actions.css may paint an action button !important (colour, background, border, opacity): a stop's colour must not beat the armed gold");
});

// ---- 12

test("the pressed gold belongs to the drawer's answers: a rule on a pressed action button (aria-pressed) sits under .d-marks", () => {
  const loose = [];
  for (const f of walk("src/styles", ".css")) {
    for (const r of rulesOf(read(f))) for (const sel of r.selectors) {
      if (!/\[aria-pressed/.test(sel) || !(hasClass(sel, "act") || hasClass(sel, "act-label"))) continue;
      const parts = sel.split(/\s*[\s>+~]\s*/).filter(Boolean);
      const at = parts.findIndex((c) => /\[aria-pressed/.test(c));
      if (!parts.slice(0, at).some((c) => hasClass(c, "d-marks"))) loose.push(`${f}: ${sel}`);
    }
  }
  assert.deepEqual(loose, [], "a pressed button elsewhere (Ampliar in a capture) must not turn gold");
  const marks = rulesOf(read("src/styles/ui/detail.css")).some((r) => r.selectors.some((sel) => /\.d-marks\s.*\.act\[aria-pressed="true"\]$/.test(sel)) && /color:\s*var\(--ink-accent\)/.test(r.body));
  assert.ok(marks, "the answer he picked is still gold (.d-marks .act[aria-pressed=\"true\"])");
});
