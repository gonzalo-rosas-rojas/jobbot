// core/dom.js in a real Chrome, through lab/core.html: data stays text, the refused things are refused, and the page's
// strict CSP is never violated. Builds the lab, serves it on port 8793 (this package's own) and always closes both.
import test, { before, after } from "node:test";
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { launch } from "../tools/cdp.mjs";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const PORT = 8793;
const URL = `http://127.0.0.1:${PORT}/dist/lab-core.html`;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

let server;
let browser;
let page;

before(async () => {
  const build = spawnSync(process.execPath, ["build.mjs", "lab", "core"], { cwd: root, encoding: "utf8" });
  assert.equal(build.status, 0, build.stdout + build.stderr);
  server = spawn(process.execPath, ["serve.mjs", String(PORT)], { cwd: root, stdio: "ignore" });
  for (let i = 0; i < 50; i++) {
    try { if ((await fetch(URL)).ok) break; } catch { await sleep(100); }
  }
  browser = await launch({ width: 1000, height: 700 });
  page = await browser.newPage();
  await page.goto(URL);
  await page.waitFor("!!window.__dom");
});

after(async () => {
  if (browser) await browser.close();
  if (server) server.kill();
});

const run = (expression) => page.eval(`(() => { const { h, sv, fill, link, twoStep, focusMark, restoreFocus, isTypingIn, readLaunch } = window.__dom; ${expression} })()`);

test("h builds the tree: attributes, dataset, text, nulls dropped, arrays flattened, nodes appended", async () => {
  const got = await run(`
    const kid = h("b", {}, "x");
    const el = h("p", { class: "a", dataset: { x: "1", skip: null }, id: "p1", hidden: true, "data-n": 0, title: false }, "hi", null, undefined, false, ["b", ["c", 7]], kid);
    return { html: el.outerHTML, kinds: Array.from(el.childNodes).map((n) => n.nodeType), same: el.lastChild === kid };
  `);
  assert.equal(got.html, '<p class="a" data-x="1" id="p1" hidden="" data-n="0">hibc7<b>x</b></p>');
  assert.deepEqual(got.kinds, [3, 3, 3, 3, 1], "text nodes, and the one element");
  assert.equal(got.same, true);
  const klass = await run(`return h("i", { class: ["one", false, "", "two"] }).className;`);
  assert.equal(klass, "one two");
  const aria = await run(`const el = h("button", { "aria-pressed": true, "aria-expanded": false, "aria-label": "x" }); return [el.getAttribute("aria-pressed"), el.getAttribute("aria-expanded"), el.getAttribute("aria-label")];`);
  assert.deepEqual(aria, ["true", "false", "x"]);
});

test("hostile data stays text: no element, no handler, no markup", async () => {
  const got = await run(`
    const evil = "<img src=x onerror=\\"window.pwned=1\\">Evil";
    const el = h("div", { title: evil }, evil, [evil]);
    document.body.append(el);
    return { text: el.textContent, imgs: document.querySelectorAll("img").length, kids: el.children.length, nodes: el.childNodes.length, pwned: typeof window.pwned, title: el.title === evil };
  `);
  assert.equal(got.imgs, 0);
  assert.equal(got.kids, 0);
  assert.equal(got.nodes, 2);
  assert.ok(got.text.includes("<img src=x onerror="));
  assert.equal(got.pwned, "undefined");
  assert.equal(got.title, true, "an attribute value is a value, not markup");
});

test("a style string throws; a style object goes through style.setProperty, custom properties included", async () => {
  const thrown = await run(`try { h("p", { style: "color: red" }); return null; } catch (e) { return e.message; }`);
  assert.match(thrown, /style/);
  const got = await run(`
    const el = h("p", { style: { "--i": 3, marginTop: "4px", "--gone": null, color: false } });
    return { i: el.style.getPropertyValue("--i"), mt: el.style.marginTop, gone: el.style.getPropertyValue("--gone"), color: el.style.color };
  `);
  assert.deepEqual(got, { i: "3", mt: "4px", gone: "", color: "" });
  const nothing = await run(`return h("p", { style: null }).hasAttribute("style");`);
  assert.equal(nothing, false);
});

test("event handlers are functions only; a string is refused", async () => {
  const thrown = await run(`try { h("button", { onclick: "alert(1)" }); return null; } catch (e) { return e.message; }`);
  assert.match(thrown, /onclick/);
  const clicks = await run(`
    let n = 0;
    const b = h("button", { onclick: () => { n += 1; }, oninput: () => {} }, "go");
    document.body.append(b);
    b.click(); b.click();
    return { n, attr: b.hasAttribute("onclick") };
  `);
  assert.deepEqual(clicks, { n: 2, attr: false });
});

test("href and src: only safe addresses are kept", async () => {
  const got = await run(`
    const keep = (name, url) => h("a", { [name]: url }).hasAttribute(name);
    return {
      js: keep("href", "javascript:alert(1)"),
      jsCase: keep("href", " JaVaScRiPt:alert(1)"),
      tab: keep("href", "java\\tscript:alert(1)"),
      data: keep("href", "data:text/html,<script>1</script>"),
      relative: keep("href", "/somewhere"),
      https: keep("href", "https://example.com/a"),
      http: keep("href", "http://example.com/a"),
      mail: keep("href", "mailto:a@example.com"),
      hash: keep("href", "#hoy/abc"),
      imgBlob: keep("src", "blob:http://127.0.0.1/abc"),
      imgData: keep("src", "data:image/png;base64,AAAA"),
      imgSvg: keep("src", "data:image/svg+xml,<svg onload=1>"),
      imgHttp: keep("src", "http://example.com/x.png"),
      blank: h("a", { href: "https://example.com", target: "_blank" }).getAttribute("rel"),
    };
  `);
  assert.deepEqual(got, {
    js: false, jsCase: false, tab: false, data: false, relative: false, https: true, http: true, mail: true, hash: true,
    imgBlob: true, imgData: true, imgSvg: false, imgHttp: false, blank: "noopener noreferrer",
  });
});

test("tags that load or run something are refused", async () => {
  for (const tag of ["script", "style", "iframe", "object", "embed", "link", "meta", "base", "SCRIPT"]) {
    const thrown = await run(`try { h(${JSON.stringify(tag)}); return null; } catch (e) { return e.message; }`);
    assert.match(thrown, /not allowed/, tag);
  }
  const thrownSv = await run(`try { sv("script"); return null; } catch (e) { return e.message; }`);
  assert.match(thrownSv, /not allowed/);
});

test("sv creates SVG elements in the SVG namespace, with their attributes", async () => {
  const got = await run(`
    const svg = sv("svg", { viewBox: "0 0 10 10", class: "b-dial", "aria-hidden": "true" }, sv("circle", { cx: 5, cy: 5, r: 2, class: ["dot", "on"] }), "label", null, [sv("title", {}, "t")]);
    document.body.append(svg);
    const circle = svg.querySelector("circle");
    return {
      ns: svg.namespaceURI, circleNs: circle.namespaceURI, klass: svg.getAttribute("class"), vb: svg.getAttribute("viewBox"),
      r: circle.getAttribute("r"), circleClass: circle.getAttribute("class"), kinds: Array.from(svg.childNodes).map((n) => n.nodeType),
      isSvg: svg instanceof SVGSVGElement, w: svg.getBoundingClientRect().width > 0,
    };
  `);
  assert.equal(got.ns, "http://www.w3.org/2000/svg");
  assert.equal(got.circleNs, "http://www.w3.org/2000/svg");
  assert.deepEqual([got.klass, got.vb, got.r, got.circleClass], ["b-dial", "0 0 10 10", "2", "dot on"]);
  assert.deepEqual(got.kinds, [1, 3, 1]);
  assert.equal(got.isSvg, true);
});

test("fill replaces the children and never prints a missing part as text", async () => {
  const got = await run(`
    const box = h("div", {}, "old");
    fill(box, h("i", {}, "a"), null, false, undefined, [h("b", {}, "b")], "t");
    const first = box.textContent;
    fill(box);
    return { first, kids: box.children.length, emptied: box.childNodes.length };
  `);
  assert.deepEqual(got, { first: "abt", kids: 0, emptied: 0 });
});

test("link is an anchor only for http(s); anything else is plain text", async () => {
  const got = await run(`
    const ok = link("https://example.com/x", "Open");
    const bad = link("javascript:alert(1)", "Open");
    const none = link(null, "Open");
    const bare = link(undefined);
    return [ok.tagName, ok.getAttribute("href"), ok.getAttribute("target"), ok.getAttribute("rel"), ok.textContent, bad.tagName, bad.className, bad.textContent, bad.hasAttribute("href"), none.tagName, bare.textContent];
  `);
  assert.deepEqual(got, ["A", "https://example.com/x", "_blank", "noopener noreferrer", "Open", "SPAN", "muted", "Open", false, "SPAN", ""]);
});

test("twoStep: the first click arms the button, a click in the same instant does not confirm, a second deliberate one does", async () => {
  await run(`
    window.__ran = 0;
    const b = h("button", { id: "two", onclick: twoStep("Hecha", () => { window.__ran += 1; }) }, "Hecha");
    document.body.append(b);
  `);
  const label = await run(`const b = document.getElementById("two"); b.click(); b.click(); return [b.textContent, window.__ran];`);
  assert.deepEqual(label, ["¿Seguro? Pulsa otra vez", 0], "a double click only arms it");
  await sleep(420);
  const done = await run(`const b = document.getElementById("two"); b.click(); return [b.textContent, window.__ran];`);
  assert.deepEqual(done, ["Hecha", 1], "the second, deliberate click runs it and restores the label");
  const again = await run(`const b = document.getElementById("two"); b.click(); return [b.textContent, window.__ran];`);
  assert.deepEqual(again, ["¿Seguro? Pulsa otra vez", 1], "and it needs arming again");
});

test("focusMark and restoreFocus find the same button in a redrawn page, with the caret of a field", async () => {
  const got = await run(`
    const draw = () => h("div", { id: "host" },
      h("ul", {}, h("li", { "data-card": "a:1" }, h("button", { "data-open": "a:1" }, "open"), h("button", { "data-action": "approve" }, "approve")),
        h("li", { "data-card": "b:2" }, h("button", { "data-action": "approve" }, "approve"))),
      h("details", { "data-fold": "night" }, h("summary", {}, "fold")),
      h("input", { id: "note", type: "text", value: "revisar lunes" }),
      h("input", { id: "slider", type: "range", min: "0", max: "10" }),
      h("button", { id: "plain" }, "x"), h("button", {}, "no name"));
    let host = draw();
    document.body.append(host);
    const out = {};
    const redo = (el) => { const mark = focusMark(el, host); const next = draw(); host.replaceWith(next); host = next; restoreFocus(mark, host); return mark; };

    host.querySelector('[data-card="b:2"] [data-action="approve"]').focus();
    out.second = redo(document.activeElement).selector;
    out.secondFocus = document.activeElement.closest("li").dataset.card;

    host.querySelector('[data-card="a:1"] [data-open]').focus();
    redo(document.activeElement);
    out.open = document.activeElement.dataset.open;

    host.querySelector("summary").focus();
    out.fold = redo(document.activeElement).selector;
    out.foldTag = document.activeElement.tagName;

    const note = host.querySelector("#note"); note.focus(); note.setSelectionRange(3, 7);
    redo(note);
    out.caret = [document.activeElement.id, document.activeElement.selectionStart, document.activeElement.selectionEnd];

    const slider = host.querySelector("#slider"); slider.focus();
    out.slider = (() => { try { redo(slider); return document.activeElement.id; } catch (e) { return "threw " + e.message; } })();

    out.none = focusMark(host.lastElementChild, host);
    out.outside = focusMark(document.body, host);
    out.nothing = focusMark(null, host);
    out.noMark = restoreFocus(null, host) === undefined;
    return out;
  `);
  assert.equal(got.second, '[data-card="b\\:2"] [data-action="approve"]');
  assert.equal(got.secondFocus, "b:2", "the button of the second card, not the first");
  assert.equal(got.open, "a:1");
  assert.equal(got.fold, 'details[data-fold="night"] > summary');
  assert.equal(got.foldTag, "SUMMARY");
  assert.deepEqual(got.caret, ["note", 3, 7]);
  assert.equal(got.slider, "slider", "a range input has no caret and must not throw");
  assert.equal(got.none, null);
  assert.equal(got.outside, null);
  assert.equal(got.nothing, null);
  assert.equal(got.noMark, true);
});

test("isTypingIn is true only while a field inside the box has the focus", async () => {
  const got = await run(`
    const box = h("div", {}, h("input", { id: "f", type: "text" }), h("button", { id: "b" }, "b"), h("textarea", { id: "ta" }), h("select", { id: "s" }, h("option", {}, "o")));
    document.body.append(box);
    const outside = h("input", { id: "elsewhere" });
    document.body.append(outside);
    const state = (id) => { document.getElementById(id).focus(); return isTypingIn(box); };
    return [state("f"), state("b"), state("ta"), state("s"), state("elsewhere"), isTypingIn(null)];
  `);
  assert.deepEqual(got, [true, false, true, true, false, false]);
});

test("readLaunch reads the token once and takes it out of the address", async () => {
  const got = await run(`
    const calls = [];
    const env = { location: { search: "?t=abc123&x=1", pathname: "/panel", hash: "#hoy" }, history: { replaceState: (a, b, url) => calls.push(url) } };
    env.top = env;
    const alone = readLaunch(env);
    const framed = readLaunch({ location: { search: "?t=zzz", pathname: "/", hash: "" }, history: { replaceState: (a, b, url) => calls.push(url) }, top: {} });
    const embed = readLaunch({ location: { search: "?embed", pathname: "/", hash: "#cola" }, history: { replaceState: (a, b, url) => calls.push(url) }, top: null, });
    return { alone, framed, embed, calls };
  `);
  assert.deepEqual(got.alone, { token: "abc123", embedded: false });
  assert.deepEqual(got.framed, { token: "zzz", embedded: true });
  assert.equal(got.embed.embedded, true);
  assert.deepEqual(got.calls, ["/panel#hoy", "/", "/#cola"]);
});

test("the page ended with no console error, exception or CSP violation", async () => {
  assert.deepEqual(page.problems(), []);
});
