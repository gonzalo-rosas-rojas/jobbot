// Lab "f2-aprender": the real APRENDER moment on the real sheet, over the real stage, driven by the real key table and cursor, with a fake
// server that does what the real one would to the snapshot (resolve gives forms back to the queue, grade moves the review on) and answers the
// dossier and the review. Driven through window.__lab by test/moments.aprender.browser.test.mjs and by hand for screenshots.
//   __lab.load(name, hour?)   a scenario: rich | plain | bare | hostile | calm | nolearn | stale | empty | heavy. `hour` (decimal, Santiago) puts the page's clock, the
//                             whole snapshot (every date in it moves with it) and the stage at that hour: the night desk is judged at 22
//   __lab.refresh(fn?)        the next poll: `fn(snapshot)` may change it first; the moment is updated like the shell does
//   __lab.hold() / release(ok, status)   the server answers an action only when released (to look at the dim row); ok=false answers with `status`
//   __lab.dossier(mode)       what /api/job answers for the dossier: "ok" | "error" | "none" | "hold" (it waits for dossierRelease())
//   __lab.queue(cards?)       the review's cards (the server's queue, which moves only when one is graded); `failNext(bool)` makes /api/learn/next fail
//   __lab.now(ms) / advance(ms)   the page's clock          __lab.state()   { acts, apis, toasts, said, sounds, opened, links } everything the fake ctx was asked
//   __lab.lang(code) / reduced(bool) / theme("light" | "dark") / snapshot() / go(hour)
//   __lab.cursor() / cursorTo("t:miss:..." | "c:fx:4")   where the keyboard's cursor is, and putting it there
//   __lab.textBoxes() / ghost(on)    the measure of A18 over the glass (test/moments.aprender.browser.test.mjs)
import aprender from "../src/moments/aprender.js";
import { model, stopCounts } from "../src/core/model.js";
import { installKeys, KEYS } from "../src/ui/keys.js";
import { createCursor } from "../src/ui/cursor.js";
import { createStage, clockText } from "../src/stage/stage.js";
import { mulberry32 } from "../src/stage/skyline.js";
import { createHud } from "../src/ui/hud.js";
import { createStops } from "../src/ui/stops.js";
import { createBar } from "../src/ui/bar.js";
import { createToast } from "../src/ui/toast.js";
import { STOP_IDS } from "../src/scene/poses.js";
import { t, setLang, setTimeZone } from "../src/i18n/index.js";
import { richOf, bareOf, hostileOf, DOSSIER_MD } from "../test/helpers/aprender-scenarios.mjs";
import empty from "../test/fixtures/snapshot.empty.json";
import rich from "../test/fixtures/snapshot.rich.json";
import heavy from "../test/fixtures/snapshot.heavy.json";

setTimeZone("America/Santiago");
const HOUR = 3600000;
const copy = (x) => JSON.parse(JSON.stringify(x));
const ISO = /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d+)?Z$/;
/** Moves every timestamp of the snapshot by `delta` ms: the same data as if it had been taken at another hour. */
function retime(value, delta) {
  if (typeof value === "string") return ISO.test(value) ? new Date(Date.parse(value) + delta).toISOString().replace(".000Z", "Z") : value;
  if (Array.isArray(value)) return value.map((v) => retime(v, delta));
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, retime(v, delta)]));
  return value;
}

const SCENARIOS = {
  rich: () => richOf(rich),
  bare: () => bareOf(empty),
  hostile: () => hostileOf(rich),
  calm: () => {
    const s = richOf(rich);
    s.needs_gonza = { total: 0, cards: [] };
    s.board.find((c) => c.state === "needs_gonza").cards = [];
    s.board.find((c) => c.state === "needs_gonza").total = 0;
    s.learn.due = 0;
    s.learn.topics.forEach((x) => { x.due = 0; });
    return s;
  },
  nolearn: () => { const s = richOf(rich); delete s.learn; return s; },
  stale: () => { const s = richOf(rich); s.controls.engine = "stale"; s.controls.last_run_at = new Date(Date.parse(s.generated_at) - 30 * HOUR).toISOString().replace(".000Z", "Z"); return s; },
  plain: () => copy(rich),   // the shared fixture as it is: what the server sends, with topic names the page did not choose
  empty: () => copy(empty),
  heavy: () => copy(heavy),
};

let snap = null;
let nowMs = 0;
let isReduced = false;
let holding = null;      // a promise the fake server waits on before it answers an action
let release = null;
let refuse = null;       // null | { status }
let dossierMode = "ok";
let dossierHold = null;
let dossierRelease = null;
let failNext = false;
let queue = [];
const log = { acts: [], apis: [], toasts: [], said: [], sounds: [], opened: [], links: [] };

const sampleQueue = () => {
  const cards = [
    { id: "c1", topic: "rust", question: "What is a lifetime in Rust, and why does the borrow checker need them?", offer: null },
    { id: "c2", topic: "system-design", question: "How would you design a pipeline that tolerates duplicated and out-of-order events?", offer: "Delta Test Co" },
    { id: "c3", topic: "sql", question: "When does a window function beat a self join?", offer: null },
    { id: "c4", topic: "rust", question: "Explain the difference between Arc and Rc, and when each is the right choice for a long running service that shares configuration between threads, tasks and plugins.", offer: null },
    { id: "c5", topic: "typescript", question: "Why is `unknown` safer than `any`?", offer: null },
  ];
  for (let i = 6; i <= 12; i += 1) cards.push({ id: "c" + i, topic: "devops", question: "Sample question " + i + " about devops?", offer: null });
  return cards;
};
const ANSWERS = {
  c1: "A lifetime names how long a reference is valid. The borrow checker uses them to prove a reference never outlives what it points to.\n\nMost are inferred; you write them when the compiler cannot tell which input a returned reference borrows from.",
  c2: "Make the consumer idempotent: a dedupe key per event, a watermark for lateness, and an ordered log per key. Reorder in a small window, not forever.",
  c3: "When you need a value per row computed over its neighbours, without collapsing the rows: running totals, ranks, gaps.",
};

// ---- the fake server: what resolve and grade do to the snapshot, and the poll that follows
function applyAction(path, body) {
  if (path === "/api/resolve") {
    const keys = new Set(body.keys || [body.key]);
    for (const list of [snap.needs_gonza, snap.board.find((c) => c.state === "needs_gonza")]) {
      const before = list.cards.length;
      list.cards = list.cards.filter((c) => !keys.has(c.key));
      list.total -= before - list.cards.length;
    }
  }
  if (path === "/api/learn/grade" && snap.learn) {
    const done = queue.shift();
    snap.learn.due = Math.max(0, snap.learn.due - 1);
    const topic = done && snap.learn.topics.find((x) => x.topic === done.topic);
    if (topic) { topic.due = Math.max(0, topic.due - 1); topic.seen += 1; }
    snap.learn.seen += 1;
  }
}

const live = document.getElementById("live");
const toast = createToast({ say: (text) => { live.textContent = text; } });
document.getElementById("shell").append(toast.el);

const session = {
  fresh: new Set(),
  said: (r, fallback) => (r.data && r.data.message) || fallback,
  async api(path) {
    log.apis.push(path);
    if (path.startsWith("/api/job")) {
      if (dossierMode === "hold") await dossierHold;
      if (dossierMode === "error") return { ok: false, status: 404, data: { message: "No existe esa postulación" } };
      return { ok: true, status: 200, data: { dossier: dossierMode === "none" ? null : DOSSIER_MD } };
    }
    if (path.startsWith("/api/learn/next")) {
      if (failNext) return { ok: false, status: 500, data: { message: "El servidor falló." } };
      const topic = path.includes("?topic=") ? decodeURIComponent(path.split("?topic=")[1]) : "";
      return { ok: true, status: 200, data: { card: queue.find((c) => !topic || c.topic === topic) || null } };
    }
    if (path.startsWith("/api/learn/answer")) {
      const id = decodeURIComponent(path.split("id=")[1]);
      return { ok: true, status: 200, data: { answer: ANSWERS[id] || null } };
    }
    return { ok: false, status: 404, data: { message: "No existe." } };
  },
  async act(path, body, opts = {}) {
    log.acts.push({ path, body });
    if (holding) await holding;
    const bad = refuse;
    refuse = null;
    const r = bad
      ? { ok: false, status: bad.status, data: { message: bad.status === 0 ? "No se pudo hablar con el panel: ¿se cerró jobbot panel?" : bad.status === 409 ? "No se pudo." : "El servidor falló." } }
      : { ok: true, status: 200, data: {} };
    toast.show(r.data.message || "Hecho.", r.ok);
    if (r.ok) applyAction(path, body);
    if (r.ok && opts.onOk) opts.onOk(r);
    if (opts.beforeRefresh) await opts.beforeRefresh(r);
    update();
    return { ...r, message: r.data.message || "" };
  },
};

const ctx = {
  session,
  sound: { play: (name) => log.sounds.push(name) },
  announce: (text) => { live.textContent = text; log.said.push(text); },
  open: (key, row) => log.opened.push([key, row]),
  openUrl: (url) => log.links.push(url),
  go: () => {},
  toast: (text, ok) => { log.toasts.push({ text, ok }); toast.show(text, ok); },
  reduced: () => isReduced,
  now: () => nowMs,
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
let hora = 22;
stops.setActive("aprender");
function frame() {
  hud.setStop("aprender");
  hud.setClock(clockText(hora));
  stage.resize();
  stage.render({ pos: STOP_IDS.indexOf("aprender"), hora });
  if (snap) { hud.setStatus(snap, nowMs); stops.setCounts(stopCounts(model(snap))); }
}
window.addEventListener("resize", frame);

// ---- the keyboard, wired the way the shell wires it: the table decides which key is which verb, APRENDER does the verb
const view = { cursor: null };
const cursor = createCursor({ view, sound: ctx.sound, reduced: () => isReduced, root: () => document.getElementById("app") });
const handlers = { move: (key) => cursor.move(key === "j" ? 1 : -1) };
for (const entry of KEYS) {
  if (entry.scope !== "row" || entry.id === "move" || !(entry.stops || []).includes("aprender")) continue;
  handlers[entry.id] = (key, event) => {
    const row = cursor.current();
    // what the shell does when the moment does not take Enter: open the application the row is of
    if (!aprender.onKey(entry.id, row, event) && entry.id === "open" && cursor.jobOf(row)) ctx.open(cursor.jobOf(row), row.dataset.row || null);
  };
}
installKeys({ handlers, stop: () => "aprender" });

function update() {
  if (!snap) return;
  aprender.update(model(snap), ctx);
  cursor.apply();
  frame();
}

window.__lab = {
  load(name, at) {
    const make = SCENARIOS[name];
    if (!make) throw new Error("no scenario " + name);
    hora = at != null ? at : 22;
    // the data is retaken at the hour of the stage (the fixtures were taken at noon in Santiago)
    snap = make();
    snap = retime(snap, (hora - 12) * HOUR);
    nowMs = Date.parse(snap.generated_at);
    // the interview that the mail announced is tomorrow at 16:00 whatever the hour of the stage (the fixture put it 28 h after noon)
    for (const x of snap.today.tasks) if (x.kind === "interview_invite") x.due_at = new Date(nowMs + (40 - hora) * HOUR).toISOString().replace(".000Z", "Z");
    for (const k of Object.keys(log)) log[k].length = 0;
    holding = null; release = null; refuse = null; dossierMode = "ok"; dossierHold = null; dossierRelease = null; failNext = false;
    queue = sampleQueue();
    view.cursor = null;
    aprender.unmount();
    aprender.mount(document.getElementById("host"), ctx);
    update();
  },
  mountOnly() { aprender.unmount(); aprender.mount(document.getElementById("host"), ctx); frame(); },
  refresh(fn) { if (fn) fn(snap); update(); },
  hold() { holding = new Promise((resolve) => { release = resolve; }); },
  release(ok = true, status = 409) { refuse = ok ? null : { status }; const go = release; holding = null; release = null; if (go) go(); },
  dossier(mode) {
    dossierMode = mode;
    if (mode === "hold") dossierHold = new Promise((resolve) => { dossierRelease = resolve; });
  },
  dossierRelease() { dossierMode = "ok"; const go = dossierRelease; dossierHold = null; dossierRelease = null; if (go) go(); },
  queue(cards) { queue = cards || sampleQueue(); },
  failNext(on) { failNext = !!on; },
  now(ms) { nowMs = ms; },
  advance(ms) { nowMs += ms; update(); },
  state: () => JSON.parse(JSON.stringify(log)),
  snapshot: () => snap,
  lang(code) { setLang(code); hud.relabel(); stops.relabel(); bar.relabel(); aprender.unmount(); aprender.mount(document.getElementById("host"), ctx); update(); },
  reduced(on) { isReduced = !!on; },
  theme(name) { document.documentElement.setAttribute("data-theme", name); },
  cursor: () => view.cursor,
  cursorTo: (key) => cursor.set(key),
  go(at) { hora = at; frame(); },
  // For the contrast test (A18): where each text of the moment is PAINTED, its colour, and a switch that makes the glyphs and the underlines
  // invisible, so a screenshot shows only what they are painted on (the glass over the sky). A text that is cut by its cell (an ellipsis) or
  // that has scrolled out of the sheet's list is laid out wider or lower than what is painted: each rect is clipped by every ancestor that clips.
  textBoxes: () => {
    const clips = (el) => { const out = []; for (let e = el; e && e !== document.body; e = e.parentElement) { const cs = getComputedStyle(e); if (cs.overflowX !== "visible" || cs.overflowY !== "visible") out.push(e.getBoundingClientRect()); } return out; };
    return [...document.querySelectorAll("#host *")].flatMap((el) => [...el.childNodes].filter((n) => n.nodeType === 3 && n.textContent.trim()).map((n) => {
      const range = document.createRange();
      range.selectNodeContents(n);
      const boxes = clips(el);
      const rects = [...range.getClientRects()].map((r) => boxes.reduce((a, b) => ({ left: Math.max(a.left, b.left), top: Math.max(a.top, b.top), right: Math.min(a.right, b.right), bottom: Math.min(a.bottom, b.bottom) }), { left: r.left, top: r.top, right: r.right, bottom: r.bottom }))
        .filter((r) => r.right - r.left > 1 && r.bottom - r.top > 1);
      return { cls: String(el.className), text: n.textContent.trim().slice(0, 40), color: getComputedStyle(el).color, rects };
    }));
  },
  ghost(on) {
    for (const el of document.querySelectorAll("#host *")) {
      if (on) { el.style.setProperty("color", "transparent", "important"); el.style.setProperty("border-color", "transparent", "important"); el.style.setProperty("background-image", "none", "important"); }
      else { el.style.removeProperty("color"); el.style.removeProperty("border-color"); el.style.removeProperty("background-image"); }
    }
  },
};
frame();
