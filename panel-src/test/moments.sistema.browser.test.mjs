// SISTEMA in a real Chrome (lab f2-sistema): the sheet at four sizes, the states of the engine, the two-step STOP and its undo, the answers of
// the server that fail, A18 measured on the pixels, and the page that never lies about the engine (spec §5.5, A9, A15, A16, A18, A19).
import test from "node:test";
import assert from "node:assert/strict";
import { openBuilt } from "./helpers/browser.mjs";
import { decodePng } from "./helpers/png.mjs";
import { contrast } from "./helpers/color.mjs";

const open = (opts = {}) => openBuilt({ args: ["lab", "f2-sistema"], file: "dist/lab-f2-sistema.html", ...opts });
const ev = (t, expr) => t.page.eval(expr);
// the lists give way to what the fonts leave of the room, and the fonts arrive after the first paint: wait for them, then for the frame that fits again.
// "loaded" is also what document.fonts says before the first font has been asked for, and the fit comes a frame after the last one arrives, so a
// fixed wait raced the fonts (it failed whenever the machine was busy): the wait is for the host to fit its content, for at most 3 s (a phone has
// no host that must fit, and a sheet that never fits is for the test that asserts it to say so).
const load = async (t, name = "night", hour = null) => {
  const r = await ev(t, `window.__lab.reduced(true), window.__lab.load(${JSON.stringify(name)}${hour == null ? "" : ", " + hour})`);
  await t.page.waitFor(`document.fonts.status === "loaded"`);
  await t.page.waitFor(`(() => { const h = document.querySelector("#host"); return !h || h.scrollHeight <= h.clientHeight; })()`, { timeout: 3000 }).catch(() => {});
  await t.page.wait(80);
  return r;
};
// tinykeys reads event.code for a letter, so a real key needs its code: "p" is KeyP.
const key = (t, k) => t.page.key(k, k.length === 1 ? { code: "Key" + k.toUpperCase(), text: k } : {});
const text = (t, sel) => ev(t, `(document.querySelector(${JSON.stringify(sel)}) || {}).textContent`);
const squash = (s) => String(s).replace(/\s+/g, " ").trim();
const SAYS_NOTHING = /\b(?:NaN|null|undefined)\b|⟦|hace -\d|-\d+ ?h\b|Invalid Date|\bsistema\.[a-z]/;

// ---- the sheet at four sizes -------------------------------------------------------------------------------------------------------

for (const [width, height, headline, shown] of [[1920, 1080, "64px", true], [3440, 1440, "80px", true], [1440, 900, "34px", true], [390, 844, "34px", false]]) {
  test(`SISTEMA at ${width}x${height}: the headline is when the engine last ran, everything is in view, no inline style but custom properties`, async () => {
    const t = await open({ width, height });
    try {
      if (width < 760) await t.page.viewport(width, height, { dpr: 1, mobile: true });
      await load(t, "night");
      const v = await ev(t, `(() => { const q = (s) => document.querySelector(s); const all = (s) => [...document.querySelectorAll(s)];
        const host = q("#host"); const h1 = q(".sistema .mega"); const tail = q(".sistema .mega .tail");
        return { h1: h1.textContent, size: getComputedStyle(tail).fontSize, family: getComputedStyle(tail).fontFamily, fits: host.scrollHeight <= host.clientHeight,
          labelled: q(".sistema").getAttribute("aria-labelledby"), focusable: h1.getAttribute("tabindex"), loading: q(".sistema").hasAttribute("data-loading"),
          styled: all(".sistema [style]").map((n) => n.getAttribute("style")), kbd: getComputedStyle(q(".act .k")).display, foot: getComputedStyle(q(".foot")).display,
          cols: getComputedStyle(q(".cols")).gridTemplateColumns.split(" ").length, wide: document.documentElement.scrollWidth <= window.innerWidth,
          rows: all(".sources-table tbody tr").length, labels: all(".sources-table tbody td").every((td) => td.dataset.label), blocks: all(".sistema [data-block]").map((b) => b.dataset.block) }; })()`);
      assert.equal(squash(v.h1), "Última corrida hace 56 min.");
      assert.equal(v.size, headline, "the answer is --fs-6, one notch down (--fs-5) on a low or narrow window, never a size of its own");
      assert.match(v.family, /Barlow Condensed/);
      if (shown) assert.equal(v.fits, true, "nothing of SISTEMA is cut or scrolled at this size");
      assert.deepEqual([v.labelled, v.focusable, v.loading], ["sistema-title", "-1", false], "the shell lands on the headline");
      assert.ok(v.styled.every((s) => /^(--[a-z-]+: [^;]+;\s*)+$/.test(s)), `the only style the page writes is a custom property: ${JSON.stringify(v.styled.slice(0, 3))}`);
      assert.deepEqual(v.blocks, ["run", "account", "sources", "errors", "limits"]);
      assert.ok(v.rows === 3 || (v.rows === 2 && width === 1440), `the three boards are listed, and on the laptop at least two (${v.rows})`);
      assert.equal(v.labels, true, "each number of a board says what it counts for the narrow page");
      if (shown) { assert.notEqual(v.kbd, "none", "the key is shown"); assert.equal(v.cols, 2); } else { assert.equal(v.kbd, "none", "a phone has no key"); assert.equal(v.cols, 1); assert.equal(v.foot, "none"); assert.equal(v.wide, true, "nothing sticks out to the side"); }
      assert.deepEqual(t.page.problems(), [], "no console error and no CSP violation");
    } finally { await t.close(); }
  });
}

test("loading: a skeleton until the first snapshot, never a spinner and never an empty instrument", async () => {
  const t = await open();
  try {
    await ev(t, `window.__lab.mountOnly()`);
    const v = await ev(t, `(() => { const q = (s) => document.querySelector(s); return { loading: q(".sistema").hasAttribute("data-loading"), busy: q(".sistema").getAttribute("aria-busy"), skeleton: document.querySelectorAll(".loading .skeleton").length,
      cols: getComputedStyle(q(".cols")).display, shown: getComputedStyle(q(".loading")).display }; })()`);
    assert.deepEqual([v.loading, v.busy, v.cols, v.shown], [true, "true", "none", "grid"]);
    assert.ok(v.skeleton >= 4);
    await load(t, "night");
    const after = await ev(t, `(() => { const q = (s) => document.querySelector(s); return { loading: q(".sistema").hasAttribute("data-loading"), busy: q(".sistema").hasAttribute("aria-busy"), skeleton: document.querySelectorAll(".loading").length, cols: getComputedStyle(q(".cols")).display }; })()`);
    assert.deepEqual([after.loading, after.busy, after.skeleton, after.cols], [false, false, 0, "grid"], "the first snapshot takes the skeleton away");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

// ---- the three truths about the engine (A15) -----------------------------------------------------------------------------------------

test("the engine: recent, stale and never each say their own thing, and nothing says what it cannot know", async () => {
  const t = await open();
  try {
    const read = () => ev(t, `(() => { const q = (s) => document.querySelector(s); const all = (s) => [...document.querySelectorAll(s)];
      return { h1: q(".mega").textContent, chip: q(".chip").textContent, engine: q(".sistema").dataset.engine, tone: q(".chip").dataset.tone, tailColor: getComputedStyle(q(".mega .tail")).color,
        task: q('.row[data-row="task"] .row-v').textContent, steps: all(".step").map((s) => [s.querySelector(".step-l").textContent, s.querySelector(".step-v").textContent]), account: q('.row[data-row="account"]').textContent,
        last: q('.row[data-row="last"] .row-v').textContent, fig: all(".fig").map((f) => f.textContent), all: q(".sistema").textContent }; })()`);
    await load(t, "night");
    const recent = await read();
    assert.equal(squash(recent.h1), "Última corrida hace 56 min.");
    assert.equal(squash(recent.chip), "Motor al día");
    assert.match(squash(recent.last), /02:04 \(hace 56 min\).*terminó bien/);
    assert.deepEqual(recent.steps, [["Descubrir", "3 avisos nuevos"], ["Preparar", "2 preparadas"], ["Enviar", "1 enviada, 1 para ti"], ["Dossiers", "2 escritos"]]);
    assert.equal(squash(recent.task), "programada, próxima 03:04");
    await load(t, "stale");
    const stale = await read();
    assert.equal(stale.engine, "stale");
    assert.equal(squash(stale.h1), "Última corrida hace 41 h.");
    assert.match(squash(stale.chip), /Motor atrasado: más de 36 h sin correr/);
    assert.notEqual(stale.tailColor, recent.tailColor, "an engine that missed a night does not shout like one that is fine");
    assert.equal(squash(stale.task), "sin tarea programada", "exists:false is a task that is not there");
    await load(t, "never");
    const never = await read();
    assert.equal(never.engine, "never");
    assert.equal(squash(never.h1), "Sin corridas todavía.");
    assert.equal(squash(never.last), "ninguna todavía", "no date is invented for a run that did not happen");
    assert.ok(never.steps.every(([, v]) => v === "sin dato"), "no step is made up for a run that never was");
    assert.match(squash(never.task), /^sin dato/, "null is 'sin dato' and never 'no existe'");
    assert.doesNotMatch(never.task, /no existe\b(?!:)|sin tarea programada/i);
    assert.match(squash(never.account), /sin verificar todavía/);
    for (const [name, v] of Object.entries({ recent, stale, never })) assert.doesNotMatch(v.all, SAYS_NOTHING, `${name} page`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the shapes of the dots are not colours: solid for what ran, a ring for what was skipped or is old, dotted for no data, a square for an error", async () => {
  const t = await open();
  try {
    await load(t, "night");
    await ev(t, `window.__lab.refresh((s) => { s.controls.last_run_steps.prepare = { ran: false, skipped: "cooldown" }; s.controls.last_run_steps.submit = { ran: false, skipped: "error" }; s.controls.last_run_steps.dossiers = null; })`);
    const shape = (sel) => ev(t, `(() => { const n = document.querySelector(${JSON.stringify(sel)}); const s = getComputedStyle(n, "::before"); return { bg: s.backgroundColor, radius: s.borderRadius, ring: s.boxShadow !== "none", dotted: s.borderStyle }; })()`);
    const solid = await shape('.step[data-step="discover"] .step-v');
    const ring = await shape('.step[data-step="prepare"] .step-v');
    const none = await shape('.step[data-step="dossiers"] .step-v');
    assert.notEqual(solid.bg, "rgba(0, 0, 0, 0)", "ran: a solid dot");
    assert.equal(ring.bg, "rgba(0, 0, 0, 0)", "skipped: a ring, not a dot");
    assert.equal(ring.ring, true);
    assert.equal(none.dotted, "dotted", "no data: a dotted ring");
    const tones = await ev(t, `[...document.querySelectorAll(".step")].map((n) => n.dataset.tone)`);
    assert.deepEqual(tones, ["ran", "skip", "bad", "none"]);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the result of the last run: ok, stopped, window closed and an error, the error with the reason as the server wrote it and in the errors of the night", async () => {
  const t = await open();
  try {
    await load(t, "night");
    const result = () => ev(t, `(() => { const r = document.querySelector(".result"); return { text: r.textContent, tone: r.dataset.tone, errors: [...document.querySelectorAll(".err")].map((e) => [e.dataset.rowId, e.textContent]) }; })()`);
    assert.deepEqual([(await result()).text, (await result()).tone], ["terminó bien", "ok"]);
    await ev(t, `window.__lab.refresh((s) => { s.controls.last_run_result = "stopped"; })`);
    assert.equal((await result()).text, "se detuvo por STOP");
    await ev(t, `window.__lab.refresh((s) => { s.controls.last_run_result = "window_closed"; })`);
    assert.equal((await result()).text, "se cerró la ventana a mitad de corrida");
    await ev(t, `window.__lab.refresh((s) => { s.controls.last_run_result = "error: el motor no pudo abrir la página <b>x</b>"; })`);
    const bad = await result();
    assert.equal(bad.tone, "bad");
    assert.equal(bad.text, "terminó con un error");
    const run = bad.errors.find(([id]) => id === "run");
    assert.ok(run, "the error of the run is the first of the errors of the night");
    assert.match(run[1], /no pudo abrir la página/);
    assert.equal(await ev(t, `document.querySelectorAll(".sistema b").length`), 0, "the reason is text, never markup");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the scheduled task: null says it does not know, exists:false says there is none, exists:true says when", async () => {
  const t = await open();
  try {
    await load(t, "night");
    const row = () => ev(t, `document.querySelector('.row[data-row="task"]').textContent`);
    await ev(t, `window.__lab.refresh((s) => { s.controls.scheduled_task = null; })`);
    const unknown = squash(await row());
    assert.match(unknown, /sin dato: el panel no pudo preguntarle a Windows/i);
    assert.doesNotMatch(unknown, /sin tarea programada/i, "not knowing is not the same as there being none");
    assert.match(unknown, /No quiere decir que no exista/);
    await ev(t, `window.__lab.refresh((s) => { s.controls.scheduled_task = { exists: false }; })`);
    assert.match(squash(await row()), /sin tarea programada/);
    await ev(t, `window.__lab.refresh((s) => { s.controls.scheduled_task = { exists: true, next_run_at: "2026-10-08T07:04:00Z" }; })`);
    assert.match(squash(await row()), /programada, próxima 04:04/);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the account of Claude: verified, or not and the way back in gold; spend with its meter and what each stage cost", async () => {
  const t = await open();
  try {
    await load(t, "night");
    const v = await ev(t, `(() => { const q = (s) => document.querySelector(s); return { account: q('.row[data-row="account"]').textContent, folder: q('.row[data-row="folder"] .row-v').textContent,
      access: q('.row[data-row="access"] .row-v').textContent, cli: q('.row[data-row="cli"] .row-v').textContent, spend: q('.row[data-row="spend"]').textContent,
      meter: q(".meter").style.getPropertyValue("--w"), hidden: document.querySelector(".fix").hidden }; })()`);
    assert.match(squash(v.account), /verificada.*d\*\*\*@example\.com · suscripción max/);
    assert.equal(v.folder, "%LOCALAPPDATA%\\demo\\claude");
    assert.match(v.access, /claude\.ai · firstParty · org org-demo/);
    assert.match(v.cli, /CLI 2\.1\.0 · comprobada hace 9 h/);
    assert.match(squash(v.spend), /US\$1,84 de US\$5,00/);
    assert.match(v.meter, /^3[67]/, "1,84 of 5,00 is 36.8 %");
    assert.equal(v.hidden, true, "no way back is offered to an account that is fine");
    await ev(t, `window.__lab.refresh((s) => { s.controls.claude_account = { ok: false, message: "La sesión no es la cuenta propia: corre jobbot claude-login", subscription: null, email: null, checked_at: "2026-10-08T05:00:00Z", auth_method: null, api_provider: null, org_id: null, config_dir: null, cli_version: null }; })`);
    const bad = await ev(t, `(() => { const q = (s) => document.querySelector(s); const fix = q(".fix"); return { account: q('.row[data-row="account"]').textContent, tone: q('.row[data-row="account"]').dataset.tone, fixColor: getComputedStyle(fix).color, fixShown: !fix.hidden,
      folderHidden: q('.row[data-row="folder"]').hidden }; })()`);
    assert.equal(bad.tone, "bad");
    assert.match(squash(bad.account), /no verificada.*La sesión no es la cuenta propia.*jobbot claude-login/);
    assert.equal(bad.fixShown, true);
    assert.equal(bad.fixColor, "rgb(251, 192, 36)", "the way back is gold: it is his to do");
    assert.equal(bad.folderHidden, true, "what the server did not send is not shown");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("limits: read only and honest, the batch threshold is 'sin dato' until something stores it, and the page offers no way to save", async () => {
  const t = await open();
  try {
    await load(t, "night");
    const v = await ev(t, `(() => { const cell = (id) => document.querySelector('[data-cell="' + id + '"]').textContent; return { title: document.querySelector(".limits .block-title").textContent, sends: cell("sends"), score: cell("score"), batch: cell("batch"), auto: cell("auto"),
      inputs: document.querySelectorAll(".sistema input, .sistema select, .sistema textarea").length, buttons: [...document.querySelectorAll(".limits button")].length }; })()`);
    assert.match(squash(v.title), /topes y umbrales\s*solo lectura, en profile\/\*\.toml/i);
    assert.match(squash(v.sends), /2 de 10.*llevas 3 de 5 aprobaciones; después, hasta 25/i);
    assert.match(squash(v.score), /60.*desde ahí un aviso pasa a «preparada»/i);
    assert.match(squash(v.batch), /sin dato.*preferencia de este navegador/i, "the batch threshold is a preference of the browser");
    assert.match(squash(v.auto), /apagada.*jobbot auto-approve.*aún no la usa el motor/i, "the server's threshold: off, how it is changed, and that the engine does not use it yet");
    assert.deepEqual([v.inputs, v.buttons], [0, 0], "nothing here pretends to save");
    await ev(t, `window.__lab.batchMin(85)`);
    assert.match(squash(await text(t, '[data-cell="batch"]')), /85/);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

// ---- the lists give way when the sheet has no room -------------------------------------------------------------------------------------

test("a night with many errors and boards: the lists are cut to what fits, say how many more, and fill the room when it grows, with no scroll at any size", async () => {
  const t = await open({ width: 1920, height: 1080 });
  try {
    await load(t, "heavy");
    await ev(t, `window.__lab.refresh((s) => { s.funnel.by_source = Object.fromEntries(Array.from({ length: 7 }, (_, i) => ["board" + i, { n: 30 - i, sent: 9 - i, answered: 1 }])); })`);
    const read = () => ev(t, `(() => { const host = document.querySelector("#host"); return { fits: host.scrollHeight <= host.clientHeight, errors: document.querySelectorAll(".err").length, boards: document.querySelectorAll(".sources-table tbody tr").length,
      more: [...document.querySelectorAll(".more")].filter((m) => !m.hidden).map((m) => m.textContent), count: document.querySelector(".errors .count").textContent }; })()`);
    const small = await read();
    assert.equal(small.fits, true, "heavy at 1920x1080 fits without a scroll");
    assert.ok(small.errors >= 1 && small.errors <= 3 && small.boards >= 2 && small.boards <= 5, JSON.stringify(small));
    assert.ok(small.more.length >= 1 && small.more.every((m) => /^y \d+ más$/.test(m)), "what is not listed is counted");
    assert.equal(small.count, "8", "the title of the errors keeps the whole count");
    await t.page.viewport(3440, 1440, { dpr: 1, mobile: false });
    await t.page.wait(300);
    const big = await read();
    assert.equal(big.fits, true);
    assert.ok(big.errors > small.errors && big.boards > small.boards, `a bigger window lists more (${JSON.stringify([small.errors, small.boards, big.errors, big.boards])})`);
    await t.page.viewport(1440, 900, { dpr: 1, mobile: false });
    await t.page.wait(300);
    const lap = await read();
    assert.equal(lap.fits, true, "and a laptop window gives them back");
    assert.ok(lap.errors <= small.errors && lap.boards <= small.boards);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

// ---- STOP: instant, with an undo; quitting STOP asks twice ---------------------------------------------------------------------------

const stopState = (t) => ev(t, `(() => { const q = (s) => document.querySelector(s); const row = q('.row[data-row="stop"]'); return { on: row.dataset.on, pending: row.dataset.pending || null, busy: row.getAttribute("aria-busy"),
  chip: !q(".chip-stop").hidden && q(".chip-stop").textContent.trim() !== "", label: q('button[data-action="engine"] .act-label').textContent, armed: q('button[data-action="engine"]').dataset.armed || null, dim: getComputedStyle(q('button[data-action="engine"]')).opacity,
  text: q(".stop-text").textContent }; })()`);

test("STOP by key p: the page shows it at once, the button is dim until the server answers, and there is an undo that quits it", async () => {
  const t = await open();
  try {
    await load(t, "night");
    assert.deepEqual([(await stopState(t)).on, (await stopState(t)).label], ["false", "Parar el motor"]);
    await ev(t, `window.__lab.hold()`);
    await key(t, "p");
    await t.page.wait(60);
    const mid = await stopState(t);
    assert.deepEqual([mid.on, mid.pending, mid.busy, mid.chip], ["true", "true", "true", true], "STOP is on in the page before the server has said anything");
    assert.ok(Number(mid.dim) < 0.5, "and the button is dim");
    assert.deepEqual((await ev(t, `window.__lab.state()`)).acts.map((a) => a.path), ["/api/stop"]);
    await key(t, "p");
    assert.equal((await ev(t, `window.__lab.state()`)).acts.length, 1, "a second press while it is pending is nothing");
    await ev(t, `window.__lab.release(true)`);
    await t.page.waitFor(`window.__lab.state().toasts.length > 0`);
    const done = await ev(t, `window.__lab.state()`);
    assert.deepEqual(done.sounds, ["stop"]);
    assert.equal(done.toasts.at(-1).undo, true, "the toast has an undo");
    assert.match(done.toasts.at(-1).text, /STOP puesto/);
    const after = await stopState(t);
    assert.deepEqual([after.on, after.pending, after.label], ["true", null, "Quitar STOP"]);
    assert.equal(await ev(t, `window.__lab.undo()`), true);
    await t.page.waitFor(`window.__lab.state().acts.length === 2`);
    assert.deepEqual((await ev(t, `window.__lab.state()`)).acts.map((a) => a.path), ["/api/stop", "/api/resume"], "the undo of STOP is RESUME, at once");
    await t.page.waitFor(`document.querySelector('.row[data-row="stop"]').dataset.on === "false"`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

for (const [status, name] of [[409, "a 409"], [500, "a 500"], [0, "no network"]]) {
  test(`STOP when the server answers ${name}: the page goes back to what it was, with the error sound, and says why once`, async () => {
    const t = await open();
    try {
      await load(t, "night");
      await ev(t, `window.__lab.hold()`);
      await key(t, "p");
      await t.page.wait(60);
      assert.equal((await stopState(t)).on, "true");
      await ev(t, `window.__lab.release(false, ${status})`);
      await t.page.waitFor(`window.__lab.state().sounds.length > 0`);
      const s = await ev(t, `window.__lab.state()`);
      assert.deepEqual(s.sounds, ["error"]);
      const after = await stopState(t);
      assert.deepEqual([after.on, after.pending, after.chip, after.label], ["false", null, false, "Parar el motor"], "STOP is not on if the server did not put it on");
      assert.ok(s.toasts.every((x) => !x.undo), "there is nothing to undo");
      const said = await ev(t, `document.querySelector(".toast").textContent`);
      assert.match(said, status === 0 ? /No se pudo hablar con el panel/ : status === 409 ? /No se pudo/ : /El servidor falló/);
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}

test("quitting STOP asks twice: the first press arms and says so, a second inside 350 ms is the same press, a deliberate one sends RESUME, and the button does the same as the key", async () => {
  const t = await open();
  try {
    await load(t, "stop");
    const first = await stopState(t);
    assert.deepEqual([first.on, first.label, first.armed], ["true", "Quitar STOP", null]);
    assert.match(squash(first.text), /puesto: no corre nada automático hasta que lo quites/);
    await key(t, "p");
    await key(t, "p");
    const armed = await stopState(t);
    assert.equal(armed.armed, "true", "the first press only arms");
    assert.equal(armed.label, "¿Seguro? Pulsa otra vez");
    assert.deepEqual((await ev(t, `window.__lab.state()`)).acts, [], "nothing has been sent: the second press was inside 350 ms");
    assert.deepEqual((await ev(t, `window.__lab.state()`)).said, ["¿Seguro? Pulsa otra vez"]);
    await t.page.wait(450);
    await t.page.click('button[data-action="engine"]');
    await t.page.waitFor(`window.__lab.state().acts.length === 1`);
    assert.deepEqual((await ev(t, `window.__lab.state()`)).acts.map((a) => a.path), ["/api/resume"]);
    await t.page.waitFor(`document.querySelector('.row[data-row="stop"]').dataset.on === "false"`);
    const done = await ev(t, `window.__lab.state()`);
    assert.equal(done.toasts.at(-1).undo, true);
    assert.match(done.toasts.at(-1).text, /STOP quitado/);
    assert.equal(await ev(t, `window.__lab.undo()`), true);
    await t.page.waitFor(`window.__lab.state().acts.length === 2`);
    assert.equal((await ev(t, `window.__lab.state()`)).acts[1].path, "/api/stop", "the undo of RESUME puts STOP back");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the arming of RESUME fades by itself after five seconds and a click elsewhere in the page does not send anything", async () => {
  const t = await open();
  try {
    await load(t, "stop");
    await key(t, "p");
    assert.equal((await stopState(t)).armed, "true");
    await t.page.click(".mega");
    assert.equal((await ev(t, `window.__lab.state()`)).acts.length, 0);
    await t.page.waitFor(`document.querySelector('button[data-action="engine"]').dataset.armed === undefined`, { timeout: 7000 });
    assert.equal((await stopState(t)).label, "Quitar STOP");
    assert.deepEqual((await ev(t, `window.__lab.state()`)).acts, []);
  } finally { await t.close(); }
});

// ---- the errors of the night open the card ---------------------------------------------------------------------------------------------

test("each error of a card has an Abrir that opens it; the error of the run has none", async () => {
  const t = await open();
  try {
    await load(t, "night");
    await ev(t, `window.__lab.refresh((s) => { s.controls.last_run_result = "error: boom"; })`);
    const rows = await ev(t, `[...document.querySelectorAll(".err")].map((e) => [e.dataset.rowId, !!e.querySelector("button[data-open]"), e.querySelector(".err-hour").textContent])`);
    assert.equal(rows[0][0], "run");
    assert.equal(rows[0][1], false, "the run itself is not a card");
    assert.ok(rows.slice(1).every(([, hasButton]) => hasButton));
    assert.ok(rows.every(([, , hour]) => /^\d\d:\d\d$/.test(hour)));
    const key0 = await ev(t, `document.querySelector(".err[data-row-id] button[data-open]").dataset.open`);
    await t.page.click(".err button[data-open]");
    const opened = (await ev(t, `window.__lab.state()`)).opened;
    assert.equal(opened.length, 1);
    assert.equal(opened[0][0], key0);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

// A trip mounts the moment halfway and its frames are counted (ACEPTACION A7): with the fonts all loaded, `document.fonts.ready` is already settled, and
// asking it anyway scheduled a second pass of the fit for the frame after every mount, which was a second long frame at CPU x4.
test("mounting SISTEMA with the fonts loaded asks for no frame of its own: the first fit is the fit (and a face that arrives later still fits it again)", async () => {
  const t = await open();
  try {
    await load(t, "night");
    await t.page.waitFor(`document.fonts.status === "loaded"`);
    await t.page.wait(300);
    await ev(t, `window.__raf = 0; (() => { const raw = window.requestAnimationFrame.bind(window); window.requestAnimationFrame = (fn) => { window.__raf += 1; return raw(fn); }; })()`);
    const before = await ev(t, `document.querySelector("#host").innerHTML.length`);
    await ev(t, `window.__lab.load("night")`);                       // the unmount, the mount and the first update, as a trip's swap does
    await t.page.wait(300);
    assert.equal(await ev(t, `window.__raf`), 0, "no requestAnimationFrame after a mount with every font loaded");
    assert.equal(await ev(t, `(() => { const h = document.querySelector("#host"); return h.scrollHeight <= h.clientHeight; })()`), true, "and the lists still fit");
    assert.equal(await ev(t, `document.querySelector("#host").innerHTML.length`), before, "the same page");
    // a face that arrives later changes how much room the text takes, and the moment still fits again on its own
    await ev(t, `document.fonts.dispatchEvent(new Event("loadingdone"))`);
    await t.page.waitFor(`window.__raf > 0`, { timeout: 2000 });
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("a refresh that changes nothing writes nothing: the page is patched, not rebuilt, and a number that changes leaves the button where it was", async () => {
  const t = await open();
  try {
    await load(t, "night");
    await ev(t, `window.__mut = []; window.__obs = new MutationObserver((r) => window.__mut.push(...r)); window.__obs.observe(document.querySelector("#host"), { subtree: true, childList: true, attributes: true, characterData: true })`);
    await ev(t, `window.__lab.refresh()`);
    await ev(t, `window.__lab.refresh()`);
    await t.page.wait(50);
    assert.equal(await ev(t, `window.__obs.takeRecords().length + window.__mut.length`), 0, "nothing was written");
    const button = await ev(t, `(() => { window.__btn = document.querySelector('button[data-action="engine"]'); return true; })()`);
    assert.equal(button, true);
    await ev(t, `window.__lab.refresh((s) => { s.claude.spent_24h = 2.5; })`);
    assert.equal(await ev(t, `window.__btn === document.querySelector('button[data-action="engine"]')`), true, "the same button node");
    assert.match(squash(await text(t, ".spend-text")), /US\$2,50/, "the spend is said once, in the account's row");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("an old picture says how old it is, and a window that is closed says when it opens", async () => {
  const t = await open();
  try {
    await load(t, "day", 12);
    const day = squash(await text(t, ".status"));
    assert.match(day, /Ventana cerrada, abre .*18:00/);
    await load(t, "night");
    await ev(t, `window.__lab.advance(20 * 60000)`);
    assert.match(squash(await text(t, ".status")), /hace 20 min|20 min/, "the snapshot is 20 minutes old and the page says so");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

// ---- language and theme --------------------------------------------------------------------------------------------------------------------

test("English says the same things with its own words, no key shows through, and the keys of the page come from the one table", async () => {
  const t = await open();
  try {
    for (const name of ["night", "stale", "never", "stop", "heavy"]) {
      await ev(t, `window.__lab.lang("es")`);
      await load(t, name);
      const es = squash(await text(t, ".sistema"));
      await ev(t, `window.__lab.lang("en")`);
      const en = squash(await text(t, ".sistema"));
      assert.notEqual(es, en, name);
      assert.doesNotMatch(en, SAYS_NOTHING, `${name} in English`);
      assert.doesNotMatch(es, SAYS_NOTHING, `${name} in Spanish`);
    }
    await ev(t, `window.__lab.lang("en")`);
    await load(t, "night");
    assert.equal(squash(await text(t, ".mega")), "Last run 56 min ago.");
    assert.equal(squash(await text(t, ".chip")), "Engine up to date");
    assert.equal(await text(t, 'button[data-action="engine"] .act-label'), "Stop the engine");
    assert.equal(await text(t, 'button[data-action="engine"] .k'), "p", "the key shown is the one of the table (A19)");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("Papel (the light sheet) swaps the ink and not one rule: the headline, the dots and the gold keep their meaning", async () => {
  const t = await open();
  try {
    await load(t, "night");
    const read = () => ev(t, `(() => { const q = (s) => document.querySelector(s); return { tail: getComputedStyle(q(".mega .tail")).color, dot: getComputedStyle(q(".chip .dot")).backgroundColor, key: getComputedStyle(q("button[data-action=engine] .k")).color, sheet: getComputedStyle(q(".sheet")).backgroundColor }; })()`);
    const dark = await read();
    await ev(t, `window.__lab.theme("light")`);
    const light = await read();
    assert.notEqual(dark.tail, light.tail);
    assert.notEqual(dark.sheet, light.sheet);
    assert.notEqual(light.dot, "rgba(0, 0, 0, 0)");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

const LOOSE = "window.__lab.refresh((s) => { s.controls.claude_account.anchored = false; s.controls.auto_approve_min = 85; })";

// ---- the anchor of the account and the auto-approval threshold (the two fields Foxtrot added) ------------------------------------------------------

test("an account that nothing pins down is a red warning in plain words, not gold, with a square for a shape; anchored is said quietly; no account says nothing of it", async () => {
  const t = await open();
  try {
    await load(t, "night");
    const read = () => ev(t, `(() => { const q = (s) => document.querySelector(s); const l = q(".loose"); const title = q(".loose-title"); return { shown: !l.hidden, anchor: q('.row[data-row="account"]').dataset.anchor || null, quiet: q(".acc-anchor").textContent,
      title: title.textContent, why: q(".loose-why").textContent, color: getComputedStyle(title).color, mark: getComputedStyle(title, "::before").backgroundColor, markRadius: getComputedStyle(title, "::before").borderRadius,
      role: l.getAttribute("role"), fixHidden: q(".fix").hidden, buttons: l.querySelectorAll("button, a, input").length }; })()`);
    const ok = await read();
    assert.deepEqual([ok.shown, ok.anchor, ok.quiet], [false, "ok", "anclada"], "anchored is said, quietly, and there is no warning");
    await ev(t, `window.__lab.refresh((s) => { s.controls.claude_account.anchored = false; })`);
    const loose = await read();
    assert.deepEqual([loose.shown, loose.anchor, loose.title], [true, "loose", "cuenta sin anclar"]);
    assert.equal(loose.quiet, "", "the quiet word goes when the warning comes");
    for (const w of ["expected_email", "expected_org_id", "profile/claude.local.toml", "nunca el que se sube", "se niega a correr con otra cuenta"]) assert.ok(loose.why.includes(w), `the way out says "${w}": ${loose.why}`);
    assert.equal(loose.color, "rgb(255, 138, 128)", "the alert colour of Tinta");
    assert.notEqual(loose.color, "rgb(251, 192, 36)", "not gold: gold is what is his to do, this is a thing that is wrong");
    assert.equal(loose.mark, loose.color, "and the shape is a square of the same colour, so it is not only a colour");
    assert.notEqual(loose.markRadius, "50%");
    assert.equal(loose.role, "note");
    assert.equal(loose.buttons, 0, "nothing here pretends to fix it: the file is edited by hand");
    await ev(t, `window.__lab.theme("light")`);
    assert.equal((await read()).color, "rgb(155, 28, 28)", "the alert colour of Papel");
    await ev(t, `window.__lab.theme("dark")`);
    // what the server did not send says nothing, and no account says "sin verificar" and nothing more
    await ev(t, `window.__lab.refresh((s) => { delete s.controls.claude_account.anchored; })`);
    const gone = await read();
    assert.deepEqual([gone.shown, gone.anchor, gone.quiet], [false, null, ""], "a field that did not come is not a verdict");
    await load(t, "never");
    const none = await read();
    assert.deepEqual([none.shown, none.anchor, none.quiet], [false, null, ""]);
    assert.match(squash(await text(t, '.row[data-row="account"]')), /sin verificar todavía/);
    await ev(t, `window.__lab.lang("en")`);
    await load(t, "night");
    await ev(t, `window.__lab.refresh((s) => { s.controls.claude_account.anchored = false; })`);
    const en = await read();
    assert.equal(en.title, "account not anchored");
    assert.match(en.why, /expected_email and expected_org_id in profile\/claude\.local\.toml.*never the one that is committed/);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the auto-approval threshold: off, 'desde N puntos' from the model, 'sin dato' when it did not come; read only, and honest that the engine does not use it yet", async () => {
  const t = await open();
  try {
    await load(t, "night");
    const cell = () => ev(t, `(() => { const c = document.querySelector('[data-cell="auto"]'); return { label: c.querySelector(".cell-l").textContent, value: c.querySelector(".cell-v").textContent, sub: c.querySelector(".cell-s").textContent, controls: c.querySelectorAll("button, input, select, a").length }; })()`);
    assert.deepEqual(await cell(), { label: "Auto-aprobación", value: "apagada", sub: "se cambia con jobbot auto-approve; aún no la usa el motor", controls: 0 });
    for (const [n, said] of [[85, "desde 85 puntos"], [1, "desde 1 punto"], [100, "desde 100 puntos"], [64, "desde 64 puntos"]]) {
      await ev(t, `window.__lab.refresh((s) => { s.controls.auto_approve_min = ${n}; })`);
      assert.equal((await cell()).value, said, "the number comes from the snapshot");
    }
    await ev(t, `window.__lab.refresh((s) => { delete s.controls.auto_approve_min; })`);
    assert.equal((await cell()).value, "sin dato");
    await ev(t, `window.__lab.refresh((s) => { s.controls.auto_approve_min = 250; })`);
    assert.equal((await cell()).value, "sin dato", "a number that is not a threshold is not shown as one");
    await ev(t, `window.__lab.refresh((s) => { s.controls.auto_approve_min = 70; })`);
    const order = await ev(t, `[...document.querySelectorAll(".cells .cell")].map((c) => c.dataset.cell)`);
    assert.deepEqual(order, ["sends", "score", "auto", "batch"], "the batch threshold stays its own cell, a preference of the browser");
    await ev(t, `window.__lab.lang("en")`);
    assert.deepEqual(await cell(), { label: "Auto-approval", value: "from 70 points", sub: "changed with jobbot auto-approve; not used by the engine yet", controls: 0 });
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

for (const [width, height] of [[1920, 1080], [3440, 1440], [1440, 900]]) {
  test(`with the warning of the account and the threshold on, SISTEMA still fits at ${width}x${height} and the warning is on screen`, async () => {
    const t = await open({ width, height });
    try {
      await load(t, "night");
      await ev(t, LOOSE);
      const v = await ev(t, `(() => { const host = document.querySelector("#host"); const w = document.querySelector(".loose-why").getBoundingClientRect(); const r = host.getBoundingClientRect();
        return { fits: host.scrollHeight <= host.clientHeight, shown: getComputedStyle(document.querySelector(".loose")).display !== "none" && w.height > 0, inside: w.bottom <= r.bottom + 1, auto: document.querySelector('[data-cell="auto"] .cell-v').textContent }; })()`);
      assert.equal(v.fits, true, "nothing is cut or scrolled");
      assert.deepEqual([v.shown, v.inside], [true, true]);
      assert.equal(v.auto, "desde 85 puntos");
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}

// ---- A18: every text keeps 4.5:1 over the glass and the sky, measured on the pixels Chrome really paints ----------------------------------------

const lum = ([r, g, b]) => { const l = (c) => ((c /= 255) <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4); return 0.2126 * l(r) + 0.7152 * l(g) + 0.0722 * l(b); };

/** For each text that is on screen: the contrast of its colour against the ground behind its glyphs (the frame with the glyphs made invisible). */
async function textContrast(t, scenario, hora, mutate = "") {
  await ev(t, `window.__lab.load(${JSON.stringify(scenario)}, ${hora})`);
  if (mutate) await ev(t, mutate);
  await t.page.waitFor(`document.fonts.status === "loaded"`);
  await t.page.wait(250);
  const boxes = (await ev(t, `window.__lab.textBoxes()`)).filter((b) => b.rects.length > 0);
  await ev(t, `window.__lab.ghost(true)`);
  const png = decodePng(Buffer.from((await t.page.send("Page.captureScreenshot", { format: "png" })).data, "base64"));
  await ev(t, `window.__lab.ghost(false)`);
  const out = [];
  for (const box of boxes) {
    const color = box.color.match(/\d+(\.\d+)?/g).map(Number).slice(0, 3);
    const grounds = [];
    for (const r of box.rects) {
      const x0 = Math.max(0, Math.floor(r.left) - 2), x1 = Math.min(png.w - 1, Math.ceil(r.right) + 2);
      const y0 = Math.max(0, Math.floor(r.top) - 2), y1 = Math.min(png.h - 1, Math.ceil(r.bottom) + 2);
      for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) grounds.push(png.at(x, y));
    }
    if (grounds.length <= 40) continue;
    grounds.sort((a, b) => lum(a) - lum(b));
    out.push({ text: box.text, cls: box.cls, ratio: contrast(color, grounds[Math.floor(grounds.length * 0.98)]) });
  }
  return out;
}

test("A18: every text of SISTEMA keeps 4.5:1 over the glass and the sky, in Tinta at night and by day, and in Papel, at 1920x1080 and 1440x900", async () => {
  for (const [width, height] of [[1920, 1080], [1440, 900]]) {
    const t = await open({ width, height });
    try {
      for (const [theme, hora, scenario, mutate] of [["dark", 3, "night"], ["dark", 9, "night"], ["dark", 12, "day"], ["dark", 17, "night"], ["dark", 3, "stale"], ["dark", 3, "stop"], ["light", 12, "night"], ["light", 3, "never"],
        ["dark", 3, "night", LOOSE], ["dark", 12, "day", LOOSE], ["dark", 17, "night", LOOSE], ["light", 12, "night", LOOSE], ["light", 3, "stale", LOOSE]]) {
        await ev(t, `window.__lab.theme(${JSON.stringify(theme)})`);
        const measured = await textContrast(t, scenario, hora, mutate);
        assert.ok(measured.length >= 40, `the headline, the figures, the rows and the lists (${measured.length})`);
        for (const r of measured) assert.ok(r.ratio >= 4.5, `"${r.text}" (${r.cls}) is ${r.ratio.toFixed(2)}:1 in ${theme}/${scenario} at ${hora}:00, ${width}x${height}`);
        if (mutate) {
          for (const cls of ["loose-title", "loose-why"]) assert.ok(measured.some((r) => r.cls.includes(cls)), `the warning of the account (${cls}) was measured, not skipped, in ${theme}/${scenario} at ${hora}:00`);
        }
      }
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  }
});
