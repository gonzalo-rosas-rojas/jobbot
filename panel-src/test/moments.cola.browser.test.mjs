test("the plane's two soft shadows come in after the picture, one at a time (each is a big blur: together with the paper they were a 70 ms frame), stay while the user moves between proposals and go with the plane", async () => {
  const t = await open();
  try {
    await ev(t, `window.__lab.reduced(true), window.__lab.load("many", 18)`);
    await t.page.waitFor(`document.querySelector(".plane-wrap").dataset.state === "ok" && document.querySelector(".plane img.shot")?.complete === true && !!document.querySelector(".plane-wrap").dataset.halo`);
    const halo = () => ev(t, `document.querySelector(".plane-wrap").dataset.halo`);
    const opacity = () => ev(t, `Object.fromEntries([...document.querySelectorAll(".plane-shade")].map((el) => [el.dataset.k, Number(getComputedStyle(el).opacity)]))`);
    assert.deepEqual(await ev(t, `[...document.querySelectorAll(".plane-stack > *")].map((el) => el.className)`), ["plane-shade", "plane-shade", "plane"], "the shades are under the plane, in its own cell");
    // the picture is loaded and painted first; the dark shade is next, the warm glow last
    const seen = [await halo()];
    for (const step of ["1", "2"]) {
      await t.page.waitFor(`document.querySelector(".plane-wrap").dataset.halo === "${step}"`);
      seen.push(step);
    }
    assert.deepEqual(seen.filter((v, i) => v !== seen[i - 1]), ["0", "1", "2"], "0 (the picture alone), then the dark shade, then the warm one");
    await t.page.waitFor(`[...document.querySelectorAll(".plane-shade")].every((el) => getComputedStyle(el).opacity === "1")`);
    assert.deepEqual(await opacity(), { dark: 1, warm: 1 });
    // the shades have the plane's own size and tilt, so they are its shadow
    const geo = await ev(t, `(() => { const r = (el) => { const b = el.getBoundingClientRect(); return [b.left, b.top, b.width, b.height].map(Math.round).join(","); }; const p = document.querySelector(".plane"); return { plane: r(p), shades: [...document.querySelectorAll(".plane-shade")].map(r), tilt: getComputedStyle(p).transform, same: [...document.querySelectorAll(".plane-shade")].every((el) => getComputedStyle(el).transform === getComputedStyle(p).transform) }; })()`);
    assert.deepEqual(geo.shades, [geo.plane, geo.plane]);
    assert.ok(geo.same && geo.tilt.startsWith("matrix3d"), "the same tilt");
    // and the perspective reaches it (it belongs to the plane's parent: a plane tilted with no perspective is a flat sheet slightly narrower, not the tilted paper)
    const seen3d = await ev(t, `(() => { const p = document.querySelector(".plane"); return { perspective: getComputedStyle(p.parentElement).perspective, nearEdge: p.getBoundingClientRect().height / p.offsetHeight }; })()`);
    assert.equal(seen3d.perspective, "1800px");
    assert.ok(seen3d.nearEdge > 1.02, `the near edge of the paper is taller than the paper (${seen3d.nearEdge.toFixed(3)}): there is perspective`);
    // moving between proposals that have a capture does not take the shadows away and bring them back
    await key(t, "j");
    for (let i = 0; i < 4; i++) { await t.page.wait(60); assert.equal(await halo(), "2"); }
    // a proposal with no capture takes the plane away, shadows included; the next one with a capture brings them back in order
    for (let i = 0; i < 2; i++) await key(t, "j");
    await t.page.waitFor(`document.querySelector(".plane-wrap").dataset.state === "none"`);
    assert.equal(await halo(), "0");
    assert.equal(await ev(t, `getComputedStyle(document.querySelector(".plane-stack")).display`), "none");
    await key(t, "k");
    await t.page.waitFor(`document.querySelector(".plane-wrap").dataset.state === "ok" && document.querySelector(".plane-wrap").dataset.halo === "1"`);
    await t.page.waitFor(`document.querySelector(".plane-wrap").dataset.halo === "2"`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

// COLA in a real browser (spec §5.3, §3.4, §9, A15, A16, A18; the winning shot is board/relay2/cola-1920.png): the page fits the sheet at the sizes of the
// spec, says what the model says, takes its keys from the one table, keeps an armed press and a dim row through a refresh, tells the engine as it
// is, treats what others wrote as text, and keeps 4.5:1 over the glass and the sky on the pixels Chrome really paints. The lab is lab/f1-cola.entry.js.
import test from "node:test";
import assert from "node:assert/strict";
import { openBuilt } from "./helpers/browser.mjs";
import { decodePng } from "./helpers/png.mjs";
import { contrast } from "./helpers/color.mjs";

const open = (opts = {}) => openBuilt({ args: ["lab", "f1-cola"], file: "dist/lab-f1-cola.html", ...opts });
const ev = (t, expr) => t.page.eval(expr);
const load = (t, name = "rich", hour = 18) => ev(t, `window.__lab.reduced(true), window.__lab.load(${JSON.stringify(name)}, ${hour})`);
// tinykeys reads event.code for a letter, so a real key needs its code: "a" is KeyA.
const key = (t, k) => t.page.key(k, k.length === 1 ? { code: "Key" + k.toUpperCase(), text: k } : {});
const shift = async (t, k) => {
  const base = { key: k, code: "Key" + k.toUpperCase(), modifiers: 8 };
  await t.page.send("Input.dispatchKeyEvent", { type: "keyDown", text: k, ...base });
  await t.page.send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
};
const text = async (t, sel) => ((await ev(t, `(document.querySelector(${JSON.stringify(sel)}) || {}).textContent`)) || "").replace(/\s+/g, " ").trim();
const state = (t) => ev(t, `window.__lab.state()`);
const acts = async (t) => (await state(t)).acts;
const approveLabel = (t) => text(t, '.prop [data-action="approve"] .act-label');

for (const [width, height, plane] of [[1920, 1080, true], [3440, 1440, true], [1440, 900, true]]) {
  test(`COLA at ${width}x${height}: the headline is the position among those that wait, the score has its six bars, the five layers say where it is, everything is in view`, async () => {
    const t = await open({ width, height });
    try {
      await load(t);
      await t.page.waitFor(`document.querySelectorAll(".cola .f-bar").length === 6 && document.querySelector(".plane-wrap").dataset.state === "ok"`);
      const v = await ev(t, `(() => { const q = (s) => document.querySelector(s); const all = (s) => [...document.querySelectorAll(s)]; const host = q("#host");
        const mega = q(".cola .mega"); const shot = q(".plane-wrap"); const planeEl = q(".plane");
        return { h1: mega.textContent, family: getComputedStyle(mega).fontFamily, n: getComputedStyle(q(".mega .n")).color, state: q(".cola").dataset.state,
          fitsY: host.scrollHeight <= host.clientHeight, fitsX: document.documentElement.scrollWidth <= innerWidth && host.scrollWidth <= host.clientWidth,
          bars: all(".f-bar").map((b) => [b.dataset.factor, b.querySelector(".f-val").textContent]), path: all(".path li").map((l) => [l.dataset.layer, l.dataset.state]),
          here: q('.path li[data-state="here"]').textContent, why: q(".why").textContent,
          verbs: all(".prop .verbs [data-action]").map((b) => [b.dataset.action, b.querySelector("kbd").textContent]), step: q(".step-text").textContent,
          figs: all(".fn .fig-n").map((n) => n.textContent), last: getComputedStyle(q('.fig-n[data-last="true"]')).color, band: q(".band path").getAttribute("d").length > 20,
          rows: all(".q-row").map((r) => [r.dataset.kind, r.querySelectorAll(".act").length]), squares: all(".trust .sq i").map((i) => i.dataset.on === "true"),
          meter: getComputedStyle(q(".cap-meter i")).width, shot: { hidden: shot.hidden, state: shot.dataset.state, transform: getComputedStyle(planeEl).transform, img: !!q(".plane img.shot"), caption: q(".plane-cap").textContent },
          lenis: q(".q-rows").hasAttribute("data-lenis-prevent"), labelled: [q(".cola").getAttribute("aria-labelledby"), q(".mega").getAttribute("tabindex")],
          foot: q(".foot-line").textContent, stop: q(".stop-btn").textContent, engine: q(".engine").textContent }; })()`);
      assert.equal(v.h1, "Por aprobar 1 de 2");
      assert.match(v.family, /Barlow Condensed/);
      assert.equal(v.n, "rgb(233, 239, 247)", "the count is in the ink of the headline: gold is what is his to do");
      assert.equal(v.state, "ready");
      assert.equal(v.fitsY, true, "nothing of COLA is cut or scrolled at this size");
      assert.equal(v.fitsX, true, "nothing pushes the page wider");
      assert.deepEqual(v.bars.map((b) => b[0]), ["fit", "compensation", "interview_style", "stability", "learning", "other"]);
      assert.deepEqual(v.bars.map((b) => b[1]), ["28 / 30", "14 / 15", "9 / 10", "18 / 20", "14 / 15", "9 / 10"], "points over what each factor could give");
      assert.deepEqual(v.path, [["aviso", "done"], ["puntaje", "done"], ["cv", "done"], ["respuestas", "done"], ["compuerta", "here"]]);
      assert.match(v.here, /aquí/);
      assert.match(v.why, /Llené el formulario/);
      assert.deepEqual(v.verbs, [["approve", "a"], ["snooze", "s"], ["open", "↵"], ["batch", "⇧A"]], "the key of each verb once, the lote's with the others");
      assert.equal(v.step, "1 de 2");
      assert.deepEqual(v.figs, ["287", "20", "15", "11", "9"], "A16: the funnel's own numbers");
      assert.equal(v.last, "rgb(91, 240, 184)", "what passed the gate is mint");
      assert.equal(v.band, true);
      assert.deepEqual(v.rows, [["pending", 1], ["approved", 1], ["blocked", 1], ["sent", 1]], "one button per row, once");
      assert.deepEqual(v.squares, [true, true, true, true, false], "four of five approvals are his");
      assert.ok(parseFloat(v.meter) > 0, "two of ten sends fill a fifth of the bar");
      assert.equal(v.shot.hidden, !plane);
      assert.equal(v.shot.state, "ok");
      assert.ok(v.shot.img, "the capture is a picture from a blob");
      assert.notEqual(v.shot.transform, "none", "the plane is tilted");
      assert.match(v.shot.caption, /Captura .*, 900×1120. No se envió nada./i);
      assert.equal(v.lenis, true);
      assert.deepEqual(v.labelled, ["cola-title", "-1"], "the shell lands on the headline");
      assert.match(v.foot, /Revisando los primeros 5: llevas 4; después, hasta 25 al día/i);
      assert.match(v.stop, /Parar el motor/);
      assert.match(v.engine, /ventana abierta · última corrida/i);
      assert.deepEqual(t.page.problems(), [], "console clean, no CSP violation");
    } finally { await t.close(); }
  });
}

test("COLA at 390x844: one column, the capture comes in the flow after the proposal, the verbs are big enough to touch, nothing pushes the page wider", async () => {
  const t = await open({ width: 390, height: 844 });
  try {
    await t.page.viewport(390, 844, { mobile: true });
    await load(t);
    await t.page.waitFor(`document.querySelectorAll(".cola .f-bar").length === 6`);
    const v = await ev(t, `(() => { const q = (s) => document.querySelector(s); const all = (s) => [...document.querySelectorAll(s)];
      const y = (el) => el.getBoundingClientRect().top + scrollY;
      return { fitsX: document.documentElement.scrollWidth <= innerWidth, plane: getComputedStyle(q(".plane-wrap")).position, transform: getComputedStyle(q(".plane")).transform,
        order: [y(q(".prop")), y(q(".plane-wrap")), y(q(".funnel")), y(q(".queue"))], small: all(".cola .act").filter((b) => b.getBoundingClientRect().height < 40).map((b) => b.dataset.action),
        bars: getComputedStyle(q(".bars")).gridTemplateColumns.split(" ").length, mega: getComputedStyle(q(".mega")).fontSize }; })()`);
    assert.equal(v.fitsX, true);
    assert.equal(v.plane, "static", "the capture is a block of the page, not at the right of a sheet that is not there");
    assert.equal(v.transform, "none", "flat on a phone");
    assert.ok(v.order[0] < v.order[1] && v.order[1] < v.order[2] && v.order[2] < v.order[3], `the proposal, its capture, the funnel, the queue (${v.order.join(", ")})`);
    assert.deepEqual(v.small, [], "every verb is at least 40 px tall");
    assert.equal(v.bars, 1, "the six bars in one column");
    assert.equal(v.mega, "64px");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("a laptop narrower than 1300 px has no room at the right of the sheet: the capture waits in the drawer", async () => {
  const t = await open({ width: 1280, height: 720 });
  try {
    await load(t);
    assert.equal(await ev(t, `getComputedStyle(document.querySelector(".plane-wrap")).display`), "none");
    assert.equal(await ev(t, `document.documentElement.scrollWidth <= innerWidth`), true);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("empty: the truth about the last run, no proposal, no capture, the funnel says it has nothing and the engine is told as it is (A15)", async () => {
  const t = await open();
  try {
    await load(t, "empty");
    const v = await ev(t, `(() => { const q = (s) => document.querySelector(s); return { state: q(".cola").dataset.state, h1: q(".mega").textContent.trim(), sub: q(".sub").textContent, warn: q(".warn").textContent,
      prop: q(".prop").hidden, plane: q(".plane-wrap").hidden, band: q(".band").hasAttribute("hidden"), figs: q(".fn").hidden, funnel: q(".funnel .none").textContent, queue: q(".queue .none").textContent, trust: q(".trust").hidden }; })()`);
    assert.equal(v.state, "empty");
    assert.equal(v.h1, "Nada por aprobar.");
    assert.match(v.sub, /Lo que el bot descubra esta noche aparece aquí/i);
    assert.match(v.warn, /El motor todavía no ha corrido: aprobar no envía nada hasta que corra\. Tarea programada: sin dato\./, "a null task is 'sin dato', never 'no existe'");
    assert.deepEqual([v.prop, v.plane, v.band, v.figs, v.trust], [true, true, true, true, true]);
    assert.match(v.funnel, /Sin avisos guardados/);
    assert.match(v.queue, /Nada más en la cola/);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("A15: the verb says what really happens: the next run sends it, it waits for the engine, it leaves when you resume; never that it was sent", async () => {
  const t = await open();
  try {
    await load(t);
    assert.equal(await approveLabel(t), "Aprobar: la próxima corrida la envía");
    await ev(t, `window.__lab.refresh((s) => { s.controls.engine = "stale"; s.controls.last_run_at = "2026-10-04T10:00:00Z"; s.controls.scheduled_task = null; })`);
    assert.equal(await approveLabel(t), "Aprobar: no sale hasta que el motor corra");
    assert.match(await text(t, ".warn"), /El motor corrió por última vez hace 3 días: aprobar no envía nada hasta que corra\. Tarea programada: sin dato\./);
    await ev(t, `window.__lab.refresh((s) => { s.controls.engine = "recent"; s.controls.last_run_at = "2026-10-07T11:04:00Z"; s.controls.stopped = true; })`);
    assert.equal(await approveLabel(t), "Aprobar: saldrá cuando reanudes");
    assert.match(await text(t, ".warn"), /STOP está activo/);
    assert.match(await text(t, ".stop-btn"), /Reanudar el motor/);
    assert.equal(await ev(t, `document.querySelector('.q-row[data-kind="approved"] .meta').textContent`), "puntaje 90 · greenhouse · saldrá cuando reanudes");
    for (const label of [await approveLabel(t), await text(t, '.q-row[data-kind="approved"] .meta')]) assert.doesNotMatch(label, /envió|enviada|enviado/);
    await ev(t, `window.__lab.refresh((s) => { s.controls.stopped = false; s.controls.scheduled_task = { exists: false }; s.controls.engine = "never"; s.controls.last_run_at = null; })`);
    assert.match(await text(t, ".warn"), /El motor todavía no ha corrido: aprobar no envía nada hasta que corra\. Sin tarea programada\./);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("a permanent verb by key: the first press arms and says so, a second inside 350 ms is the same press, then the proposal is dim until the server answers and the next one takes its place", async () => {
  const t = await open();
  try {
    await load(t);
    await key(t, "a");
    assert.equal(await approveLabel(t), "¿Seguro? Pulsa otra vez");
    assert.equal(await ev(t, `document.querySelector('.prop [data-action="approve"]').dataset.armed`), "true");
    assert.deepEqual([(await state(t)).said, await acts(t)], [["¿Seguro? Pulsa otra vez"], []], "said aloud for the screen reader, and nothing was sent");
    await key(t, "a");
    assert.deepEqual(await acts(t), [], "a second press inside 350 ms is a double click, not a decision");
    await t.page.wait(450);
    await ev(t, `window.__lab.hold()`);
    await key(t, "a");
    await t.page.waitFor(`window.__lab.state().acts.length === 1`);
    assert.deepEqual(await acts(t), [{ path: "/api/approve", body: { key: "p:1" } }]);
    const dim = await ev(t, `(() => { const p = document.querySelector(".prop"); const a = p.querySelector(".acts"); return [p.dataset.pending, getComputedStyle(a).opacity, getComputedStyle(a).pointerEvents]; })()`);
    assert.deepEqual(dim, ["true", "0.3", "none"], "the proposal answers in under 100 ms: it is at 30 % from the first instant");
    await ev(t, `window.__lab.release(true)`);
    await t.page.waitFor(`document.querySelector(".mega").textContent === "Por aprobar 1 de 1"`);
    assert.equal(await text(t, ".role-title"), "Full-stack Engineer", "the next proposal took the place of the one that went");
    assert.deepEqual(await ev(t, `[".q-kind", ".co", ".role", ".meta", ".act"].map((s) => document.querySelector('.q-row[data-kind="approved"] ' + s).textContent)`), ["Aprobada", "Tramontana", "Backend Engineer · US$4.900 – US$6.000", "puntaje 90 · greenhouse · sale en la próxima corrida", "↵Abrir"], "the approved row of the queue stays");
    assert.ok((await state(t)).sounds.includes("approve"));
    assert.equal(await ev(t, `document.querySelectorAll('.trust .sq i[data-on="true"]').length`), 5, "the fifth approval is his");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the approved proposal slides into the queue and the next one takes the focus with a short entrance; with reduced motion nothing moves, and j never animates a row", async () => {
  const t = await open();
  try {
    await load(t);
    await ev(t, `window.__lab.reduced(false)`);
    await key(t, "j");
    await t.page.waitFor(`document.querySelector(".mega").textContent === "Por aprobar 2 de 2"`);
    assert.equal(await ev(t, `document.querySelectorAll(".q-row[data-enter]").length`), 0, "walking with j animates no row");
    await key(t, "k");
    await t.page.waitFor(`document.querySelector(".mega").textContent === "Por aprobar 1 de 2"`);
    await key(t, "a");
    await t.page.wait(450);
    await key(t, "a");
    await t.page.waitFor(`!!document.querySelector('.q-row[data-card="p:1"][data-kind="approved"]')`);
    const v = await ev(t, `({ row: document.querySelector('.q-row[data-card="p:1"]').dataset.enter, anim: getComputedStyle(document.querySelector('.q-row[data-card="p:1"]')).animationName, prop: document.querySelector(".prop").dataset.enter })`);
    assert.deepEqual([v.row, v.anim, v.prop], ["true", "cola-row-in", "true"]);
    await t.page.waitFor(`!document.querySelector(".q-row[data-enter]") && !document.querySelector(".prop").dataset.enter`);
    // reduced motion: the same approval moves nothing
    await ev(t, `window.__lab.load("rich", 18), window.__lab.reduced(true)`);
    await key(t, "a");
    await t.page.wait(450);
    await key(t, "a");
    await t.page.waitFor(`!!document.querySelector('.q-row[data-card="p:1"][data-kind="approved"]')`);
    assert.equal(await ev(t, `document.querySelectorAll("[data-enter]").length`), 0);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

for (const [name, status, said] of [["409", false, /No se pudo/], ["500", 500, /Error del servidor/], ["no network", 0, /Sin conexión/]]) {
  test(`a no (${name}) gives the proposal back: not dim, still there, the headline did not move, it sounds like an error and the approval is not repeated`, async () => {
    const t = await open();
    try {
      await load(t);
      await key(t, "a");
      await t.page.wait(450);
      await ev(t, `window.__lab.hold()`);
      await key(t, "a");
      await t.page.waitFor(`document.querySelector(".prop").dataset.pending === "true"`);
      await ev(t, `window.__lab.release(${status})`);
      await t.page.waitFor(`!document.querySelector(".prop").dataset.pending`);
      assert.equal(await ev(t, `getComputedStyle(document.querySelector(".prop .acts")).opacity`), "1");
      assert.equal(await text(t, ".mega"), "Por aprobar 1 de 2");
      const s = await state(t);
      assert.ok(s.sounds.includes("error"));
      assert.match(s.toasts[0].message, said, "the toast says why");
      assert.equal(s.acts.length, 1, "nothing is sent twice by itself");
      assert.equal(await ev(t, `document.querySelector('.prop [data-action="approve"]').dataset.armed || ""`), "", "the press is spent");
    } finally { await t.close(); }
  });
}

test("j and k walk the proposals by score and stop at the ends; the headline, the facts and the capture follow", async () => {
  const t = await open();
  try {
    await load(t, "many");
    assert.equal(await text(t, ".mega"), "Por aprobar 1 de 7");
    await key(t, "k");
    assert.equal(await text(t, ".mega"), "Por aprobar 1 de 7", "no wrap at the top");
    await key(t, "j");
    await t.page.waitFor(`document.querySelector(".mega").textContent === "Por aprobar 2 de 7"`);
    assert.equal(await text(t, ".role-title"), "Full-stack Engineer");
    assert.equal(await text(t, ".step-text"), "2 de 7");
    assert.match((await state(t)).said.at(-1), /Brisa Mobility, Full-stack Engineer\. 2 de 7\./, "said aloud");
    await t.page.waitFor(`window.__lab.state().calls.some((c) => c === "/api/shot?key=p%3A2")`);
    await t.page.waitFor(`document.querySelector(".plane-wrap").dataset.state === "ok"`);
    assert.equal(await ev(t, `[...document.querySelectorAll('.q-row[data-kind="pending"]')].map((r) => r.dataset.card).join()`), "p:1,p:3,p:4,p:5", "the others, best first, are the rows under it; the rest are one j away");
    for (let i = 0; i < 9; i++) await key(t, "j");
    assert.equal(await text(t, ".mega"), "Por aprobar 7 de 7", "no wrap at the bottom");
    assert.equal(await text(t, ".queue .q-more"), "y 2 más");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("a proposal that goes away hands the place to the next one at the same index; a refresh that changes nothing writes nothing, and one that changes a number keeps the armed press", async () => {
  const t = await open();
  try {
    await load(t, "many");
    await key(t, "j");
    await t.page.waitFor(`document.querySelector(".mega").textContent === "Por aprobar 2 de 7" && document.querySelectorAll(".f-bar:not(.skel)").length === 6 && document.querySelector(".plane-wrap").dataset.state === "ok"`);
    await key(t, "a");
    await ev(t, `(() => { const b = document.querySelector('.prop [data-action="approve"]'); window.__keep = b; b.focus(); })()`);
    await ev(t, `(() => { window.__mut = 0; window.__obs = new MutationObserver((l) => { window.__mut += l.length; }); window.__obs.observe(document.querySelector(".cola"), { subtree: true, childList: true, attributes: true, characterData: true }); })()`);
    await ev(t, `window.__lab.refresh()`);
    assert.equal(await ev(t, `window.__mut`), 0, "an identical snapshot changes nothing in the DOM");
    await ev(t, `window.__lab.refresh((s) => { s.pipeline.seen += 5; s.trust.sent_24h = 3; })`);
    const s = await ev(t, `({ same: window.__keep === document.querySelector('.prop [data-action="approve"]'), armed: window.__keep.dataset.armed, label: window.__keep.textContent, focused: document.activeElement === window.__keep, fig: document.querySelector(".fn .fig-n").textContent, sent: document.querySelector(".trust-b p").textContent })`);
    assert.deepEqual([s.same, s.armed, s.focused, s.fig], [true, "true", true, "292"]);
    assert.match(s.label, /¿Seguro\?/);
    assert.match(s.sent, /3 de 10/);
    // the proposal in focus (p:2) is settled somewhere else: the one now at its place (p:3) takes over
    await ev(t, `window.__lab.refresh((s) => { s.awaiting_approval.cards = s.awaiting_approval.cards.filter((c) => c.key !== "p:2"); s.awaiting_approval.total -= 1; })`);
    assert.equal(await text(t, ".mega"), "Por aprobar 2 de 6");
    assert.equal(await text(t, ".role-title"), "Staff Platform Engineer");
    assert.equal(await ev(t, `document.querySelector('.prop [data-action="approve"]').dataset.armed || ""`), "", "an armed press belongs to the proposal it was made on");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the lote is drawn before it is sent: Shift A shows who, Esc drops it, ↵ inside 350 ms is not the confirmation, ↵ later sends ONE request with every key", async () => {
  const t = await open();
  try {
    await load(t);
    assert.equal(await ev(t, `document.querySelector(".confirm").hidden`), true);
    await shift(t, "a");
    await t.page.waitFor(`!document.querySelector(".confirm").hidden`);
    assert.match(await text(t, ".confirm"), /^¿aprobar 2\? Ancla Security 92 · Brisa Mobility 89 · al llegar a 5 aprobadas, jobbot envía solo · ↵ confirmar Esc cancelar$/i);
    assert.deepEqual([await acts(t), (await state(t)).said.at(-1)], [[], "Aprobar 2: pulsa Enter para confirmar o Escape para cancelar."], "nothing is sent by drawing it");
    await key(t, "Escape");
    await t.page.waitFor(`document.querySelector(".confirm").hidden`);
    assert.deepEqual(await acts(t), []);
    await shift(t, "a");
    await t.page.waitFor(`!document.querySelector(".confirm").hidden`);
    await key(t, "Enter");
    assert.deepEqual(await acts(t), [], "a ↵ inside 350 ms of the first press is not a decision");
    await t.page.wait(450);
    await key(t, "Enter");
    await t.page.waitFor(`window.__lab.state().acts.length === 1`);
    assert.deepEqual(await acts(t), [{ path: "/api/approve", body: { keys: ["p:1", "p:2"] } }], "one request with every key");
    await t.page.waitFor(`document.querySelector(".cola").dataset.state === "empty"`);
    assert.equal(await text(t, ".mega"), "Nada por aprobar.");
    assert.ok((await state(t)).sounds.includes("approve"));
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the lote's threshold is a preference: a threshold nobody reaches draws nothing and says so; the confirm button works by click too; a 409 gives every row back", async () => {
  const t = await open();
  try {
    await load(t);
    await ev(t, `window.__lab.batchMin(95)`);
    assert.match(await text(t, ".batch-btn"), /Ninguna llega a 95/);
    await shift(t, "a");
    assert.equal(await ev(t, `document.querySelector(".confirm").hidden`), true, "nothing to draw");
    assert.match((await state(t)).said.at(-1), /Ninguna propuesta llega a 95/);
    await ev(t, `window.__lab.batchMin(null)`);
    assert.match(await text(t, ".batch-btn"), /Aprobar las de 85 o más \(2\)/);
    await t.page.click(".batch-btn");
    await t.page.waitFor(`!document.querySelector(".confirm").hidden`);
    await t.page.wait(450);
    await ev(t, `window.__lab.hold()`);
    await t.page.click('.confirm [data-batch="confirm"]');
    await t.page.waitFor(`window.__lab.state().acts.length === 1`);
    assert.equal(await ev(t, `document.querySelector(".prop").dataset.pending`), "true", "the proposals in the lote are dim while the server answers");
    await ev(t, `window.__lab.release(false)`);
    await t.page.waitFor(`!document.querySelector(".prop").dataset.pending`);
    assert.equal(await text(t, ".mega"), "Por aprobar 1 de 2", "a 409 gives the rows back");
    assert.ok((await state(t)).sounds.includes("error"));
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("s asks until when and has a way back: the menu's digit sends the request, the toast offers Deshacer, and Deshacer puts the proposal back", async () => {
  const t = await open();
  try {
    await load(t);
    await key(t, "s");
    await t.page.waitFor(`!!document.querySelector(".menu")`);
    assert.match(await text(t, ".menu"), /Posponer hasta cuándo.*Mañana.*En tres días.*En una semana.*En dos semanas/);
    assert.deepEqual(await acts(t), [], "asking sends nothing");
    await key(t, "2");
    await t.page.waitFor(`window.__lab.state().acts.length === 1`);
    assert.deepEqual(await acts(t), [{ path: "/api/snooze", body: { key: "p:1", until: "3d" } }]);
    await t.page.waitFor(`document.querySelector(".mega").textContent === "Por aprobar 1 de 1"`);
    const s = await state(t);
    assert.ok(s.toasts.some((x) => x.undo), "the toast offers the way back");
    await ev(t, `window.__lab.undo()`);
    await t.page.waitFor(`window.__lab.state().acts.length === 2`);
    assert.deepEqual((await acts(t))[1], { path: "/api/unsnooze", body: { key: "p:1" } });
    await t.page.waitFor(`document.querySelector(".mega").textContent === "Por aprobar 2 de 2"`); // it comes back above the one he is looking at, which stays
    // a closed menu sends nothing
    await key(t, "s");
    await t.page.waitFor(`!!document.querySelector(".menu")`);
    await key(t, "Escape");
    await t.page.waitFor(`!document.querySelector(".menu")`);
    assert.equal((await acts(t)).length, 2);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("Enter opens the detail of what is in focus, o the posting, a button or a row the same; a row of the queue 'preparada' is put in focus first, 'Contestar' goes to APRENDER", async () => {
  const t = await open();
  try {
    await load(t);
    await key(t, "Enter");
    await t.page.waitFor(`window.__lab.state().opened.length === 1`);
    assert.deepEqual((await state(t)).opened, [["p:1", "card:p:1"]]);
    await key(t, "o");
    await t.page.waitFor(`window.__lab.state().links.length === 1`);
    assert.deepEqual((await state(t)).links, ["https://example.com/p/1"]);
    await t.page.click('.q-row[data-kind="pending"] .what');
    await t.page.waitFor(`document.querySelector(".mega").textContent === "Por aprobar 2 de 2"`);
    assert.equal(await text(t, ".role-title"), "Full-stack Engineer", "it is looked at in full before it is decided");
    await t.page.click('.q-row[data-kind="blocked"] [data-action="answer"]');
    await t.page.waitFor(`window.__lab.state().went.length === 1`);
    assert.deepEqual((await state(t)).went, [["aprender", null]]);
    await t.page.click('.q-row[data-kind="approved"] [data-action="open"]');
    await t.page.waitFor(`window.__lab.state().opened.length === 2`);
    assert.deepEqual((await state(t)).opened[1], ["p:21", "card:p:21"]);
    assert.equal((await acts(t)).length, 0, "opening changes nothing on the server");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("p stops the engine in one press (SISTEMA's rule) and resumes it in two; the page tells which; keys typed in a field are ignored", async () => {
  const t = await open();
  try {
    await load(t);
    await key(t, "p");
    await t.page.waitFor(`window.__lab.state().acts.length === 1`);
    assert.notEqual(await ev(t, `document.querySelector(".stop-btn").dataset.armed`), "true", "stopping is not armed: it goes at once");
    assert.deepEqual(await acts(t), [{ path: "/api/stop", body: {} }]);
    await t.page.waitFor(`/Reanudar el motor/.test(document.querySelector(".stop-btn").textContent)`);
    assert.match(await text(t, ".engine"), /STOP activo/);
    // a field: the letters are the person's, not the page's
    await ev(t, `(() => { const i = document.createElement("input"); i.type = "text"; i.id = "probe"; document.body.append(i); i.focus(); })()`);
    await key(t, "a");
    await key(t, "s");
    await key(t, "j");
    assert.equal(await ev(t, `[document.querySelector('.prop [data-action="approve"]').dataset.armed || "", !document.querySelector(".menu"), document.querySelector(".mega").textContent]`).then((r) => r.join("|")), "|true|Por aprobar 1 de 2");
    assert.equal(await ev(t, `document.getElementById("probe").value`), "asj");
    await ev(t, `document.getElementById("probe").blur()`);
    await key(t, "p");
    assert.equal(await ev(t, `document.querySelector(".stop-btn").dataset.armed`), "true", "resuming arms first");
    assert.match(await text(t, ".stop-btn"), /¿Seguro\?/);
    assert.match(await ev(t, `document.querySelector(".stop-btn").getAttribute("aria-label")`), /¿Seguro\?/, "the accessible name follows the armed text");
    assert.equal((await acts(t)).length, 1, "and sends nothing yet");
    await t.page.wait(450);
    await key(t, "p");
    await t.page.waitFor(`window.__lab.state().acts.length === 2`);
    assert.deepEqual((await acts(t))[1], { path: "/api/resume", body: {} });
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the six bars say they are loading, then fill; a detail that fails says so and offers to try again; the capture says it is loading, that the proposal has none, or that it failed", async () => {
  const t = await open();
  try {
    await ev(t, `window.__lab.detail("slow"), window.__lab.shot("slow")`);
    await load(t);
    const wait = await ev(t, `({ skel: document.querySelectorAll(".f-bar.skel").length, busy: document.querySelector(".bars").getAttribute("aria-busy"), shot: document.querySelector(".plane-wrap").dataset.state, shotSkel: !!document.querySelector(".shot-skel") })`);
    assert.deepEqual(wait, { skel: 6, busy: "true", shot: "loading", shotSkel: true });
    await ev(t, `window.__lab.detailRelease(), window.__lab.shotRelease()`);
    await t.page.waitFor(`document.querySelectorAll(".f-bar:not(.skel)").length === 6 && document.querySelector(".plane-wrap").dataset.state === "ok"`);
    // failures
    await ev(t, `window.__lab.detail("error"), window.__lab.shot("error")`);
    await ev(t, `window.__lab.load("rich", 18)`);
    await t.page.waitFor(`!!document.querySelector(".bars [data-retry]") && document.querySelector(".plane-wrap").dataset.state === "error"`);
    assert.match(await text(t, ".bars"), /No se pudo cargar cómo se armó el puntaje/);
    assert.match(await text(t, ".plane-note"), /No se pudo cargar la captura/);
    await t.page.wait(300);
    assert.equal((await state(t)).calls.filter((c) => c.startsWith("/api/job")).length, 1, "a failed detail is not asked again by itself (it would be a loop); the retry button asks");
    await ev(t, `window.__lab.detail("ok"), window.__lab.shot("ok")`);
    await t.page.click(".bars [data-retry]");
    await t.page.waitFor(`document.querySelectorAll(".f-bar:not(.skel)").length === 6`);
    await t.page.click(".plane-wrap .plane-retry");
    await t.page.waitFor(`document.querySelector(".plane-wrap").dataset.state === "ok"`);
    // a proposal with no capture says so
    await ev(t, `window.__lab.load("many", 18)`);
    for (let i = 0; i < 3; i++) await key(t, "j");
    await t.page.waitFor(`document.querySelector(".mega").textContent === "Por aprobar 4 de 7"`);
    await t.page.waitFor(`document.querySelector(".plane-wrap").dataset.state === "none"`);
    assert.match(await text(t, ".plane-note"), /no tiene captura/);
    assert.equal(await ev(t, `getComputedStyle(document.querySelector(".plane")).display`), "none");
    assert.match(await text(t, '.path li[data-layer="respuestas"]'), /sin captura/);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("text from outside is only text: no node, no link, no inline script, long words wrap, a reversed-order mark is gone", async () => {
  const t = await open();
  try {
    await load(t, "hostile");
    await t.page.waitFor(`document.querySelectorAll(".f-bar").length === 6`);
    const v = await ev(t, `(() => { const q = (s) => document.querySelector(s); const host = q("#host"); return { imgs: document.querySelectorAll(".cola img:not(.shot)").length, anchors: document.querySelectorAll(".cola a").length,
      hrefs: [...document.querySelectorAll("[href]")].map((a) => a.getAttribute("href")).filter((h) => /^java/i.test(h)), facts: q(".facts").textContent, title: q(".role-title").textContent, fitsX: host.scrollWidth <= host.clientWidth && document.documentElement.scrollWidth <= innerWidth,
      big: q('.q-row[data-kind="pending"] .co').textContent.length, bidi: /[\\u202a-\\u202e\\u2066-\\u2069]/.test(document.body.textContent), ctl: /[\\u0000-\\u0008]/.test(q(".cola").textContent) }; })()`);
    assert.equal(v.imgs, 0);
    assert.equal(v.anchors, 0, "the dangerous address is not a link: the verb `o` only follows http(s)");
    assert.deepEqual(v.hrefs, []);
    assert.match(v.facts, /<img src=x onerror=alert\(1\)>/i, "it is shown as the characters it is");
    assert.ok(v.title.length <= 121, "cut, with an ellipsis");
    assert.equal(v.fitsX, true);
    assert.ok(v.big <= 61);
    assert.equal(v.bidi, false);
    assert.equal(v.ctl, false);
    await key(t, "o");
    assert.deepEqual((await state(t)).links, ["https://example.com/apply/p/1"], "the card's own address is not http(s), so it is dropped; the posting's apply address, which is, is the one `o` follows");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("English: the same page, the same numbers, the words of the other language", async () => {
  const t = await open();
  try {
    await load(t);
    await ev(t, `window.__lab.lang("en")`);
    assert.equal(await text(t, ".mega"), "To approve 1 of 2");
    assert.equal(await approveLabel(t), "Approve: the next run sends it");
    assert.deepEqual(await ev(t, `[...document.querySelectorAll(".fn .fig-n")].map((n) => n.textContent)`), ["287", "20", "15", "11", "9"]);
    assert.equal(await ev(t, `document.querySelector(".bars").getAttribute("aria-label")`), "How the score was worked out");
    assert.match(await text(t, ".stop-btn"), /Stop the engine/);
    await ev(t, `window.__lab.lang("es")`);
    assert.equal(await text(t, ".mega"), "Por aprobar 1 de 2");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

// ---- A18 over the glass and over the sky: the text measured on the pixels Chrome really paints ----

const lum = ([r, g, b]) => { const l = (c) => ((c /= 255) <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4); return 0.2126 * l(r) + 0.7152 * l(g) + 0.0722 * l(b); };

/** The contrast of each text against the ground behind its glyphs: the frame with the glyphs made invisible (lab: ghost), at the brightest 2 % of its pixels. */
async function textContrast(t) {
  const boxes = await ev(t, `window.__lab.textBoxes()`);
  await ev(t, `window.__lab.ghost(true)`);
  const png = decodePng(Buffer.from((await t.page.send("Page.captureScreenshot", { format: "png" })).data, "base64"));
  await ev(t, `window.__lab.ghost(false)`);
  const rows = [];
  for (const box of boxes) {
    const color = box.color.match(/\d+(\.\d+)?/g).map(Number).slice(0, 3);
    const grounds = [];
    for (const r of box.rects) {
      const x0 = Math.max(0, Math.floor(r.left) - 2), x1 = Math.min(png.w - 1, Math.ceil(r.right) + 2);
      const y0 = Math.max(0, Math.floor(r.top) - 2), y1 = Math.min(png.h - 1, Math.ceil(r.bottom) + 2);
      for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) grounds.push(png.at(x, y));
    }
    if (grounds.length <= 40) continue; // text scrolled out of the window
    grounds.sort((a, b) => lum(a) - lum(b));
    rows.push({ text: box.text, cls: box.cls, plane: box.onPlane, ratio: contrast(color, grounds[Math.floor(grounds.length * 0.98)]) });
  }
  return rows;
}

for (const [theme, width, height, hours] of [["dark", 1920, 1080, [9, 12, 17]], ["dark", 1440, 900, [12]], ["dark", 3440, 1440, [12]], ["light", 1920, 1080, [12]]]) {
  test(`A18 on COLA in ${theme === "dark" ? "Tinta" : "Papel"} at ${width}x${height}: every text of the sheet and of the capture's caption keeps 4.5:1 at ${hours.join(", ")} h`, async () => {
    const t = await open({ width, height });
    try {
      for (const hour of hours) {
        await ev(t, `document.documentElement.setAttribute("data-theme", ${JSON.stringify(theme)})`);
        await load(t, "rich", hour);
        await t.page.waitFor(`document.querySelectorAll(".f-bar").length === 6 && document.querySelector(".plane-wrap").dataset.state === "ok"`);
        await ev(t, `window.__lab.go(${hour})`);
        await t.page.wait(300);
        const rows = await textContrast(t);
        assert.ok(rows.length > 70, `the head, the proposal, the funnel, the queue, the trust and the caption (${rows.length})`);
        assert.ok(rows.some((r) => r.plane), "the caption of the capture was measured over the sky");
        for (const r of rows) assert.ok(r.ratio >= 4.5, `"${r.text}" (${r.cls}) is ${r.ratio.toFixed(2)}:1 at ${hour}:00 in ${theme}, ${width}x${height}`);
      }
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}
