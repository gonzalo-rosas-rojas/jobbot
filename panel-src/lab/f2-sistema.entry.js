// Lab "f2-sistema": the real SISTEMA moment on the real sheet, over the real stage, driven by the real key table, with a fake server that
// applies STOP and RESUME to the snapshot. Driven through window.__lab by test/moments.sistema.browser.test.mjs and by hand for screenshots.
//   __lab.load(name, hour?)   a scenario of test/helpers/sistema-core.mjs: night | stale | never | stop | heavy | day, or a fixture: rich | empty | heavy0
//                             (the stage is put at the scenario's own hour in Santiago, or at `hour`, decimal)
//   __lab.refresh(fn?)        the next poll: `fn(snapshot)` may change it first; the moment is updated like the shell does
//   __lab.hold() / release(ok, status)   the server answers only when released (to look at the dim button); ok=false answers with `status` (409, 500, 0 = no network)
//   __lab.now(ms) / advance(ms)          the page's clock (the snapshot keeps its own `generated_at`)
//   __lab.undo()              presses "Deshacer" of the last toast that has one
//   __lab.state()             { acts, toasts, said, sounds, opened } everything the fake ctx was asked, in order
//   __lab.lang(code) / reduced(bool) / theme("light" | "dark") / batchMin(n | null) / snapshot()
//   __lab.textBoxes() / ghost(on)        the measure of A18 over the glass (test/moments.sistema.browser.test.mjs)
import sistema from "../src/moments/sistema.js";
import { model, stopCounts } from "../src/core/model.js";
import { installKeys, KEYS } from "../src/ui/keys.js";
import { createStage, clockText } from "../src/stage/stage.js";
import { mulberry32 } from "../src/stage/skyline.js";
import { createHud } from "../src/ui/hud.js";
import { createStops } from "../src/ui/stops.js";
import { createBar } from "../src/ui/bar.js";
import { createToast } from "../src/ui/toast.js";
import { STOP_IDS } from "../src/scene/poses.js";
import { horaOf } from "../src/scene/clock.js";
import { t, setLang, setTimeZone } from "../src/i18n/index.js";
import { scenariosOf } from "../test/helpers/sistema-core.mjs";
import empty from "../test/fixtures/snapshot.empty.json";
import rich from "../test/fixtures/snapshot.rich.json";
import heavy from "../test/fixtures/snapshot.heavy.json";

const SCENARIOS = scenariosOf({ rich, empty, heavy });
setTimeZone("America/Santiago");

let snap = null;
let nowMs = 0;
let isReduced = false;
let holding = null;      // a promise the fake server waits on
let release = null;
let refuse = null;       // null | { status }
let batch = null;
const log = { acts: [], toasts: [], said: [], sounds: [], opened: [] };

// ---- the fake server: what STOP and RESUME do to the snapshot, and the poll that follows
function applyAction(path) {
  if (path === "/api/stop") snap.controls.stopped = true;
  if (path === "/api/resume") snap.controls.stopped = false;
}

const live = document.getElementById("live");
const toast = createToast({ say: (text) => { live.textContent = text; } });
document.getElementById("shell").append(toast.el);
let lastUndo = null;

const session = {
  fresh: new Set(),
  async act(path, body, opts = {}) {
    log.acts.push({ path, body });
    if (holding) await holding;
    const bad = refuse;
    refuse = null;
    const r = bad
      ? { ok: false, status: bad.status, data: { message: bad.status === 0 ? "No se pudo hablar con el panel: ¿se cerró jobbot panel?" : bad.status === 409 ? "No se pudo." : "El servidor falló." } }
      : { ok: true, status: 200, data: {} };
    // the real session says what happened in a toast of its own, then reads again
    toast.show(r.data.message || "Hecho.", r.ok);
    if (opts.beforeRefresh) await opts.beforeRefresh(r);
    if (r.ok) applyAction(path);
    update();
    return { ...r, message: r.data.message || "" };
  },
};

const ctx = {
  session,
  sound: { play: (name) => log.sounds.push(name) },
  announce: (text) => { live.textContent = text; log.said.push(text); },
  open: (key, row) => log.opened.push([key, row]),
  go: () => {},
  toast: (text, ok, extra) => { lastUndo = extra && extra.undo ? extra.undo : null; log.toasts.push({ text, ok, undo: !!lastUndo }); toast.show(text, ok, extra || {}); },
  reduced: () => isReduced,
  now: () => nowMs,
  settings: { get: (key) => (key === "batchMin" ? batch : undefined) },
};

// ---- the page around it (the same pieces as the HOY lab), so the sheet is judged against the sky it will have
const sheetEl = document.getElementById("app");
const canvas = document.getElementById("sky");
const stage = createStage({ canvas, dialHost: document.getElementById("dial"), t, getSheetRect: () => sheetEl.getBoundingClientRect(), rand: mulberry32(20261007) });
const hud = createHud({ onSearch: () => {} });
const stops = createStops({ onSelect: () => {} });
const bar = createBar({ onHelp: () => {} });
hud.center.append(stops.el);
document.getElementById("shell").prepend(hud.el);
document.getElementById("shell").append(bar.el);
let hora = 3;
stops.setActive("sistema");
function frame() {
  hud.setStop("sistema");
  hud.setClock(clockText(hora));
  stage.resize();
  stage.render({ pos: STOP_IDS.indexOf("sistema"), hora });
  if (snap) { hud.setStatus(snap, nowMs); stops.setCounts(stopCounts(model(snap))); }
}
window.addEventListener("resize", frame);

// ---- the keyboard, wired the way the shell wires it: the table decides which key is which verb, SISTEMA does the verb
const handlers = {};
for (const entry of KEYS) {
  if ((entry.scope === "row" || entry.scope === "stop") && (entry.stops || []).includes("sistema")) handlers[entry.id] = (key, event) => sistema.onKey(entry.id, null, event);
}
installKeys({ handlers, stop: () => "sistema" });

function update() {
  if (!snap) return;
  sistema.update(model(snap), ctx);
  frame();
}

window.__lab = {
  load(name, at) {
    const fn = SCENARIOS[name];
    snap = JSON.parse(JSON.stringify(fn ? fn() : name === "rich" ? rich : name === "empty" ? empty : heavy));
    nowMs = Date.parse(snap.generated_at);
    hora = at != null ? at : horaOf(nowMs);
    log.acts.length = log.toasts.length = log.said.length = log.sounds.length = log.opened.length = 0;
    lastUndo = null;
    batch = null;
    holding = null;
    refuse = null;
    sistema.unmount();
    sistema.mount(document.getElementById("host"), ctx);
    update();
  },
  mountOnly() { sistema.unmount(); sistema.mount(document.getElementById("host"), ctx); frame(); },
  refresh(fn) { if (fn) fn(snap); update(); },
  hold() { holding = new Promise((resolve) => { release = resolve; }); },
  release(ok = true, status = 409) { refuse = ok ? null : { status }; const go = release; holding = null; release = null; if (go) go(); },
  now(ms) { nowMs = ms; },
  advance(ms) { nowMs += ms; update(); },
  undo() { const run = lastUndo; lastUndo = null; if (run) run(); return !!run; },
  state: () => JSON.parse(JSON.stringify(log)),
  snapshot: () => snap,
  lang(code) { setLang(code); hud.relabel(); stops.relabel(); bar.relabel(); update(); },
  reduced(on) { isReduced = !!on; },
  theme(name) { document.documentElement.setAttribute("data-theme", name); },
  batchMin(n) { batch = n; update(); },
  go(at) { hora = at; frame(); },
  // For the contrast test (A18): where each text of the moment is painted, its colour, and a switch that makes the glyphs and the underlines
  // invisible, so a screenshot shows only what they are painted on (the glass over the sky).
  textBoxes: () => [...document.querySelectorAll("#host *")].flatMap((el) => [...el.childNodes].filter((n) => n.nodeType === 3 && n.textContent.trim()).map((n) => {
    const range = document.createRange();
    range.selectNodeContents(n);
    return { cls: String(el.className), text: n.textContent.trim().slice(0, 40), color: getComputedStyle(el).color, rects: [...range.getClientRects()].map((r) => ({ left: r.left, top: r.top, right: r.right, bottom: r.bottom })) };
  })),
  ghost(on) {
    for (const el of document.querySelectorAll("#host *")) {
      if (on) { el.style.setProperty("color", "transparent", "important"); el.style.setProperty("border-color", "transparent", "important"); el.style.setProperty("background-image", "none", "important"); }
      else { el.style.removeProperty("color"); el.style.removeProperty("border-color"); el.style.removeProperty("background-image"); }
    }
  },
};
frame();
