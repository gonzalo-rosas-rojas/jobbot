// The scene engine on the real Lenis, in Chrome (lab f0-engine). Review findings A1, M1 and M2 of REVIEW-F0-front.md: the unit tests of
// the engine run on a fake Lenis and could not see that a page left between two stops shows no sheet at all.
import test from "node:test";
import assert from "node:assert/strict";
import { openBuilt } from "./helpers/browser.mjs";

const H = 900;
const open = (opts = {}) => openBuilt({ args: ["lab", "f0-engine"], file: "dist/lab-f0-engine.html", width: 1440, height: H, ...opts });
const ev = (t, expr) => t.page.eval(expr);
const state = (t) => ev(t, "window.__lab.state()");
const wheel = (t, deltaY = 100, x = 30, y = 450) => t.page.send("Input.dispatchMouseEvent", { type: "mouseWheel", x, y, deltaX: 0, deltaY });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** A page at rest: on a mark (to a pixel) and the sheet solid. This is what a person sees; anything else is a page with nothing to read. */
function assertLegible(s, label) {
  const off = Math.abs(s.y - Math.round(s.y / H) * H);
  const top = Math.abs(s.y - 4500) < 1.5;                      // the last mark is the end of the page (5 x 900 would be past it)
  assert.ok(off < 1.5 || top, `${label}: scrollY ${s.y} is ${off.toFixed(1)} px from a mark`);
  assert.ok(s.opacity >= 0.99, `${label}: the sheet is at ${s.opacity.toFixed(3)} (scrollY ${s.y})`);
}

async function arriveAt(t, id) {
  await ev(t, `window.__lab.clearArrivals(); window.__lab.jumpTo(${JSON.stringify(id)}, { push: false })`);
  await t.page.waitFor(`window.__lab.state().jumping === false && window.__lab.arrivals().length >= 1`, { timeout: 4000 });
  await sleep(200);
}

test("A1 with the real Lenis: after two wheel notches, a wheel in the middle of a trip, or a click in the middle of a trip, the page ends on a stop with the sheet solid", async () => {
  const t = await open();
  try {
    await arriveAt(t, "hoy");
    assertLegible(await state(t), "arrived at HOY");

    await wheel(t); await sleep(30); await wheel(t);
    await sleep(1600);
    assertLegible(await state(t), "two notches of 100 px");

    await arriveAt(t, "hoy");
    await ev(t, `window.__lab.jumpTo("sistema")`);
    await sleep(250);
    await wheel(t, 120);
    await sleep(2400);
    assertLegible(await state(t), "a wheel 250 ms into a trip to SISTEMA");

    await arriveAt(t, "hoy");
    await ev(t, `window.__lab.jumpTo("cola")`);
    await sleep(200);
    for (const type of ["mousePressed", "mouseReleased"]) await t.page.send("Input.dispatchMouseEvent", { type, x: 30, y: 450, button: "left", clickCount: 1 });
    await sleep(1400);
    assertLegible(await state(t), "a click 200 ms into a trip to COLA");

    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("A1: the stop the page rests on is announced once (as a manual arrival) and the glide does not announce it again", async () => {
  const t = await open();
  try {
    await arriveAt(t, "hoy");
    await ev(t, `window.__lab.clearArrivals()`);
    for (let i = 0; i < 5; i++) { await wheel(t); await sleep(30); }     // 500 px: past the half of a floor, so the nearest stop is PROCESOS
    await sleep(2200);
    const seen = await ev(t, `window.__lab.arrivals()`);
    assert.equal(seen.length, 1, JSON.stringify(seen));
    assert.equal(seen[0].id, "procesos");
    assert.equal(seen[0].manual, true);
    assert.equal(seen[0].push, false);
    assertLegible(await state(t), "after the glide");
  } finally { await t.close(); }
});

test("A1: no glide while a field has the focus or a drawer is open (canSnap), and snap() puts the page right when asked", async () => {
  const t = await open();
  try {
    await arriveAt(t, "hoy");
    await ev(t, `document.getElementById("field").focus()`);
    await wheel(t); await sleep(30); await wheel(t);
    await sleep(1400);
    const typing = await state(t);
    assert.ok(Math.abs(typing.y - H) > 20, `the page should stay put while typing (scrollY ${typing.y})`);
    await ev(t, `document.getElementById("field").blur()`);
    assert.equal(await ev(t, `window.__lab.snap()`), true);
    await sleep(900);
    assertLegible(await state(t), "snap() after blur");

    await ev(t, `window.__lab.hold(true)`);
    await wheel(t); await sleep(30); await wheel(t);
    await sleep(1400);
    const held = await state(t);
    assert.ok(Math.abs(held.y - Math.round(held.y / H) * H) > 20, `the page should stay put with a drawer open (scrollY ${held.y})`);
    await ev(t, `window.__lab.hold(false)`);
    assert.equal(await ev(t, `window.__lab.snap()`), true);
    await sleep(900);
    assertLegible(await state(t), "snap() after the drawer closed");
  } finally { await t.close(); }
});

test("A1 under reduced motion: the page is not snapped, but the sheet of the nearest stop stays solid wherever it rests", async () => {
  const t = await open({ media: { reducedMotion: true } });
  try {
    assert.equal(await ev(t, `window.__lab.reduced()`), true);
    await arriveAt(t, "hoy");
    await wheel(t); await sleep(30); await wheel(t);
    await sleep(900);
    const s = await state(t);
    assert.ok(Math.abs(s.y - H) > 20, `reduced motion does not snap (scrollY ${s.y})`);
    assert.ok(s.opacity >= 0.99, `the sheet is at ${s.opacity} between two stops under reduced motion`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("M1 with the real Lenis: the tail of a wheel flick does not cancel a trip started by the keyboard, and a wheel on a list moves the list and not the trip", async () => {
  const t = await open();
  try {
    await arriveAt(t, "hoy");
    for (let i = 0; i < 4; i++) { await wheel(t, 100); await sleep(16); }
    await ev(t, `window.__lab.clearArrivals(); window.__lab.jumpTo("sistema")`);
    for (let i = 0; i < 45; i++) { await wheel(t, Math.max(1, 90 - i * 2)); await sleep(16); }
    await sleep(1500);
    const s = await state(t);
    assert.ok(Math.abs(s.y - 4500) < 1.5, `the trip landed on SISTEMA (scrollY ${s.y})`);
    assert.deepEqual((await ev(t, `window.__lab.arrivals()`)).map((a) => [a.id, a.manual]), [["sistema", false]]);

    await arriveAt(t, "hoy");
    await ev(t, `window.__lab.clearArrivals(); window.__lab.jumpTo("sistema")`);
    await sleep(100);
    for (let i = 0; i < 5; i++) { await wheel(t, 120, 700, 450); await sleep(40); }
    await sleep(1500);
    const l = await state(t);
    assert.ok(Math.abs(l.y - 4500) < 1.5, `the trip landed although the list was scrolled (scrollY ${l.y})`);
    assert.ok((await ev(t, `document.getElementById("list").scrollTop`)) > 100, "and the list moved");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("M2 with a real tab list: End and Home on a focused tab travel to the last and the first stop (the window handler does not undo them)", async () => {
  const t = await open();
  try {
    await arriveAt(t, "hoy");
    await ev(t, `window.__lab.focusTab("hoy"); window.__lab.clearArrivals()`);
    await t.page.key("End");
    await sleep(1400);
    assert.ok(Math.abs((await state(t)).y - 4500) < 1.5, "End travelled to SISTEMA");
    assert.equal((await ev(t, `window.__lab.arrivals()`)).at(-1).id, "sistema");
    await t.page.key("Home");
    await sleep(1400);
    const s = await state(t);
    assert.ok(Math.abs(s.y - H) < 1.5, `Home travelled to the first tab, HOY (scrollY ${s.y})`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the sheet is solid in under 700 ms after the last notch of a wheel gesture (Lenis's own smoothing used to take about 900), whatever the length of the gesture", async () => {
  const t = await open();
  try {
    const took = [];
    for (const notches of [3, 6, 9, 14]) {
      await arriveAt(t, "hoy");
      for (let i = 0; i < notches; i++) { await wheel(t); await sleep(16); }
      const last = Date.now();
      let solid = null;
      while (Date.now() - last < 1500 && solid === null) {
        const s = await state(t);
        if (s.opacity >= 0.99 && Date.now() - last > 60) solid = Date.now() - last;
        else await sleep(10);
      }
      took.push([notches, solid]);
      assert.ok(solid !== null && solid <= 700, `${notches} notches: the sheet was solid ${solid} ms after the last one`);
      await sleep(900);
      assertLegible(await state(t), `${notches} notches, at rest`);
    }
    console.warn(`wheel to solid sheet (ms after the last notch): ${JSON.stringify(took)}`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});
