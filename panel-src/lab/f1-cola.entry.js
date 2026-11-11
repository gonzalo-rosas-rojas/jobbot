// Lab "f1-cola": the real COLA moment on the real sheet, over the real stage, driven by the real key table and cursor, with a fake server that
// applies what an action does to the snapshot and answers the detail and the capture of a proposal. Driven through window.__lab by
// test/moments.cola.browser.test.mjs and by hand for screenshots. The people and companies are invented.
//   __lab.load(name, hour)     "rich" (two proposals, the mock's) | "many" (seven) | "one" | "empty" | "hostile" (long and hostile texts)
//   __lab.refresh(fn?)         the next poll: `fn(snapshot)` may change it first; the moment is updated like the shell does
//   __lab.hold() / release(ok) the server answers only when released (to look at the dim row), ok=false answers 409, ok=500 answers 500, ok=0 no network
//   __lab.detail(mode)         what GET /api/job answers: "ok" | "error" | "slow" (until __lab.detailRelease())
//   __lab.shot(mode)           what GET /api/shot answers: "ok" | "error" | "slow" (until __lab.shotRelease())
//   __lab.state()              { acts, opened, went, links, said, sounds, toasts, calls } everything the fake ctx was asked, in order
//   __lab.undo()               presses the "Deshacer" of the last toast that offered one
//   __lab.lang(code)           the language and an update          __lab.reduced(bool)   prefers-reduced-motion for the moment
//   __lab.batchMin(n|null)     the lote's threshold the settings answer (null: no setting, so the default)
//   __lab.now(ms) / advance(ms)  the page's clock                  __lab.cursor() / cursorTo("c:p:1")   where the keyboard's cursor is, and putting it there
//   __lab.textBoxes() / ghost(on)   the measure of A18: where each text is painted, and a switch that hides glyphs so a screenshot shows only the glass
import cola from "../src/moments/cola.js";
import { model, stopCounts } from "../src/core/model.js";
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

setTimeZone("America/Santiago");

const AT = "2026-10-07T21:00:00Z";            // 18:00 in Santiago: the hour of the gate
const iso = (hoursAgo) => new Date(Date.parse(AT) - hoursAgo * 3600000).toISOString();

let snap = null;
let nowMs = 0;
let hora = 18;
let isReduced = false;
let holding = null;
let release = null;
let refuse = null;                              // null | false (409) | 500 | 0 (no network)
let batchMin = null;
let detailMode = "ok";
let detailGate = null;
let shotMode = "ok";
let shotGate = null;
let lastUndo = null;
const snoozed = new Map();
const log = { acts: [], opened: [], went: [], links: [], said: [], sounds: [], toasts: [], calls: [] };

// ---- the data: invented proposals shaped like the server's cards
const card = (key, over = {}) => ({
  key, company: "Empresa " + key, title: "Rol " + key, score: 80, state: "prepared", source: "greenhouse", url: "https://example.com/" + key.replace(":", "/"),
  reason: null, reason_kind: null, reason_short: null, note: null, approved: false, has_screenshot: true, has_cv: true, has_dossier: false,
  updated_at: iso(10), actions: ["approve"], marks: [], posted_at: iso(100), first_seen: iso(90), salary_min: 4900, salary_max: 6000, days_in_state: 1,
  snoozed_until: null, waiting_since: null, age_days: 4, prune_in_days: 14, ...over,
});

const PEOPLE = [
  card("p:1", { company: "Ancla Security", title: "Rust Engineer", score: 92, source: "ashby", salary_min: 5400, salary_max: 6900, updated_at: "2026-10-07T07:12:00Z", first_seen: "2026-10-06T03:14:00Z", days_in_state: 1 }),
  card("p:2", { company: "Brisa Mobility", title: "Full-stack Engineer", score: 89, source: "greenhouse", salary_min: 4900, salary_max: 6000, updated_at: iso(9) }),
  card("p:3", { company: "Cobalto Labs", title: "Staff Platform Engineer", score: 84, source: "lever", salary_min: 6500, salary_max: 8200, updated_at: iso(8) }),
  card("p:4", { company: "Dique Studio", title: "Backend Engineer", score: 78, source: "ashby", salary_min: 4200, salary_max: 5200, updated_at: iso(7), has_screenshot: false }),
  card("p:5", { company: "Eclipse Works", title: "Data Engineer", score: 74, source: "greenhouse", salary_min: null, salary_max: 4800, updated_at: iso(6) }),
  card("p:6", { company: "Faro Digital", title: "Software Engineer", score: 71, source: "lever", salary_min: 3900, salary_max: 4500, updated_at: iso(5) }),
  card("p:7", { company: "Glaciar Co", title: "Infrastructure Engineer", score: 66, source: "ashby", salary_min: 4000, salary_max: 4000, updated_at: iso(4) }),
];

function scenario(name) {
  const s = JSON.parse(JSON.stringify(name === "empty" ? empty : rich));
  s.generated_at = AT;
  if (name === "empty") return s;
  const take = { rich: 2, many: 7, one: 1, hostile: 2 }[name] || 2;
  const cards = PEOPLE.slice(0, take).map((c) => ({ ...c }));
  if (name === "hostile") {
    cards[0].company = "<img src=x onerror=alert(1)> " + "Empresa-con-un-nombre-larguísimo-sin-espacios-".repeat(5);
    cards[0].title = "\u202eReversed title\u202c " + "Ingeniero/a de plataformas y confiabilidad, con responsabilidad sobre todo ".repeat(4);
    cards[0].url = ["java", "script:alert(1)"].join("");
    cards[0].source = "ashby\u0000<b>x</b>";
    cards[1].company = "A".repeat(200);
  }
  s.awaiting_approval = { total: cards.length, cards };
  s.controls = { ...s.controls, last_run_at: "2026-10-07T11:04:00Z", engine: "recent", window_open: true, next_window: null };
  s.pipeline = { ...s.pipeline, seen: 287, above: 20, with_cv: 15, complete: 11, sent: 9, threshold: 60 };
  s.trust = { reviewed: 4, first_n: 5, review_phase: true, sent_24h: 2, cap: 10, cap_after: 25 };
  const needs = card("p:20", { company: "Delta Forge", title: "Backend Engineer", score: 80, state: "needs_gonza", source: "greenhouse", reason: "faltan respuestas", reason_kind: "missing_answers", reason_short: "faltan 3 respuestas", has_screenshot: false, actions: ["resolve"], waiting_since: iso(8) });
  s.needs_gonza = { total: 1, cards: [needs] };
  const approved = card("p:21", { company: "Tramontana", title: "Backend Engineer", score: 90, approved: true, actions: [], updated_at: iso(3) });
  s.board = (s.board || []).map((col) => (col.state === "prepared" ? { ...col, cards: [approved] } : col.state === "needs_gonza" ? { ...col, cards: [needs] } : col));
  s.activity = { ...(s.activity || {}), moves: [
    { at: "2026-10-07T02:40:00Z", key: "p:30", company: "Tramontana", title: "Backend Engineer", to: "submitted" },
    { at: "2026-10-07T03:12:00Z", key: "p:31", company: "Orbe Studio", title: "Full-stack Engineer", to: "submitted" },
  ] };
  return s;
}

// ---- the fake server: what each action does to the snapshot
function applyAction(path, body) {
  const out = snap.awaiting_approval;
  const take = (key) => {
    const at = out.cards.findIndex((c) => c.key === key);
    if (at < 0) return null;
    const [c] = out.cards.splice(at, 1);
    out.total -= 1;
    return c;
  };
  if (path === "/api/approve") {
    for (const key of body.keys || [body.key]) {
      const c = take(key);
      if (!c) continue;
      const prepared = snap.board.find((col) => col.state === "prepared");
      if (prepared) prepared.cards.push({ ...c, approved: true, actions: [] });
      if (snap.trust && snap.trust.reviewed < snap.trust.first_n) snap.trust.reviewed += 1;
    }
  } else if (path === "/api/snooze") {
    const c = take(body.key);
    if (c) snoozed.set(body.key, c);
  } else if (path === "/api/unsnooze") {
    const c = snoozed.get(body.key);
    if (c) { snoozed.delete(body.key); out.cards.push(c); out.total += 1; }
  } else if (path === "/api/stop") {
    snap.controls.stopped = true;
  } else if (path === "/api/resume") {
    snap.controls.stopped = false;
  }
}

const said = (r, fallback) => (r.data && r.data.message) || fallback;
const session = {
  get fresh() { return new Set(); },
  said,
  async api(path) {
    log.calls.push(path);
    if (!path.startsWith("/api/job?key=")) return { ok: false, status: 404, data: null };
    if (detailMode === "slow") await detailGate;
    if (detailMode === "error") return { ok: false, status: 500, data: { message: "boom" } };
    const key = decodeURIComponent(path.split("=")[1]);
    const c = [...(snap.awaiting_approval.cards || []), ...[...snoozed.values()]].find((x) => x.key === key);
    const score = c ? c.score : 80;
    const parts = [["fit", 30], ["compensation", 15], ["interview_style", 10], ["stability", 20], ["learning", 15], ["other", 10]];
    let left = score;
    const detail = {};
    parts.forEach(([id, max], i) => { const v = i === parts.length - 1 ? Math.max(0, left) : Math.min(max, Math.round((score / 100) * max)); detail[id] = v; left -= v; });
    return { ok: true, status: 200, data: { key, location: key === "p:1" ? "Remoto, cualquier país" : "", remote: key !== "p:1", apply_url: "https://example.com/apply/" + key.replace(":", "/"), score_detail: detail } };
  },
  async blob(path) {
    log.calls.push(path);
    if (shotMode === "slow") await shotGate;
    if (shotMode === "error") return null;
    return drawForm(decodeURIComponent(path.split("=")[1]));
  },
  async act(path, body, opts = {}) {
    log.acts.push({ path, body });
    if (holding) await holding;
    let r;
    if (refuse === false) r = { ok: false, status: 409, data: { message: "No se pudo: ya estaba aprobada." } };
    else if (refuse === 500) r = { ok: false, status: 500, data: { message: "Error del servidor." } };
    else if (refuse === 0) r = { ok: false, status: 0, data: { message: "Sin conexión." } };
    else r = { ok: true, status: 200, data: { message: "Hecho." } };
    refuse = null;
    log.toasts.push({ message: said(r, r.ok ? "Hecho." : "Error"), ok: r.ok });
    if (r.ok && opts.onOk) opts.onOk(r);
    if (opts.beforeRefresh) await opts.beforeRefresh(r);
    if (r.ok) applyAction(path, body);
    update();
    return { ...r, message: said(r, "") };
  },
};

/** A dry-run capture the way the bot leaves it: the form, filled. Drawn on a canvas, so no picture is bundled. */
function drawForm(key) {
  const c = document.createElement("canvas");
  c.width = 900; c.height = 1120;
  const g = c.getContext("2d");
  g.fillStyle = "#f3f1ec"; g.fillRect(0, 0, 900, 1120);
  g.fillStyle = "#e4e1d8"; g.fillRect(0, 0, 900, 78);
  g.fillStyle = "#2b3a55"; g.fillRect(40, 20, 38, 38);
  g.fillStyle = "#1a2234"; g.font = "600 28px sans-serif"; g.fillText("Empresa " + key, 96, 48);
  g.font = "600 40px sans-serif"; g.fillText("Rol de ejemplo", 40, 150);
  g.fillStyle = "#6b7280"; g.font = "22px sans-serif"; g.fillText("Remoto · Jornada completa", 40, 186);
  const field = (label, y, h = 54, w = 820) => { g.fillStyle = "#6b7280"; g.font = "18px sans-serif"; g.fillText(label, 40, y - 10); g.fillStyle = "#ffffff"; g.fillRect(40, y, w, h); g.strokeStyle = "#c9c5ba"; g.strokeRect(40.5, y + 0.5, w, h); g.fillStyle = "#1a2234"; g.font = "22px sans-serif"; g.fillText("Dato de prueba", 56, y + 34); };
  field("Nombre", 250); field("Correo", 350); field("Teléfono", 450); field("Currículum", 550); field("Carta de presentación", 650, 200); field("Sueldo mensual esperado (USD)", 920);
  g.setLineDash([8, 6]); g.strokeStyle = "#8a8578"; g.strokeRect(40, 1020, 300, 56); g.setLineDash([]);
  g.fillStyle = "#8a8578"; g.font = "22px sans-serif"; g.fillText("Enviar postulación", 66, 1056);
  return new Promise((resolve) => c.toBlob((b) => resolve(b), "image/png"));
}

const live = document.getElementById("live");
const sound = { play: (name) => log.sounds.push(name) };
const menu = createMenu({ sound });
const ctx = {
  session,
  menu,
  sound,
  settings: { get: (key) => (key === "batchMin" && batchMin != null ? batchMin : undefined) },
  announce: (text) => { live.textContent = text; log.said.push(text); },
  toast: (text, ok, extra) => { log.toasts.push({ message: text, ok, undo: !!(extra && extra.undo) }); lastUndo = extra && extra.undo ? extra.undo : null; },
  open: (key, row) => log.opened.push([key, row]),
  go: (stop, opts) => log.went.push([stop, opts || null]),
  openUrl: (url) => log.links.push(url),
  reduced: () => isReduced,
  now: () => nowMs,
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
stops.setActive("cola");
function frame() {
  hud.setStop("cola");
  hud.setClock(clockText(hora));
  stage.resize();
  stage.render({ pos: STOP_IDS.indexOf("cola"), hora });
  if (snap) { hud.setStatus(snap, nowMs); stops.setCounts(stopCounts(model(snap))); }
}
window.addEventListener("resize", frame);

// ---- the keyboard, wired the way the shell wires it: the table decides which key is which verb, COLA does the verb
const view = { cursor: null };
const cursor = createCursor({ view, sound, reduced: () => isReduced, root: () => document.getElementById("app") });
const handlers = {};
for (const entry of KEYS) {
  if (entry.scope !== "global" && entry.scope !== "local" && (entry.stops || []).includes("cola")) handlers[entry.id] = (key, event) => cola.onKey(entry.id, cursor.current(), event);
}
installKeys({ handlers, stop: () => "cola", locked: () => menu.isOpen() });

function update() {
  if (!snap) return;
  cola.update(model(snap), ctx);
  cursor.apply();
  frame();
}

window.__lab = {
  load(name = "rich", at) {
    snap = scenario(name);
    snoozed.clear();
    nowMs = Date.parse(AT);
    if (at != null) hora = at;
    log.acts.length = log.opened.length = log.went.length = log.links.length = log.said.length = log.sounds.length = log.toasts.length = log.calls.length = 0;
    view.cursor = null;
    lastUndo = null;
    cola.unmount();
    cola.mount(document.getElementById("host"), ctx);
    update();
    return snap.awaiting_approval.cards.length;
  },
  refresh(fn) { if (fn) fn(snap); update(); },
  hold() { holding = new Promise((resolve) => { release = resolve; }); },
  release(ok = true) { refuse = ok === true ? null : ok; const go = release; holding = null; release = null; if (go) go(); },
  detail(mode) { detailMode = mode; if (mode === "slow") detailGate = new Promise((resolve) => { window.__lab.detailRelease = () => { detailMode = "ok"; resolve(); }; }); },
  shot(mode) { shotMode = mode; if (mode === "slow") shotGate = new Promise((resolve) => { window.__lab.shotRelease = () => { shotMode = "ok"; resolve(); }; }); },
  undo() { if (lastUndo) lastUndo(); },
  now(ms) { nowMs = ms; },
  advance(ms) { nowMs += ms; update(); },
  state: () => JSON.parse(JSON.stringify(log)),
  snapshot: () => snap,
  lang(code) { setLang(code); hud.relabel(); stops.relabel(); bar.relabel(); update(); },
  reduced(on) { isReduced = !!on; },
  batchMin(n) { batchMin = n; update(); },
  cursor: () => view.cursor,
  cursorTo: (key) => cursor.set(key),
  go(at) { hora = at; frame(); },
  // For the contrast test (A18): where each text is painted, its colour, and a switch that makes the glyphs invisible, so a screenshot
  // shows only what they are painted on (the glass over the sky, or the sky itself for the capture's caption).
  textBoxes: () => [...document.querySelectorAll("#host *")].flatMap((el) => [...el.childNodes].filter((n) => n.nodeType === 3 && n.textContent.trim()).map((n) => {
    const range = document.createRange();
    range.selectNodeContents(n);
    return { cls: String(el.className), text: n.textContent.trim().slice(0, 40), color: getComputedStyle(el).color, rects: [...range.getClientRects()].map((r) => ({ left: r.left, top: r.top, right: r.right, bottom: r.bottom })), onPlane: !!el.closest(".plane-wrap") };
  })),
  ghost(on) {
    for (const el of document.querySelectorAll("#host *")) {
      if (on) { el.style.setProperty("color", "transparent", "important"); el.style.setProperty("border-color", "transparent", "important"); el.style.setProperty("text-shadow", "none", "important"); }
      else { el.style.removeProperty("color"); el.style.removeProperty("border-color"); el.style.removeProperty("text-shadow"); }
    }
  },
};
frame();
