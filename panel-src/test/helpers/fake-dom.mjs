// A small DOM for the tests that run without a browser (Node): enough of Node, Element, document and CSS.escape for core/dom.js `h()`, the
// drawer, the help sheet and PROCESOS' own render to be built and driven. It is NOT a browser: no layout (every connected element has a client
// rect, so the cursor can walk it), no style cascade, no animation. Selectors: tag, #id, .class, [attr], [attr="v"], `:scope >`, descendant (space),
// child (>) and commas; anything else throws, so a test never passes on a selector the fake silently ignored.
//
//   const { document, flush } = installFakeDom();   // sets globalThis.document, Node, Element, CSS (call once per test file)

class FakeNode {
  constructor() { this.parentNode = null; }
  get parentElement() { return this.parentNode && this.parentNode.nodeType === 1 ? this.parentNode : null; }
  get nextSibling() { const p = this.parentNode; return p ? p.childNodes[p.childNodes.indexOf(this) + 1] || null : null; }
  get previousSibling() { const p = this.parentNode; return p ? p.childNodes[p.childNodes.indexOf(this) - 1] || null : null; }
  get isConnected() { let n = this; while (n.parentNode) n = n.parentNode; return n === fakeDocument.documentElement; }
  remove() { if (this.parentNode) this.parentNode.removeChild(this); }
}

class FakeText extends FakeNode {
  constructor(text) { super(); this.nodeType = 3; this.data = String(text); }
  get textContent() { return this.data; }
  set textContent(v) { this.data = String(v); }
}

const kebab = (s) => s.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase());
const camel = (s) => s.replace(/-([a-z])/g, (_, c) => c.toUpperCase());
const BOOLEAN = new Set(["hidden", "inert", "disabled", "open"]);

function parseSelector(text) {
  // { scoped, parts: [{ comb: " " | ">", tag, id, classes, attrs }] }; ":scope >" or ":scope " at the start makes the chain start at the element asked
  const out = { scoped: false, parts: [] };
  let i = 0;
  let comb = " ";
  let cur = null;
  const push = () => { if (cur) { out.parts.push(cur); cur = null; } };
  const fail = () => { throw new Error(`the fake DOM does not understand this selector: ${text}`); };
  while (i < text.length) {
    const ch = text[i];
    if (ch === " " || ch === ">") {
      push();
      let j = i;
      let gt = false;
      while (j < text.length && (text[j] === " " || text[j] === ">")) { if (text[j] === ">") gt = true; j += 1; }
      comb = gt ? ">" : " ";
      i = j;
      continue;
    }
    if (text.startsWith(":scope", i) && !cur && out.parts.length === 0 && !out.scoped) { out.scoped = true; i += 6; continue; }
    cur ??= { comb, tag: null, id: null, classes: [], attrs: [] };
    if (ch === "#" || ch === ".") {
      const m = /^[\w-]+/.exec(text.slice(i + 1));
      if (!m) fail();
      if (ch === "#") cur.id = m[0]; else cur.classes.push(m[0]);
      i += 1 + m[0].length;
    } else if (ch === "[") {
      const m = /^\[([\w-]+)(?:="((?:[^"\\]|\\.)*)")?\]/.exec(text.slice(i));
      if (!m) fail();
      cur.attrs.push([m[1], m[2] === undefined ? null : m[2].replace(/\\(.)/g, "$1")]);
      i += m[0].length;
    } else {
      const m = /^[a-zA-Z][\w-]*/.exec(text.slice(i));
      if (!m) fail();
      cur.tag = m[0].toUpperCase();
      i += m[0].length;
    }
  }
  push();
  if (out.scoped && out.parts.length) out.parts[0].comb = out.parts[0].comb === ">" ? ">" : " ";
  if (out.parts.length === 0) fail();
  return out;
}

function compoundMatches(el, c) {
  if (c.tag && el.tagName !== c.tag) return false;
  if (c.id && el.getAttribute("id") !== c.id) return false;
  for (const k of c.classes) if (!el.classList.contains(k)) return false;
  for (const [name, value] of c.attrs) {
    if (!el.hasAttribute(name)) return false;
    if (value !== null && el.getAttribute(name) !== value) return false;
  }
  return true;
}

function matchesChain(el, sel, scope, index = sel.parts.length - 1) {
  const c = sel.parts[index];
  if (!compoundMatches(el, c)) return false;
  if (index === 0) {
    if (!sel.scoped) return true;
    return c.comb === ">" ? el.parentNode === scope : !!scope && scope.contains(el) && el !== scope;
  }
  if (c.comb === ">") return el.parentElement ? matchesChain(el.parentElement, sel, scope, index - 1) : false;
  for (let up = el.parentElement; up; up = up.parentElement) if (matchesChain(up, sel, scope, index - 1)) return true;
  return false;
}

function selectorList(text) {
  return String(text).split(",").map((s) => s.trim()).filter(Boolean).map(parseSelector);
}

class FakeElement extends FakeNode {
  constructor(tag) {
    super();
    this.nodeType = 1;
    this.tagName = String(tag).toUpperCase();
    this.attrs = new Map();
    this.childNodes = [];
    this.listeners = new Map();
    this.scrollTop = 0;
    this.value = "";
    const owner = this;
    this.dataset = new Proxy({}, {
      get: (_, key) => (typeof key === "string" && owner.attrs.has("data-" + kebab(key)) ? owner.attrs.get("data-" + kebab(key)) : undefined),
      set: (_, key, value) => { owner.attrs.set("data-" + kebab(key), String(value)); return true; },
      deleteProperty: (_, key) => { owner.attrs.delete("data-" + kebab(key)); return true; },
      has: (_, key) => owner.attrs.has("data-" + kebab(key)),
      ownKeys: () => [...owner.attrs.keys()].filter((k) => k.startsWith("data-")).map((k) => camel(k.slice(5))),
      getOwnPropertyDescriptor: (_, key) => (owner.attrs.has("data-" + kebab(key)) ? { enumerable: true, configurable: true, value: owner.attrs.get("data-" + kebab(key)) } : undefined),
    });
    const props = new Map();
    this.style = { setProperty: (k, v) => props.set(k, String(v)), getPropertyValue: (k) => props.get(k) || "", removeProperty: (k) => props.delete(k) };
    this.classList = {
      contains: (c) => this.className.split(/\s+/).includes(c),
      add: (...cs) => { const set = new Set(this.className.split(/\s+/).filter(Boolean)); cs.forEach((c) => set.add(c)); this.className = [...set].join(" "); },
      remove: (...cs) => { this.className = this.className.split(/\s+/).filter((c) => c && !cs.includes(c)).join(" "); },
      toggle: (c, force) => { const on = force === undefined ? !this.classList.contains(c) : !!force; this.classList[on ? "add" : "remove"](c); return on; },
    };
  }
  get className() { return this.attrs.get("class") || ""; }
  set className(v) { this.attrs.set("class", String(v)); }
  get id() { return this.attrs.get("id") || ""; }
  set id(v) { this.attrs.set("id", String(v)); }
  get hidden() { return this.attrs.has("hidden"); }
  set hidden(v) { if (v) this.attrs.set("hidden", ""); else this.attrs.delete("hidden"); }
  get inert() { return this.attrs.has("inert"); }
  set inert(v) { if (v) this.attrs.set("inert", ""); else this.attrs.delete("inert"); }
  get title() { return this.attrs.get("title") || ""; }
  set title(v) { this.attrs.set("title", String(v)); }
  setAttribute(name, value) { this.attrs.set(name, BOOLEAN.has(name) && value === "" ? "" : String(value)); }
  getAttribute(name) { return this.attrs.has(name) ? this.attrs.get(name) : null; }
  hasAttribute(name) { return this.attrs.has(name); }
  removeAttribute(name) { this.attrs.delete(name); }
  toggleAttribute(name, force) { const on = force === undefined ? !this.attrs.has(name) : !!force; if (on) this.attrs.set(name, ""); else this.attrs.delete(name); return on; }

  get children() { return this.childNodes.filter((k) => k.nodeType === 1); }
  get firstChild() { return this.childNodes[0] || null; }
  get lastChild() { return this.childNodes[this.childNodes.length - 1] || null; }
  get firstElementChild() { return this.children[0] || null; }
  get textContent() { return this.childNodes.map((k) => k.textContent).join(""); }
  set textContent(v) { this.replaceChildren(String(v)); }

  #adopt(kid) {
    const node = kid instanceof FakeNode ? kid : new FakeText(kid);
    if (node.parentNode) node.parentNode.removeChild(node);
    node.parentNode = this;
    return node;
  }
  append(...kids) { for (const k of kids) this.childNodes.push(this.#adopt(k)); }
  appendChild(k) { this.append(k); return k; }
  prepend(...kids) { this.childNodes.unshift(...kids.map((k) => this.#adopt(k))); }
  insertBefore(node, ref) {
    const adopted = this.#adopt(node);
    const at = ref ? this.childNodes.indexOf(ref) : -1;
    if (at < 0) this.childNodes.push(adopted); else this.childNodes.splice(at, 0, adopted);
    return node;
  }
  removeChild(node) { const at = this.childNodes.indexOf(node); if (at >= 0) { this.childNodes.splice(at, 1); node.parentNode = null; } return node; }
  replaceChildren(...kids) {
    for (const k of this.childNodes) k.parentNode = null;
    this.childNodes = [];
    this.append(...kids);
  }
  replaceWith(...kids) {
    const p = this.parentNode;
    if (!p) return;
    const at = p.childNodes.indexOf(this);
    this.parentNode = null;
    p.childNodes.splice(at, 1);
    const adopted = kids.map((k) => p.#adopt(k));
    p.childNodes.splice(at, 0, ...adopted);
  }
  contains(node) { for (let n = node; n; n = n.parentNode) if (n === this) return true; return false; }
  closest(selector) {
    const list = selectorList(selector);
    for (let el = this; el; el = el.parentElement) if (list.some((parts) => matchesChain(el, parts, null))) return el;
    return null;
  }
  matches(selector) { return selectorList(selector).some((parts) => matchesChain(this, parts, null)); }
  querySelectorAll(selector) {
    const list = selectorList(selector);
    const out = [];
    const walk = (node) => {
      for (const kid of node.children) {
        if (list.some((parts) => matchesChain(kid, parts, this))) out.push(kid);
        walk(kid);
      }
    };
    walk(this);
    return out;
  }
  querySelector(selector) { return this.querySelectorAll(selector)[0] || null; }

  addEventListener(type, fn, opts) { const l = this.listeners.get(type) || []; l.push({ fn, opts }); this.listeners.set(type, l); }
  removeEventListener(type, fn) { this.listeners.set(type, (this.listeners.get(type) || []).filter((l) => l.fn !== fn)); }
  /** Bubbles up the parents; `event.defaultPrevented` tells whether a listener called preventDefault(). */
  dispatchEvent(event) {
    event.target ??= this;
    event.defaultPrevented = false;
    event.preventDefault ??= () => { event.defaultPrevented = true; };
    let stopped = false;
    event.stopPropagation ??= () => { stopped = true; };
    for (let n = this; n && !stopped; n = n.parentNode) {
      for (const l of [...((n.listeners && n.listeners.get(event.type)) || [])]) l.fn.call(n, event);
      if (event.bubbles === false) break;
    }
    return !event.defaultPrevented;
  }
  click() { return this.dispatchEvent({ type: "click", bubbles: true }); }
  focus() { if (this.isConnected) fakeDocument.activeElement = this; }
  blur() { if (fakeDocument.activeElement === this) fakeDocument.activeElement = fakeDocument.body; }
  getClientRects() { return this.isConnected && !this.closest("[hidden]") ? [{ top: 0, bottom: 0, left: 0, right: 0, width: 0, height: 0 }] : []; }
  getBoundingClientRect() { return { top: 0, bottom: 0, left: 0, right: 0, width: 0, height: 0 }; }
  scrollBy() {}
}

const documentElement = new FakeElement("html");
const body = new FakeElement("body");
documentElement.append(body);
const fakeDocument = {
  documentElement,
  body,
  activeElement: body,
  createElement: (tag) => new FakeElement(tag),
  createElementNS: (ns, tag) => new FakeElement(tag),
  createTextNode: (text) => new FakeText(text),
  addEventListener() {},
  removeEventListener() {},
  querySelector: (s) => body.querySelector(s),
  querySelectorAll: (s) => body.querySelectorAll(s),
  getElementById: (id) => body.querySelector("#" + id),
};

/** Puts the fake on `globalThis`. The returned `document` is the same object every time; `reset()` empties the body and gives the focus back to it. */
export function installFakeDom() {
  globalThis.Node = FakeNode;
  globalThis.Element = FakeElement;
  globalThis.document = fakeDocument;
  globalThis.CSS = { escape: (s) => String(s).replace(/["\\]/g, "\\$&") };
  return {
    document: fakeDocument,
    reset() { body.replaceChildren(); fakeDocument.activeElement = body; },
  };
}

export { FakeElement, FakeText };
