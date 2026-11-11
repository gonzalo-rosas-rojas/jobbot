// A big base makes the first answer of the server slow (7 to 12 s on his real one). The page must never look broken while it waits: the sky, the HUD and
// the dial are painted at once (A1), the sheet says in calm words that it is loading (no zero, no figure that is not known: A16), a second quiet line
// comes after three seconds, a failure is a sentence with a button to try again, and when the answer comes the sheet fills with no flash in between.
// The refresh loop behind it is held to its own rules here too: one snapshot read in flight at a time, and a hidden page asks nothing.
// The demo's made-up server takes `?lag=<ms>` (every read of the snapshot waits that long) and counts its reads in window.__DEMO__.stats;
// `?poll=<ms>` (with ?debug=1) shortens the refresh loop so a test does not wait half a minute for it.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, mkdirSync } from "node:fs";
import path from "node:path";
import { openPage } from "./helpers/page.mjs";

const rich = JSON.parse(readFileSync(new URL("./fixtures/snapshot.rich.json", import.meta.url), "utf8"));
const SHOTS = process.env.LOADING_SHOTS || "";
const ev = (t, expr) => t.page.eval(expr);
const press = async (t, k, code) => {
  await t.page.send("Input.dispatchKeyEvent", { type: "keyDown", key: k, code: code || (/^\d$/.test(k) ? "Digit" + k : "Key" + k.toUpperCase()), text: k });
  await t.page.send("Input.dispatchKeyEvent", { type: "keyUp", key: k, code: code || (/^\d$/.test(k) ? "Digit" + k : "Key" + k.toUpperCase()) });
};
/** A finger or a mouse on the thing where it is: no scrollIntoView first (on the desktop the window's scroll IS the camera, and moving it would travel the page). */
const tap = async (t, selector) => {
  const at = await ev(t, `(() => { const r = document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`);
  for (const type of ["mouseMoved", "mousePressed", "mouseReleased"]) await t.page.send("Input.dispatchMouseEvent", { type, x: at.x, y: at.y, button: "left", clickCount: 1 });
};
const BAD = /\b(null|undefined|NaN)\b|\[object|⟦/;

/** What the sheet shows right now, as a person would see it. */
const sheet = (t) => ev(t, `(() => {
  const q = (s) => document.querySelector(s); const vis = (el) => !!el && el.getClientRects().length > 0 && getComputedStyle(el).visibility !== "hidden";
  const loading = q(".sheet-loading"), more = q(".loading-more"), app = q("#app"), host = q(".sheet-host");
  return { at: Math.round(performance.now()), loading: vis(loading), title: q(".loading-title")?.textContent.trim() ?? null, eyebrow: q(".loading-eyebrow")?.textContent.trim() ?? null, more: vis(more) ? more.textContent.trim() : null,
    role: loading?.getAttribute("role"), live: loading?.getAttribute("aria-live"), busy: app.getAttribute("aria-busy"), host: vis(host), moment: app.dataset.moment, appText: app.innerText,
    sky: (() => { const c = q("#sky"); if (!c || !vis(c) || c.width === 0) return null; const b = document.createElement("canvas"); b.width = c.width; b.height = c.height; return c.toDataURL() !== b.toDataURL(); })(),
    counts: [...document.querySelectorAll("#stops .stop-count")].filter(vis).map((x) => x.textContent), status: vis(q(".hud-status")) ? q(".hud-status").textContent : "", title2: document.title, newTask: vis(q(".flat-new")),
    banner: vis(q(".sheet-banner")) ? q(".sheet-banner").textContent.trim() : null, retry: vis(q(".sheet-retry")) ? q(".sheet-retry").textContent.trim() : null,
    mono: [...document.querySelectorAll("#app *")].filter((e) => vis(e) && [...e.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim()) && getComputedStyle(e).fontFamily.toLowerCase().includes("mono") && parseFloat(getComputedStyle(e).fontSize) < 11).map((e) => e.className) };
})()`);

// ---- the sheet says it is loading, then fills ------------------------------------------------------------------------------------------------

for (const [w, h] of [[1920, 1080], [390, 844]]) {
  test(`${w}: with a snapshot that takes six seconds the sky and the loading state are there inside one second, with no zero; a quiet second line at three seconds; the sheet fills with no flash`, async () => {
    const t = await openPage({ query: "?debug=1&clock=9&lang=es&lag=6000", width: w, height: h, wait: false });
    try {
      await t.page.waitFor(`!!document.querySelector(".sheet-loading") && document.querySelector(".sheet-loading").getClientRects().length > 0`, { timeout: 4000 });
      let s = await sheet(t);
      assert.ok(s.at < 1000, `inside one second of the page opening: ${s.at} ms`);
      assert.equal(s.loading, true);
      assert.equal(s.title, "Cargando tus datos…");
      assert.equal(s.eyebrow?.toLowerCase(), "hoy", "it says whose stop it is");
      assert.deepEqual([s.role, s.live, s.busy], ["status", "polite", "true"], "a polite status, and the sheet is busy");
      assert.equal(s.more, null, "the second line is not there yet");
      assert.equal(s.host, false, "the skeleton of the moment is not what he sees");
      assert.doesNotMatch(s.appText, /\d/, "no zero and no figure on the sheet: " + JSON.stringify(s.appText));
      assert.doesNotMatch(s.appText, BAD);
      assert.deepEqual(s.counts, [], "the stops carry no count that is not known");
      assert.equal(s.status, "", "the HUD says nothing it cannot know");
      assert.equal(s.title2, "jobbot", "the demo names itself in the tab (forja PO-09); the real page says Panel (page.real, acceptance A1)");
      assert.equal(s.newTask, false, "no 'Nueva tarea' before there is anything to add to");
      assert.deepEqual(s.mono, [], "mono text below 11 px");
      if (w >= 760) assert.equal(s.sky, true, "the sky is painted without waiting for the data");
      assert.equal(await ev(t, `document.querySelectorAll("#dial *").length > 0 || getComputedStyle(document.getElementById("dial")).display === "none"`), true);
      assert.doesNotMatch(await ev(t, `document.body.innerText`), BAD);
      if (SHOTS) { mkdirSync(SHOTS, { recursive: true }); await t.page.shot(path.join(SHOTS, `loading-${w}-tinta.png`)); }
      // three seconds: a second, quiet line, inside the same status
      await t.page.waitFor(`performance.now() > 3400`, { timeout: 5000 });
      s = await sheet(t);
      assert.equal(s.more, "Tu base es grande; sigo cargando.");
      assert.equal(s.title, "Cargando tus datos…", "the first line stays");
      assert.equal(await ev(t, `!!document.querySelector(".sheet-loading .loading-more")`), true, "inside the status, so it is read out");
      assert.equal(await ev(t, `document.querySelector(".sheet-banner") === null || document.querySelector(".sheet-banner").hidden`), true, "no alarm");
      if (SHOTS) await t.page.shot(path.join(SHOTS, `loading-slow-${w}-tinta.png`));
      // the sampler: every frame is either the loading state alone, or the filled sheet; never the empty skeleton in between
      await ev(t, `window.__frames = []; (function f() { const q = (s) => document.querySelector(s); const vis = (el) => !!el && el.getClientRects().length > 0;
        __frames.push({ loading: vis(q(".sheet-loading")), host: vis(q(".sheet-host")), text: (q(".sheet-host")?.innerText || "").trim().length, busy: q("#app").getAttribute("aria-busy") }); requestAnimationFrame(f); })()`);
      await t.page.waitFor(`document.getElementById("app").getAttribute("aria-busy") !== "true"`, { timeout: 12000 });
      await t.page.wait(300);
      s = await sheet(t);
      assert.deepEqual([s.loading, s.host], [false, true], "the data is in: the loading state is gone and the sheet is the moment");
      assert.ok(s.appText.trim().length > 50, "the sheet has its words");
      assert.doesNotMatch(s.appText, BAD);
      const frames = await ev(t, `__frames`);
      assert.ok(frames.length > 20, `${frames.length} frames sampled`);
      const bad = frames.filter((f) => !((f.loading && !f.host && f.busy === "true") || (!f.loading && f.host && f.text > 20 && f.busy !== "true")));
      assert.deepEqual(bad.slice(0, 3), [], "no frame between the two states");
      assert.ok(frames.some((f) => f.loading) && frames.some((f) => f.host), "both states were seen");
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}

test("1440: another stop says the same, with its own name, and the data fills that stop when it comes", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es&lag=5000", width: 1440, height: 900, wait: false });
  try {
    await t.page.waitFor(`!!document.querySelector(".sheet-loading")`, { timeout: 4000 });
    await t.page.wait(400);
    await press(t, "3");
    await t.page.waitFor(`location.hash === "#cola"`, { timeout: 8000 });
    let s = await sheet(t);
    assert.deepEqual([s.loading, s.title, s.moment, s.busy, s.host], [true, "Cargando tus datos…", "cola", "true", false]);
    assert.equal(s.eyebrow?.toLowerCase(), "cola");
    assert.doesNotMatch(s.appText, /\d/);
    await t.page.waitFor(`document.getElementById("app").getAttribute("aria-busy") !== "true"`, { timeout: 12000 });
    s = await sheet(t);
    assert.deepEqual([s.loading, s.host, s.moment], [false, true, "cola"]);
    assert.ok(s.appText.trim().length > 30);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("390: the tabs work while it loads, and the language change words the loading state in the other language", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es&lag=5000", width: 390, height: 844, wait: false });
  try {
    await t.page.waitFor(`!!document.querySelector(".sheet-loading")`, { timeout: 4000 });
    await t.page.click("#tab-sistema");
    await t.page.waitFor(`document.getElementById("app").dataset.moment === "sistema"`);
    let s = await sheet(t);
    assert.deepEqual([s.loading, s.eyebrow?.toLowerCase(), s.busy], [true, "sistema", "true"]);
    await press(t, "l");
    await t.page.waitFor(`document.documentElement.lang === "en"`);
    s = await sheet(t);
    assert.deepEqual([s.title, s.eyebrow?.toLowerCase(), s.loading], ["Loading your data…", "system", true]);
    await t.page.waitFor(`document.getElementById("app").getAttribute("aria-busy") !== "true"`, { timeout: 12000 });
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

// ---- a failure is a sentence with a way out --------------------------------------------------------------------------------------------------

test("1440: if the first read fails the banner says so in words with a Retry button, the loading state is not left spinning, and Retry reads again and fills the sheet", async () => {
  let reads = 0;
  const t = await openPage({
    kind: "real", query: "?t=tok-1&debug=1&clock=9&lang=es", width: 1440, height: 900, wait: false,
    answer: async (p) => {
      if (!p.startsWith("/api/snapshot")) return undefined;
      reads += 1;
      if (reads === 1) return { status: 500, body: { ok: false, message: "Error interno: boom" } };
      await new Promise((r) => setTimeout(r, 800));
      return { status: 200, body: rich };
    },
  });
  try {
    await t.page.waitFor(`!!document.querySelector(".sheet-retry") && document.querySelector(".sheet-retry").getClientRects().length > 0`, { timeout: 8000 });
    let s = await sheet(t);
    assert.match(s.banner, /boom/, "the server's own words reach him");
    assert.equal(s.retry, "Reintentar");
    assert.deepEqual([s.loading, s.busy, s.host], [false, "false", false], "not loading any more, and no empty skeleton either");
    assert.doesNotMatch(s.appText, /\d/);
    assert.doesNotMatch(s.appText, BAD);
    if (SHOTS) { mkdirSync(SHOTS, { recursive: true }); await t.page.shot(path.join(SHOTS, "error-1440-tinta.png")); }
    await tap(t, ".sheet-retry");
    await t.page.waitFor(`document.querySelector(".sheet-loading")?.getClientRects().length > 0`, { timeout: 3000 });
    s = await sheet(t);
    assert.deepEqual([s.busy, s.banner], ["true", null], "reading again: the loading state is back and the banner is gone");
    await t.page.waitFor(`document.getElementById("app").getAttribute("aria-busy") !== "true"`, { timeout: 8000 });
    s = await sheet(t);
    assert.deepEqual([s.loading, s.host, s.banner, s.retry], [false, true, null, null]);
    assert.equal(reads, 2);
    assert.ok(s.appText.trim().length > 50);
    const noise = t.page.problems().filter((p) => !/500|Failed to load resource/.test(p.text));
    assert.deepEqual(noise, []);
  } finally { await t.close(); }
});

test("390: a wrong key is a banner with the explanation and the same button, and no zero on the sheet", async () => {
  const t = await openPage({
    kind: "real", query: "?t=wrong&debug=1&clock=9&lang=es", width: 390, height: 844, wait: false,
    answer: async (p) => (p.startsWith("/api/snapshot") ? { status: 401, body: { ok: false, message: "Falta el token" } } : undefined),
  });
  try {
    await t.page.waitFor(`!!document.querySelector(".sheet-retry") && document.querySelector(".sheet-retry").getClientRects().length > 0`, { timeout: 8000 });
    const s = await sheet(t);
    assert.match(s.banner, /clave|enlace/i);
    assert.equal(s.retry, "Reintentar");
    assert.doesNotMatch(s.appText, /\d/);
    assert.deepEqual([s.loading, s.busy], [false, "false"]);
    if (SHOTS) { mkdirSync(SHOTS, { recursive: true }); await t.page.shot(path.join(SHOTS, "error-390-tinta.png")); }
  } finally { await t.close(); }
});

// ---- the refresh loop, seen from the page ---------------------------------------------------------------------------------------------------

test("1440: with a server that takes four seconds there are never two snapshot reads in flight, and hiding the tab stops the loop; showing it reads at once", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es&lag=4000&poll=1000", width: 1440, height: 900, wait: false });
  try {
    const stats = () => ev(t, `({ ...window.__DEMO__.stats })`);
    await t.page.waitFor(`document.getElementById("app").getAttribute("aria-busy") !== "true"`, { timeout: 12000 });
    // let it poll a few times while every read takes four seconds
    await t.page.waitFor(`window.__DEMO__.stats.total >= 3`, { timeout: 20000 });
    let s = await stats();
    assert.equal(s.max, 1, `never two reads at once (${JSON.stringify(s)})`);
    // hide the tab: the read in flight may end, then nothing more is asked
    await ev(t, `Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "hidden" }); document.dispatchEvent(new Event("visibilitychange"))`);
    await t.page.waitFor(`window.__DEMO__.stats.inflight === 0`, { timeout: 6000 });
    const hiddenAt = (await stats()).total;
    await t.page.wait(4500);                                        // four turns of the loop, and more than one read's worth
    s = await stats();
    assert.deepEqual([s.total, s.inflight], [hiddenAt, 0], "a hidden page asks nothing");
    // back: one read now, not at the next turn of a loop that was stopped
    await ev(t, `delete document.visibilityState; document.dispatchEvent(new Event("visibilitychange"))`);
    await t.page.waitFor(`window.__DEMO__.stats.total === ${hiddenAt + 1}`, { timeout: 1500 });
    await t.page.waitFor(`window.__DEMO__.stats.inflight === 0`, { timeout: 6000 });
    s = await stats();
    assert.equal(s.max, 1, "still never two at once");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});
