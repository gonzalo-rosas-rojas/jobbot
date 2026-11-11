// Acceptance test of the encargo docs/encargos/nota-del-demo-en-hoy.md: the public demo says in one line what jobbot is, that its data is
// fictional, and links to its code and its showcase, inside the sheet the demo opens on, at every width.
// (1) scripts/demo/demo.js hands the page its line as data, `__DEMO__.about`, every text a pair [es, en], the same in every scenario and whatever
//     the language the page loaded in; and the demo adds nothing to the document by itself (no element, no <style>, no position).
// (2) HOY's view says it in the language of the page at the moment it is drawn (the key `l` changes it), and says nothing without it.
// (3) HOY draws it, through the moment the shell mounts (moments/hoy.js mount + update): the first thing of its sheet, the line, the note and two
//     links that open in a new tab, in both languages, on a desktop and on a phone, in every scenario; with no `about` (the real panel), none of it.
// (4) The real panel has none of it: no dictionary has these words and the built page has neither address; the published demo has both, and
//     its policy names only the page's own <style>.
// (5) The note lives in the flow of the sheet: no rule of src/styles that reaches it positions it, lifts it or moves it.
// (6) main.js hands the demo's `about` to the shell and the shell puts it in the context of the moments (read in the source).
// No browser: the demo runs in a vm, the view is pure, HOY is mounted in a small document of this file (enough of the DOM for core/dom.js and the
// moment: elements, text, attributes, dataset, classList, the insertion methods and a querySelector of tags, classes and attributes).
import test from "node:test";
import assert from "node:assert/strict";
import vm from "node:vm";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";
import { DEMO_SOURCE, loadDemo } from "./helpers/demo-host.mjs";
import { setLang, setTimeZone } from "../src/i18n/index.js";
import { model } from "../src/core/model.js";
import { hoyView } from "../src/moments/hoy/view.js";
import hoy from "../src/moments/hoy.js";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (rel) => fs.readFileSync(path.join(ROOT, rel), "utf8").replace(/\r\n/g, "\n");
const plain = (x) => (x === undefined ? undefined : JSON.parse(JSON.stringify(x)));   // what the vm hands back, as plain objects of this realm
const fresh = (lang) => { setLang(lang); setTimeZone("America/Santiago"); };

const REPO = "https://github.com/gonzalo-rosas-rojas/jobbot";
const SHOWCASE = "https://gonzalo-rosas-rojas.github.io/jobbot-showcase/";
const SAYS = {
  es: { line: "jobbot busca avisos, adapta el CV y llena el formulario de noche; tú apruebas cada envío.", note: "Demo con datos ficticios: nada se envía.", links: [{ href: REPO, text: "Código" }, { href: SHOWCASE, text: "Cómo funciona" }] },
  en: { line: "jobbot finds postings, tailors the CV and fills the form overnight; you approve every send.", note: "Demo with fictional data: nothing is sent.", links: [{ href: REPO, text: "Code" }, { href: SHOWCASE, text: "How it works" }] },
};
const ABOUT = {
  line: [SAYS.es.line, SAYS.en.line],
  note: [SAYS.es.note, SAYS.en.note],
  links: [{ href: REPO, text: ["Código", "Code"] }, { href: SHOWCASE, text: ["Cómo funciona", "How it works"] }],
};
const WORDS = [SAYS.es.line, SAYS.en.line, SAYS.es.note, SAYS.en.note];
const SCENARIOS = ["", "?scenario=urgent", "?scenario=calm", "?scenario=empty", "?scenario=stale", "?scenario=heavy"];

// ---- a small document ------------------------------------------------------------------------------------------------------------------

class Node {
  constructor(type) { this.nodeType = type; this.parentNode = null; this.childNodes = []; }
  get parentElement() { return this.parentNode && this.parentNode.nodeType === 1 ? this.parentNode : null; }
  get firstChild() { return this.childNodes[0] || null; }
  get lastChild() { return this.childNodes[this.childNodes.length - 1] || null; }
  get nextSibling() { const p = this.parentNode; return p ? p.childNodes[p.childNodes.indexOf(this) + 1] || null : null; }
  get previousSibling() { const p = this.parentNode; return p ? p.childNodes[p.childNodes.indexOf(this) - 1] || null : null; }
  get isConnected() { let n = this; while (n.parentNode) n = n.parentNode; return n === doc.documentElement; }
  remove() { if (this.parentNode) this.parentNode.removeChild(this); }
  contains(other) { for (let n = other; n; n = n.parentNode) if (n === this) return true; return false; }
}
class Text extends Node {
  constructor(text) { super(3); this.data = String(text); }
  get textContent() { return this.data; }
  set textContent(v) { this.data = String(v); }
}
const kid = (x) => (x instanceof Node ? x : new Text(x));
class Element extends Node {
  constructor(tag, ns = null) {
    super(1);
    this.tagName = String(tag).toUpperCase(); this.localName = String(tag).toLowerCase(); this.namespaceURI = ns;
    this.attributes = new Map(); this.listeners = [];
    // dataset reflects the data-* attributes, as in a browser: h() writes "data-action", a row reads dataset.action
    const attr = (k) => "data-" + String(k).replace(/[A-Z]/g, (c) => "-" + c.toLowerCase());
    this.dataset = new Proxy({}, {
      get: (_, k) => (typeof k === "string" && this.attributes.has(attr(k)) ? this.attributes.get(attr(k)) : undefined),
      set: (_, k, v) => { this.attributes.set(attr(k), String(v)); return true; },
      deleteProperty: (_, k) => { this.attributes.delete(attr(k)); return true; },
      has: (_, k) => this.attributes.has(attr(k)),
      ownKeys: () => [...this.attributes.keys()].filter((a) => a.startsWith("data-")).map((a) => a.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase())),
      getOwnPropertyDescriptor: (_, k) => (this.attributes.has(attr(k)) ? { value: this.attributes.get(attr(k)), enumerable: true, configurable: true, writable: true } : undefined),
    });
    const props = new Map();
    this.style = { setProperty: (k, v) => props.set(k, String(v)), removeProperty: (k) => props.delete(k), getPropertyValue: (k) => props.get(k) || "", props };
    const self = this;
    this.classList = {
      add: (...c) => { const s = new Set(self.className.split(/\s+/).filter(Boolean)); c.forEach((x) => s.add(x)); self.className = [...s].join(" "); },
      remove: (...c) => { self.className = self.className.split(/\s+/).filter((x) => x && !c.includes(x)).join(" "); },
      toggle: (c, on) => { const has = self.classList.contains(c); const want = on === undefined ? !has : !!on; if (want && !has) self.classList.add(c); if (!want && has) self.classList.remove(c); return want; },
      contains: (c) => self.className.split(/\s+/).includes(c),
    };
  }
  get children() { return this.childNodes.filter((n) => n.nodeType === 1); }
  get firstElementChild() { return this.children[0] || null; }
  get className() { return this.attributes.get("class") || ""; }
  set className(v) { this.attributes.set("class", String(v)); }
  get id() { return this.attributes.get("id") || ""; }
  set id(v) { this.attributes.set("id", String(v)); }
  get hidden() { return this.attributes.has("hidden"); }
  set hidden(v) { if (v) this.attributes.set("hidden", ""); else this.attributes.delete("hidden"); }
  get title() { return this.attributes.get("title") || ""; }
  set title(v) { this.attributes.set("title", String(v)); }
  get href() { return this.attributes.get("href") || ""; }
  set href(v) { this.attributes.set("href", String(v)); }
  get target() { return this.attributes.get("target") || ""; }
  set target(v) { this.attributes.set("target", String(v)); }
  get rel() { return this.attributes.get("rel") || ""; }
  set rel(v) { this.attributes.set("rel", String(v)); }
  get textContent() { return this.childNodes.map((n) => n.textContent).join(""); }
  set textContent(v) { this.replaceChildren(); if (String(v) !== "") this.append(String(v)); }
  setAttribute(k, v) { this.attributes.set(k, String(v)); }
  getAttribute(k) { return this.attributes.has(k) ? this.attributes.get(k) : null; }
  hasAttribute(k) { return this.attributes.has(k); }
  removeAttribute(k) { this.attributes.delete(k); }
  toggleAttribute(k, on) { const want = on === undefined ? !this.attributes.has(k) : !!on; if (want) this.attributes.set(k, this.attributes.get(k) || ""); else this.attributes.delete(k); return want; }
  addEventListener(type, fn) { this.listeners.push([type, fn]); }
  removeEventListener(type, fn) { this.listeners = this.listeners.filter(([t, f]) => t !== type || f !== fn); }
  insertBefore(node, ref) {
    if (node.parentNode) node.parentNode.removeChild(node);
    const at = ref ? this.childNodes.indexOf(ref) : -1;
    this.childNodes.splice(at < 0 ? this.childNodes.length : at, 0, node);
    node.parentNode = this;
    return node;
  }
  appendChild(node) { return this.insertBefore(node, null); }
  removeChild(node) { const i = this.childNodes.indexOf(node); if (i >= 0) this.childNodes.splice(i, 1); node.parentNode = null; return node; }
  append(...xs) { for (const x of xs) this.insertBefore(kid(x), null); }
  prepend(...xs) { const ref = this.firstChild; for (const x of xs) this.insertBefore(kid(x), ref); }
  before(...xs) { const p = this.parentNode; for (const x of xs) p.insertBefore(kid(x), this); }
  after(...xs) { const p = this.parentNode; let ref = this.nextSibling; for (const x of xs) p.insertBefore(kid(x), ref); }
  replaceWith(...xs) { const p = this.parentNode; const ref = this.nextSibling; p.removeChild(this); for (const x of xs) p.insertBefore(kid(x), ref); }
  replaceChildren(...xs) { for (const n of [...this.childNodes]) this.removeChild(n); this.append(...xs); }
  focus() { doc.activeElement = this; }
  blur() { if (doc.activeElement === this) doc.activeElement = doc.body; }
  closest(sel) { for (let n = this; n && n.nodeType === 1; n = n.parentNode) if (matches(n, sel)) return n; return null; }
  matches(sel) { return matches(this, sel); }
  querySelectorAll(sel) { return descendants(this).filter((n) => matches(n, sel)); }
  querySelector(sel) { return this.querySelectorAll(sel)[0] || null; }
  getBoundingClientRect() { return { top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0 }; }
  getClientRects() { return []; }
}
const descendants = (el, out = []) => { for (const n of el.childNodes) if (n.nodeType === 1) { out.push(n); descendants(n, out); } return out; };
/** tag, .class, #id, [attr], [attr="v"] and their compounds, in a list split by commas; anything else matches nothing. */
function matches(el, sel) {
  return String(sel).split(",").some((one) => {
    const parts = one.trim().match(/^([a-z][a-z0-9-]*|\*)?((?:\.[\w-]+|#[\w-]+|\[[^\]]+\])*)$/i);
    if (!parts) return false;
    if (parts[1] && parts[1] !== "*" && parts[1].toLowerCase() !== el.localName) return false;
    for (const bit of parts[2].match(/\.[\w-]+|#[\w-]+|\[[^\]]+\]/g) || []) {
      if (bit[0] === ".") { if (!el.className.split(/\s+/).includes(bit.slice(1))) return false; }
      else if (bit[0] === "#") { if (el.id !== bit.slice(1)) return false; }
      else {
        const m = /^\[([\w-]+)(?:([~^$*]?=)"?([^"\]]*)"?)?\]$/.exec(bit);
        if (!m || !el.hasAttribute(m[1])) return false;
        const v = el.getAttribute(m[1]);
        if (m[2] === "=" && v !== m[3]) return false;
        if (m[2] === "~=" && !v.split(/\s+/).includes(m[3])) return false;
      }
    }
    return true;
  });
}
const doc = {
  documentElement: new Element("html"), body: new Element("body"), head: new Element("head"), activeElement: null, visibilityState: "visible",
  createElement: (tag) => new Element(tag),
  createElementNS: (ns, tag) => new Element(tag, ns),
  createTextNode: (text) => new Text(text),
  addEventListener() {}, removeEventListener() {},
  querySelector: (sel) => doc.documentElement.querySelector(sel),
  querySelectorAll: (sel) => doc.documentElement.querySelectorAll(sel),
  getElementById: (id) => descendants(doc.documentElement).find((n) => n.id === id) || null,
};
doc.documentElement.append(doc.head, doc.body);
doc.activeElement = doc.body;

/** Runs `fn` with the small document as the page's (document, Node, CSS, requestAnimationFrame), and gives the globals back after. */
async function inDocument(fn) {
  const names = ["document", "Node", "Element", "CSS", "requestAnimationFrame", "cancelAnimationFrame"];
  const saved = names.map((n) => [n, Object.getOwnPropertyDescriptor(globalThis, n)]);
  Object.assign(globalThis, { document: doc, Node, Element, CSS: { escape: (s) => String(s).replace(/["\\]/g, "\\$&") }, requestAnimationFrame: () => 0, cancelAnimationFrame() {} });
  try { return await fn(); } finally {
    for (const [n, d] of saved) { if (d) Object.defineProperty(globalThis, n, d); else delete globalThis[n]; }
  }
}

/** HOY as the shell shows it: mounted into a host of the sheet and updated with a model twice (the first picture and a refresh), with the context it is given. Returns the host. */
async function drawHoy(snap, { lang, about, flat = false }) {
  fresh(lang);
  return inDocument(() => {
    const host = new Element("div");
    host.className = "sheet-host";
    doc.body.replaceChildren(host);
    const nowMs = Date.parse(snap.generated_at);
    const ctx = {
      session: { act: async () => ({ ok: true }), fresh: new Set(), refresh() {} }, sound: { play() {} }, announce() {}, toast() {},
      open() {}, go() {}, reduced: () => true, flat: () => flat, now: () => nowMs, lastVisit: () => nowMs - 5 * 3600000,
      openUrl() {}, ask: null, afterLink() {}, ...(about === undefined ? {} : { about }),
    };
    hoy.mount(host, ctx);
    hoy.update(model(snap), ctx);
    const firstOf = () => { const s = host.children.find((n) => n.localName === "section"); return s ? s.firstElementChild : null; };
    const p = firstOf(), kids = p ? [...p.childNodes] : [], links = p ? anchors(p) : [];
    hoy.update(model(snap), ctx);                               // the next refresh (every 30 s) draws the same page again
    // a refresh that changes nothing writes nothing (hoy/render.js): the same <p>, the same two <a>, and no child added or replaced
    const again = firstOf();
    assert.ok(again === p, "after a refresh the first element of HOY is the same node");
    assert.ok(again === null || (again.childNodes.length === kids.length && kids.every((k, i) => again.childNodes[i] === k)), "and it got no new children");
    assert.ok(again === null || (anchors(again).length === links.length && links.every((a, i) => anchors(again)[i] === a)), "and its links are the same nodes (a focused link keeps the focus)");
    const clone = host;                                         // the tree as drawn, kept after unmount for the assertions
    const kept = [...host.childNodes];
    hoy.unmount();
    clone.replaceChildren(...kept);
    return clone;
  });
}

const textOf = (el) => el.textContent.replace(/\s+/g, " ").trim();
const anchors = (el) => descendants(el).filter((n) => n.localName === "a");
const relTokens = (a) => a.rel.split(/\s+/).filter(Boolean).sort();

/** The note HOY drew in `host`, checked against what it must say in `lang`; returns it. */
function checkNote(host, lang, at) {
  const says = SAYS[lang];
  const section = host.children.find((n) => n.localName === "section") || host.firstElementChild;
  assert.ok(section, `${at}: HOY drew its section`);
  const first = section.firstElementChild;
  assert.ok(first, `${at}: the section has a first element`);
  const text = textOf(first);
  assert.ok(text.includes(says.line), `${at}: the first thing of HOY's sheet says the line «${says.line}»; it says «${text}»`);
  assert.ok(!first.hidden, `${at}: and it is not hidden`);
  for (let n = first; n && n !== host; n = n.parentNode) assert.ok(!n.hidden, `${at}: no box around it is hidden`);
  const order = [says.line, says.note, ...says.links.map((l) => l.text)].map((w) => text.indexOf(w));
  assert.ok(order.every((i) => i >= 0), `${at}: it says the line, the note «${says.note}» and the words of both links: «${text}»`);
  assert.deepEqual(order, [...order].sort((a, b) => a - b), `${at}: in that order: the line, the note, then the two links`);
  let rest = text;
  for (const w of [says.line, says.note, ...says.links.map((l) => l.text)]) rest = rest.replace(w, " ");
  assert.match(rest, /^[\s·|]*$/, `${at}: and nothing else but spaces and a separator between the links: «${rest.trim()}»`);
  const links = anchors(first);
  assert.deepEqual(links.map((a) => ({ href: a.getAttribute("href"), text: textOf(a) })), says.links, `${at}: two links, the code and the showcase, with their words`);
  for (const a of links) {
    assert.equal(a.getAttribute("target"), "_blank", `${at}: ${a.getAttribute("href")} opens in a new tab, so the demo is not lost inside the showcase's iframe`);
    const rel = relTokens(a);
    assert.ok(rel.includes("noopener"), `${at}: ${a.getAttribute("href")} has rel noopener (rel="${a.rel}")`);
    assert.ok(rel.every((r) => r === "noopener" || r === "noreferrer"), `${at}: and nothing else in its rel but noreferrer (rel="${a.rel}")`);
  }
  for (const n of [first, ...descendants(first)]) {
    for (const p of ["position", "z-index", "top", "left", "right", "bottom", "inset", "transform", "translate", "float"]) {
      assert.equal(n.style.props.get(p), undefined, `${at}: no inline ${p} on the note or inside it`);
    }
  }
  return first;
}

/** Whether `host` holds any of the demo's words or links. */
function saysNothing(host, at) {
  const text = textOf(host);
  for (const w of WORDS) assert.ok(!text.includes(w), `${at}: HOY says «${w}»`);
  for (const a of anchors(host)) assert.ok(![REPO, SHOWCASE].includes(a.getAttribute("href")), `${at}: HOY links to ${a.getAttribute("href")}`);
}

const snapshots = new Map();
async function snapshotOf(search) {
  if (!snapshots.has(search)) snapshots.set(search, plain((await loadDemo({ search }).api("/api/snapshot")).data));
  return snapshots.get(search);
}

// ---- (1) the demo hands its line over as data and adds nothing to the document ------------------------------------------------------------

test("the demo hands the page its line about itself as data: __DEMO__.about, every text a pair [es, en], the same in every scenario and whatever the language it loaded in", () => {
  for (const search of SCENARIOS) {
    for (const lang of ["es", "en"]) {
      const about = loadDemo({ search, lang }).about;
      assert.ok(about && typeof about === "object", `${search || "default"} ${lang}: __DEMO__.about is there`);
      assert.deepEqual(plain(about), ABOUT, `${search || "default"} ${lang}: the line, the note and the two links, as pairs`);
    }
  }
  for (const lang of ["es", "en"]) {
    assert.ok(SAYS[lang].line.startsWith("jobbot") && SAYS[lang].line.length <= 120, `${lang}: one line of 120 characters or fewer that starts with jobbot`);
  }
});

test("the demo adds nothing to the document by itself: no element, no <style>, no listener for the page's load, no position in its source", () => {
  const made = [];
  const inserted = [];
  const loads = [];
  const box = (name) => ({ append: (...x) => inserted.push([name, "append", x.length]), prepend: (...x) => inserted.push([name, "prepend", x.length]), appendChild: () => inserted.push([name, "appendChild", 1]), insertBefore: () => inserted.push([name, "insertBefore", 1]) });
  const page = { lang: "es", ...box("html") };
  const context = vm.createContext({
    window: {}, URL, URLSearchParams, setTimeout, clearTimeout, console,
    location: { search: "", href: "http://127.0.0.1:47213/" },
    document: {
      currentScript: { nonce: "test" }, documentElement: page, head: box("head"), body: box("body"),
      addEventListener: (type, fn) => loads.push([type, fn]),
      createElement: (tag) => { made.push(tag); return { getContext: () => ({}), setAttribute() {}, style: {} }; },
      getElementById: () => null, querySelector: () => null,
    },
    MutationObserver: class { observe() {} },
    localStorage: { getItem: () => null, setItem() {} },
  });
  vm.runInContext(DEMO_SOURCE, context, { filename: "demo.js" });
  for (const [, fn] of loads) fn();
  assert.deepEqual(made, [], "the demo makes no element while it loads (a capture makes its canvas only when the page asks for one)");
  assert.deepEqual(inserted, [], "and puts nothing in the document");
  assert.deepEqual(loads.map(([type]) => type), [], "nor waits for the page to load to add something");
  const source = DEMO_SOURCE.replace(/\r\n/g, "\n");
  assert.doesNotMatch(source, /createElement\(\s*["'`](?!canvas["'`])/, "the only element the demo script makes is the canvas of a capture");
  assert.doesNotMatch(source, /document\.(body|head)\b|insertAdjacent|\.prepend\(/, "it never reaches into the body or the head of the page");
  assert.doesNotMatch(source, /position\s*:|z-index|demo-badge/, "and nothing of the old badge (position fixed over the stage) is left");
});

// ---- (2) the view, in the language of the moment it is drawn -----------------------------------------------------------------------------

test("HOY's view says the line in the language of the page when it is drawn, so the key l changes it; without about it says nothing", async () => {
  const about = loadDemo({ lang: "en" }).about;                 // the page loaded in English: what counts is the language when HOY is drawn
  for (const search of SCENARIOS) {
    const snap = await snapshotOf(search);
    const m = model(snap);
    const nowMs = Date.parse(snap.generated_at);
    for (const lang of ["es", "en", "es"]) {
      fresh(lang);
      assert.deepEqual(plain(hoyView(m, { nowMs, about }).about), SAYS[lang], `${search || "default"}: in ${lang}`);
      assert.deepEqual(plain(hoyView(m, { nowMs, about, flat: true }).about), SAYS[lang], `${search || "default"}: in ${lang}, on a phone`);
    }
    for (const none of [undefined, null]) {
      fresh("es");
      assert.equal(hoyView(m, { nowMs, about: none }).about, null, `${search || "default"}: about ${none}: nothing (main.js hands demo && demo.about, ${none} on the real panel)`);
    }
    assert.equal(hoyView(m, { nowMs }).about, null, `${search || "default"}: no about at all: nothing`);
  }
  // what reaches the view from outside goes through clean and safeHref, as everything else of HOY
  fresh("es");
  const snap = await snapshotOf("");
  const odd = { line: ["jobbot\u202e lee\u0007 al revés", "x"], note: ["Demo\u2066 ficticia", "x"], links: [{ href: "javascript:alert(1)", text: ["Malo", "Bad"] }, { href: REPO, text: ["\u2066Código\u0007", "Code"] }] };
  const v = plain(hoyView(model(snap), { nowMs: Date.parse(snap.generated_at), about: odd }).about);
  assert.deepEqual(v, { line: "jobbot lee al revés", note: "Demo ficticia", links: [{ href: REPO, text: "Código" }] }, "no bidi or control character is drawn, and a link that is not http(s) is dropped");
});

// ---- (3) HOY draws it, first in its sheet ------------------------------------------------------------------------------------------------

test("HOY draws it first in its sheet, in the flow: the line, the note and two links that open in a new tab, in both languages, on a desktop and a phone, in every scenario", async () => {
  const about = loadDemo().about;
  for (const search of SCENARIOS) {
    const snap = await snapshotOf(search);
    for (const lang of ["es", "en"]) {
      for (const flat of [false, true]) {
        const at = `${search || "default"} ${lang}${flat ? " phone" : ""}`;
        const host = await drawHoy(snap, { lang, about, flat });
        checkNote(host, lang, at);
        const other = SAYS[lang === "es" ? "en" : "es"];
        assert.ok(!textOf(host).includes(other.line) && !textOf(host).includes(other.note), `${at}: and not the other language`);
      }
    }
  }
});

test("the key l: HOY drawn again after the language changed says the line in the new language, with the same object from the demo", async () => {
  const about = loadDemo({ lang: "es" }).about;
  const snap = await snapshotOf("");
  let previous = null;
  for (const lang of ["es", "en", "es", "en"]) {
    const host = await drawHoy(snap, { lang, about });
    const note = checkNote(host, lang, `after switching to ${lang}`);
    if (previous) assert.notEqual(textOf(note), previous, `switching to ${lang} changed what it says`);
    previous = textOf(note);
  }
});

test("the real panel shows none of it: HOY with no about in its context draws no line, no note and no link; no dictionary has its words; the built page has neither address", async () => {
  for (const search of SCENARIOS) {
    const snap = await snapshotOf(search);
    for (const lang of ["es", "en"]) for (const about of [undefined, null]) saysNothing(await drawHoy(snap, { lang, about }), `${search || "default"} ${lang} about ${about}`);
  }
  // the words are the demo's, never the panel's: no dictionary file has them
  const dicts = ["es", "en"].flatMap((l) => fs.readdirSync(path.join(ROOT, "src", "i18n", l)).map((f) => `src/i18n/${l}/${f}`));
  for (const file of dicts) {
    const text = read(file);
    for (const w of [...WORDS, REPO, SHOWCASE]) assert.ok(!text.includes(w), `${file} has «${w}»`);
  }
  // the page Rust serves (node build.mjs) and the published demo (node build.mjs demo --publish), both built here
  const dir = path.join(ROOT, "dist");
  fs.mkdirSync(dir, { recursive: true });
  const realOut = path.join(dir, `about-real-${process.pid}.html`), demoOut = path.join(dir, `about-demo-${process.pid}.html`);
  try {
    execFileSync(process.execPath, ["build.mjs", "--out", path.relative(ROOT, realOut)], { cwd: ROOT, stdio: "pipe" });
    const real = fs.readFileSync(realOut, "utf8");
    const account = new URL(REPO).host + "/" + new URL(REPO).pathname.split("/")[1];   // the address of the account, whatever repository follows
    for (const w of [...WORDS, REPO, SHOWCASE, account, "github.io"]) assert.ok(!real.includes(w), `the real page has «${w}»`);
    execFileSync(process.execPath, ["build.mjs", "demo", "--publish", "--out", path.relative(ROOT, demoOut)], { cwd: ROOT, stdio: "pipe" });
    const demo = fs.readFileSync(demoOut, "utf8");
    for (const w of [...WORDS, REPO, SHOWCASE]) assert.ok(demo.includes(w), `the published demo has «${w}»`);
    const meta = /<meta http-equiv="Content-Security-Policy" content="([^"]*)">/.exec(demo);
    const styleSrc = (/style-src ([^;]*)/.exec(meta ? meta[1] : "") || [, ""])[1].trim().split(/\s+/).filter(Boolean);
    assert.equal((demo.match(/<style>/g) || []).length, 1, "the published demo has the page's one <style>");
    assert.equal(styleSrc.length, 1, `and its policy names that style and no style the demo would make while it runs: ${styleSrc.join(" ")}`);
  } finally { fs.rmSync(realOut, { force: true }); fs.rmSync(demoOut, { force: true }); }
});

// ---- (5) in the flow of the sheet ---------------------------------------------------------------------------------------------------------

/** Every rule of a stylesheet, with its selector as written and its declarations, the ones inside @media and @supports included. */
function rules(css) {
  const out = [];
  const text = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const walk = (body) => {
    let i = 0;
    while (i < body.length) {
      const open = body.indexOf("{", i);
      if (open < 0) break;
      const head = body.slice(i, open).trim();
      let depth = 1, j = open + 1;
      while (j < body.length && depth) { if (body[j] === "{") depth++; else if (body[j] === "}") depth--; j++; }
      const inner = body.slice(open + 1, j - 1);
      if (head.startsWith("@")) { if (/^@(media|supports|layer|container)/.test(head)) walk(inner); }
      else out.push({ selector: head, body: inner });
      i = j;
    }
  };
  walk(text);
  return out;
}

test("the note lives in the flow of the sheet: no rule of src/styles that reaches it (its classes, a note role, an anchor of HOY) positions it, lifts it or moves it", async () => {
  const host = await drawHoy(await snapshotOf(""), { lang: "es", about: loadDemo().about });
  const note = checkNote(host, "es", "default es");
  // its own classes: the ones no other element HOY drew carries (a shared one, like label, says nothing about the note)
  const others = descendants(host).filter((n) => n !== note);
  const classes = note.className.split(/\s+/).filter(Boolean).filter((c) => !others.some((n) => n.className.split(/\s+/).includes(c)));
  const files = [];
  const walk = (dir) => { for (const f of fs.readdirSync(dir)) { const p = path.join(dir, f); if (fs.statSync(p).isDirectory()) walk(p); else if (f.endsWith(".css")) files.push(p); } };
  walk(path.join(ROOT, "src", "styles"));
  // a rule reaches the note when one of its own classes, or its role, is anywhere in the selector (the note itself, or its links), or when it names
  // the first element of HOY by position (.hoy > :first-child, :first-of-type, .hoy > p)
  const reaches = (selector) => selector.split(",").some((one) => (one.match(/\.[\w-]+/g) || []).some((c) => classes.includes(c.slice(1))) || /\[role=["']?note/.test(one) || /\.hoy\s*>\s*(p\b|[^\s,>+~]*:first-(child|of-type))/.test(one));
  // out of the flow (a position, an offset, a transform, a float, a negative margin, also -.5em or a subtraction in calc) or hidden (display none, visibility
  // hidden, opacity 0) or moved by the flex order of .hoy
  const BAD = /(^|;)\s*(position\s*:\s*(?!static\b)|z-index\s*:|top\s*:|left\s*:|right\s*:|bottom\s*:|inset(-[a-z-]+)?\s*:|transform\s*:|translate\s*:|float\s*:|margin(-[a-z-]+)?\s*:[^;]*((?<![\w-])-\.?\d|calc\([^;]*\s-\s)|display\s*:\s*none|visibility\s*:\s*hidden|opacity\s*:\s*0*\.?0*\s*(;|!|$)|order\s*:)/i;
  let seen = 0;
  const own = [], ofLinks = [];
  for (const file of files) {
    for (const r of rules(fs.readFileSync(file, "utf8"))) {
      if (!reaches(r.selector)) continue;
      seen++;
      const body = r.body.replace(/\s+/g, " ");
      assert.doesNotMatch(body, BAD, `${path.relative(ROOT, file)}: ${r.selector} { ${r.body.trim()} } moves the note out of the flow or hides it`);
      for (const one of r.selector.split(",")) {
        const last = one.trim().split(/\s+|>/).filter(Boolean).pop() || "";
        if ((last.match(/\.[\w-]+/g) || []).some((c) => classes.includes(c.slice(1)))) own.push(body);
        else if (/^a\b/.test(last) && (one.match(/\.[\w-]+/g) || []).some((c) => classes.includes(c.slice(1)))) ofLinks.push(body);
      }
    }
  }
  assert.ok(classes.length > 0, `the note has a class of its own, that nothing else of HOY carries: class="${note.className}"`);
  assert.ok(seen > 0, `some rule of src/styles gives the note (.${classes.join(", .")}) its look`);
  // its look (§4 of the encargo): the grey of the labels and a reading width on the note, its links gold and underlined
  assert.ok(own.some((b) => /max-width\s*:/.test(b)) && own.some((b) => /(^|;)\s*color\s*:/.test(b)), `a rule of the note gives it a color and a max-width: ${own.join(" | ")}`);
  assert.ok(ofLinks.some((b) => /(^|;)\s*color\s*:\s*var\(--(ink-accent|gold)/.test(b)) && ofLinks.some((b) => /text-decoration(-line)?\s*:[^;]*underline/.test(b)), `a rule of the note's links makes them gold and underlined: ${ofLinks.join(" | ")}`);
  const all = files.map((f) => fs.readFileSync(f, "utf8")).join("\n");
  assert.doesNotMatch(all, /demo-badge/, "no stylesheet keeps the old badge");
});

// ---- (6) the wiring --------------------------------------------------------------------------------------------------------------------------

test("the wiring: main.js hands the demo's about to the shell, and the shell puts it in the context of the moments (null or absent on the real panel)", () => {
  const main = read("src/main.js");
  const held = /const\s+(\w+)\s*=\s*window\.__DEMO__\s*(\|\||\?\?)\s*null\s*;/.exec(main);
  assert.ok(held, "main.js keeps the demo in one constant: const demo = window.__DEMO__ || null;");
  const name = held[1];
  assert.equal((main.match(/__DEMO__/g) || []).length, 1, "and reads __DEMO__ once");
  const call = /createShell\(\{([\s\S]*?)\}\);/.exec(main);
  assert.ok(call, "main.js calls createShell({ ... })");
  assert.match(call[1], new RegExp(`\\babout:\\s*(${name}\\s*&&\\s*${name}\\.about|${name}\\?\\.about)\\s*(,|$)`), `main.js hands createShell the about of ${name}: ${call[1].trim()}`);
  const shell = read("src/ui/shell.js");
  const sig = /export function createShell\(\{([\s\S]*?)\}\)\s*\{/.exec(shell);
  assert.ok(sig && /(^|[\s,])about\s*(,|$|=)/.test(sig[1]), `createShell takes about: { ${sig ? sig[1].trim() : ""} }`);
  const ctx = /const ctx = \{([\s\S]*?)\n  \};/.exec(shell);
  assert.ok(ctx, "the shell builds the context of the moments: const ctx = { ... };");
  assert.match(ctx[1], /(^|[\s,{])about\s*:\s*about\s*(\|\||\?\?)\s*null\s*,|(^|[\s,{])about\s*:\s*about\s*,|(^|[\s,{])about\s*,/m, `the context carries about: ${ctx[1].includes("about") ? "" : "it has no about"}`);
});
