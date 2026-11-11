// Lab "f0-hoy": the real HOY moment on the real sheet, over the real stage, driven by the real key table and cursor, with a fake
// server that applies what an action does to the snapshot. Driven through window.__lab by test/moments.hoy.browser.test.mjs
// and by hand for screenshots.
//   __lab.load(name, hour)     "empty" | "rich" | "heavy" snapshot, at its own generated_at (or `hour`, decimal, on the stage's clock)
//   __lab.refresh(fn?)         the next poll: `fn(snapshot)` may change it first; the moment is updated like the shell does
//   __lab.hold() / release(ok) the server answers only when released (to look at the dim row), ok=false answers 409
//   __lab.fresh([marks])       what the diff calls new for the next update       __lab.now(ms)   the page's clock
//   __lab.visit(hoursAgo)      when he was last here (0: never): "since your last visit" appears or not
//   __lab.state()              { acts, opened, went, links, said, sounds } everything the fake ctx was asked, in order
//   __lab.lang(code)           the language and an update          __lab.reduced(bool)   prefers-reduced-motion for the moment
//   __lab.visibility(hidden)   the tab hides or shows again (the "did you do it?" line asks when it shows)
//   __lab.cursor() / cursorTo("c:fx:12" | "t:101")   where the keyboard's cursor is, and putting it there
import hoy from "../src/moments/hoy.js";
import { model, stopCounts, turnCount } from "../src/core/model.js";
import { installKeys, KEYS } from "../src/ui/keys.js";
import { createCursor } from "../src/ui/cursor.js";
import { createMenu } from "../src/ui/menu.js";
import { createStage, clockText } from "../src/stage/stage.js";
import { mulberry32 } from "../src/stage/skyline.js";
import { createHud } from "../src/ui/hud.js";
import { createStops } from "../src/ui/stops.js";
import { createBar } from "../src/ui/bar.js";
import { STOP_IDS } from "../src/scene/poses.js";
import { t, setLang, setTimeZone } from "../src/i18n/index.js";
import empty from "../test/fixtures/snapshot.empty.json";
import rich from "../test/fixtures/snapshot.rich.json";
import heavy from "../test/fixtures/snapshot.heavy.json";

const FIXTURES = { empty, rich, heavy };
setTimeZone("America/Santiago");

let snap = null;
let nowMs = 0;
let freshSet = new Set();
let isReduced = false;
let holding = null; // a promise the fake server waits on
let release = null;
let refuse = false;
let visitMs = 0;
const log = { acts: [], opened: [], went: [], links: [], said: [], sounds: [] };

// ---- the fake server: what each action does to the snapshot, and the poll that follows
function applyAction(path, body) {
  const dropCard = (key) => {
    for (const list of [snap.awaiting_approval, snap.needs_gonza]) {
      const before = list.cards.length;
      list.cards = list.cards.filter((c) => c.key !== key);
      list.total -= before - list.cards.length;
    }
  };
  if (path === "/api/task/done") {
    const before = snap.today.tasks.length;
    snap.today.tasks = snap.today.tasks.filter((x) => x.id !== body.id);
    snap.today.tasks_total -= before - snap.today.tasks.length;
  } else if (["/api/approve", "/api/confirm", "/api/resolve", "/api/snooze"].includes(path) || (path === "/api/mark" && body.state === "withdrawn")) {
    dropCard(body.key);
  } else if (path === "/api/task/add") {
    const id = 1 + snap.today.tasks.reduce((m, x) => Math.max(m, x.id), 0);
    snap.today.tasks.push({ id, text: body.text, job_key: null, due_at: body.due_at, overdue: false, link: null, kind: "action_required", created_at: new Date(nowMs).toISOString() });
    snap.today.tasks_total += 1;
  }
}

const session = {
  get fresh() { return freshSet; },
  async act(path, body, opts = {}) {
    log.acts.push({ path, body });
    if (holding) await holding;
    const r = refuse ? { ok: false, status: 409, data: { message: "No se pudo." } } : { ok: true, status: 200, data: {} };
    refuse = false;
    if (opts.beforeRefresh) await opts.beforeRefresh(r);
    if (r.ok) applyAction(path, body);
    update();
    return { ...r, message: "" };
  },
};

const live = document.getElementById("live");
const menu = createMenu({ sound: { play: (name) => log.sounds.push(name) } });
const ctx = {
  session,
  menu,
  sound: { play: (name) => log.sounds.push(name) },
  announce: (text) => { live.textContent = text; log.said.push(text); },
  open: (key, row) => log.opened.push([key, row]),
  go: (stop, opts) => log.went.push([stop, opts || null]),
  openUrl: (url) => log.links.push(url),
  reduced: () => isReduced,
  now: () => nowMs,
  lastVisit: () => visitMs,
};

// ---- the page around it (the same pieces as the frame lab), so the sheet is judged against the sky it will have
const sheetEl = document.getElementById("app");
const canvas = document.getElementById("sky");
const stage = createStage({ canvas, dialHost: document.getElementById("dial"), t, getSheetRect: () => sheetEl.getBoundingClientRect(), rand: mulberry32(20261007) });
const hud = createHud({ onSearch: () => {} });
const stops = createStops({ onSelect: () => {} });
const bar = createBar({ onHelp: () => {} });
hud.center.append(stops.el);
document.getElementById("shell").prepend(hud.el);
document.getElementById("shell").append(bar.el);
let hora = 9;
// The stops bar is lit once: on a phone `setActive` scrolls the lit tab into view and, from inside the sticky bar, that moves the
// document too, which would hide the page this lab is here to show.
stops.setActive("hoy");
function frame() {
  hud.setStop("hoy");
  hud.setClock(clockText(hora));
  stage.resize();
  stage.render({ pos: STOP_IDS.indexOf("hoy"), hora });
  if (snap) { hud.setStatus(snap, nowMs); stops.setCounts(stopCounts(model(snap))); }
}
window.addEventListener("resize", frame);

// ---- the keyboard, wired the way the shell wires it: the table decides which key is which verb, HOY does the verb
const view = { cursor: null };
const cursor = createCursor({ view, sound: ctx.sound, reduced: () => isReduced, root: () => document.getElementById("app") });
const handlers = { move: (key) => cursor.move(key === "j" ? 1 : -1) };
for (const entry of KEYS) {
  if (entry.scope === "row" && entry.id !== "move") handlers[entry.id] = (key, event) => hoy.onKey(entry.id, cursor.current(), event);
}
// the stop's own key (a new task) is not a row key: the moment hears it all the same
handlers.newtask = (key, event) => hoy.onKey("newtask", cursor.current(), event);
installKeys({ handlers, stop: () => "hoy", locked: () => menu.isOpen() });

function update() {
  if (!snap) return;
  hoy.update(model(snap), ctx);
  cursor.apply();
  frame();
}

window.__lab = {
  load(name, at) {
    snap = JSON.parse(JSON.stringify(FIXTURES[name]));
    nowMs = Date.parse(snap.generated_at);
    if (at != null) hora = at;
    log.acts.length = log.opened.length = log.went.length = log.links.length = log.said.length = log.sounds.length = 0;
    view.cursor = null;
    hoy.unmount();
    hoy.mount(document.getElementById("host"), ctx);
    update();
    return turnCount(model(snap));
  },
  refresh(fn) { if (fn) fn(snap); update(); },
  hold() { holding = new Promise((resolve) => { release = resolve; }); },
  release(ok = true) { refuse = !ok; const go = release; holding = null; release = null; if (go) go(); },
  fresh(marks) { freshSet = new Set(marks); },
  visit(hoursAgo) { visitMs = hoursAgo ? nowMs - hoursAgo * 3600000 : 0; update(); },
  now(ms) { nowMs = ms; },
  advance(ms) { nowMs += ms; update(); },
  state: () => JSON.parse(JSON.stringify(log)),
  snapshot: () => snap,
  lang(code) { setLang(code); hud.relabel(); stops.relabel(); bar.relabel(); update(); },
  reduced(on) { isReduced = !!on; },
  // the tab goes away and comes back (what happens after "o" opens a posting in another tab)
  visibility(hidden) {
    Object.defineProperty(document, "hidden", { configurable: true, get: () => hidden });
    document.dispatchEvent(new Event("visibilitychange"));
  },
  cursor: () => view.cursor,
  cursorTo: (key) => cursor.set(key),
  go(at) { hora = at; frame(); },
};
frame();
