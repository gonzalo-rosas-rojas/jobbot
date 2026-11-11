// COLA, 18:00, la compuerta (spec §5.3): "¿Qué sale esta noche y qué quieres revisar antes?". The moment where the proposals that wait for
// his yes are read one by one and approved. This file only joins the parts and speaks the moment contract of spec §5; the work is in ./cola/:
//   view.js     the page as data (every figure from the snapshot, every word from i18n, the engine told as it is)        pure, tested in Node
//   actions.js  what the keys and buttons do (the request of each verb, the two presses, the drawn lote, the dim row)    no DOM, tested in Node
//   shot.js     the capture of the dry run as a tilted sheet of paper at the right of the sheet
//   render.js   the page, built once and patched
//
// The contract: default export { id, mount(host, ctx), update(model, ctx), unmount(), onKey(actionId, row, event) }.
//   mount     builds the page into `host` (a skeleton until the first `update`)
//   update    called with every new model while COLA is on screen, and once after `mount`; patches, never rebuilds
//   onKey     a key of §3.4 with the row the cursor is on (the element, or null). `move` (j k) walks the proposals; the others act on the row
//             the cursor is on, or on the proposal in focus when it is on none. Returns true when the key was COLA's.
// What it takes from `ctx`: session.act, session.api, session.blob, session.said, sound.play, announce, open(jobKey, rowId), go(stopId, opts),
// reduced(), now() and, optionally, settings.get("batchMin") (the lote's threshold; 85 until something stores it), menu (ui/menu.js: the little
// menu of `s`), ask(actionId, row) (to replace that menu), toast(text, ok, { undo }) (with it a skip leaves a "Deshacer"), openUrl(url).

import { t } from "../i18n/index.js";
import { colaView, orderedProposals } from "./cola/view.js";
import { createColaActions, enterIsLots } from "./cola/actions.js";
import { createCola } from "./cola/render.js";
import { createShot } from "./cola/shot.js";
import { typingIn } from "../ui/keyfilter.js";

const DETAILS_MAX = 40;                 // the cache of /api/job answers: a handful of proposals is all that is ever on screen
const SNOOZE_ITEMS = [["1", "1d"], ["2", "3d"], ["3", "7d"], ["4", "14d"]];

let S = null;

const batchOf = (ctx) => (ctx.settings && typeof ctx.settings.get === "function" ? ctx.settings.get("batchMin") : undefined);

/** The view of this moment, worked out from the model it was last given and what it knows now (focus, detail, the page's clock). */
function viewOf(me) {
  return colaView(me.model, {
    nowMs: me.ctx.now(),
    focusKey: me.focusKey,
    batchMin: batchOf(me.ctx),
    intent: me.actions.engineState().intent,
    detail: (key) => me.details.get(key) || null,
  });
}

/** The proposal on screen is `focusKey`; when it is gone (approved, skipped, settled elsewhere) the one now at its place takes over. */
function settleFocus(me) {
  const keys = orderedProposals(me.model.snap).map((c) => c.key);
  if (keys.length === 0) { me.focusKey = null; me.focusAt = 0; return keys; }
  if (!keys.includes(me.focusKey)) me.focusKey = keys[Math.min(me.focusAt, keys.length - 1)];
  me.focusAt = keys.indexOf(me.focusKey);
  return keys;
}

/** What `GET /api/job?key=` says about the proposal in focus: the six bars, where it is, the posting. Asked once per proposal (and again only when he asks). */
function loadDetail(me, key) {
  if (me.details.has(key)) return;               // asked once: a failure is not asked again by itself (retryDetail forgets it and asks)
  me.details.set(key, { status: "loading" });
  while (me.details.size > DETAILS_MAX) me.details.delete(me.details.keys().next().value);
  const ask = typeof me.ctx.session.api === "function" ? () => me.ctx.session.api("/api/job?key=" + encodeURIComponent(key)) : () => Promise.reject(new Error("no api"));
  Promise.resolve().then(ask).then(
    (r) => { if (S === me) { me.details.set(key, r && r.ok && r.data ? { status: "ok", data: r.data } : { status: "error" }); paint(me); } },
    () => { if (S === me) { me.details.set(key, { status: "error" }); paint(me); } },
  );
}

/** Works the view out again and draws it: from `update` (a new model), from the focus moving and from the actions (a press, a dim row). */
function paint(me) {
  if (!me.model) return;
  settleFocus(me);
  if (me.focusKey) loadDetail(me, me.focusKey);
  me.view = viewOf(me);
  me.ui.patch(me.view);
}

const rowOf = (me, row) => (row && row.dataset && row.dataset.row ? me.ui.rowView(row.dataset.row) : null);

/** The menu of `s`: until when to set the proposal aside. Without a menu (and no `ask` of the host) the default answer is taken. */
function askUntil(me) {
  const ctx = me.ctx;
  if (ctx.ask) return (id, row) => ctx.ask(id, row);
  if (!ctx.menu) return null;
  return (id, row) => new Promise((resolve) => {
    const sel = `[data-row="${CSS.escape(row.id)}"]`;
    const from = me.ui.el.querySelector(sel + ' [data-action="snooze"]') || me.ui.el.querySelector(sel) || me.ui.el;
    // the menu closes first and runs the item after (ui/menu.js, pick): a close for any reason but a pick is a "no"
    ctx.menu.open({
      title: t("cola.snooze.title"),
      items: SNOOZE_ITEMS.map(([key, until]) => ({ key, label: t("cola.snooze." + until), run: () => resolve({ until }) })),
      from,
      onClose: (why) => { if (why !== "pick") resolve(null); },
    });
  });
}

function mount(host, ctx) {
  if (S) unmount();
  const openUrl = ctx.openUrl || ((url) => window.open(url, "_blank", "noopener,noreferrer"));
  const me = { ctx, host, model: null, view: null, focusKey: null, focusAt: 0, details: new Map(), off: [] };
  me.actions = createColaActions({
    session: ctx.session,
    sound: ctx.sound,
    announce: ctx.announce,
    t,
    open: (key, rowId) => { me.actions.batch.close(); return ctx.open(key, rowId); },
    go: (stop, opts) => ctx.go(stop, opts),
    openUrl,
    ask: askUntil(me),
    toast: typeof ctx.toast === "function" ? ctx.toast : null,
    // the engine's switch changes what the whole page says (the head, the verb's label, the foot), so it draws the view again; the rest repaint their own part
    onChange: (id) => { if (S !== me || !me.ui) return; if (id === "engine") paint(me); else me.ui.repaint(id); },
  });
  me.shot = createShot({ session: ctx.session });
  me.ui = createCola({
    actions: me.actions,
    shot: me.shot,
    reduced: ctx.reduced,
    on: {
      run: (id, row) => me.actions.run(id, row),
      step: (dir) => step(me, dir),
      focus: (key) => focus(me, key),
      batchOpen: () => openBatch(me),
      batchConfirm: () => me.actions.batch.confirm(),
      batchCancel: () => me.actions.batch.cancel(),
      engine: () => me.actions.engine(!!(me.view && me.view.engine.stopped)),
      retryDetail: () => { if (me.focusKey) { me.details.delete(me.focusKey); paint(me); } },
    },
  });
  host.append(me.ui.el);
  // Enter and Esc of the drawn lote: the confirmation is a part of the page, not a box with keys of its own, so the page listens while it is open
  const onKeydown = (e) => lotKey(me, e);
  document.addEventListener("keydown", onKeydown, true);
  me.off.push(() => document.removeEventListener("keydown", onKeydown, true));
  S = me;
}

function update(model, ctx) {
  if (!S || !model) return;
  S.ctx = ctx;
  S.model = model;
  S.actions.settle();
  paint(S);
  S.actions.batch.sync(orderedProposals(model.snap));
  closeLotForDrawer(S);
}

function focus(me, key) {
  if (!me.model || key === me.focusKey) return false;
  if (!orderedProposals(me.model.snap).some((c) => c.key === key)) return false;
  me.focusKey = key;
  me.ctx.sound.play("step");
  paint(me);
  const f = me.view && me.view.focus;
  if (f) me.ctx.announce(t("cola.said.focus", { company: f.company, role: f.title, at: f.step.at, total: f.step.total }));
  return true;
}

/** j (1) and k (-1) walk the proposals by score, and stop at both ends (no wrap: the end of the list is a fact). */
function step(me, dir) {
  if (!me.model) return false;
  const keys = orderedProposals(me.model.snap).map((c) => c.key);
  if (keys.length < 2) return false;
  const at = Math.max(0, keys.indexOf(me.focusKey));
  const next = Math.min(keys.length - 1, Math.max(0, at + dir));
  if (next === at) return false;
  return focus(me, keys[next]);
}

function openBatch(me) {
  const view = me.view;
  if (!view) return false;
  if (!view.batch.can) { me.ctx.announce(view.batch.none); return false; }
  if (me.actions.batch.isOpen()) return true;
  me.ctx.announce(view.batch.said);
  const opened = me.actions.batch.open(view.batch.picks);
  if (opened) me.ui.focusConfirm();
  return opened;
}

/** The drawer belongs to another question: a lote left drawn under it closes. */
function closeLotForDrawer(me) {
  const d = me.ctx.drawer;
  if (me.actions.batch.isOpen() && d && typeof d.isOpen === "function" && d.isOpen()) me.actions.batch.close();
}

/** ↵ confirms the drawn lote and Esc drops it; any other key leaves it open. A held key never confirms (actions.js). */
function lotKey(me, e) {
  if (S !== me || !me.actions.batch.isOpen()) return;
  closeLotForDrawer(me);
  if (!me.actions.batch.isOpen()) return;
  if (e.key !== "Enter" && e.key !== "Escape") return;
  if (typingIn(e.target) || (e.key === "Enter" && !enterIsLots(e.target, { inConfirm: me.ui.inConfirm, bare: [document.body, document.documentElement] }))) return;
  e.preventDefault();
  e.stopPropagation();
  if (e.key === "Escape") me.actions.batch.cancel();
  else if (!e.repeat) me.actions.batch.confirm();
}

function onKey(actionId, row, event) {
  if (!S || !S.view) return false;
  const me = S;
  const prevent = () => { if (event && event.preventDefault) event.preventDefault(); };
  if (actionId === "move") {
    prevent();
    step(me, event && (event.key === "k" || event.key === "ArrowUp") ? -1 : 1);
    return true;
  }
  if (actionId === "batch") { prevent(); openBatch(me); return true; }
  if (actionId === "engine") { prevent(); me.actions.engine(!!me.view.engine.stopped); return true; }
  if (me.actions.batch.isOpen()) return false;  // while the lote is drawn the keys of the rows wait; its own Enter and Esc are lotKey's, and a control keeps its Enter
  const view = rowOf(me, row) || me.ui.focusView();
  if (!view || !view.actions.some((a) => a.id === actionId)) return false;
  prevent();
  me.actions.run(actionId, view);
  return true;
}

function unmount() {
  if (!S) return;
  const me = S;
  S = null;
  for (const off of me.off) off();
  me.actions.destroy();
  me.ui.destroy();
  me.ui.el.remove();                       // only its own section: the shell may have mounted the next moment in the same host
}

export const id = "cola";
export { mount, update, unmount, onKey };
export default { id, mount, update, unmount, onKey };
