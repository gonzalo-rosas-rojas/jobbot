// The whole page in Chrome, on a desktop window (spec A1, A2, A3, A10, A13, A16, A19 and the review items M10, M11, B8, B9): the frame the shell
// builds, the stage behind it, the keys, the trips, the address, the wheel, the layers of Esc. The flat page is test/page.flat.browser.test.mjs.
import test from "node:test";
import assert from "node:assert/strict";
import { openPage, wheel, trace } from "./helpers/page.mjs";

const SIZES = [[1920, 1080], [3440, 1440], [1440, 900]];
const ev = (t, expr) => t.page.eval(expr);
const key = (t, k, code = k.length === 1 && /\d/.test(k) ? "Digit" + k : "Key" + k.toUpperCase()) => t.page.key(k, { code, text: k.length === 1 ? k : undefined });
const state = (t) => ev(t, `({ y: Math.round(scrollY), moment: document.getElementById("app").dataset.moment, hash: location.hash, active: document.querySelector('#stops [aria-selected="true"]')?.dataset.stop || null, opacity: document.getElementById("app").style.opacity, vh: innerHeight })`);
const settle = (t, ms = 900) => t.page.wait(ms);

test("the page boots on three desktop windows with a clean console, one frame, one clock and data in HOY", async () => {
  for (const [width, height] of SIZES) {
    const t = await openPage({ query: "?debug=1&clock=9&lang=es", width, height });
    try {
      await t.page.waitFor(`/Te tocan/i.test(document.querySelector("#app h1").textContent)`);
      const v = await ev(t, `(() => { const q = (s) => document.querySelector(s); const vis = (el) => !!el && getComputedStyle(el).display !== "none" && getComputedStyle(el).visibility !== "hidden";
        return { title: document.title, brand: q(".hud-brand").textContent.toUpperCase(), moment: q("#app").dataset.moment, scrollH: document.documentElement.scrollHeight, vh: innerHeight, hscroll: document.documentElement.scrollWidth - innerWidth,
          sky: vis(q("#sky")), dial: vis(q("#dial")), mode: window.__app.mode(), lenis: document.documentElement.classList.contains("lenis"),
          clocks: [q(".hud-clock"), q(".dial-time")].filter(vis).length, tabs: [...document.querySelectorAll('#stops [role="tab"]')].map((x) => x.dataset.stop),
          dangling: [...document.querySelectorAll("[aria-controls]")].filter((x) => !document.getElementById(x.getAttribute("aria-controls"))).length,
          sheet: q("#app").getBoundingClientRect().width, scripts: document.querySelectorAll("script").length, live: !!document.getElementById("live") }; })()`);
      const at = `${width}x${height}`;
      assert.equal(v.title, "jobbot", `${at}: the public demo names itself in the tab (the real page says Panel: page.real.browser.test.mjs)`);
      // from the arrival of the document (a cold Chrome under load takes its own time to start; the page owns what comes after)
      const first = await ev(t, `performance.now() - performance.getEntriesByType("navigation")[0].responseEnd`);
      assert.ok(first < 1000, `${at}: HOY with data ${Math.round(first)} ms after the document arrived`);
      assert.equal(await ev(t, `[document.querySelectorAll("main").length, document.querySelectorAll("#live").length, history.scrollRestoration, document.body.dataset.stop].join()`), "1,1,manual,hoy", at);
      assert.equal(v.brand, "JOBBOT", `${at}: and in the mark of the HUD`);
      assert.equal(v.moment, "hoy", at);
      assert.equal(v.scrollH, 6 * v.vh, `${at}: six stops of one window each`);
      assert.equal(v.hscroll, 0, `${at}: no sideways scroll`);
      assert.deepEqual([v.sky, v.dial, v.mode, v.lenis], [true, true, "stage", true], at);
      assert.equal(v.clocks, 1, `${at}: one clock on the page, not two (review M11)`);
      assert.deepEqual(v.tabs, ["hoy", "procesos", "cola", "aprender", "sistema"], at);
      assert.equal(v.dangling, 0, `${at}: every aria-controls names an element (review B8)`);
      assert.ok(v.live, at);
      assert.equal(Math.round(v.sheet), Math.round(Math.min(Math.max(960, 0.58 * width), 1400)), `${at}: the sheet of spec A13`);
      assert.deepEqual(t.page.problems(), [], at);
      // nothing leaves this machine: the page, its fonts under /a/<launch id>/ and (the demo has no server) nothing else
      for (const url of t.requests) assert.ok(new RegExp(`^http://127\\.0\\.0\\.1:${t.port}/(dist/|a/${t.launchId}/|favicon\\.ico)`).test(url) || url.startsWith("blob:"), `${at}: ${url}`);
    } finally { await t.close(); }
  }
});

test("the sheet is never wider than the window between 760 and 1010 px (review M10)", async () => {
  for (const width of [760, 800, 1000]) {
    const t = await openPage({ query: "?debug=1&clock=9", width, height: 800 });
    try {
      const v = await ev(t, `(() => { const r = document.getElementById("app").getBoundingClientRect(); return { left: r.left, right: r.right, vw: innerWidth, hscroll: document.documentElement.scrollWidth - innerWidth, mode: window.__app.mode() }; })()`);
      assert.equal(v.mode, "stage", `${width}`);
      assert.ok(v.right <= v.vw + 0.5, `${width}: the sheet ends at ${v.right} in a ${v.vw} px window`);
      assert.ok(v.left >= 0 && v.hscroll === 0, `${width}`);
    } finally { await t.close(); }
  }
});

test("1 to 5 and 0 travel; a neighbour takes 420 ms; the address follows and Back and Forward walk the stops (A3)", async () => {
  const t = await openPage({ query: "?debug=1&clock=9" });
  try {
    const marks = (await ev(t, `window.__app.scene.marks`)).map(Math.round);
    assert.deepEqual(marks, [0, 1, 2, 3, 4, 5].map((i) => i * 1080));
    assert.deepEqual(await state(t), { y: 1080, moment: "hoy", hash: "#hoy", active: "hoy", opacity: "", vh: 1080 }, "it opens on HOY, where the address says");
    // the trip lasts from the press of the key to the moment the page says where it is (the address is written on arrival)
    await ev(t, `window.__keyAt = 0; window.__arrivedAt = 0; addEventListener("keydown", () => { __keyAt = __keyAt || performance.now(); }, true);
      (function watch() { if (location.hash === "#procesos" && !__arrivedAt) __arrivedAt = performance.now(); else requestAnimationFrame(watch); })()`);
    await key(t, "2");
    await settle(t, 900);
    const took = await ev(t, `__arrivedAt - __keyAt`);
    assert.ok(Math.abs(took - 420) <= 30, `a neighbour takes ${Math.round(took)} ms`);
    let s = await state(t);
    assert.deepEqual([s.y, s.moment, s.hash, s.active, s.opacity], [2160, "procesos", "#procesos", "procesos", ""]);
    for (const [k, id, y] of [["3", "cola", 3240], ["4", "aprender", 4320], ["5", "sistema", 5400]]) {
      await key(t, k);
      await settle(t, 700);
      s = await state(t);
      assert.deepEqual([s.y, s.moment, s.hash, s.active], [y, id, "#" + id, id], k);
    }
    await key(t, "0");
    await settle(t, 1000);
    s = await state(t);
    assert.deepEqual([s.y, s.moment, s.hash, s.active], [0, "prologo", "#prologo", null], "0 is the prologue; no tab is lit there");
    await key(t, "1");
    await settle(t, 600);
    // one history entry per trip: Back walks them in reverse, Forward again
    await ev(t, `history.back()`);
    await settle(t, 1000);
    assert.equal((await state(t)).moment, "prologo");
    await ev(t, `history.back()`);
    await settle(t, 1000);
    assert.equal((await state(t)).moment, "sistema");
    await ev(t, `history.forward()`);
    await settle(t, 1000);
    assert.equal((await state(t)).moment, "prologo");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("an address that names a stop opens on it with no trip, and the stops bar travels too", async () => {
  const t = await openPage({ query: "?debug=1&clock=9#cola" });
  try {
    await settle(t, 300);
    const first = await state(t);
    assert.deepEqual([first.y, first.moment, first.hash, first.active], [3240, "cola", "#cola", "cola"], "it is there from the first frame");
    await t.page.click("#tab-aprender");
    await settle(t, 700);
    assert.deepEqual([(await state(t)).moment, (await state(t)).hash], ["aprender", "#aprender"]);
    // a tab is a button: Enter on it is the browser's (it does not open an application)
    await ev(t, `document.getElementById("tab-sistema").focus()`);
    await key(t, "Enter", "Enter");
    await settle(t, 700);
    assert.equal((await state(t)).moment, "aprender", "Enter on a tab is a click, not the page's Enter: the tab list selects with the arrows");
  } finally { await t.close(); }
});

test("the wheel belongs to a list while the pointer is over it, and to the camera everywhere else", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&scenario=heavy" });
  try {
    await t.page.waitFor(`document.querySelector("#app ul.rows")`);
    const over = (sel) => ev(t, `(() => { const r = document.querySelector(${JSON.stringify(sel)}).getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`);
    // HOY's list: the page does not move
    const list = await over("#app ul.rows");
    await wheel(t.page, list.x, list.y, 300);
    await settle(t, 700);
    assert.equal((await state(t)).y, 1080, "a wheel over HOY's list leaves the stage where it is");
    // PROCESOS has a list that scrolls by itself: it scrolls, the camera does not
    await key(t, "2");
    await settle(t, 800);
    await t.page.waitFor(`document.querySelector("#app .scroller")`);
    const sc = await over("#app .scroller");
    const before = await ev(t, `document.querySelector("#app .scroller").scrollTop`);
    await wheel(t.page, sc.x, sc.y, 240);
    await settle(t, 700);
    const after = await ev(t, `document.querySelector("#app .scroller").scrollTop`);
    assert.ok(after > before, `the list scrolled (${before} to ${after})`);
    assert.equal((await state(t)).y, 2160, "and the stage did not move");
    // over the sky (right of the sheet) the wheel is the camera's: a gesture past the half of a floor carries the page to the next stop (a shorter one
    // is carried back to this stop 220 ms after its last notch, which the engine tests and scene.engine.browser.test.mjs measure)
    for (let i = 0; i < 6; i++) { await wheel(t.page, 1700, 700, 100); await t.page.wait(16); }
    await settle(t, 900);
    assert.equal((await state(t)).y, 3240, "the wheel over the sky moves the camera to the next stop");
  } finally { await t.close(); }
});

test("Esc closes one layer at a time (the help, the drawer, the cursor) and a trip in the air stops", async () => {
  const t = await openPage({ query: "?debug=1&clock=9" });
  try {
    await t.page.waitFor(`document.querySelector("#app .row[data-card], #app .next[data-card]")`);
    // the help: a modal dialog over a page that is inert (review B9)
    await key(t, "?", "Slash");
    await t.page.waitFor(`document.querySelector(".help")`);
    assert.equal(await ev(t, `document.getElementById("doc").inert`), true, "the page behind the help is inert");
    assert.equal(await ev(t, `document.activeElement.className`), "help-close");
    await key(t, "Escape", "Escape");
    await t.page.waitFor(`!document.querySelector(".help")`);
    assert.equal(await ev(t, `document.getElementById("doc").inert`), false, "the page is back");
    // a row under the keyboard, then its application in the drawer
    await key(t, "j");
    assert.equal(await ev(t, `document.querySelectorAll('[data-cursor="true"]').length`), 1);
    await key(t, "Enter", "Enter");
    await t.page.waitFor(`document.getElementById("detail").classList.contains("open")`);
    assert.match(await ev(t, `location.hash`), /^#hoy\//, "the address names the application");
    // with the drawer open the digits do not travel (§3.6)
    await key(t, "3");
    await settle(t, 600);
    assert.equal((await state(t)).moment, "hoy");
    await key(t, "Escape", "Escape");
    await t.page.waitFor(`!document.getElementById("detail").classList.contains("open")`);
    assert.equal(await ev(t, `location.hash`), "#hoy", "closing the drawer takes its history entry back");
    assert.equal(await ev(t, `document.querySelectorAll('[data-cursor="true"]').length`), 1, "the cursor is still on its row");
    await key(t, "Escape", "Escape");
    assert.equal(await ev(t, `document.querySelectorAll('[data-cursor="true"]').length`), 0, "the next Esc clears the cursor");
    // Esc in the middle of a trip stops it where it is
    await key(t, "5");
    await settle(t, 200);
    await key(t, "Escape", "Escape");
    const cut = await state(t);
    assert.ok(cut.y < 5 * cut.vh, "it did not go all the way");
    await settle(t, 900);
    const rest = await state(t);
    assert.ok(rest.y % rest.vh < 1.5 || rest.vh - (rest.y % rest.vh) < 1.5, `the page rests on a stop, not between two floors: ${cut.y} -> ${rest.y} of ${rest.vh}`);
    assert.notEqual(rest.moment, "sistema");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the keys of HOY act on the urgent scenario: approve asks twice, postpone opens its menu, a task is done, and the answers reach the page", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&scenario=urgent" });
  try {
    await t.page.waitFor(`document.querySelector("#app .row[data-card]")`);
    const rows = () => ev(t, `[...document.querySelectorAll("#app li.row")].map((r) => ({ id: r.dataset.row, actions: r.dataset.actions }))`);
    const before = await rows();
    // j walks the rows: the first row of the list is the one under the cursor
    await key(t, "j");
    assert.equal(await ev(t, `document.querySelectorAll('[data-cursor="true"]').length`), 1);
    // a task: h marks it done (the server of the demo answers and the next snapshot has one less)
    const task = before.find((r) => /done/.test(r.actions));
    assert.ok(task, "urgent has a task to do");
    await ev(t, `window.__app.shell.cursor.set(${JSON.stringify(task.id.startsWith("task:") ? "t:" + task.id.slice(5) : "c:" + task.id.slice(5))})`);
    await key(t, "h");                                          // done cannot be undone: the first press arms it, the second (not a held key) does it
    assert.equal(await ev(t, `document.querySelector('#app [data-armed="true"]') != null`), true, "the first press only arms it");
    await settle(t, 450);
    await key(t, "h");
    await t.page.waitFor(`document.querySelector(".toast.show")`);
    await t.page.waitFor(`!document.querySelector('#app li.row[data-row=${JSON.stringify(task.id)}]')`);
    // a proposal: a once starts the two steps
    const approvable = (await rows()).find((r) => /approve/.test(r.actions));
    assert.ok(approvable, "and a proposal to approve");
    await ev(t, `window.__app.shell.cursor.set("c:" + ${JSON.stringify(approvable.id.replace(/^card:/, ""))})`);
    await key(t, "a");
    assert.equal(await ev(t, `document.querySelector('#app [data-armed="true"]') != null`), true, "the first press only arms it");
    // s opens the menu of postponing, as a box that reads its own keys; Esc closes it and the page is as it was
    await key(t, "s");
    await t.page.waitFor(`document.querySelector(".menu")`);
    assert.equal(await ev(t, `document.querySelector(".menu").hasAttribute("data-local-keys")`), true);
    await key(t, "3");
    await settle(t, 500);
    assert.equal((await state(t)).moment, "hoy", "a digit inside the menu is the menu's, not a trip");
    await key(t, "Escape", "Escape");
    await t.page.waitFor(`!document.querySelector(".menu")`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

/** What each stop prints on an empty install that no page should: the words of a missing value. Returns { key: sentence } for the offenders. */
async function emptyOffenders(lang, keys) {
  const t = await openPage({ query: `?debug=1&clock=9&scenario=empty&lang=${lang}` });
  const bad = {};
  try {
    await t.page.waitFor(`window.__app.model()`);
    for (const k of keys) {
      await key(t, k);
      await settle(t, k === "0" ? 900 : 600);
      const text = await ev(t, `document.body.innerText`);
      const found = /.{0,40}(\bnull\b|undefined|NaN|⟦).{0,40}/s.exec(text);
      if (found) bad[k] = found[0].replace(/\s+/g, " ");
    }
    assert.deepEqual(t.page.problems(), [], lang);
  } finally { await t.close(); }
  return bad;
}

test("an empty install says so in words: no null, undefined or NaN on any stop, in either language", async () => {
  for (const lang of ["es", "en"]) assert.deepEqual(await emptyOffenders(lang, ["0", "1", "2", "3", "4", "5"]), {}, lang);
});

test("calm motion, asked by the system or by the setting: a trip is a cut with a 180 ms crossfade, nothing is left half faded, the sky holds still (A10)", async () => {
  for (const mode of ["system", "setting"]) {
    const t = await openPage(mode === "system" ? { query: "?debug=1&clock=9", media: { reducedMotion: true } } : { query: "?debug=1&clock=9&motion=calm" });
    try {
      assert.equal(await ev(t, "document.documentElement.dataset.motion"), "calm", mode);
      await trace(t.page, 500);
      await key(t, "3");
      await settle(t, 600);
      const samples = await ev(t, `__trace`);
      const ys = [...new Set(samples.map(([, y]) => y))];
      assert.deepEqual(ys.filter((y) => y > 1080 && y < 3240), [], `${mode}: no frame between the two stops: ${ys.join(",")}`);
      const s = await state(t);
      assert.deepEqual([s.y, s.moment, s.opacity], [3240, "cola", ""], mode);
      assert.equal(await ev(t, "__app.engine.S.jump"), null, mode);
      assert.deepEqual(t.page.problems(), [], mode);
    } finally { await t.close(); }
  }
  // the crossfade itself: the sheet of the stop that arrives comes in over 180 ms (read the animation at the cut)
  const t = await openPage({ query: "?debug=1&clock=9", media: { reducedMotion: true } });
  try {
    const fade = await ev(t, `new Promise((resolve) => { new MutationObserver((_, o) => { o.disconnect(); resolve(document.getElementById("app").getAnimations().map((a) => a.effect.getTiming().duration)); }).observe(document.getElementById("app"), { attributes: true, attributeFilter: ["data-moment"] }); __app.go("cola"); })`);
    assert.ok(fade.includes(180), `the sheet comes in over 180 ms (${fade})`);
  } finally { await t.close(); }
});

test("an action that has a way back says so in the toast: COLA's skip puts Deshacer there, and Deshacer brings the card back (ctx.toast with { undo })", async () => {
  const t = await openPage({ query: "?debug=1&clock=18#cola", width: 1440, height: 900 });
  try {
    await t.page.waitFor(`document.querySelectorAll("#app [data-row]").length >= 2`);
    const rows = await ev(t, `document.querySelectorAll("#app [data-row]").length`);
    await key(t, "s");
    await t.page.waitFor(`!!document.querySelector("[role=menu], .menu")`);
    await key(t, "1");
    await t.page.waitFor(`document.querySelectorAll("#app [data-row]").length === ${rows - 1}`);
    const toast = await ev(t, `({ text: document.querySelector(".toast").textContent, undo: [...document.querySelectorAll(".toast button")].filter((b) => b.getClientRects().length).map((b) => b.textContent.trim()) })`);
    assert.match(toast.text, /pospuesta/i);
    assert.deepEqual(toast.undo, ["Deshacer"]);
    await t.page.click(".toast button");
    await t.page.waitFor(`document.querySelectorAll("#app [data-row]").length === ${rows}`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("?now= fixes the page's now: what HOY says about time agrees with it, and the sky follows its hour", async () => {
  const t = await openPage({ query: "?debug=1&now=2026-10-08T13:00:00Z" });
  try {
    assert.equal(await ev(t, `__app.shell.ctx.now()`), Date.parse("2026-10-08T13:00:00Z"));
    assert.match(await ev(t, `document.querySelector("#app .eyebrow").textContent`), /10:00/, "Santiago is UTC-3 in October (summer time)");
    assert.ok(Math.abs((await ev(t, `__app.clock.hora()`)) - 10) < 0.05, "the sky is at 10:00 too");
  } finally { await t.close(); }
});

test("the help lists what the stop answers (wired.js): HOY its keys, PROCESOS what its moment takes, SISTEMA only the engine's (A19)", async () => {
  const { helpRows } = await import("../src/ui/help.js");
  const { liveSet } = await import("../src/ui/wired.js");
  const t = await openPage({ query: "?debug=1&clock=9&lang=es" });
  try {
    for (const [digit, stop] of [["1", "hoy"], ["2", "procesos"], ["5", "sistema"], ["4", "aprender"]]) {
      await key(t, digit);
      await t.page.waitFor(`document.getElementById("app").dataset.moment === ${JSON.stringify(stop)}`);
      await t.page.wait(450);
      await key(t, "?", "Slash");
      await t.page.waitFor(`!!document.querySelector(".help")`);
      const listed = await ev(t, `[...document.querySelectorAll(".help dt")].map((e) => e.textContent)`);
      assert.deepEqual(listed, helpRows(stop, liveSet(stop)).map((r) => r.display), stop);
      await key(t, "Escape", "Escape");
      await t.page.waitFor(`!document.querySelector(".help")`);
    }
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("l, t, m are the same keys everywhere: language, Papel (the sheet only: the HUD and the bar keep the sky's ink) and sound", async () => {
  const t = await openPage({ query: "?debug=1&clock=9" });
  try {
    await t.page.waitFor(`document.querySelectorAll("#app [data-card]").length > 0`);
    await key(t, "l");
    await t.page.waitFor(`document.documentElement.lang === "en"`);
    assert.match(await ev(t, `document.querySelector("#app h1").textContent`), /\S/);
    assert.equal(await ev(t, `document.querySelector(".toast").classList.contains("show")`), true, "the language says so in the toast");
    await key(t, "l");
    await t.page.waitFor(`document.documentElement.lang === "es"`);
    await key(t, "t");
    await t.page.waitFor(`document.documentElement.getAttribute("data-theme") === "light"`);
    assert.equal(await ev(t, `getComputedStyle(document.getElementById("app")).backgroundColor`), "rgb(237, 240, 246)", "Papel: the sheet is paper");
    assert.equal(await ev(t, `getComputedStyle(document.body).backgroundColor`), "rgb(10, 14, 25)", "and nothing else changes: the page behind is still night");
    const inks = () => ev(t, `[".hud kbd", ".bar kbd", ".hud-brand", ".bar-help"].map((s) => getComputedStyle(document.querySelector(s)).color)`);
    const papel = await inks();
    await key(t, "t");
    await t.page.waitFor(`!document.documentElement.getAttribute("data-theme")`);
    assert.deepEqual(papel, await inks(), "the HUD and the bar are on the sky, which Papel does not change: their ink is the same in both");
    await key(t, "t");
    await t.page.waitFor(`document.documentElement.getAttribute("data-theme") === "light"`);
    await key(t, "t");
    await t.page.waitFor(`!document.documentElement.getAttribute("data-theme")`);
    await key(t, "m");
    await t.page.waitFor(`__app.sound.enabled === true`);
    await key(t, "m");
    await t.page.waitFor(`__app.sound.enabled === false`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

// ---- Holding a key (review B10): j and k keep walking a list, every other key fires once
const hold = async (t, k, code, n, text = k.length === 1 ? k : undefined) => {
  for (let i = 0; i < n; i++) await t.page.send("Input.dispatchKeyEvent", { type: "keyDown", key: k, code, text, autoRepeat: i > 0 });
  await t.page.send("Input.dispatchKeyEvent", { type: "keyUp", key: k, code });
};

test("holding j or k walks the rows of HOY as far as they go and no further, one cursor at a time", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&scenario=heavy" });
  try {
    await t.page.waitFor(`__app.shell.cursor.rows().length >= 7`);
    const total = await ev(t, `__app.shell.cursor.rows().length`);
    assert.ok(total >= 7, `a list with something to walk (${total} rows)`);
    const at = () => ev(t, `(() => { const rows = __app.shell.cursor.rows(); const cur = __app.shell.cursor.current(); const r = cur.getBoundingClientRect(); return { index: rows.indexOf(cur), inView: r.top >= 0 && r.bottom <= innerHeight, marks: document.querySelectorAll('[data-cursor="true"]').length }; })()`);
    await hold(t, "j", "KeyJ", 6);
    let s = await at();
    assert.deepEqual([s.index, s.marks], [5, 1], "six presses of one hold are six steps");
    await hold(t, "j", "KeyJ", total + 10);
    s = await at();
    assert.deepEqual([s.index, s.marks, s.inView], [total - 1, 1, true], "past the end it stops on the last row, in view");
    await hold(t, "k", "KeyK", 4);
    s = await at();
    assert.equal(s.index, total - 5);
    await hold(t, "k", "KeyK", total + 10);
    s = await at();
    assert.deepEqual([s.index, s.inView], [0, true], "and back to the first");
    assert.equal((await state(t)).moment, "hoy");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("holding j with an application open in the drawer walks the applications one by one and the drawer shows the last one it reached", async () => {
  const t = await openPage({ query: "?debug=1&clock=9" });
  try {
    await t.page.waitFor(`document.querySelectorAll("#app [data-card]").length > 3`);
    await key(t, "j");
    await key(t, "Enter", "Enter");
    await t.page.waitFor(`document.getElementById("detail").classList.contains("open") && !!document.querySelector("#detail .d-body")`);
    const first = await ev(t, `__app.drawer.key`);
    await hold(t, "j", "KeyJ", 6);
    await t.page.wait(700);
    const v = await ev(t, `({ key: __app.drawer.key, shown: (document.querySelector("#detail [data-job]") || {}).dataset?.job || null, hash: decodeURIComponent(location.hash), entries: history.length })`);
    assert.notEqual(v.key, first, "it moved");
    assert.equal(v.shown, v.key, "what the drawer shows is where the keys got to (no older answer landed over a newer one)");
    assert.equal(v.hash, "#hoy/" + v.key);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("a key that is not a list key fires once however long it is held: 3 travels once", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&scenario=urgent" });
  try {
    await t.page.waitFor(`document.querySelectorAll("#app [data-card]").length > 0`);
    const entries = await ev(t, "history.length");
    await hold(t, "3", "Digit3", 8);
    await t.page.waitFor(`document.getElementById("app").dataset.moment === "cola"`);
    await t.page.wait(900);
    assert.equal(await ev(t, "history.length"), entries + 1, "one trip, one entry");
    assert.equal((await state(t)).moment, "cola");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("holding j in COLA steps through the proposals without a stumble", async () => {
  const t = await openPage({ query: "?debug=1&clock=18#cola", width: 1440, height: 900 });
  try {
    await t.page.waitFor(`document.querySelectorAll("#app [data-row]").length >= 2`);
    await hold(t, "j", "KeyJ", 5);
    await hold(t, "k", "KeyK", 5);
    await t.page.wait(300);
    assert.equal((await state(t)).moment, "cola");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("holding j in the long table of PROCESOS keeps the cursor row in view, the page where it is, and k walks back up", async () => {
  const t = await openPage({ query: "?debug=1&clock=15&scenario=heavy#procesos", width: 1440, height: 900 });
  try {
    await t.page.waitFor(`__app.shell.cursor.rows().length >= 60`);
    const at = () => ev(t, `(() => { const c = __app.shell.cursor, cur = c.current(), rows = c.rows(); let sc = cur.parentElement; while (sc && !(sc.scrollHeight > sc.clientHeight + 1 && /(auto|scroll)/.test(getComputedStyle(sc).overflowY))) sc = sc.parentElement;
      const r = cur.getBoundingClientRect(), s = (sc || document.documentElement).getBoundingClientRect(); return { index: rows.indexOf(cur), inView: r.top >= s.top - 1 && r.bottom <= Math.max(s.bottom, innerHeight) + 1, y: Math.round(scrollY) }; })()`);
    await hold(t, "j", "KeyJ", 30);
    await t.page.wait(300);
    let s = await at();
    assert.deepEqual([s.index, s.inView, s.y], [29, true, 2 * 900], "thirty steps, the row in view, the camera on PROCESOS");
    await hold(t, "k", "KeyK", 12);
    await t.page.wait(300);
    s = await at();
    assert.deepEqual([s.index, s.inView, s.y], [17, true, 2 * 900]);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});
