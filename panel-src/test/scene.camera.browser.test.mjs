// The camera on the mounted page: a long trip mounts the moment of the stop it aims at and no other (spec §3.3, "sin poses intermedias"; the cost of the
// moments mounted on the way was the cause of ACEPTACION A3, first visit, and A7). The sequence of moments the sheet showed is read from the mutations of
// `data-moment` on #app, which the shell sets at every mount.
import test from "node:test";
import assert from "node:assert/strict";
import { openPage, press, arrived, ev, DIGIT } from "./helpers/acceptance.mjs";

const WATCH = `window.__moments = []; new MutationObserver(() => { const m = document.getElementById("app").dataset.moment; if (__moments.at(-1) !== m) __moments.push(m); }).observe(document.getElementById("app"), { attributes: true, attributeFilter: ["data-moment"] });`;

test("a trip across several stops mounts the stop it leaves and the one it arrives at, and none of those it flies over", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: 1920, height: 1080 });
  try {
    await t.page.waitFor(`!!window.__app.model()`);
    await t.page.wait(400);
    await ev(t, WATCH);
    for (const [from, to] of [["hoy", "sistema"], ["sistema", "procesos"], ["procesos", "aprender"], ["aprender", "hoy"], ["hoy", "cola"]]) {
      if ((await ev(t, `document.getElementById("app").dataset.moment`)) !== from) { await press(t, DIGIT[from]); await arrived(t, from, 6000); await t.page.wait(300); }
      await ev(t, `__moments.length = 0`);
      await press(t, DIGIT[to]);
      await arrived(t, to, 6000);
      await t.page.wait(300);
      assert.deepEqual(await ev(t, `__moments.slice()`), [to], `${from} to ${to}: the sheet showed ${JSON.stringify(await ev(t, `__moments.slice()`))}`);
      const end = await ev(t, `({ y: scrollY, vh: innerHeight, opacity: document.getElementById("app").style.opacity })`);
      assert.ok(Math.abs(end.y / end.vh - Math.round(end.y / end.vh)) * end.vh < 1.5, `${to}: on its mark`);
      assert.equal(end.opacity, "", `${to}: the sheet is solid`);
    }
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("while a long trip is in the air the sheet is invisible between the two ends, and a trip of one floor still swaps at the half", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: 1920, height: 1080 });
  try {
    await t.page.waitFor(`!!window.__app.model()`);
    await t.page.wait(400);
    await ev(t, `window.__seen = []; (function f() { const a = document.getElementById("app"); __seen.push([scrollY / innerHeight, a.style.opacity === "" ? 1 : Number(a.style.opacity), a.dataset.moment]); requestAnimationFrame(f); })()`);
    await ev(t, `__seen.length = 0`);
    await press(t, DIGIT.sistema);
    await arrived(t, "sistema", 6000);
    const long = await ev(t, `__seen.slice()`);
    const middle = long.filter(([pos]) => pos > 1.55 && pos < 4.45);
    assert.ok(middle.length >= 2, `${middle.length} frames in the middle of the trip`);
    assert.ok(middle.every(([, opacity]) => opacity === 0), `the sheet was visible in the middle: ${JSON.stringify(middle.filter(([, o]) => o > 0).slice(0, 3))}`);
    assert.ok(long.every(([pos, , moment]) => (moment === "hoy" || moment === "sistema")), "only the two ends");
    await t.page.wait(300);
    await ev(t, `__seen.length = 0`);
    await press(t, DIGIT.aprender);
    await arrived(t, "aprender", 6000);
    const adjacent = await ev(t, `__seen.slice()`);
    assert.ok(adjacent.some(([pos]) => pos > 4.6) && adjacent.some(([pos]) => pos < 4.4), "the recorder saw both halves of the trip");
    assert.ok(adjacent.filter(([pos]) => pos > 4.6).every(([, , moment]) => moment === "sistema"), "in the first half the sheet is still SISTEMA's");
    assert.ok(adjacent.filter(([pos]) => pos < 4.4).every(([, , moment]) => moment === "aprender"), "and past the half it is APRENDER's");
  } finally { await t.close(); }
});

test("a sheet that is fading on a trip does not take clicks (without the cost of inert on its whole subtree), and it takes them again on arrival", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: 1920, height: 1080 });
  try {
    await t.page.waitFor(`!!window.__app.model()`);
    await t.page.wait(400);
    // a button of the sheet counts its clicks; the page clicks it by hand at each frame of the trip where the sheet is below half
    await ev(t, `(() => {
      const button = document.querySelector("#app button");
      window.__probe = { clicks: 0, faded: 0, inertWrites: 0 };
      button.addEventListener("click", () => { __probe.clicks++; });
      const app = document.getElementById("app");
      new MutationObserver(() => { __probe.inertWrites++; }).observe(app, { attributes: true, attributeFilter: ["inert"] });
      (function f() { const a = document.getElementById("app"); if (__app.engine.S.jump && Number(a.style.opacity || 1) < 0.5) { __probe.faded++; button.click(); } requestAnimationFrame(f); })();
    })()`);
    await press(t, DIGIT.sistema);
    await arrived(t, "sistema", 6000);
    await t.page.wait(300);
    const probe = await ev(t, `({ ...__probe })`);
    assert.ok(probe.faded >= 3, `the sheet was below half for ${probe.faded} frames of the trip`);
    assert.equal(probe.clicks, 0, `${probe.clicks} clicks reached a control of the sheet while it was fading`);
    assert.equal(probe.inertWrites, 0, "and the trip did not touch the inert attribute of the whole sheet");
    const end = await ev(t, `({ inert: document.getElementById("app").inert, opacity: document.getElementById("app").style.opacity, moment: document.getElementById("app").dataset.moment })`);
    assert.deepEqual(end, { inert: false, opacity: "", moment: "sistema" });
    const live = await ev(t, `(() => { const b = document.querySelector("#app button"); let n = 0; b.addEventListener("click", () => n++); b.click(); return n; })()`);
    assert.equal(live, 1, "on arrival the sheet takes clicks");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});
