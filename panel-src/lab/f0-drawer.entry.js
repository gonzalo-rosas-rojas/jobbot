// The drawer and the content of an application on the real styles, over the demo's fictional server, WITHOUT the shell: a sheet with
// a plain list of the applications on one side and the drawer on the other, and what a test needs of the real page (`window.__app`).
// It is built as a demo page, because that is what gives it the made-up server (`window.__DEMO__`, scripts/demo/demo.js):
//   node build.mjs demo --entry lab/f0-drawer.entry.js --out dist/demo-drawer.html
// The keys it wires are the ones the drawer touches once the shell exists (T21): j and k step, Esc closes, l changes the language, and 1-5
// travel (go) unless the drawer is open (installKeys' drawerOpen).
// Query: ?t=<token> (what the session sends), ?lang=es|en, ?theme=light|dark, ?motion=calm or ?reduced=1 (the drawer's own slide is a cut),
// ?own=1 (a stop's own page of the application, `content` of createDrawer, for every application but demo:4).
//   ?hoy=1   for the eye, not for the tests: the real HOY on the sheet, the real stage behind it (sky, dial, skyline, the beam), the HUD and the
//            bottom bar, so the drawer is judged against what it will sit on; ?clock=<decimal hour> (default 9) is the hour of the sky.
// `window.__lab`:  log.opens (the open/close events the drawer told the shell), log.went (the stops asked for), reduced(bool).
import { initLang, t, tn, lang, setLang, onLang } from "../src/i18n/index.js";
import { sound } from "../src/sound/index.js";
import { createSession } from "../src/core/session.js";
import { createRouter } from "../src/ui/router.js";
import { createDrawer } from "../src/ui/drawer.js";
import { createToast } from "../src/ui/toast.js";
import { installKeys } from "../src/ui/keys.js";
import { createAnnouncer } from "../src/ui/announce.js";
import { h, fill } from "../src/core/dom.js";
import { stopCounts } from "../src/core/model.js";
import hoy from "../src/moments/hoy.js";
import { createCursor } from "../src/ui/cursor.js";
import { createMenu } from "../src/ui/menu.js";
import { createStage, clockText } from "../src/stage/stage.js";
import { mulberry32 } from "../src/stage/skyline.js";
import { createHud } from "../src/ui/hud.js";
import { createStops } from "../src/ui/stops.js";
import { createBar } from "../src/ui/bar.js";
import { STOP_IDS } from "../src/scene/poses.js";

initLang();
const params = new URLSearchParams(location.search);
const real = params.get("hoy") === "1";
let isReduced = params.get("reduced") === "1" || params.get("motion") === "calm";
let stopId = "hoy";
const log = { opens: [], went: [] };
const STOPS = ["hoy", "procesos", "cola", "aprender", "sistema"];

const session = createSession({ token: params.get("t") || "", demo: window.__DEMO__ || null });
const announcer = createAnnouncer();
const toast = createToast({ say: (text) => announcer.say(text) });
const router = createRouter({});
const list = h("ul", { class: "lab-list", "aria-label": "applications" });
const detailEl = h("aside", { id: "detail", class: "detail", tabindex: "-1", inert: true });
detailEl.setAttribute("data-lenis-prevent", "");
const host = h("div", { class: "sheet-host" }, real ? null : list);
const sheet = h("main", { id: "app", class: "sheet", "data-moment": "hoy" }, host);
document.getElementById("shell").append(sheet, detailEl, toast.el, announcer.el);

const jobKeys = () => [...new Set([...sheet.querySelectorAll("[data-card]")].map((e) => e.dataset.card))];
const ctx = {
  t, tn, lang, session, sound,
  drawer: { open: (key, origin) => drawer.open(key, origin), close: () => drawer.close(), isOpen: () => drawer.isOpen(), step: (dir) => drawer.step(dir), reload: () => drawer.reload(), get key() { return drawer.key; } },
  go: (id) => { log.went.push(id); stopId = id; drawer.close({ silent: true }); },
  model: () => session.model,
  announce: (text) => announcer.say(text),
  toast: (text, ok) => toast.show(text, ok),
  reduced: () => isReduced,
};
const drawer = createDrawer({
  el: detailEl, session, router, stop: () => stopId, sound, getCtx: () => ctx, jobKeys, toast: (text, ok) => toast.show(text, ok),
  onStep: (key) => { const row = sheet.querySelector(`[data-card="${CSS.escape(key)}"]`); if (row) row.scrollIntoView({ block: "nearest" }); },
  onOpenChange: (open) => log.opens.push(open ? "open" : "close"),
  // ?own=1: a stop with a page of its own for the application, and one application ("demo:4") for which it has none (it leaves the common content)
  content: params.get("own") === "1" ? (detail, card) => (card.key === "demo:4" ? null : h("div", { class: "detail-body d-body d-own", "data-job": card.key }, h("h2", { id: "detail-title" }, "own " + card.key))) : null,
});

function cardsOf(snap) {
  const out = [];
  for (const col of snap.board || []) out.push(...col.cards);
  out.push(...((snap.awaiting_approval || {}).cards || []), ...((snap.needs_gonza || {}).cards || []));
  return [...new Map(out.map((c) => [c.key, c])).values()];
}

function draw(model) {
  if (!model || real) return;
  fill(list, ...cardsOf(model.snap).map((c) => h("li", {},
    h("button", { class: "lab-row", type: "button", "data-card": c.key, onclick: (e) => drawer.open(c.key, e.currentTarget) }, [c.company, c.title].filter(Boolean).join(" — ")))));
}

// ---- the eye's version: the real HOY and the frame (the same pieces as lab/f0-hoy), only with ?hoy=1
const view = { cursor: null };
let frame = () => {};
let hora = Number(params.get("clock")) || 9;
if (real) {
  const menu = createMenu({ sound });
  const cursor = createCursor({ view, sound, reduced: () => isReduced, root: () => sheet });
  const hoyCtx = {
    session, menu, sound, announce: ctx.announce, reduced: ctx.reduced, now: () => Date.now(), lastVisit: () => 0,
    open: (key, row) => drawer.open(key, typeof row === "string" ? sheet.querySelector(`[data-row="${CSS.escape(row)}"]`) : row),
    go: (id) => ctx.go(id),
  };
  hoy.mount(host, hoyCtx);
  const stage = createStage({ canvas: document.getElementById("sky"), dialHost: document.getElementById("dial"), t, getSheetRect: () => sheet.getBoundingClientRect(), rand: mulberry32(20261007) });
  const hud = createHud({ onSearch: () => {} });
  const stops = createStops({ onSelect: () => {} });
  const bar = createBar({ onHelp: () => {} });
  hud.center.append(stops.el);
  document.getElementById("shell").prepend(hud.el);
  document.getElementById("shell").append(bar.el);
  stops.setActive("hoy");
  frame = () => {
    hud.setStop("hoy");
    hud.setClock(clockText(hora));
    stage.resize();
    stage.render({ pos: STOP_IDS.indexOf("hoy"), hora, reduced: isReduced });
    if (session.model) { hud.setStatus(session.model.snap, Date.now()); stops.setCounts(stopCounts(session.model)); }
  };
  window.addEventListener("resize", frame);
  window.__lab = { ...(window.__lab || {}), cursor, menu };
  session.subscribe((event) => { if (event.type === "snapshot") { hoy.update(event.model, hoyCtx); cursor.apply(); frame(); } });
  frame();
}

session.subscribe((event) => {
  if (event.type === "snapshot") draw(event.model);
  else if (event.type === "toast") toast.show(event.toast.message, event.toast.ok);
});
onLang(() => { drawer.rerender(); frame(); });
installKeys({
  handlers: {
    close: () => { if (drawer.isOpen()) drawer.close(); },
    move: (key) => { if (drawer.isOpen()) drawer.step(key === "j" ? 1 : -1); else if (real && window.__lab.cursor) window.__lab.cursor.move(key === "j" ? 1 : -1); },
    lang: () => setLang(lang() === "es" ? "en" : "es"),
    go: (key) => ctx.go(STOPS[Number(key) - 1]),
  },
  stop: () => stopId,
  drawerOpen: () => drawer.isOpen(),
});
router.listen(({ job }) => (job ? drawer.open(job) : drawer.close({ fromPop: true })));
window.__lab = { ...(window.__lab || {}), log, reduced(on) { isReduced = !!on; } };
window.__app = { session, sound, drawer, shell: { drawer }, model: () => session.model, rooms: { mounted: () => stopId }, go: ctx.go };
session.start();
