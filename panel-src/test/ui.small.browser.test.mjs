import test from "node:test";
import assert from "node:assert/strict";
import { openBuilt } from "./helpers/browser.mjs";

const open = (opts = {}) => openBuilt({ args: ["lab", "f0-small"], file: "dist/lab-f0-small.html", ...opts });
const ev = (t, expr) => t.page.eval(expr);

for (const [width, height, sheetWidth, gutter] of [[1920, 1080, 1114, 63.4], [3440, 1440, 1400, 64]]) {
  test(`A13: the sheet is ${sheetWidth} px wide at ${width}, glass without border or corner, Hoy as tall as its content and Procesos to the floor`, async () => {
    const t = await open({ width, height });
    try {
      const measure = () => ev(t, `(() => { const el = document.getElementById("app"); const r = el.getBoundingClientRect(); const s = getComputedStyle(el);
        return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, bg: s.backgroundColor, filter: s.backdropFilter, border: s.borderTopWidth + " " + s.borderLeftWidth, radius: s.borderTopLeftRadius, color: s.color }; })()`);
      await ev(t, `window.__lab.rows(3)`);
      const hoy = await measure();
      assert.ok(Math.abs(hoy.width - sheetWidth) <= 1.5, `width ${hoy.width}`);
      assert.ok(Math.abs(hoy.left - gutter) <= 1, `left ${hoy.left}`);
      assert.ok(Math.abs(hoy.top - 78) <= 1, `top ${hoy.top}`);
      assert.ok(hoy.bottom < height - 200, `Hoy is as tall as its content (${hoy.bottom})`);
      assert.deepEqual([hoy.bg, hoy.border, hoy.radius], ["rgba(8, 12, 22, 0.72)", "0px 0px", "0px"]);
      assert.match(hoy.filter, /blur\(22px\)/);
      assert.equal(hoy.color, "rgb(233, 239, 247)");
      await ev(t, `window.__lab.moment("procesos")`);
      const procesos = await measure();
      assert.ok(Math.abs(procesos.bottom - (height - 26)) <= 1, `Procesos reaches the floor margin (${procesos.bottom})`);
      assert.equal(procesos.width, hoy.width);
      await ev(t, `document.documentElement.setAttribute("data-theme", "light")`);
      const papel = await measure();
      assert.deepEqual([papel.bg, papel.filter, papel.color], ["rgb(237, 240, 246)", "none", "rgb(19, 31, 53)"], "Papel changes only the sheet's tokens");
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}

test("the prologue has no sheet: a veil of --night-0 at 55 % from the HUD to the floor, without blur", async () => {
  const t = await open();
  try {
    await ev(t, `window.__lab.moment("prologo")`);
    const v = await ev(t, `(() => { const el = document.getElementById("app"); const r = el.getBoundingClientRect(); const s = getComputedStyle(el); return { top: r.top, bottom: r.bottom, bg: s.backgroundColor, filter: s.backdropFilter }; })()`);
    assert.deepEqual(v, { top: 56, bottom: 1080, bg: "rgba(6, 8, 14, 0.55)", filter: "none" });
  } finally { await t.close(); }
});

test("the toast: bottom right above the bar, 4 s, an error stays, Undo undoes once, a polite one waits behind an error, no live region of its own", async () => {
  const t = await open();
  try {
    const hidden = await ev(t, `getComputedStyle(document.getElementById("toast")).visibility`);
    assert.equal(hidden, "hidden");
    await ev(t, `window.__lab.toast.show("Aprobada", true)`);
    let s = await ev(t, `window.__lab.toastState()`);
    assert.deepEqual([s.cls, s.text, s.timers, s.undo, s.close, s.live, s.role], ["toast show ok", "Aprobada", 1, false, false, null, null]);
    assert.deepEqual(await ev(t, `window.__lab.said`), ["Aprobada"], "the one live region says it");
    await t.page.wait(250);                                             // the 150 ms entrance
    const box = await ev(t, `(() => { const r = document.getElementById("toast").getBoundingClientRect(); const s = getComputedStyle(document.getElementById("toast")); return { right: 1920 - r.right, bottom: 1080 - r.bottom, visibility: s.visibility, font: s.fontSize }; })()`);
    assert.ok(Math.abs(box.right - 63.4) <= 1 && Math.abs(box.bottom - 52) <= 1, `${JSON.stringify(box)}`);
    assert.deepEqual([box.visibility, box.font], ["visible", "13px"]);
    await ev(t, `window.__lab.tick(3999)`);
    assert.equal((await ev(t, `window.__lab.toastState()`)).cls, "toast show ok", "still there at 3.999 s");
    await ev(t, `window.__lab.tick(1)`);
    assert.equal((await ev(t, `window.__lab.toastState()`)).cls.split(" ").includes("show"), false, "gone at 4 s");

    await ev(t, `window.__lab.undone = 0; window.__lab.toast.show("Pospuesta", true, { undo: () => { window.__lab.undone += 1; } })`);
    s = await ev(t, `window.__lab.toastState()`);
    assert.deepEqual([s.undo, s.undoText], [true, "Deshacer"]);
    await t.page.wait(250);
    await t.page.click(".toast-undo");
    assert.equal(await ev(t, `window.__lab.undone`), 1);
    assert.equal((await ev(t, `window.__lab.toastState()`)).cls.split(" ").includes("show"), false, "Undo closes the toast");

    await ev(t, `window.__lab.toast.show("No se pudo guardar", false)`);
    s = await ev(t, `window.__lab.toastState()`);
    assert.deepEqual([s.cls, s.timers, s.close], ["toast show bad", 0, true]);
    await ev(t, `window.__lab.tick(60000)`);
    assert.equal((await ev(t, `window.__lab.toastState()`)).cls, "toast show bad", "an error does not go away by itself");
    await ev(t, `window.__lab.toast.polite("Hito", true)`);
    assert.equal((await ev(t, `window.__lab.toastState()`)).text, "No se pudo guardar", "good news waits behind an error");
    await t.page.wait(250);
    await t.page.click(".toast-close");
    s = await ev(t, `window.__lab.toastState()`);
    assert.deepEqual([s.cls, s.text, s.timers], ["toast show ok", "Hito", 1], "closing the error lets the waiting message in, with its own 4 s");
    await ev(t, `window.__lab.toast.show("uno"); window.__lab.toast.show("dos")`);
    assert.equal((await ev(t, `window.__lab.toastState()`)).timers, 1, "a new message replaces the old and restarts the clock");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the title is Panel, always; the icon carries the count in tokens: --gold bar when something is yours, --text-dim when nothing is", async () => {
  const t = await open();
  try {
    assert.equal(await ev(t, `document.title`), "Panel");
    await ev(t, `window.__lab.title("empty")`);
    await t.page.waitFor(`!!document.querySelector('link[rel="icon"]') && document.querySelector('link[rel="icon"]').href.startsWith("blob:")`);
    const zero = await ev(t, `window.__lab.icon()`);
    assert.deepEqual(zero.tile, [16, 22, 38], "the tile is --night-2");
    assert.deepEqual(zero.stripe, [158, 172, 190], "no count: the bar is --text-dim");
    const first = zero.href;
    await ev(t, `window.__lab.title("rich")`);
    await t.page.waitFor(`document.querySelector('link[rel="icon"]').href !== ${JSON.stringify(first)}`);
    const some = await ev(t, `window.__lab.icon()`);
    assert.deepEqual(some.stripe, [251, 192, 36], "something is yours: the bar is --gold");
    await ev(t, `window.__lab.title(null)`);
    assert.equal(await ev(t, `document.title`), "Panel");
    await ev(t, `document.title = "(3) Panel"; window.__lab.title("rich")`);
    assert.equal(await ev(t, `document.title`), "Panel", "anything else that wrote the title is undone");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the cursor: j and k move over the visible rows, redraws keep it by key, a long list scrolls its own box and never the page", async () => {
  const t = await open();
  try {
    await ev(t, `window.__lab.cursor.move(1)`);
    assert.deepEqual(await ev(t, `window.__lab.keyed()`), ["row 0"]);
    assert.equal(await ev(t, `window.__lab.view.cursor`), "c:k0");
    await ev(t, `window.__lab.cursor.move(1)`);
    assert.equal(await ev(t, `window.__lab.view.cursor`), "t:t1");
    await ev(t, `window.__lab.redraw()`);
    assert.deepEqual(await ev(t, `window.__lab.keyed()`), ["row 1"], "the cursor finds its row again after a redraw");
    for (let i = 0; i < 20; i++) await ev(t, `window.__lab.cursor.move(1)`);
    await t.page.wait(700);                                            // the smooth scroll of the list
    const s = await ev(t, `(() => { const list = document.getElementById("rows"); const row = list.querySelector('[data-cursor="true"]'); const a = row.getBoundingClientRect(); const b = list.getBoundingClientRect();
      return { text: row.textContent, top: list.scrollTop, inside: a.top >= b.top - 1 && a.bottom <= b.bottom + 1, page: window.scrollY }; })()`);
    assert.deepEqual([s.text, s.inside, s.page], ["row 21", true, 0]);
    assert.ok(s.top > 400, `the list scrolled (${s.top})`);
    await ev(t, `window.__lab.cursor.move(-100)`);
    assert.equal(await ev(t, `window.__lab.view.cursor`), "c:k0", "the first row stops the cursor");
    await ev(t, `window.__lab.rows(0)`);
    assert.equal(await ev(t, `window.__lab.view.cursor`), null, "a row that is gone takes the cursor with it");
    assert.ok((await ev(t, `window.__lab.ticks`)).length >= 22, "every move ticks");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("under 760 px the sheet is a block in the page and the toast is a bar across the bottom", async () => {
  const t = await open({ width: 390, height: 844 });
  try {
    await ev(t, `window.__lab.toast.show("Hecho")`);
    await t.page.wait(250);
    const m = await ev(t, `(() => { const s = getComputedStyle(document.getElementById("app")); const r = document.getElementById("toast").getBoundingClientRect();
      return { pos: s.position, margin: s.marginLeft, width: document.getElementById("app").getBoundingClientRect().width, toastLeft: r.left, toastRight: 390 - r.right, hscroll: document.documentElement.scrollWidth - innerWidth }; })()`);
    assert.deepEqual([m.pos, m.margin, m.width], ["static", "16px", 358]);
    assert.deepEqual([m.toastLeft, m.toastRight], [16, 16]);
    assert.ok(m.hscroll <= 0);
  } finally { await t.close(); }
});
