// The flat page: a phone, under 760 px (spec §3.11, A14 and the review item B7). No stage, no canvas, no Lenis: a header with the stops as tabs
// (the prologue is the first one, "Anoche"), one sheet, the bottom bar after it; the same actions as buttons, and a way in for the three that
// were only a key. The page passes from flat to desktop and back when the window is resized, without a reload.
import test from "node:test";
import assert from "node:assert/strict";
import { openPage } from "./helpers/page.mjs";

const ev = (t, expr) => t.page.eval(expr);
const state = (t) => ev(t, `({ mode: window.__app.mode(), moment: document.getElementById("app").dataset.moment, hash: location.hash, y: Math.round(scrollY), x: Math.round(scrollX),
  active: document.querySelector('#stops [aria-selected="true"]')?.dataset.stop || null, hscroll: document.documentElement.scrollWidth - innerWidth })`);
// HOY's skeleton has its h1 before the first snapshot; the page is ready (and Nueva tarea shown) once the sheet is no longer busy
const open = async (query = "?debug=1&clock=9") => {
  const t = await openPage({ query, width: 390, height: 844 });
  try { await t.page.waitFor(`!document.getElementById("app").hasAttribute("aria-busy")`); } catch (e) { await t.close(); throw e; }
  return t;
};

test("it boots flat: no canvas, no Lenis, six tabs with Anoche first, one clock, one sheet and the bar after it", async () => {
  const t = await open();
  try {
    const v = await ev(t, `(() => { const q = (s) => document.querySelector(s); const vis = (el) => !!el && el.getClientRects().length > 0 && getComputedStyle(el).visibility !== "hidden";   // a child of a display:none parent has no boxes
      const app = q("#app").getBoundingClientRect(), bar = q(".bar").getBoundingClientRect();
      return { title: document.title, brand: q(".hud-brand").textContent.toUpperCase(), mode: window.__app.mode(), lenis: document.documentElement.classList.contains("lenis"),
        sky: vis(q("#sky")), dial: vis(q("#dial")), clocks: [q(".hud-clock"), q(".dial-time")].filter(vis).length,
        tabs: [...document.querySelectorAll('#stops [role="tab"]')].map((x) => x.dataset.stop), labels: [...document.querySelectorAll('#stops [role="tab"]')].map((x) => x.textContent.trim()),
        active: q('#stops [aria-selected="true"]')?.dataset.stop, hscroll: document.documentElement.scrollWidth - innerWidth, sheets: document.querySelectorAll("#app").length,
        barAfterSheet: bar.top >= app.bottom - 1, hudSticky: getComputedStyle(q(".hud")).position, newTask: vis(q(".flat-new")), dangling: [...document.querySelectorAll("[aria-controls]")].filter((x) => !document.getElementById(x.getAttribute("aria-controls"))).length }; })()`);
    assert.equal(v.title, "jobbot", "the public demo names itself");
    assert.equal(v.brand, "JOBBOT");
    assert.deepEqual([v.mode, v.lenis, v.sky, v.dial], ["flat", false, false, false]);
    assert.equal(v.clocks, 1, "one clock on the page");
    assert.deepEqual(v.tabs, ["prologo", "hoy", "procesos", "cola", "aprender", "sistema"]);
    assert.match(v.labels[0], /anoche/i, "the prologue is Anoche");
    assert.equal(v.active, "hoy");
    assert.equal(v.hscroll, 0, "no sideways scroll on a phone");
    assert.ok(v.barAfterSheet, "the bar comes after the sheet");
    assert.equal(v.hudSticky, "sticky");
    assert.equal(v.dangling, 0);
    assert.equal(v.newTask, true, "Nueva tarea is a button on HOY");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("every target a finger meets is 44 px or more (review B7), and the mono text is never 9 or 10 px", async () => {
  const t = await open();
  try {
    const v = await ev(t, `(() => [...document.querySelectorAll('#stops [role="tab"], .hud-search, .bar-help, .flat-new')].map((el) => { const r = el.getBoundingClientRect(); return { el: el.className + " " + (el.dataset.stop || ""), w: Math.round(r.width), h: Math.round(r.height) }; }))()`);
    assert.ok(v.length >= 9, "six tabs, the search, the help and the new task");
    for (const x of v) assert.ok(x.w >= 44 && x.h >= 44, `${x.el}: ${x.w}x${x.h}`);
    const small = await ev(t, `(() => { const bad = []; for (const el of document.querySelectorAll("#shell *")) { if (![...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim())) continue; const cs = getComputedStyle(el); const px = parseFloat(cs.fontSize); if (px > 8.5 && px < 11 && cs.fontFamily.toLowerCase().includes("mono")) bad.push(el.className + " " + px); } return bad; })()`);
    assert.deepEqual(small, [], "mono text at 9 to 10 px");
  } finally { await t.close(); }
});

test("a tab swaps the moment, goes to the top and writes the address; Back walks the tabs; the strip never moves the document", async () => {
  const t = await open();
  try {
    await ev(t, `window.scrollTo(0, 400)`);
    await t.page.click("#tab-sistema");
    await t.page.waitFor(`document.getElementById("app").dataset.moment === "sistema"`);
    let s = await state(t);
    assert.deepEqual([s.mode, s.moment, s.hash, s.y, s.x, s.active, s.hscroll], ["flat", "sistema", "#sistema", 0, 0, "sistema", 0]);
    const inside = await ev(t, `(() => { const a = document.querySelector('#stops [aria-selected="true"]').getBoundingClientRect(), s = document.querySelector(".hud-center").getBoundingClientRect(); return { left: a.left - s.left, right: s.right - a.right, strip: document.querySelector(".hud-center").scrollLeft }; })()`);
    assert.ok(inside.left >= -0.5 && inside.right >= -0.5, `the lit tab is inside the strip: ${JSON.stringify(inside)}`);
    assert.ok(inside.strip > 0, "the strip scrolled to show it");
    await t.page.click("#tab-prologo");
    await t.page.waitFor(`document.getElementById("app").dataset.moment === "prologo"`);
    s = await state(t);
    assert.deepEqual([s.moment, s.hash, s.active, s.y], ["prologo", "#prologo", "prologo", 0]);
    assert.match(await ev(t, `document.querySelector("#app h1").textContent`), /anoche/i);
    await ev(t, `history.back()`);
    await t.page.waitFor(`document.getElementById("app").dataset.moment === "sistema"`);
    assert.equal((await state(t)).hash, "#sistema");
    await ev(t, `history.back()`);
    await t.page.waitFor(`document.getElementById("app").dataset.moment === "hoy"`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the keys of the desktop work on a flat page too: 3 goes to COLA, ? opens the help and Esc closes it", async () => {
  const t = await open();
  try {
    await t.page.key("3", { code: "Digit3", text: "3" });
    await t.page.waitFor(`document.getElementById("app").dataset.moment === "cola"`);
    assert.equal((await state(t)).hash, "#cola");
    await t.page.key("?", { code: "Slash", text: "?" });
    await t.page.waitFor(`!!document.querySelector("dialog[open], [role=dialog]")`);
    await t.page.key("Escape", { code: "Escape" });
    await t.page.waitFor(`!document.querySelector("dialog[open], [role=dialog]:not([hidden])")`);
  } finally { await t.close(); }
});

test("Nueva tarea opens the form of the key n, and a task written there is added", async () => {
  const t = await open("?debug=1&clock=9");
  try {
    await t.page.click(".flat-new");
    await t.page.waitFor(`!!document.querySelector(".menu-input")`);
    assert.equal(await ev(t, `document.activeElement === document.querySelector(".menu-input")`), true, "the field has the focus");
    await t.page.send("Input.insertText", { text: "Llamar a Ana de Lumen Pay" });
    await ev(t, `(() => { const d = document.querySelector(".menu-select"); d.value = "2h"; d.dispatchEvent(new Event("change", { bubbles: true })); document.querySelector(".menu-input").focus(); })()`);   // due in 2 hours: it is the first thing HOY shows
    await t.page.key("Enter", { code: "Enter", text: "\r" });
    await t.page.waitFor(`!document.querySelector(".menu-input")`);
    await t.page.waitFor(`document.getElementById("app").textContent.includes("Llamar a Ana de Lumen Pay")`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("a window that crosses 760 px changes between the flat page and the stage without a reload, and keeps the stop", async () => {
  const t = await open();
  try {
    await ev(t, `window.__kept = 7`);
    await t.page.click("#tab-cola");
    await t.page.waitFor(`document.getElementById("app").dataset.moment === "cola"`);
    await t.page.viewport(1440, 900);
    await t.page.waitFor(`window.__app.mode() === "stage"`);
    await t.page.wait(700);
    let v = await ev(t, `(() => { const vis = (el) => getComputedStyle(el).display !== "none"; return { kept: window.__kept, moment: document.getElementById("app").dataset.moment, hash: location.hash, sky: vis(document.getElementById("sky")), lenis: document.documentElement.classList.contains("lenis"),
      h: document.documentElement.scrollHeight, y: Math.round(scrollY), tabs: document.querySelectorAll('#stops [role="tab"]').length, flatNew: vis(document.querySelector(".flat-actions")) }; })()`);
    assert.deepEqual([v.kept, v.moment, v.hash, v.sky, v.lenis, v.tabs, v.flatNew], [7, "cola", "#cola", true, true, 5, false], "the same page, now with the stage");
    assert.equal(v.h, 6 * 900);
    assert.equal(v.y, 3 * 900, "the camera is on COLA");
    await t.page.viewport(390, 844);
    await t.page.waitFor(`window.__app.mode() === "flat"`);
    await t.page.wait(500);
    v = await ev(t, `(() => ({ kept: window.__kept, moment: document.getElementById("app").dataset.moment, sky: getComputedStyle(document.getElementById("sky")).display, lenis: document.documentElement.classList.contains("lenis"), y: Math.round(scrollY),
      tabs: document.querySelectorAll('#stops [role="tab"]').length, active: document.querySelector('#stops [aria-selected="true"]')?.dataset.stop }))()`);
    assert.deepEqual([v.kept, v.moment, v.sky, v.lenis, v.y, v.tabs, v.active], [7, "cola", "none", false, 0, 6, "cola"]);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("a phone in English says the same in English", async () => {
  const t = await open("?debug=1&clock=9&lang=en");
  try {
    const v = await ev(t, `({ lang: document.documentElement.lang, tab: document.querySelector("#tab-prologo").textContent.trim(), newTask: document.querySelector(".flat-new").textContent.trim() })`);
    assert.equal(v.lang, "en");
    assert.match(v.newTask, /new task/i);
    assert.match(v.tab, /night/i);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});
