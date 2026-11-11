import test from "node:test";
import assert from "node:assert/strict";
import { openBuilt } from "./helpers/browser.mjs";

const open = (opts = {}) => openBuilt({ args: ["lab", "f0-dial"], file: "dist/lab-f0-dial.html", ...opts });
const ringPx = (t) => t.page.eval(`document.querySelector(".dial .face").getBoundingClientRect().width`);
const count = (t, selector) => t.page.eval(`document.querySelectorAll(${JSON.stringify(selector)}).length`);

test("A13: the ring is about 455 px wide at 1920x1080 in HOY and about 84 px in the dock; the page is clean", async () => {
  const t = await open();
  try {
    await t.page.eval(`window.__lab.place(1, 9)`);
    const hero = await ringPx(t);
    assert.ok(Math.abs(hero - 455) <= 3, `hero ring ${hero}`);
    assert.equal(await t.page.eval(`document.querySelector("#dial").classList.contains("is-docked")`), false);
    await t.page.eval(`window.__lab.place(2, 9)`);
    const dock = await ringPx(t);
    assert.ok(Math.abs(dock - 83.8) <= 2, `docked ring ${dock}`);
    assert.equal(await t.page.eval(`document.querySelector("#dial").classList.contains("is-docked")`), true);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("A13: about 629 px at 3440x1440", async () => {
  const t = await open({ width: 3440, height: 1440 });
  try {
    await t.page.eval(`window.__lab.place(1, 9)`);
    const hero = await ringPx(t);
    assert.ok(Math.abs(hero - 629) <= 3, `hero ring ${hero}`);
  } finally { await t.close(); }
});

test("the hand, the runs and the glow follow the hour and the stop", async () => {
  const t = await open();
  try {
    await t.page.eval(`window.__lab.place(3, 22)`);
    assert.equal(await t.page.eval(`document.querySelector(".hand").getAttribute("transform")`), "rotate(330.00 300 300)");
    assert.equal(await count(t, ".run-done"), 5);
    assert.equal(await count(t, ".run-now"), 1);
    assert.equal(await t.page.eval(`document.querySelector(".dial-time").textContent`), "22:00");
    assert.equal(await t.page.eval(`document.querySelector(".mark-glow").getAttribute("transform")`), "rotate(270.00 300 300)", "COLA is the 18:00 mark");
    await t.page.eval(`window.__lab.place(3, 12)`);
    assert.equal(await count(t, ".run-now"), 0, "no run is now while Gonza is busy");
    assert.equal(await count(t, ".run-done"), 16);
    assert.equal(await count(t, ".mark"), 6);
  } finally { await t.close(); }
});

test("A8: the labels come from i18n and follow the language", async () => {
  const t = await open();
  try {
    const busy = () => t.page.eval(`document.querySelector(".busy-label textPath").textContent`);
    assert.match(await busy(), /^OCUPADO/);
    await t.page.eval(`window.__lab.lang("en")`);
    assert.match(await busy(), /^BUSY/);
    assert.match(await t.page.eval(`document.querySelector(".dial-svg").getAttribute("aria-label")`), /^24-hour dial/);
  } finally { await t.close(); }
});

// The text of the dial is drawn in SVG units and the dial is scaled, so the size on the screen is units x (box / 720). A 1080p monitor
// with a browser has about 950 px of window, not 1080, and a laptop 768 or less: the sizes below are windows, not screens (review A2).
const WINDOWS = [[1366, 657], [1366, 768], [1440, 760], [1440, 900], [1536, 730], [1536, 864], [1680, 1050], [1920, 950], [1920, 1080], [2560, 1300], [3440, 1440]];

for (const [width, height] of WINDOWS) {
  test(`§4.2 + A2: every text of the dial that is drawn measures 11 px or more on the screen (12 from 2400) in the HOY pose of a ${width}x${height} window`, async () => {
    const t = await open({ width, height });
    try {
      const floor = width >= 2400 ? 12 : 11;
      await t.page.eval(`window.__lab.place(1, 9)`);
      const rows = await t.page.eval(`(() => {
        const unit = document.querySelector(".dial-svg").getBoundingClientRect().width / 720;
        const dial = document.querySelector("#dial");
        const drawn = (el) => { const cs = getComputedStyle(el); return cs.display !== "none" && cs.opacity !== "0" && cs.visibility !== "hidden"; };
        return { docked: dial.classList.contains("is-docked"), unit, rows: [...document.querySelectorAll(".dial-num, .dial-micro, .dial-time")].map((el) => ({
          cls: el.getAttribute("class"), text: el.textContent, drawn: drawn(el), px: parseFloat(getComputedStyle(el).fontSize) * unit })) };
      })()`);
      assert.equal(rows.docked, false, "none of these windows docks the dial in HOY");
      const shown = rows.rows.filter((r) => r.drawn);
      for (const r of shown) assert.ok(r.px >= floor - 0.05, `"${r.text}" (${r.cls}) is ${r.px.toFixed(2)} px, under ${floor}`);
      for (const text of ["00", "06", "12", "18"]) assert.ok(shown.some((r) => r.text === text), `the numeral ${text} is always drawn`);
      assert.ok(shown.some((r) => r.cls === "dial-time"), "and the time");
      if (width >= 1920 && height >= 1000) assert.equal(shown.length, 11, "a big window keeps all eleven texts");
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}
