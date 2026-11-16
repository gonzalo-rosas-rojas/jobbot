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
import { ENGINE_REF } from "./shared/engine.js";
import { typingIn } from "../ui/keyfilter.js";
import { openInNewTab } from "./shared/util.js";

const DETAILS_MAX = 40;                 // the cache of /api/job answers: a handful of proposals is all that is ever on screen
const SNOOZE_ITEMS = [["1", "1d"], ["2", "3d"], ["3", "7d"], ["4", "14d"]];

const bareTargets = () => [document.body, document.documentElement];   // where the focus is when no control has it

let S = null;

const stoppedNow = (state) => !!(state.view && state.view.engine.stopped);

const batchOf = (ctx) => (ctx.settings && typeof ctx.settings.get === "function" ? ctx.settings.get("batchMin") : undefined);

/** The view of this moment, worked out from the model it was last given and what it knows now (focus, detail, the page's clock). */
function viewOf(state) {
  return colaView(state.model, {
    nowMs: state.ctx.now(),
    focusKey: state.focusKey,
    batchMin: batchOf(state.ctx),
    intent: state.actions.engineState().intent,
    detail: (key) => state.details.get(key) || null,
  });
}

/** The keys of the proposals in the order j and k walk them (best score first). */
const proposalKeys = (state) => orderedProposals(state.model.snap).map((c) => c.key);

/** The proposal on screen is `focusKey`; when it is gone (approved, skipped, settled elsewhere) the one now at its place takes over. */
function settleFocus(state) {
  const keys = proposalKeys(state);
  if (keys.length === 0) { state.focusKey = null; state.focusAt = 0; return; }
  if (!keys.includes(state.focusKey)) state.focusKey = keys[Math.min(state.focusAt, keys.length - 1)];
  state.focusAt = keys.indexOf(state.focusKey);
}

/** What `GET /api/job?key=` says about the proposal in focus: the six bars, where it is, the posting. Asked once per proposal (and again only when he asks). */
function loadDetail(state, key) {
  if (state.details.has(key)) return;               // asked once: a failure is not asked again by itself (retryDetail forgets it and asks)
  state.details.set(key, { status: "loading" });
  while (state.details.size > DETAILS_MAX) state.details.delete(state.details.keys().next().value);
  const ask = typeof state.ctx.session.api === "function" ? () => state.ctx.session.api("/api/job?key=" + encodeURIComponent(key)) : () => Promise.reject(new Error("no api"));
  Promise.resolve().then(ask).then(
    (r) => { if (S === state) { state.details.set(key, r && r.ok && r.data ? { status: "ok", data: r.data } : { status: "error" }); refresh(state); } },
    (e) => {
      console.error(e);                           // the page shows only "error" (no text of the server's); the reason stays in the console
      if (S === state) { state.details.set(key, { status: "error" }); refresh(state); }
    },
  );
}

/** Decides what the page shows: the proposal in focus and, for it, the detail to ask the server for (once). Draws nothing. */
function decideFocus(state) {
  settleFocus(state);
  if (state.focusKey) loadDetail(state, state.focusKey);
}

/** Works the view out from what is decided and patches the page. Decides nothing and asks nothing. */
function draw(state) {
  state.view = viewOf(state);
  state.ui.patch(state.view);
}

/** Decides and then draws: from `update` (a new model), from the focus moving and from the actions (a press, a dim row). */
function refresh(state) {
  if (!state.model) return;
  decideFocus(state);
  draw(state);
}

const rowOf = (state, row) => (row && row.dataset && row.dataset.row ? state.ui.rowView(row.dataset.row) : null);

/** The menu of `s`: until when to set the proposal aside. Without a menu (and no `ask` of the host) the default answer is taken. */
function askUntil(state) {
  const ctx = state.ctx;
  if (ctx.ask) return (id, row) => ctx.ask(id, row);
  if (!ctx.menu) return null;
  return (id, row) => new Promise((resolve) => {
    const sel = `[data-row="${CSS.escape(row.id)}"]`;
    const from = state.ui.el.querySelector(sel + ' [data-action="snooze"]') || state.ui.el.querySelector(sel) || state.ui.el;
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
  const openUrl = ctx.openUrl || openInNewTab;
  // a drawn lote cannot be confirmed from behind an application's page: opening one closes the lote first
  const openClosingLot = (key, rowId) => { state.actions.batch.close(); return ctx.open(key, rowId); };
  const state = { ctx, host, model: null, view: null, focusKey: null, focusAt: 0, details: new Map(), off: [] };
  state.actions = createColaActions({
    session: ctx.session,
    sound: ctx.sound,
    announce: ctx.announce,
    t,
    open: openClosingLot,
    go: (stop, opts) => ctx.go(stop, opts),
    openUrl,
    ask: askUntil(state),
    toast: typeof ctx.toast === "function" ? ctx.toast : null,
    // the engine's switch changes what the whole page says (the head, the verb's label, the foot), so it draws the view again; the rest repaint their own part
    onChange: (id) => { if (S !== state || !state.ui) return; if (id === ENGINE_REF) refresh(state); else state.ui.repaint(id); },
  });
  state.shot = createShot({ session: ctx.session });
  state.ui = createCola({
    actions: state.actions,
    shot: state.shot,
    reduced: ctx.reduced,
    on: {
      run: (id, row) => state.actions.run(id, row),
      step: (dir) => step(state, dir),
      focus: (key) => focus(state, key),
      batchOpen: () => openBatch(state),
      batchConfirm: () => state.actions.batch.confirm(),
      batchCancel: () => state.actions.batch.cancel(),
      engine: () => state.actions.engine(stoppedNow(state)),
      retryDetail: () => { if (state.focusKey) { state.details.delete(state.focusKey); refresh(state); } },
    },
  });
  host.append(state.ui.el);
  // Enter and Esc of the drawn lote: the confirmation is a part of the page, not a box with keys of its own, so the page listens while it is open
  const onKeydown = (e) => lotKey(state, e);
  document.addEventListener("keydown", onKeydown, true);
  state.off.push(() => document.removeEventListener("keydown", onKeydown, true));
  S = state;
}

function update(model, ctx) {
  if (!S || !model) return;
  S.ctx = ctx;
  S.model = model;
  S.actions.settle();
  refresh(S);
  S.actions.batch.sync(orderedProposals(model.snap));
  closeLotForDrawer(S);
}

function focus(state, key) {
  if (!state.model || key === state.focusKey) return false;
  if (!proposalKeys(state).includes(key)) return false;
  state.focusKey = key;
  state.ctx.sound.play("step");
  refresh(state);
  const f = state.view && state.view.focus;
  if (f) state.ctx.announce(t("cola.said.focus", { company: f.company, role: f.title, at: f.step.at, total: f.step.total }));
  return true;
}

/** j (1) and k (-1) walk the proposals by score, and stop at both ends (no wrap: the end of the list is a fact). */
function step(state, dir) {
  if (!state.model) return false;
  const keys = proposalKeys(state);
  if (keys.length < 2) return false;
  const at = Math.max(0, keys.indexOf(state.focusKey));
  const next = Math.min(keys.length - 1, Math.max(0, at + dir));
  if (next === at) return false;
  return focus(state, keys[next]);
}

function openBatch(state) {
  const view = state.view;
  if (!view) return false;
  if (!view.batch.can) { state.ctx.announce(view.batch.none); return false; }
  if (state.actions.batch.isOpen()) return true;
  state.ctx.announce(view.batch.said);
  const opened = state.actions.batch.open(view.batch.picks);
  if (opened) state.ui.focusConfirm();
  return opened;
}

/** The drawer belongs to another question: a lote left drawn under it closes. */
function closeLotForDrawer(state) {
  const d = state.ctx.drawer;
  if (state.actions.batch.isOpen() && d && typeof d.isOpen === "function" && d.isOpen()) state.actions.batch.close();
}

/** ↵ confirms the drawn lote and Esc drops it; any other key leaves it open. A held key never confirms (actions.js). */
function lotKey(state, e) {
  if (S !== state || !state.actions.batch.isOpen()) return;
  closeLotForDrawer(state);
  if (!state.actions.batch.isOpen()) return;
  if (e.key !== "Enter" && e.key !== "Escape") return;
  const controlKeepsEnter = e.key === "Enter" && !enterIsLots(e.target, { inConfirm: state.ui.inConfirm, bare: bareTargets() });
  if (typingIn(e.target) || controlKeepsEnter) return;
  e.preventDefault();
  e.stopPropagation();
  if (e.key === "Escape") state.actions.batch.cancel();
  else if (!e.repeat) state.actions.batch.confirm();
}

function onKey(actionId, row, event) {
  if (!S || !S.view) return false;
  const state = S;
  const prevent = () => { if (event && event.preventDefault) event.preventDefault(); };
  if (actionId === "move") {
    prevent();
    step(state, event && (event.key === "k" || event.key === "ArrowUp") ? -1 : 1);
    return true;
  }
  if (actionId === "batch") { prevent(); openBatch(state); return true; }
  if (actionId === "engine") { prevent(); state.actions.engine(stoppedNow(state)); return true; }
  if (state.actions.batch.isOpen()) return false;  // while the lote is drawn the keys of the rows wait; its own Enter and Esc are lotKey's, and a control keeps its Enter
  const view = rowOf(state, row) || state.ui.focusView();
  if (!view || !view.actions.some((a) => a.id === actionId)) return false;
  prevent();
  state.actions.run(actionId, view);
  return true;
}

function unmount() {
  if (!S) return;
  const state = S;
  S = null;
  for (const off of state.off) off();
  state.actions.destroy();
  state.ui.destroy();
  state.ui.el.remove();                       // only its own section: the shell may have mounted the next moment in the same host
}

export const id = "cola";
export { mount, update, unmount, onKey };
export default { id, mount, update, unmount, onKey };
