// The acceptance matrix, part 4: A18 measured on what the MOUNTED page paints (spec A18: every text of the sheet, the mono of 11 px included, 4.5:1 over glass and sky;
// A17's ground for the HUD and the bar). For each hour 0 to 23, in Tinta and in Papel, at 1920x1080 and 1440x900: the HUD, the stops bar, the sheet of HOY and of
// PROCESOS and the bottom bar (every run of text against the window captured with its glyphs made transparent), and the labels of the dial at its big pose (HOY)
// and its docked one (PROCESOS) against the sky, the pool, the light and the halo they sit on. Mike's sweep of APRENDER (moments.aprender.browser) and the lab sweeps
// of the other moments are not repeated; COLA and SISTEMA are measured here at seven hours so that every stop is on the mounted page at least once.
import test from "node:test";
import assert from "node:assert/strict";
import { openPage, ev, press, travel, pageContrast, dialContrast } from "./helpers/acceptance.mjs";

const NEEDS = Number(process.env.CONTRAST_NEEDS) || 4.5;
const HOURS = Array.from({ length: 24 }, (_, i) => i);
const OPENS = `!!document.querySelector("#app .sheet-host h1") && document.fonts.status === "loaded" && !!window.__app.model()`;

const armed = new WeakSet();
async function at(t, hour, theme, extra = "") {
  // A first visit every time: the page remembers when it was last seen (it writes panel.lastVisit as it unloads) and from the second visit on HOY has no
  // "since your last visit" rows, the ones with the hour (.s-hour). So every load starts without that key.
  if (!armed.has(t)) { armed.add(t); await t.page.send("Page.addScriptToEvaluateOnNewDocument", { source: `try { localStorage.removeItem("panel.lastVisit"); } catch (e) {}` }); }
  await t.page.goto(`${t.url.split("?")[0]}?debug=1&clock=${hour}&lang=es&theme=${theme}${extra}#hoy`);       // the address names the stop: without it the page opens where it was left
  await t.page.waitFor(OPENS, { timeout: 12000 });
  await t.page.wait(350);                                  // the entrance of the rows (150 ms) and the fade of the sheet are over
}

/** The sentences for what is under NEEDS in one measurement of the page. */
function lowRuns(rows, label) {
  return rows.filter((r) => r.ratio < NEEDS).map((r) => `${label} ${r.part} "${r.text}" (${r.cls.trim().split(/\s+/)[0]}, ${Math.round(r.fs)}px) ${r.ratio.toFixed(2)}:1`);
}
function lowLabels(rows, label) {
  return rows.filter((r) => r.min < NEEDS).map((r) => `${label} dial "${r.text}" ${r.min.toFixed(2)}:1`);
}

for (const [width, height] of [[1920, 1080], [1440, 900]]) {
  for (const theme of ["dark", "light"]) {
    test(`A18 on the mounted page at ${width}x${height} in ${theme === "dark" ? "Tinta" : "Papel"}: the HUD, the stops bar, the sheet${theme === "dark" ? " (HOY and PROCESOS)" : " (HOY)"}, the bottom bar and the dial keep ${NEEDS}:1 at every hour from 0 to 23`, async () => {
      const t = await openPage({ query: `?debug=1&clock=9&lang=es&theme=${theme}`, width, height });
      try {
        const fails = [];
        for (const hour of HOURS) {
          const label = `${hour}:00`;
          await at(t, hour, theme);
          const hoy = await pageContrast(t);
          assert.ok((await ev(t, `document.querySelectorAll(".s-hour").length`)) > 0, `${label}: HOY shows its "since your last visit" rows with their hours, so the sweep reads them`);
          assert.ok(hoy.length >= 60, `${label}: the HUD, the bar and the sheet of HOY are measured (${hoy.length} runs)`);
          fails.push(...lowRuns(hoy, `${label} hoy`));
          const big = await dialContrast(t);
          assert.ok(big.length >= 5 && big.every((r) => r.ink > 10 && r.touching > 10), `${label}: the measure sees the glyphs of the dial (${big.map((r) => r.text + ":" + r.ink).join(" ")})`);
          fails.push(...lowLabels(big, `${label} hoy`));
          if (theme === "dark") {
            await travel(t, "procesos");
            await t.page.wait(250);
            const proc = await pageContrast(t);
            assert.ok(proc.length >= 25, `${label}: the sheet of PROCESOS is measured (${proc.length} runs)`);
            fails.push(...lowRuns(proc, `${label} procesos`));
            fails.push(...lowLabels(await dialContrast(t), `${label} procesos`));
          }
        }
        // the same sentence at several hours is one finding: say it once with the hours it happens at
        const grouped = new Map();
        for (const f of fails) { const m = /^(\d+):00 (\S+) (.*) ([\d.]+):1$/.exec(f); const k = m ? `${m[2]} ${m[3]}` : f; grouped.set(k, [...(grouped.get(k) || []), m ? `${m[1]}h ${m[4]}` : ""]); }
        assert.deepEqual([...grouped].map(([k, v]) => `${k}: ${v.join(", ")}`), []);
        assert.deepEqual(t.page.problems(), []);
      } finally { await t.close(); }
    });
  }
}

test(`A18 on the mounted page: COLA and SISTEMA keep ${NEEDS}:1 in Tinta at 1920x1080, hours 0, 6, 9, 12, 15, 18 and 21`, async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: 1920, height: 1080 });
  try {
    const fails = [];
    for (const hour of [0, 6, 9, 12, 15, 18, 21]) {
      await at(t, hour, "dark");
      for (const id of ["cola", "sistema"]) {
        await travel(t, id);
        await t.page.wait(300);
        const rows = await pageContrast(t);
        assert.ok(rows.length >= 25, `${id} at ${hour}:00 is measured (${rows.length} runs)`);
        fails.push(...lowRuns(rows, `${hour}:00 ${id}`));
      }
    }
    assert.deepEqual(fails, []);
    assert.deepEqual(t.page.problems().filter((p) => !/status of 404.*\/api\/(shot|job)/.test(p.text)), []);
  } finally { await t.close(); }
});

test("A18: the measure is not vacuous: a text whose colour is the colour of its ground, or a dial label stripped of its halo, is reported under 4.5:1", async () => {
  const t = await openPage({ query: "?debug=1&clock=13&lang=es", width: 1920, height: 1080 });
  try {
    await t.page.waitFor(OPENS);
    await t.page.wait(400);
    const named = (rows) => new Set(lowRuns(rows, "control").map((s) => s.replace(/ [\d.]+:1$/, "")));
    const clean = named(await pageContrast(t));
    await ev(t, `document.querySelector(".hud-brand").style.setProperty("color", "#1c2234", "important"); document.querySelector(".bar-label").style.setProperty("color", "#2a3347", "important"); document.querySelector("#app .sheet-host h1").style.setProperty("color", "#151c2e", "important")`);
    const broken = [...named(await pageContrast(t))].filter((s) => !clean.has(s));
    assert.ok(broken.some((s) => /hud/.test(s)) && broken.some((s) => /bar/.test(s)) && broken.some((s) => /sheet/.test(s)), `three texts made dark on dark are all reported: ${broken.join(" | ")}`);
    await ev(t, `document.querySelector(".hud-brand").style.removeProperty("color"); document.querySelector(".bar-label").style.removeProperty("color"); document.querySelector("#app .sheet-host h1").style.removeProperty("color")`);
    // the dial: labels of a dark slate over the sky of 13:00 and the halo (dark too) behind them
    const good = await dialContrast(t);
    assert.ok(good.every((r) => r.min >= NEEDS), `the dial at 13:00 is fine first (${good.map((r) => r.text + " " + r.min.toFixed(1)).join(", ")})`);
    await ev(t, `for (const el of document.querySelectorAll("#dial text")) el.style.setProperty("fill", "#3a4258", "important")`);
    const dial = await dialContrast(t);
    assert.ok(dial.length >= 5 && dial.every((r) => r.min < NEEDS), `a dial of dark labels is reported label by label (${dial.map((r) => r.text + " " + r.min.toFixed(1)).join(", ")})`);
  } finally { await t.close(); }
});
