import test from "node:test";
import assert from "node:assert/strict";
import { openBuilt } from "./helpers/browser.mjs";
import { contrast } from "./helpers/color.mjs";

// The drawer on the real styles, over the demo's made-up server, without the shell (lab/f0-drawer.entry.js, a demo build of its own).
const ARGS = ["demo", "--entry", "lab/f0-drawer.entry.js", "--out", "dist/demo-drawer-ui.html"];
const open = (opts = {}) => openBuilt({ args: ARGS, file: "dist/demo-drawer-ui.html", query: "?t=tok123", ...opts });
const ev = (t, expr) => t.page.eval(expr);
const ready = (t) => t.page.waitFor(`document.querySelectorAll("[data-card]").length > 2 && document.fonts.status === "loaded"`);
// The demo's own "demo data" badge writes a <style> without the page's nonce, which the real CSP refuses: that pair of messages is the demo's.
const problems = (t) => t.page.problems().filter((p) => !/Applying inline style violates/.test(p.text) && !/^CSP violation: style-src-elem inline\s*$/.test(p.text));
const D = `document.getElementById("detail")`;
// tinykeys reads event.code for a letter or a digit, so a real key needs its code: "j" is KeyJ and "3" is Digit3
const key = (t, k) => t.page.key(k, k.length === 1 ? { code: /[0-9]/.test(k) ? "Digit" + k : "Key" + k.toUpperCase(), text: k } : {});
const shown = (t) => t.page.waitFor(`${D}.classList.contains("open") && !!${D}.querySelector(".d-body") && getComputedStyle(${D}).opacity === "1"`);
const rgbOf = (css) => css.match(/\d+(\.\d+)?/g).slice(0, 3).map(Number);

test("it sits right of the sheet, 22 px after its edge, as wide as --drawer, and takes nothing from the sheet; on a phone it covers the page", async () => {
  for (const [width, height, wide] of [[1920, 1080, 400], [3440, 1440, 447]]) {
    const t = await open({ width, height });
    try {
      await ready(t);
      const rect = (sel) => ev(t, `(() => { const r = document.querySelector(${JSON.stringify(sel)}).getBoundingClientRect(); return { l: r.left, t: r.top, r: r.right, b: r.bottom, w: r.width }; })()`);
      const before = await rect("#app");
      await ev(t, `document.querySelector("[data-card]").click()`);
      await shown(t);
      const sheet = await rect("#app");
      const drawer = await rect("#detail");
      assert.deepEqual([sheet.l, sheet.w], [before.l, before.w], `${width}: the sheet keeps its place and width`);
      assert.ok(Math.abs(drawer.w - wide) <= 1, `${width}: ${drawer.w} wide, ${wide} expected`);
      assert.ok(Math.abs(drawer.l - (sheet.r + 22)) <= 1, `${width}: starts 22 px after the sheet (${drawer.l} vs ${sheet.r})`);
      assert.ok(Math.abs(drawer.t - 78) <= 1 && Math.abs(drawer.b - (height - 26)) <= 1, `${width}: from under the HUD to 26 px above the floor (${drawer.t}, ${drawer.b})`);
      assert.ok(drawer.r <= width, `${width}: inside the window`);
      assert.deepEqual(problems(t), []);
    } finally { await t.close(); }
  }
  const phone = await open({ width: 390, height: 844 });
  try {
    await ready(phone);
    await ev(phone, `document.querySelector("[data-card]").click()`);
    await shown(phone);
    const box = await ev(phone, `(() => { const r = ${D}.getBoundingClientRect(); return { l: r.left, t: r.top, w: r.width, h: r.height, pos: getComputedStyle(${D}).position }; })()`);
    assert.deepEqual(box, { l: 0, t: 0, w: 390, h: 844, pos: "fixed" });
    assert.deepEqual(problems(phone), []);
  } finally { await phone.close(); }
});

// A laptop window (1366 or 1440 wide) has no room right of a 960 px sheet: the drawer then covers the right edge of the sheet, but it never
// leaves the window and never makes the page wider.
test("on a narrow desktop window the drawer stays inside the window, over the right edge of the sheet, and the page does not grow sideways", async () => {
  for (const [width, height] of [[1366, 768], [1024, 768]]) {
    const t = await open({ width, height });
    try {
      await ready(t);
      await ev(t, `document.querySelector("[data-card]").click()`);
      await shown(t);
      const box = await ev(t, `(() => { const r = ${D}.getBoundingClientRect(); const s = document.getElementById("app").getBoundingClientRect(); return { l: r.left, r: r.right, w: r.width, sheetR: s.right, scroll: document.documentElement.scrollWidth }; })()`);
      assert.ok(box.r <= width && box.l >= 0, `${width}: inside the window (${box.l}, ${box.r})`);
      assert.ok(Math.abs(box.w - 400) <= 1, `${width}: still --drawer wide (${box.w})`);
      assert.ok(box.l <= box.sheetR, `${width}: it covers the sheet's edge, as there is no room beside it`);
      assert.ok(box.scroll <= width, `${width}: no sideways scroll`);
      assert.deepEqual(problems(t), []);
    } finally { await t.close(); }
  }
});

test("it is not modal: nothing behind goes inert, the list keeps answering; one history entry, Back and Esc close it, and the focus goes back to its row", async () => {
  const t = await open();
  try {
    await ready(t);
    const keys = await ev(t, `[...document.querySelectorAll("[data-card]")].slice(0, 3).map((e) => e.dataset.card)`);
    const entries = await ev(t, "history.length");
    await ev(t, `document.querySelector('[data-card="${keys[0]}"]').click()`);
    await shown(t);
    assert.equal(await ev(t, `document.getElementById("app").inert || !!document.getElementById("app").closest("[inert]")`), false, "the sheet is alive");
    assert.equal(await ev(t, "history.length"), entries + 1, "one history entry for the drawer");
    assert.equal(await ev(t, "decodeURIComponent(location.hash)"), "#hoy/" + keys[0]);
    assert.equal(await ev(t, `document.activeElement.id`), "detail", "the focus went in");
    assert.deepEqual(await ev(t, "__lab.log.opens"), ["open"]);
    // another row, while it is open: the drawer shows that one, on the same history entry, and does not announce an opening twice
    await ev(t, `document.querySelector('[data-card="${keys[1]}"]').click()`);
    await t.page.waitFor(`!!${D}.querySelector('[data-job="${keys[1]}"]')`);
    assert.equal(await ev(t, "history.length"), entries + 1);
    assert.equal(await ev(t, "decodeURIComponent(location.hash)"), "#hoy/" + keys[1]);
    assert.deepEqual(await ev(t, "__lab.log.opens"), ["open"]);
    // Esc: it closes, the entry is taken back, the focus is on the row it grew out of
    await key(t, "Escape");
    await t.page.waitFor(`!${D}.classList.contains("open")`);
    assert.deepEqual(await ev(t, "__lab.log.opens"), ["open", "close"]);
    assert.equal(await ev(t, `document.activeElement.dataset.card`), keys[0]);
    await t.page.waitFor(`location.hash === ""`);
    assert.equal(await ev(t, `${D}.inert`), true, "closed it is inert: nothing in it takes the focus");
    // Back closes it too
    await ev(t, `document.querySelector('[data-card="${keys[2]}"]').click()`);
    await shown(t);
    await ev(t, "history.back()");
    await t.page.waitFor(`!${D}.classList.contains("open")`);
    assert.deepEqual(await ev(t, "__lab.log.opens"), ["open", "close", "open", "close"]);
    assert.deepEqual(problems(t), []);
  } finally { await t.close(); }
});

test("j and k step through the list: one 'open' for the whole visit, the address follows, Esc goes back to the row on screen, and a cut under reduced motion", async () => {
  const t = await open();
  try {
    await ready(t);
    const keys = await ev(t, `[...document.querySelectorAll("[data-card]")].slice(0, 3).map((e) => e.dataset.card)`);
    await ev(t, `document.querySelector('[data-card="${keys[0]}"]').click()`);
    await shown(t);
    const entries = await ev(t, "history.length");
    await ev(t, "document.activeElement.blur()");
    await key(t, "j");
    await t.page.waitFor(`!!${D}.querySelector('[data-job="${keys[1]}"]')`);
    assert.ok((await ev(t, `${D}.querySelector(".detail-body").getAnimations().length`)) >= 1, "the content slides the way it went");
    assert.equal(await ev(t, "history.length"), entries, "stepping rewrites the entry, it does not add one");
    assert.equal(await ev(t, "decodeURIComponent(location.hash)"), "#hoy/" + keys[1]);
    assert.match(await ev(t, `${D}.querySelector(".seq .num").textContent`), /^2 de \d+$/);
    await ev(t, "__lab.reduced(true)");
    await key(t, "j");
    await t.page.waitFor(`!!${D}.querySelector('[data-job="${keys[2]}"]')`);
    assert.equal(await ev(t, `${D}.querySelector(".detail-body").getAnimations().length`), 0, "under reduced motion it is a cut");
    await key(t, "k");
    await t.page.waitFor(`!!${D}.querySelector('[data-job="${keys[1]}"]')`);
    await key(t, "Escape");
    await t.page.waitFor(`!${D}.classList.contains("open")`);
    assert.equal(await ev(t, `document.activeElement.dataset.card`), keys[1], "the focus is on the row of the application that was on screen");
    assert.deepEqual(await ev(t, "__lab.log.opens"), ["open", "close"]);
    assert.deepEqual(problems(t), []);
  } finally { await t.close(); }
});

test("the keys 1-5 do not travel while it is open (drawerOpen), and leaving the stop closes it without touching the history", async () => {
  const t = await open();
  try {
    await ready(t);
    await ev(t, `document.querySelector("[data-card]").click()`);
    await shown(t);
    const entries = await ev(t, "history.length");
    const hash = await ev(t, "location.hash");
    await ev(t, "document.activeElement.blur()");
    await key(t, "3");
    await new Promise((resolve) => setTimeout(resolve, 150));
    assert.deepEqual(await ev(t, "__lab.log.went"), [], "3 would go to COLA: not with the drawer open");
    assert.equal(await ev(t, `${D}.classList.contains("open")`), true);
    await ev(t, `__app.go("procesos")`);
    await t.page.waitFor(`!${D}.classList.contains("open")`);
    assert.equal(await ev(t, "history.length"), entries);
    assert.equal(await ev(t, "location.hash"), hash, "a silent close leaves the address to whoever moved");
    assert.deepEqual(await ev(t, "__lab.log.went"), ["procesos"]);
    assert.deepEqual(await ev(t, "__lab.log.opens"), ["open", "close"]);
    await key(t, "3"); // closed, the same key travels
    await t.page.waitFor(`__lab.log.went.length === 2`);
    assert.deepEqual(await ev(t, "__lab.log.went"), ["procesos", "cola"]);
    assert.deepEqual(problems(t), []);
  } finally { await t.close(); }
});

// PROCESOS has its own page of an application (`moment.drawer`): the shell hands it over as `content`. The drawer keeps all of its behaviour
// around it (history, focus, j/k), and an application the stop has no page for gets the common content.
test("a stop's own page replaces the common content, an application it has none for keeps the common one, and j/k and Esc work the same", async () => {
  const t = await open({ query: "?t=tok123&own=1" });
  try {
    await ready(t);
    const keys = await ev(t, `[...document.querySelectorAll("[data-card]")].slice(0, 3).map((e) => e.dataset.card)`);
    await ev(t, `document.querySelector('[data-card="${keys[0]}"]').click()`);
    await t.page.waitFor(`!!${D}.querySelector('.d-own[data-job="${keys[0]}"]')`);
    assert.equal(await ev(t, `!!${D}.querySelector(".d-now")`), false, "the common content is not drawn under it");
    assert.equal(await ev(t, `${D}.querySelectorAll(".detail-bar").length`), 1, "the bar is the drawer's own");
    await ev(t, "document.activeElement.blur()");
    await key(t, "j");
    await t.page.waitFor(`!!${D}.querySelector('[data-job="${keys[1]}"]')`);
    assert.equal(await ev(t, `decodeURIComponent(location.hash)`), "#hoy/" + keys[1]);
    // the one application the stop has no page for shows the common content
    await ev(t, `__app.drawer.open("demo:4")`);
    await t.page.waitFor(`!!${D}.querySelector('.d-now[data-tone]') && !${D}.querySelector(".d-own")`);
    await key(t, "Escape");
    await t.page.waitFor(`!${D}.classList.contains("open")`);
    assert.deepEqual(problems(t), []);
  } finally { await t.close(); }
});

test("Papel turns the drawer with the sheet: dark ink on paper, every text at 4.5:1 or more, the armed button too", async () => {
  const t = await open({ query: "?t=tok123&theme=light&lang=es" });
  try {
    await ready(t);
    await ev(t, `__app.drawer.open("demo:15")`);
    await shown(t);
    const paint = await ev(t, `(() => {
      const bg = getComputedStyle(${D}).backgroundColor;
      const c = (sel) => { const e = ${D}.querySelector(sel); return e ? getComputedStyle(e).color : null; };
      return { bg, h2: c(".d-head h2"), dim: c(".d-meta"), sentence: c(".d-sentence"), detail: c(".d-ldetail"), chip: c(".chip.you, .chip.robot, .chip"), button: c(".d-actions .act") };
    })()`);
    assert.deepEqual(rgbOf(paint.bg), [237, 240, 246], "the paper");
    for (const [name, css] of Object.entries(paint)) {
      if (name === "bg" || !css) continue;
      assert.ok(contrast(rgbOf(css), rgbOf(paint.bg)) >= 4.5, `${name} ${css} on ${paint.bg}`);
    }
    assert.ok(rgbOf(paint.h2)[0] < 60, "dark ink");
    // the first press on a permanent action only arms it: the button then says so in words and in colour, and the colour is legible too
    await ev(t, `${D}.querySelector("[data-action=approve]").click()`);
    await t.page.waitFor(`${D}.querySelector("[data-action=approve]").hasAttribute("data-armed")`);
    const armed = await ev(t, `(() => { const b = ${D}.querySelector("[data-action=approve]"); return { color: getComputedStyle(b).color, bg: getComputedStyle(b).backgroundColor, word: b.textContent }; })()`);
    const behind = rgbOf(armed.bg).length === 3 && !/rgba\(.*,\s*0\)$/.test(armed.bg) && armed.bg !== "transparent" ? rgbOf(armed.bg) : rgbOf(paint.bg);
    assert.ok(contrast(rgbOf(armed.color), behind) >= 4.5, `armed ${armed.color} on ${armed.bg}`);
    assert.ok(armed.word.length > 0);
    assert.deepEqual(problems(t), []);
  } finally { await t.close(); }
});
