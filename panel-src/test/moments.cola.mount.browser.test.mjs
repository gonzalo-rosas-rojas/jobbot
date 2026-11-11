// Arriving at COLA must not cost a long frame (A7's spirit, at the speed of a person's own machine): the first visit to the stop mounts the
// moment, asks for the capture of the dry run and paints the plane. On the heaviest scenario, with no CPU throttling, no animation frame of the
// 1.5 s after the key may be longer than 60 ms. It runs in the real shell over the demo's fictional data (the demo's "server" draws the capture
// on a canvas, which is what used to run as one 70-100 ms task, the anonymous setTimeout of a profile), and watches with the page's own frames
// (a requestAnimationFrame recorder), with the Long Animation Frames and the long tasks Chrome reports (PerformanceObserver) for the why.
// A machine busy with other work can show a long frame that is not the page's: up to three tries, and one clean try is the proof (a page
// that always paints in one long task fails all three).
import test from "node:test";
import assert from "node:assert/strict";
import { openBuilt } from "./helpers/browser.mjs";

const PAGE = "dist/demo-cola-mount.html";
const LIMIT = 60;                  // ms: the longest frame we accept after the key
const WINDOW = 1500;               // ms after the key
const ev = (t, expr) => t.page.eval(expr);

const WATCH = `(() => {
  window.__w = { t0: 0, frames: [], loaf: [], tasks: [], last: 0 };
  const seen = (list, e, extra) => list.push({ at: Math.round(e.startTime), dur: Math.round(e.duration), ...extra });
  try { new PerformanceObserver((l) => l.getEntries().forEach((e) => seen(__w.loaf, e, { block: Math.round(e.blockingDuration || 0), scripts: (e.scripts || []).map((s) => (s.invoker || "") + " " + Math.round(s.duration)) }))).observe({ type: "long-animation-frame" }); } catch (e) { /* not in this Chrome */ }
  try { new PerformanceObserver((l) => l.getEntries().forEach((e) => seen(__w.tasks, e, {}))).observe({ type: "longtask" }); } catch (e) { /* not in this Chrome */ }
  (function f(now) { if (__w.t0) { if (__w.last) __w.frames.push({ at: Math.round(now), dt: Math.round(now - __w.last) }); __w.last = now; } requestAnimationFrame(f); })(performance.now());
})()`;

async function arrive() {
  const t = await openBuilt({ args: ["demo", "--out", PAGE], file: PAGE, query: "?debug=1&clock=9&lang=es&scenario=heavy", width: 1920, height: 1080 });
  try {
    await t.page.waitFor(`!!(window.__app && window.__app.model && window.__app.model())`, 15000);
    await t.page.wait(1500);                                            // the boot is over: the first visit to COLA is what is measured
    await ev(t, WATCH);
    await ev(t, `__w.t0 = performance.now()`);
    await t.page.key("3", { code: "Digit3", text: "3" });
    await t.page.waitFor(`document.querySelector(".cola .plane-wrap")?.dataset.state === "ok" && document.querySelector(".cola .plane img.shot")?.complete === true`, 8000);
    await t.page.wait(WINDOW);
    const r = await ev(t, `(() => { const t0 = __w.t0; const rel = (l) => l.filter((x) => x.at >= t0 - 20).map((x) => ({ ...x, at: x.at - Math.round(t0) }));
      return { frames: rel(__w.frames), loaf: rel(__w.loaf), tasks: rel(__w.tasks), mounted: !!document.querySelector(".cola .mega"), plane: document.querySelector(".cola .plane-wrap")?.dataset.state }; })()`);
    return { ...r, problems: t.page.problems() };
  } finally { await t.close(); }
}

test("arriving at COLA on the heaviest scenario with a CPU at full speed: no frame longer than 60 ms in the 1.5 s after the key", async () => {
  const tries = [];
  for (let i = 0; i < 3; i++) {
    const r = await arrive();
    assert.equal(r.mounted, true, "COLA is on screen");
    assert.equal(r.plane, "ok", "the capture arrived and was painted inside the window (it is part of what is measured)");
    assert.ok(r.frames.length >= 30, `control: the recorder saw ${r.frames.length} frames`);
    assert.deepEqual(r.problems, []);
    const worst = Math.max(...r.frames.map((f) => f.dt));
    const tasks = r.tasks.filter((x) => x.dur > LIMIT);
    tries.push({ worst, long: r.frames.filter((f) => f.dt > LIMIT), tasks, loaf: r.loaf.filter((x) => x.dur > LIMIT) });
    if (worst <= LIMIT && tasks.length === 0) {
      console.warn(`COLA arrival: the longest frame of the 1.5 s was ${worst} ms on try ${i + 1} (limit ${LIMIT}); before the capture was painted in slices and its shadows brought in one by one it was 83 to 133 ms`);
      return;
    }
  }
  assert.fail("every try had a long frame: " + JSON.stringify(tries));
});
