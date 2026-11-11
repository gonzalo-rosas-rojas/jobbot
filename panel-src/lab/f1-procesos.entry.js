// Lab "f1-procesos": the real PROCESOS moment on the real sheet, over the real stage, driven by the real key table and cursor, with the real
// Lenis (so the wheel test is the page's own), a fake server that applies what an action does to the snapshot, and a STAND-IN drawer that
// draws the moment's own page of one application (the common drawer, ui/drawer.js, has no way yet to ask the moment for its content).
// Driven through window.__lab by test/moments.procesos.browser.test.mjs and by hand for screenshots. Every company and person is invented.
//   __lab.load(name, hour)     "empty" | "rich" | "heavy" | "stress" (120 open, the mock's ugly frame) | "hostile" (long and hostile texts)
//   __lab.refresh(fn?)         the next poll: `fn(snapshot)` may change it first; the moment is updated like the shell does
//   __lab.hold() / release(ok) the server answers only when released (to look at the dim row), ok=false answers 409
//   __lab.state()              { acts, opened, went, links, said, sounds } everything the fake ctx was asked, in order
//   __lab.openDrawer(key) / closeDrawer()   the stand-in drawer          __lab.lang(code)   the language and an update
//   __lab.reduced(bool)        prefers-reduced-motion for the moment    __lab.now(ms) / advance(ms)   the page's clock
//   __lab.cursor() / cursorTo("c:fx:1")   where the keyboard's cursor is, and putting it there          __lab.scrolls()   { page, list } how far each has scrolled
//   __lab.fail(on)             makes the next view throw (the error state)       __lab.reset()   forgets the frozen order, like a new page load
//   __lab.textBoxes() / ghost(on)   the measure of A18: where each text is painted, and a switch that hides glyphs so a screenshot shows only the glass
import Lenis from "lenis";
import procesos, { resetSession } from "../src/moments/procesos.js";
import { model, stopCounts, LIVE } from "../src/core/model.js";
import { installKeys, KEYS } from "../src/ui/keys.js";
import { createCursor } from "../src/ui/cursor.js";
import { createMenu } from "../src/ui/menu.js";
import { createStage, clockText } from "../src/stage/stage.js";
import { mulberry32 } from "../src/stage/skyline.js";
import { createHud } from "../src/ui/hud.js";
import { createStops } from "../src/ui/stops.js";
import { createBar } from "../src/ui/bar.js";
import { STOP_IDS } from "../src/scene/poses.js";
import { h, fill } from "../src/core/dom.js";
import { t, setLang, setTimeZone } from "../src/i18n/index.js";
import empty from "../test/fixtures/snapshot.empty.json";
import rich from "../test/fixtures/snapshot.rich.json";
import heavy from "../test/fixtures/snapshot.heavy.json";

setTimeZone("America/Santiago");
const FIXTURES = { empty, rich, heavy };

let snap = null;
let nowMs = 0;
let hora = 9;
let isReduced = false;
let holding = null;
let release = null;
let refuse = false;
let openKey = null;
const log = { acts: [], opened: [], went: [], links: [], said: [], sounds: [], reloads: 0 };

// ---- the data: invented applications shaped like the server's cards
const FIRST = ["Alder", "Brisa", "Cobre", "Duna", "Espiga", "Faro", "Granito", "Hielo", "Isla", "Jade", "Kora", "Lumen", "Musgo", "Nogal", "Orilla", "Pampa", "Quilla", "Roble", "Sauce", "Tinta"];
const SECOND = ["Labs", "Data", "Cloud", "Pay", "Soft", "Ops", "Net", "AI", "Works", "Systems"];
const ROLES = ["Backend Engineer", "Platform Engineer", "Rust Developer", "Data Engineer", "Staff Engineer", "Site Reliability Engineer"];
const DAY = 86400000;

function addCard(state, n) {
  const col = snap.board.find((c) => c.state === state);
  const key = "st:" + state + ":" + n;
  const days = 1 + ((n * 7) % (state === "ghosted" ? 40 : state === "submitted" ? 15 : 9));
  const company = FIRST[(n * 3) % FIRST.length] + " " + SECOND[(n * 7 + state.length) % SECOND.length];
  col.cards.push({
    key, company, title: ROLES[n % ROLES.length], score: 60 + ((n * 13) % 38), state, source: ["greenhouse", "ashby", "lever"][n % 3], url: "https://example.com/st/" + n,
    reason: null, reason_kind: null, reason_short: null, note: null, approved: n % 4 === 0, has_screenshot: false, has_cv: true, has_dossier: false,
    updated_at: new Date(nowMs - days * DAY).toISOString(), actions: [], marks: state === "submitted" ? ["replied", "technical_test", "interview", "rejected", "ghosted", "withdrawn"] : ["rejected", "withdrawn"],
    posted_at: null, first_seen: new Date(nowMs - (days + 3) * DAY).toISOString(), salary_min: 4800, salary_max: 6500, days_in_state: days, snoozed_until: null, waiting_since: null, age_days: days + 3, prune_in_days: null,
  });
  col.total += 1;
}

/** The frame of the mock's "ugly" case: 120 open applications, so the list must scroll inside the sheet. */
function stress() {
  const live = () => snap.board.filter((c) => LIVE.includes(c.state)).reduce((n, c) => n + c.total, 0);
  const plan = { submitted: 28, replied: 6, technical_test: 3, interview: 3, offer: 1, ghosted: 5 };
  let i = 0;
  for (const [state, many] of Object.entries(plan)) for (let k = 0; k < many && live() < 120; k++) addCard(state, 100 + i++);
}

function hostile() {
  const evil = ["<img src=x onerror=alert(1)>", "\u202eevil.example", "A".repeat(180), "Línea\nrota\u0000con control", "((((", "x".repeat(60) + " " + "y".repeat(60)];
  let i = 0;
  for (const col of snap.board) {
    for (const c of col.cards) {
      if (i < evil.length) { c.company = evil[i]; c.title = evil[(i + 2) % evil.length] + " " + "R".repeat(90); }
      if (i === 1) c.url = ["java", "script:alert(1)"].join(""); // the build refuses the literal: a link of this kind must never become a link
      i += 1;
    }
  }
}

const scenario = (name, at) => {
  snap = JSON.parse(JSON.stringify(name === "stress" || name === "hostile" ? (name === "stress" ? heavy : rich) : FIXTURES[name]));
  nowMs = at != null ? at : Date.parse(snap.generated_at);
  if (name === "stress") stress();
  if (name === "hostile") hostile();
};

// ---- the fake server: what each action does to the snapshot
function moveCard(key, to) {
  let found = null;
  for (const col of snap.board) {
    const at = col.cards.findIndex((c) => c.key === key);
    if (at >= 0) { found = col.cards.splice(at, 1)[0]; col.total -= 1; break; }
  }
  if (!found) return;
  found.state = to;
  found.days_in_state = 0;
  found.updated_at = new Date(nowMs).toISOString();
  const col = snap.board.find((c) => c.state === to);
  if (col) { col.cards.push(found); col.total += 1; }
  for (const list of [snap.awaiting_approval, snap.needs_gonza]) {
    const before = list.cards.length;
    list.cards = list.cards.filter((c) => c.key !== key);
    list.total -= before - list.cards.length;
  }
}
function applyAction(path, body) {
  if (path === "/api/mark") moveCard(body.key, body.state);
  else if (path === "/api/confirm") moveCard(body.key, "submitted");
  else if (path === "/api/snooze") {
    for (const col of snap.board) for (const c of col.cards) if (c.key === body.key) c.snoozed_until = new Date(nowMs + 3 * DAY).toISOString();
  }
}

const session = {
  get fresh() { return new Set(); },
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

// ---- the context the shell gives a moment
const live = document.getElementById("live");
const menu = createMenu({ sound: { play: (name) => log.sounds.push(name) } });
const view = { cursor: null };
const cursor = createCursor({ view, sound: { play: () => {} }, reduced: () => isReduced, root: () => document.getElementById("app") });
const ctx = {
  session,
  menu,
  cursor,
  sound: { play: (name) => log.sounds.push(name) },
  announce: (text) => { live.textContent = text; log.said.push(text); },
  open: (key, row) => { log.opened.push([key, row]); openDrawer(key); },
  go: (stop, opts) => log.went.push([stop, opts || null]),
  openUrl: (url) => log.links.push(url),
  reduced: () => isReduced,
  now: () => nowMs,
  model: () => model(snap),
  drawer: { reload: () => { log.reloads += 1; paintDrawer(); } },
};

// ---- the stand-in drawer: the page of one application, from a made-up detail
const detailEl = document.getElementById("detail");
const cardOf = (key) => [...snap.board.flatMap((c) => c.cards), ...snap.awaiting_approval.cards, ...snap.needs_gonza.cards].find((c) => c.key === key) || null;
function detailOf(c) {
  const at = Date.parse(c.updated_at);
  const sentAt = at - (c.days_in_state || 0) * DAY;
  const events = [{ at: new Date(sentAt - 3 * DAY).toISOString(), from: null, to: "scored", note: null }, { at: new Date(sentAt - 2 * DAY).toISOString(), from: "scored", to: "prepared", note: null }];
  if (!["scored", "prepared", "needs_gonza"].includes(c.state)) {
    events.push({ at: new Date(sentAt).toISOString(), from: "prepared", to: "submitted", note: null });
    if (c.state !== "submitted") events.push({ at: new Date(at).toISOString(), from: "submitted", to: c.state, note: null });
  }
  const s = Number(c.score) || 0;
  const parts = [0.34, 0.16, 0.22, 0.16, 0.12].map((w) => Math.round(s * w));
  return {
    card: c, location: "Remote, anywhere", remote: true, apply_url: c.url, first_seen: c.first_seen, posted_at: null, salary_min: c.salary_min, salary_max: c.salary_max,
    score_detail: { fit: parts[0], compensation: parts[1], stability: parts[2], learning: parts[3], interview_style: parts[4], other: Math.max(0, s - parts.reduce((a, b) => a + b, 0)) },
    gaps: [], events: events.reverse(), dossier: null, description: "A posting.",
  };
}
function paintDrawer() {
  const c = openKey && cardOf(openKey);
  if (!c) return;
  fill(detailEl, h("div", { class: "detail-bar" }, h("button", { id: "close-detail", class: "btn small", type: "button", onclick: () => closeDrawer() }, t("common.close"))), procesos.drawer(detailOf(c), c, ctx));
}
function openDrawer(key) {
  if (!cardOf(key)) return;
  openKey = key;
  paintDrawer();
  detailEl.classList.add("open");
  detailEl.inert = false;
  lenis.stop();
}
function closeDrawer() {
  if (!openKey) return;
  const back = document.querySelector(`#app [data-card="${CSS.escape(openKey)}"]`);
  openKey = null;
  detailEl.classList.remove("open");
  detailEl.inert = true;
  detailEl.replaceChildren();
  lenis.start();
  if (back) back.focus({ preventScroll: true });
}
function stepDrawer(dir) {
  const keys = [...new Set([...document.querySelectorAll("#app [data-card]")].map((e) => e.dataset.card))];
  const at = keys.indexOf(openKey);
  if (at < 0 || keys.length < 2) return;
  const next = keys[(at + dir + keys.length) % keys.length];
  cursor.set("c:" + next);
  openKey = next;
  paintDrawer();
}

// ---- the page around it (the same pieces as the other labs), so the sheet is judged against the sky it will have
const sheetEl = document.getElementById("app");
const stage = createStage({ canvas: document.getElementById("sky"), dialHost: document.getElementById("dial"), t, getSheetRect: () => sheetEl.getBoundingClientRect(), rand: mulberry32(20261007) });
const hud = createHud({ onSearch: () => {} });
const stops = createStops({ onSelect: () => {} });
const bar = createBar({ onHelp: () => {} });
hud.center.append(stops.el);
document.getElementById("shell").prepend(hud.el);
document.getElementById("shell").append(bar.el);
stops.setActive("procesos");
function frame() {
  hud.setStop("procesos");
  hud.setClock(clockText(hora));
  stage.resize();
  stage.render({ pos: STOP_IDS.indexOf("procesos"), hora });
  if (snap) { hud.setStatus(snap, nowMs); stops.setCounts(stopCounts(model(snap))); }
}
window.addEventListener("resize", frame);

// the camera: the page's own wheel, so a test can tell the list from the stage (the list is `data-lenis-prevent`)
const lenis = new Lenis({ autoRaf: true });

// ---- the keyboard, wired the way the shell wires it: the table decides which key is which verb, the moment does the verb
const handlers = {
  move: (key) => { if (openKey) stepDrawer(key === "j" ? 1 : -1); else cursor.move(key === "j" ? 1 : -1); },
  close: () => { if (openKey) closeDrawer(); },
};
for (const entry of KEYS) {
  if ((entry.scope === "row" || entry.scope === "stop") && (entry.stops || []).includes("procesos") && entry.id !== "move") handlers[entry.id] = (key, event) => procesos.onKey(entry.id, cursor.current(), event);
}
installKeys({ handlers, stop: () => "procesos", locked: () => menu.isOpen(), drawerOpen: () => !!openKey });

function update() {
  if (!snap) return;

  procesos.update(model(snap), ctx);
  cursor.apply();
  if (openKey) paintDrawer();
  frame();
}

window.__lab = {
  load(name, at) {
    scenario(name);
    if (at != null) hora = at;
    log.acts.length = log.opened.length = log.went.length = log.links.length = log.said.length = log.sounds.length = 0;
    view.cursor = null;
    closeDrawer();
    procesos.unmount();
    procesos.mount(document.getElementById("host"), ctx);
    update();
    return stopCounts(model(snap)).procesos;
  },
  refresh(fn) { if (fn) fn(snap); update(); },
  hold() { holding = new Promise((resolve) => { release = resolve; }); },
  release(ok = true) { refuse = !ok; const go = release; holding = null; release = null; if (go) go(); },
  now(ms) { nowMs = ms; },
  advance(ms) { nowMs += ms; update(); },
  state: () => JSON.parse(JSON.stringify(log)),
  snapshot: () => snap,
  lang(code) { setLang(code); hud.relabel(); stops.relabel(); bar.relabel(); update(); if (openKey) paintDrawer(); },
  reduced(on) { isReduced = !!on; },
  cursor: () => view.cursor,
  cursorTo: (key) => cursor.set(key),
  openDrawer,
  closeDrawer,
  scrolls: () => ({ page: window.scrollY, list: (document.querySelector(".procesos .scroller") || {}).scrollTop || 0 }),
  reset() { resetSession(); },
  // mounted and waiting for its first snapshot: the skeleton
  loading() { procesos.unmount(); procesos.mount(document.getElementById("host"), ctx); },
  // what a half-formed model does to the view: it throws, and the page must say so instead of dying
  fail(on) { if (on) procesos.update({ get snap() { throw new Error("a half-formed snapshot"); } }, ctx); else update(); },
  go(at) { hora = at; frame(); },
  // For the contrast test (A18): where each text of the moment is painted, its colour, and a switch that makes the glyphs and the underlines
  // invisible, so a screenshot shows only what they are painted on (the sheet over the sky).
  textBoxes: () => [...document.querySelectorAll("#host *, #detail *")].flatMap((el) => [...el.childNodes].filter((n) => n.nodeType === 3 && n.textContent.trim()).map((n) => {
    const range = document.createRange();
    range.selectNodeContents(n);
    // what a scroller clips is not on screen: only the part of a text inside the box that holds it counts
    const clip = el.closest(".scroller, #detail");
    const box = clip ? clip.getBoundingClientRect() : null;
    const rects = [...range.getClientRects()].map((r) => (box ? { left: Math.max(r.left, box.left), top: Math.max(r.top, box.top), right: Math.min(r.right, box.right), bottom: Math.min(r.bottom, box.bottom) } : { left: r.left, top: r.top, right: r.right, bottom: r.bottom })).filter((r) => r.right > r.left && r.bottom > r.top);
    return { cls: String(el.className), text: n.textContent.trim().slice(0, 40), color: getComputedStyle(el).color, rects };
  })),
  ghost(on) {
    for (const el of document.querySelectorAll("#host *, #detail *")) {
      if (on) { el.style.setProperty("color", "transparent", "important"); el.style.setProperty("border-color", "transparent", "important"); el.style.setProperty("text-decoration-color", "transparent", "important"); }
      else { el.style.removeProperty("color"); el.style.removeProperty("border-color"); el.style.removeProperty("text-decoration-color"); }
    }
  },
};
frame();
