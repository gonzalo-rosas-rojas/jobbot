import test from "node:test";
import assert from "node:assert/strict";
import { openBuilt } from "./helpers/browser.mjs";

const open = (opts = {}) => openBuilt({ args: ["lab", "f0-frame"], file: "dist/lab-f0-frame.html", ...opts });
const ev = (t, expr) => t.page.eval(expr);
const SNAP = `{ controls: { stopped: false, window_open: false, next_window: "2026-10-07 18:00", last_run_at: "2026-10-07T05:00:00Z", engine: "recent" } }`;
const COUNTS = `{ procesos: 74, cola: 2, aprender: 12, sistema: "recent" }`;

for (const [width, height] of [[1920, 1080], [3440, 1440]]) {
  test(`the HUD is 56 px, centres the stops bar and keeps every part inside the gutters at ${width}x${height}`, async () => {
    const t = await open({ width, height });
    try {
      await ev(t, `window.__lab.counts(${COUNTS}); window.__lab.status(${SNAP}); window.__lab.go("hoy", 9)`);
      const m = await ev(t, `(() => {
        const r = (sel) => { const b = document.querySelector(sel).getBoundingClientRect(); return { left: b.left, right: b.right, top: b.top, bottom: b.bottom }; };
        const status = document.querySelector(".hud-status");
        return { hud: r("#hud"), stops: r("#stops"), brand: r(".hud-brand"), clock: r(".hud-clock"), status: r(".hud-status"), overflow: status.scrollWidth - status.clientWidth, vw: innerWidth };
      })()`);
      assert.equal(Math.round(m.hud.bottom - m.hud.top), 56);
      assert.ok(Math.abs((m.stops.left + m.stops.right) / 2 - m.vw / 2) <= 2, `the stops bar is centred (${(m.stops.left + m.stops.right) / 2} vs ${m.vw / 2})`);
      assert.ok(m.brand.left >= 23 && m.brand.left <= 65, `the mark starts at the gutter (${m.brand.left})`);
      assert.ok(m.vw - m.clock.right >= 23 && m.vw - m.clock.right <= 65, `the clock ends at the gutter (${m.vw - m.clock.right})`);
      assert.ok(m.status.right <= m.clock.left, "the status does not run under the clock");
      assert.ok(m.overflow <= 1, `the status text fits its column (${m.overflow} px too wide)`);
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}

// Review focus 3: the narrow desktop (a 1024 laptop, a 1280 window, a 1920 screen zoomed to 200 %) must not make the HUD's parts run into
// each other. Every part that is on screen keeps clear of the next one and inside the window, and the status wraps instead of being cut.
for (const [width, height] of [[1024, 768], [1280, 720], [1440, 900], [1600, 900], [960, 540], [800, 600]]) {
  test(`the HUD's parts keep clear of each other and inside the window at ${width}x${height}`, async () => {
    const t = await open({ width, height });
    try {
      await ev(t, `window.__lab.counts(${COUNTS}); window.__lab.status(${SNAP}); window.__lab.go("procesos", 0.43)`);
      const m = await ev(t, `(() => {
        const parts = [".hud-brand", ".hud-stop", ".stops-list", ".hud-status", ".hud-search", ".hud-clock"].map((sel) => {
          const el = document.querySelector(sel);
          const shown = !el.hidden && getComputedStyle(el).display !== "none";
          const b = el.getBoundingClientRect();
          return { sel, shown, left: b.left, right: b.right, top: b.top, bottom: b.bottom, clipped: el.scrollWidth - el.clientWidth };
        }).filter((p) => p.shown);
        return { parts, vw: innerWidth, hscroll: document.documentElement.scrollWidth - innerWidth, hudBottom: document.querySelector("#hud").getBoundingClientRect().bottom };
      })()`);
      const sorted = [...m.parts].sort((a, b) => a.left - b.left);
      for (let i = 1; i < sorted.length; i++) assert.ok(sorted[i].left - sorted[i - 1].right >= 8, `${sorted[i - 1].sel} (to ${Math.round(sorted[i - 1].right)}) and ${sorted[i].sel} (from ${Math.round(sorted[i].left)}) touch or overlap`);
      for (const p of m.parts) {
        assert.ok(p.left >= 0 && p.right <= m.vw, `${p.sel} sticks out of the window (${Math.round(p.left)}..${Math.round(p.right)} of ${m.vw})`);
        assert.ok(p.top >= 0 && p.bottom <= 56, `${p.sel} leaves the 56 px strip (${Math.round(p.top)}..${Math.round(p.bottom)})`);
        assert.ok(p.clipped <= 1, `${p.sel} is cut (${p.clipped} px of text hidden)`);
      }
      assert.ok(m.hscroll <= 0, `no sideways scroll (${m.hscroll})`);
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}

test("the stops bar is a tab list of five: roving tabindex, one selected, counts as text, the engine dot, the caption", async () => {
  const t = await open();
  try {
    await ev(t, `window.__lab.counts(${COUNTS}); window.__lab.go("procesos", 0.43)`);
    const s = await ev(t, `(() => {
      const tabs = [...document.querySelectorAll('#stops [role="tab"]')];
      const dot = document.querySelector("#tab-sistema .stop-dot");
      return {
        list: document.querySelector('#stops [role="tablist"]').getAttribute("aria-label"),
        ids: tabs.map((x) => x.dataset.stop), selected: tabs.filter((x) => x.getAttribute("aria-selected") === "true").map((x) => x.dataset.stop),
        tabindex: tabs.map((x) => x.tabIndex), keys: tabs.map((x) => x.querySelector(".stop-key").textContent),
        counts: tabs.map((x) => x.querySelector(".stop-count").hidden ? "" : x.querySelector(".stop-count").textContent),
        label: document.querySelector("#tab-procesos").getAttribute("aria-label"),
        here: [...document.querySelectorAll(".stop-here")].filter((x) => getComputedStyle(x).display !== "none").map((x) => x.closest(".stop").dataset.stop),
        dot: { hidden: dot.hidden, state: dot.dataset.state, bg: getComputedStyle(dot).backgroundColor },
        gold: getComputedStyle(document.querySelector("#tab-procesos")).color, hudName: document.querySelector(".hud-stop").textContent,
        night: getComputedStyle(document.querySelector(".stops-night")).display, controls: document.querySelector("#tab-hoy").getAttribute("aria-controls"),
      };
    })()`);
    assert.equal(s.list, "Paradas");
    assert.deepEqual(s.ids, ["hoy", "procesos", "cola", "aprender", "sistema"]);
    assert.deepEqual(s.selected, ["procesos"]);
    assert.deepEqual(s.tabindex, [-1, 0, -1, -1, -1]);
    assert.deepEqual(s.keys, ["1", "2", "3", "4", "5"]);
    assert.deepEqual(s.counts, ["", "74", "2", "12", ""]);
    assert.equal(s.label, "Procesos. 74 procesos vivos");
    assert.deepEqual(s.here, ["procesos"], "'estás aquí' sits under the active stop only");
    assert.deepEqual(s.dot, { hidden: false, state: "recent", bg: "rgb(233, 239, 247)" });
    assert.equal(s.gold, "rgb(251, 192, 36)");
    assert.equal(s.hudName, "Procesos");
    assert.equal(s.night, "none");
    assert.equal(s.controls, "app");

    await ev(t, `window.__lab.go("prologo", 22); window.__lab.counts({ cola: 0, sistema: "stale" })`);
    const p = await ev(t, `(() => ({
      selected: [...document.querySelectorAll('#stops [aria-selected="true"]')].length, tabindex: [...document.querySelectorAll('#stops [role="tab"]')].map((x) => x.tabIndex),
      night: getComputedStyle(document.querySelector(".stops-night")).display, text: document.querySelector(".stops-night").textContent, hudName: document.querySelector(".hud-stop").textContent,
      cola: document.querySelector("#tab-cola .stop-count").hidden, dot: { state: document.querySelector("#tab-sistema .stop-dot").dataset.state, bg: getComputedStyle(document.querySelector("#tab-sistema .stop-dot")).backgroundColor },
    }))()`);
    assert.equal(p.selected, 0, "in the prologue no stop is lit");
    assert.deepEqual(p.tabindex, [0, -1, -1, -1, -1], "the list still has a tab stop");
    assert.equal(p.night, "block");
    assert.equal(p.text, "Anoche");
    assert.equal(p.hudName, "Anoche");
    assert.equal(p.cola, true, "a zero says nothing");
    assert.deepEqual(p.dot, { state: "stale", bg: "rgba(0, 0, 0, 0)" }, "the engine dot is hollow when it is behind");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("a click, an arrow, Home and End select; Ctrl K and ? atajos are wired; the HUD says the window, the run and the hour", async () => {
  const t = await open();
  try {
    await ev(t, `window.__lab.counts(${COUNTS}); window.__lab.status(${SNAP}); window.__lab.go("hoy", 9)`);
    await t.page.click("#tab-cola");
    await ev(t, `document.querySelector("#tab-hoy").focus()`);
    await t.page.key("ArrowRight");
    await t.page.key("End");
    await t.page.key("ArrowRight");                                       // wraps to hoy
    const got = await ev(t, `({ seen: window.__lab.seen(), focus: document.activeElement.dataset.stop })`);
    assert.deepEqual(got.seen.picked, ["cola", "procesos", "sistema", "hoy"]);
    assert.equal(got.focus, "hoy");
    await t.page.click(".hud-search");
    await t.page.click(".bar-help");
    assert.deepEqual(await ev(t, `[window.__lab.seen().search, window.__lab.seen().help]`), [1, 1]);
    const txt = await ev(t, `({ status: document.querySelector(".hud-status").textContent, clock: document.querySelector(".hud-clock").textContent, kbd: [...document.querySelectorAll(".hud-search kbd")].map((x) => x.textContent), bar: document.querySelector(".bar-help").textContent })`);
    assert.deepEqual(txt, { status: "Cerrada, abre 18:00 · Corrida hace 7 h", clock: "09:00", kbd: ["Ctrl", "K"], bar: "?Atajos" });
    await ev(t, `window.__lab.lang("en")`);
    const en = await ev(t, `({ status: document.querySelector(".hud-status").textContent, bar: document.querySelector(".bar-help").textContent, here: document.querySelector("#tab-hoy .stop-here").textContent, tab: document.querySelector("#tab-procesos").getAttribute("aria-label"), list: document.querySelector(".stops-list").getAttribute("aria-label") })`);
    assert.deepEqual(en, { status: "Closed, opens 18:00 · Last run 7 h ago", bar: "?Shortcuts", here: "You are here", tab: "Processes. 74 live processes", list: "Stops" });
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("icons draw from the sprite: every symbol of the closed list resolves to a box, with the 1.5 stroke", async () => {
  const t = await open();
  try {
    const out = await ev(t, `(() => { const bad = []; const ids = [...document.querySelectorAll('symbol[id^="i-"]')].map((s) => s.id.slice(2)); for (const id of ids) { const b = window.__lab.icon(id); if (!(b.w > 0 || b.h > 0)) bad.push(id); } return { bad, n: ids.length, stroke: getComputedStyle(document.querySelector("symbol#i-check")).strokeWidth }; })()`);
    assert.deepEqual(out.bad, []);
    assert.equal(out.n, 33);
    assert.equal(out.stroke, "1.5px");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("under 760 px there is no stage: the sky and the dial are gone, the HUD is a sticky two-row strip and the bar sits in the page", async () => {
  const t = await open({ width: 390, height: 844 });
  try {
    await ev(t, `window.__lab.counts(${COUNTS}); window.__lab.go("cola", 18)`);
    await ev(t, `window.__lab.go("sistema", 5.5)`);
    const reach = await ev(t, `document.querySelector("#tab-sistema").getBoundingClientRect().right`);
    assert.ok(reach <= 390, `the active tab is scrolled into view in the strip (right edge ${reach})`);
    await ev(t, `window.__lab.go("cola", 18)`);
    const m = await ev(t, `(() => {
      const d = (sel) => getComputedStyle(document.querySelector(sel)).display;
      const pos = (sel) => getComputedStyle(document.querySelector(sel)).position;
      const hud = document.querySelector("#hud").getBoundingClientRect();
      return { sky: d("#sky"), dial: d("#dial"), hudPos: pos("#hud"), stage: pos("#stage"), bar: pos("#bar"), key: d(".stop-key"), icon: d(".hud-search .icon"), kbd: d(".hud-search kbd"), status: d(".hud-status"),
        hudHeight: hud.height, hscroll: document.documentElement.scrollWidth - innerWidth,
        underlineRoom: document.querySelector(".hud-center").getBoundingClientRect().bottom - document.querySelector("#tab-cola").getBoundingClientRect().bottom };
    })()`);
    assert.ok(m.underlineRoom >= 4, `the strip scrolls sideways, which clips what sticks out below it: the active tab's underline needs room (${m.underlineRoom} px)`);
    assert.deepEqual({ sky: m.sky, dial: m.dial, hudPos: m.hudPos, stage: m.stage, bar: m.bar, key: m.key, icon: m.icon, kbd: m.kbd, status: m.status },
      { sky: "none", dial: "none", hudPos: "sticky", stage: "relative", bar: "static", key: "none", icon: "block", kbd: "none", status: "none" });
    assert.ok(m.hudHeight > 56 && m.hudHeight < 120, `two rows (${m.hudHeight})`);
    assert.ok(m.hscroll <= 0, `no sideways scroll at 390 px (${m.hscroll})`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});
