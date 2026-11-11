// The acceptance matrix, part 5: what the mounted page looks like and what it says (spec A13 geometry, A15 the truth about the engine, A16 counts from the
// model, A17 the light). The geometry is read from getBoundingClientRect on the real shell at the five windows; the words come from the dictionaries (never
// copied by hand) and the data from the snapshots of test/fixtures varied by the test, served to the page as Rust would (kind "real").
import test from "node:test";
import assert from "node:assert/strict";
import { openPage, ev, run, press, travel, arrived, STOPS, fixture, answerWith } from "./helpers/acceptance.mjs";
import { polygonHitsRect } from "../src/stage/beam.js";
import { clockText } from "../src/stage/stage.js";

const OPENS = `!!document.querySelector("#app .sheet-host h1") && document.fonts.status === "loaded" && !!window.__app.model()`;
const dict = async (lang, scope) => (await import(`../src/i18n/${lang}/${scope}.js`)).default;
const clamp = (x, a, b) => Math.min(b, Math.max(a, x));

// ---- A13 ----------------------------------------------------------------------------------------------------------------------------------

const GEOMETRY = [[1920, 1080, 1114, 400, 455], [3440, 1440, 1400, 447, 629], [1440, 900, 960, 400, null], [1366, 768, 960, 400, null]];
for (const [width, height, sheetW, drawerW, ring] of GEOMETRY) {
  test(`A13 at ${width}x${height}: the sheet is ${sheetW} px at gutter ${Math.round(clamp(0.033 * width, 24, 64))}, the drawer ${drawerW} px right of it at 22 px without taking its width${ring ? `, the dial ring ${ring} px in HOY` : ""}, and the glass measures its content except in PROCESOS`, async () => {
    const t = await openPage({ query: "?debug=1&clock=9&lang=es", width, height });
    try {
      await t.page.waitFor(OPENS);
      const rect = (id) => run(t, () => { const a = document.getElementById("app").getBoundingClientRect(), host = document.querySelector("#app .sheet-host"); return { left: a.left, right: a.right, width: a.width, top: a.top, bottom: a.bottom, vh: innerHeight, over: host.scrollHeight - host.clientHeight, ring: document.querySelector(".dial .face")?.getBoundingClientRect().width }; });
      const gutter = clamp(0.033 * width, 24, 64);
      const wide = clamp(0.58 * width, 960, 1400);
      const wrong = [];
      const seen = {};
      for (const id of ["hoy", "procesos", "cola", "aprender", "sistema"]) {
        await travel(t, id);
        await t.page.wait(250);
        const r = seen[id] = await rect(id);
        if (Math.abs(r.width - Math.min(wide, width - 2 * gutter)) > 1.5) wrong.push(`${id}: the sheet is ${r.width.toFixed(1)} px wide, the clamp says ${Math.min(wide, width - 2 * gutter).toFixed(1)}`);
        if (Math.abs(r.left - gutter) > 1.5) wrong.push(`${id}: the sheet starts at ${r.left.toFixed(1)}, the gutter is ${gutter.toFixed(1)}`);
        if (width === 3440 && Math.abs(r.right - 1464) > 2) wrong.push(`${id}: its right edge is ${r.right.toFixed(0)}, the verdict says about 1460`);
        if (r.bottom > r.vh + 0.5) wrong.push(`${id}: the sheet ends at ${r.bottom}, below the window (${r.vh})`);
      }
      if (ring) {
        await travel(t, "hoy");
        await t.page.wait(250);
        const got = (await rect("hoy")).ring;
        if (Math.abs(got - ring) > 3) wrong.push(`the dial ring is ${got?.toFixed(0)} px in HOY, the spec says about ${ring}`);
      }
      // "en HOY y COLA el vidrio mide lo que su contenido y a 1080 de alto deja ciudad debajo; solo PROCESOS llega al pie"
      if (height >= 1080) {
        for (const id of ["hoy", "cola"]) if (seen[id].vh - seen[id].bottom < 80) wrong.push(`${id}: the glass ends ${Math.round(seen[id].vh - seen[id].bottom)} px above the foot, no city below it`);
        for (const id of ["hoy", "cola", "aprender", "sistema"]) if (seen[id].over > 1) wrong.push(`${id}: its content is ${seen[id].over} px taller than its glass (it scrolls inside)`);
      }
      if (seen.procesos.vh - seen.procesos.bottom > 40) wrong.push(`procesos: the glass stops ${Math.round(seen.procesos.vh - seen.procesos.bottom)} px above the foot, it should reach it`);
      assert.deepEqual(wrong, []);
      // the drawer: right of the sheet, 22 px away, as wide as --drawer, and the sheet does not give any width for it
      await travel(t, "procesos");
      await t.page.waitFor(`document.querySelectorAll("#app [data-card]").length > 0`);
      const before = (await rect("procesos")).width;
      await press(t, "j");
      await press(t, "Enter");
      await t.page.waitFor(`document.getElementById("detail").classList.contains("open")`);
      await t.page.wait(450);
      const d = await run(t, () => { const d = document.getElementById("detail").getBoundingClientRect(), a = document.getElementById("app").getBoundingClientRect(); return { left: d.left, right: d.right, width: d.width, gap: d.left - a.right, sheet: a.width, vw: innerWidth }; });
      if (width >= 1920) {
        assert.ok(Math.abs(d.width - drawerW) <= 1.5, `the drawer is ${d.width.toFixed(1)} px wide, --drawer is ${drawerW}`);
        assert.ok(Math.abs(d.gap - 22) <= 1.5, `the drawer is ${d.gap.toFixed(1)} px from the sheet, 22 in the spec`);
      }
      assert.ok(d.right <= d.vw + 0.5, `the drawer ends at ${d.right.toFixed(0)} inside a ${d.vw} px window`);
      assert.ok(Math.abs(d.sheet - before) <= 0.5, `opening the drawer left the sheet at ${before.toFixed(1)} px, it is ${d.sheet.toFixed(1)}`);
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}

// ---- A15 ----------------------------------------------------------------------------------------------------------------------------------

const HOUR = 3600000;
const NOW = "2026-10-07T15:00:00Z";                         // the generated_at of the fixtures: "7 h ago" is exactly that
const iso = (ms) => new Date(ms).toISOString().replace(".000Z", "Z");
const controlsOf = (patch) => { const s = fixture("rich"); Object.assign(s.controls, patch); return s; };
const when = (h) => iso(Date.parse(NOW) - h * HOUR);

async function engineText(snapshot, extra = "") {
  const t = await openPage({ kind: "real", query: `?t=tok&debug=1&clock=9&lang=es&now=${NOW}${extra}`, width: 1920, height: 1080, answer: answerWith(snapshot) });
  try {
    await t.page.waitFor(OPENS);
    const out = { hud: await ev(t, `document.querySelector(".hud-status")?.textContent || ""`) };
    for (const id of ["hoy", "cola", "sistema"]) { await travel(t, id); await t.page.wait(250); out[id] = await ev(t, `document.querySelector("#app").innerText.replace(/\\s+/g, " ")`); }
    out.problems = t.page.problems().filter((p) => !/status of 404.*\/api\/(job|shot)\?key=/.test(p.text));
    return out;
  } finally { await t.close(); }
}

test("A15: SISTEMA, COLA, HOY and the HUD tell the truth of the engine: never ran, ran 7 h ago, missed a night", async () => {
  const sis = await dict("es", "sistema"), cola = await dict("es", "cola"), hud = await dict("es", "hud"), hoy = await dict("es", "hoy");
  // the mono texts are upper-cased by the page (text-transform): everything is compared in lower case
  const has = (text, needle) => text.toLowerCase().includes(needle.toLowerCase());
  // a sentence of the dictionary with its {slots} as a pattern: the words around the slots are literal
  const tmpl = (s) => new RegExp(s.split(/\{\w+\}/).map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join(".+"), "i");
  // never: no run, no task data, no account (a fresh install)
  const never = await engineText(controlsOf({ engine: "never", last_run_at: null, scheduled_task: null, claude_account: null }));
  assert.ok(has(never.sistema, sis["sistema.head.never.lead"]) && has(never.sistema, sis["sistema.head.never.tail"]), "SISTEMA: Sin corridas todavía.");
  assert.ok(has(never.sistema, sis["sistema.chip.never"]), "the chip of the engine says it never ran");
  assert.ok(has(never.sistema, sis["sistema.task.unknown"]), "a task nobody asked Windows about is 'sin dato', not 'does not exist'");
  assert.ok(!has(never.sistema, sis["sistema.task.missing"]), "and never 'sin tarea programada'");
  assert.match(never.cola, tmpl(cola["cola.warn.never"]), "COLA says that approving sends nothing until the engine runs");
  assert.ok(has(never.hud, hud["hud.run.never"]), `the HUD: ${never.hud}`);
  assert.ok(has(never.hoy, hoy["hoy.foot.never"]) || has(never.hoy, hoy["hoy.calm.never"]), "HOY says it too");
  assert.deepEqual(never.problems, []);
  // stale: the last run is 41 h old and there is no task registered
  const stale = await engineText(controlsOf({ engine: "stale", last_run_at: when(41), scheduled_task: { exists: false, next_run_at: null } }));
  assert.ok(has(stale.sistema, sis["sistema.chip.stale"].replace("{hours}", "36")), `SISTEMA's chip: ${stale.sistema.slice(0, 200)}`);
  assert.ok(has(stale.sistema, sis["sistema.task.missing"]) && !has(stale.sistema, sis["sistema.task.unknown"]), "exists:false is 'sin tarea programada' and not 'sin dato'");
  assert.match(stale.cola, tmpl(cola["cola.warn.stale"]), "COLA warns that the engine missed a night");
  assert.deepEqual(stale.problems, []);
  // recent, with the task unknown (null) and then registered (exists:true)
  const recent = await engineText(controlsOf({ engine: "recent", last_run_at: when(7), scheduled_task: null }));
  assert.ok(has(recent.sistema, sis["sistema.head.run.lead"]), "SISTEMA: Última corrida");
  assert.ok(has(recent.sistema, sis["sistema.task.unknown"]) && !has(recent.sistema, sis["sistema.task.missing"]), "null is 'sin dato' even when the engine is fine");
  assert.ok(!tmpl(cola["cola.warn.stale"]).test(recent.cola) && !tmpl(cola["cola.warn.never"]).test(recent.cola), "a recent engine has no warning in COLA");
  assert.match(recent.hud, /corrida.*7 ?h/i, `the HUD words the run from last_run_at: ${recent.hud}`);
  assert.match(recent.hoy, /última corrida.*7 ?h/i, "and so does HOY");
  const registered = await engineText(controlsOf({ engine: "recent", last_run_at: when(7), scheduled_task: { exists: true, next_run_at: iso(Date.parse(NOW) + 11 * HOUR) } }));
  assert.match(registered.sistema, /programada/i);
  assert.ok(!has(registered.sistema, sis["sistema.task.missing"]) && !has(registered.sistema, sis["sistema.task.unknown"]));
  assert.deepEqual(registered.problems, []);
});

// ---- A16 ----------------------------------------------------------------------------------------------------------------------------------

/** What the page says its counts are: the headline number, and the three counts of the stops bar. */
async function counts(snapshot) {
  const t = await openPage({ kind: "real", query: `?t=tok&debug=1&clock=9&lang=es&now=${NOW}`, width: 1920, height: 1080, answer: answerWith(snapshot) });
  try {
    await t.page.waitFor(OPENS);
    await t.page.wait(250);
    // with something to do the headline is one run that ends in the number ("Te tocan trece."); with nothing it is the calm title ("Nada pendiente.") and the count is zero
    const v = await run(t, () => ({ calm: document.querySelector("#app .sheet-host h1")?.textContent.trim() ?? "", bar: Object.fromEntries([...document.querySelectorAll("#stops [role=tab]")].map((el) => [el.dataset.stop, el.querySelector(".stop-count")?.textContent.trim() ?? null])) }));
    const words = await dict("es", "hoy");
    const byWord = new Map(Object.entries(words).filter(([k]) => k.startsWith("hoy.num.")).map(([k, w]) => [w, Number(k.slice(8))]));
    const word = v.calm === words["hoy.calm.title"] ? "" : v.calm.replace(/\.$/, "").split(/\s+/).pop();
    const n = word === "" ? 0 : /^\d+$/.test(word) ? Number(word) : byWord.get(word);
    return { n, word: word || v.calm, procesos: Number(v.bar.procesos), cola: Number(v.bar.cola), aprender: Number(v.bar.aprender) };
  } finally { await t.close(); }
}

test("A16: the counts of HOY and of the stops bar follow the snapshot: more tasks, approvals, stuck forms, live processes and due questions move exactly their own number", async () => {
  const base = await counts(fixture("rich"));
  assert.ok(Number.isInteger(base.n) && base.n > 0, `HOY says ${base.word}`);
  const bump = async (mutate) => { const s = fixture("rich"); mutate(s); return counts(s); };
  assert.equal((await bump((s) => { s.today.tasks_total += 3; })).n, base.n + 3, "three more tasks");
  assert.equal((await bump((s) => { s.awaiting_approval.total += 2; })).n, base.n + 2, "two more waiting for his approval");
  assert.equal((await bump((s) => { s.needs_gonza.total += 4; })).n, base.n + 4, "four more stuck forms");
  const live = await bump((s) => { s.board.find((c) => c.state === "submitted").total += 7; s.awaiting_approval.total += 2; s.learn.due += 5; });
  assert.deepEqual([live.procesos, live.cola, live.aprender], [base.procesos + 7, base.cola + 2, base.aprender + 5], "the bar: PROCESOS the live total, COLA the approvals, APRENDER what is due");
  const heavy = await counts(fixture("heavy"));
  const f = fixture("heavy");
  assert.equal(heavy.procesos, ["submitted", "replied", "technical_test", "interview", "offer", "ghosted"].reduce((n, st) => n + f.board.find((c) => c.state === st).total, 0), "PROCESOS counts every live process of the heavy board, none cut at the 25 of the closed columns");
  assert.equal(heavy.cola, f.awaiting_approval.total);
  const empty = await counts(fixture("empty"));
  assert.deepEqual([empty.n, empty.procesos, empty.cola, empty.aprender], [0, 0, 0, 0], `an empty install counts zero everywhere (${empty.word})`);
  // above twenty the headline is the figure, below it a word
  assert.equal((await bump((s) => { s.today.tasks_total += 30; })).word, String(base.n + 30), "above 20 it is the figure");
});

// ---- A17 ----------------------------------------------------------------------------------------------------------------------------------

const BEAMS = [[9, "sun"], [0.43, "window"], [18, "horizon"], [22, "window"]];
for (const [width, height] of [[1920, 1080], [3440, 1440]]) {
  test(`A17 at ${width}x${height} on the mounted page: at 09:00, 00:26, 18:00 and 22:00 the light is born at a body you can see, its polygon never touches the sheet and not a visible pixel of the sheet's side of the sky is lit`, async () => {
    const t = await openPage({ query: "?debug=1&clock=9&lang=es", width, height });
    try {
      const wrong = [];
      for (const [hour, kind] of BEAMS) {
        await t.page.goto(`${t.url.split("?")[0]}?debug=1&clock=${hour}&lang=es`);
        await t.page.waitFor(OPENS);
        await t.page.wait(450);
        for (const stop of ["hoy", "procesos"]) {
          if (stop === "procesos") { await travel(t, "procesos"); await t.page.wait(250); }
          const v = await run(t, () => {
            const g = document.getElementById("sky").getContext("2d"), c = document.getElementById("sky"), dpr = c.width / innerWidth;
            const S = __app.scene, hora = __app.clock.hora(), sheet = document.getElementById("app").getBoundingClientRect();
            const snap = () => g.getImageData(0, 0, c.width, c.height).data;
            __app.stage.render({ pos: S.pos, hora, noBeam: true });
            const dark = snap();
            // The stage paints the city in two layers: the far one on #sky and the near ones on .sky-front, over the dial (so a dial sunk into the city hides behind them).
            // A window of a near building is therefore not on #sky; the body of the light is looked for in the picture a person sees: #sky with that layer over it.
            const front = document.querySelector("#stage .sky-front");
            let seen = dark;
            if (front) { const m = document.createElement("canvas"); m.width = c.width; m.height = c.height; const mg = m.getContext("2d"); mg.drawImage(c, 0, 0); mg.drawImage(front, 0, 0); seen = mg.getImageData(0, 0, m.width, m.height).data; }
            const out = __app.stage.render({ pos: S.pos, hora });
            const lit = snap();
            const edge = Math.floor(sheet.right * dpr);
            // A pixel counts when it moves by more than 6 levels of 255. Adding a light makes Chrome rasterise the canvas again and the one-pixel edges of the small
            // windows of the far skyline (anti-aliasing at fractional coordinates) move by tens of levels at scattered pixels, 1 to 4 in a 16x16 block, which nobody can see;
            // a light is a smooth glow that moves hundreds of pixels per block. So left of the sheet's edge a BLOCK is lit when more than 24 of its 256 pixels moved.
            let outside = 0, inside = 0;
            const BLOCK = 16, perBlock = new Map();
            for (let y = 0; y < c.height; y++) for (let x = 0; x < c.width; x++) {
              const i = (y * c.width + x) * 4;
              if (Math.max(Math.abs(dark[i] - lit[i]), Math.abs(dark[i + 1] - lit[i + 1]), Math.abs(dark[i + 2] - lit[i + 2])) <= 6) continue;
              if (x >= edge) { outside++; continue; }
              const k = Math.floor(y / BLOCK) * 4096 + Math.floor(x / BLOCK);
              perBlock.set(k, (perBlock.get(k) || 0) + 1);
            }
            for (const n of perBlock.values()) if (n > 24) inside++;
            // the body: a window is a lit pixel at the origin of the beam in the picture WITHOUT the light
            const o = out.beam ? out.beam.beam : null;
            let around = Infinity, at = 0;
            if (o) { const px = (x, y) => { const i = (Math.round(y * dpr) * c.width + Math.round(x * dpr)) * 4; return seen[i] + seen[i + 1] + seen[i + 2]; }; at = px(o.x, o.y); for (let dy = -4; dy <= 4; dy++) for (let dx = -4; dx <= 4; dx++) around = Math.min(around, px(o.x + dx, o.y + dy)); }
            return { kind: out.beam ? out.beam.kind : null, polygon: out.beam ? out.beam.polygon : null, origin: o ? { x: o.x, y: o.y } : null, hub: { x: out.layout.cx, y: out.layout.cy }, sheet: { left: sheet.left, right: sheet.right, top: sheet.top, bottom: sheet.bottom }, inside, outside, at, around, vw: innerWidth, vh: innerHeight };
          });
          const label = `${clockText(hour)} ${stop}`;
          if (v.kind !== kind) { wrong.push(`${label}: the body is ${v.kind}, it should be the ${kind}`); continue; }
          if (!v.polygon || v.polygon.length < 3) { wrong.push(`${label}: no light at ${width}x${height}`); continue; }
          if (polygonHitsRect(v.polygon, v.sheet)) wrong.push(`${label}: the polygon of the light reaches the sheet`);
          if (v.polygon.some(([x]) => x < v.sheet.right - 0.5)) wrong.push(`${label}: a corner of the polygon is left of the sheet's edge`);
          if (v.inside !== 0) wrong.push(`${label}: ${v.inside} blocks of 16x16 px left of the sheet's edge are lit by the light`);
          if (v.outside < 2000) wrong.push(`${label}: the light is not really painted (${v.outside} pixels)`);
          if (kind === "sun" && (Math.abs(v.origin.x - v.hub.x) > 1.5 || Math.abs(v.origin.y - v.hub.y) > 1.5)) wrong.push(`${label}: the sun is at ${v.origin.x},${v.origin.y}, the hub of the dial at ${v.hub.x},${v.hub.y}`);
          if (kind === "window" && !(v.at >= 60 && v.at >= 2 * v.around)) wrong.push(`${label}: nothing lit at the origin of the light (${v.at} there, ${Math.round(v.around)} at the darkest of the 9x9 around it): the window is not there`);
          if (kind === "horizon" && v.origin.y < 0.85 * v.vh) wrong.push(`${label}: the horizon light is born at y ${Math.round(v.origin.y)} of ${v.vh}`);
          if (v.origin.x < v.sheet.right) wrong.push(`${label}: the body is left of the sheet's edge`);
        }
      }
      assert.deepEqual(wrong, []);
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}
