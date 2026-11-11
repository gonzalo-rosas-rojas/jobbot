// PROCESOS, the stable place (spec §5.2): "¿Dónde está cada postulación y qué sigue?". Every live application is a pile of five layers in
// the lane of its stage, and a pile that changes lane leaves a gap where it was and travels to its new place. This file only joins the
// parts and speaks the moment contract of spec §5; the work is in ./procesos/:
//   view.js            the page as data (every count from the model, every word from i18n, hostile text cleaned)        pure, tested in Node
//   place.js           the order of the piles, frozen for the session, with tombstones                                    pure
//   cadence.js         the rhythm of the follow-ups (8-10, 15-17, 45 days)                                               pure
//   search.js          uFuzzy over company and role, the matched letters as text parts                                   pure
//   drawer-content.js  the page of ONE application for the drawer (score, plates, trail, ruler, mails, next step)         pure view + nodes
//   render.js, cells.js  the DOM, built once and patched
// The verbs (open, link, snooze, withdraw, register, confirm, the little menus, the two steps of what cannot be undone) are HOY's own
// (hoy/actions.js, hoy/forms.js): one behaviour for the same key in every stop.
//
// The contract: default export { id, mount(host, ctx), update(model, ctx), unmount(), onKey(actionId, row, event), arrive(opts), drawer(detail, card, ctx) }.
//   mount    builds the page into `host` (a skeleton until the first `update`)
//   update   called with every new model while PROCESOS is on screen, and once after `mount`; patches, never rebuilds
//   onKey    a key of the stop or of a row (§3.4): `f 1`-`f 4`, `/`, `v`, `n`, or `Enter`, `o`, `s`, `x`, `r`, `c` on the row the cursor is on;
//            returns true when the key was PROCESOS'
//   arrive   `go("procesos", { filter })`: HOY's "y M más" asks for the filter of what is his (works before or after the mount)
//   drawer   the content of the drawer for the application it opens, to be given to the common drawer (see the note in drawer-content.js)
// What it takes from `ctx`: session.act and session.fresh, sound.play, announce, open(jobKey, rowId), reduced(), now() and, optionally,
// menu (ui/menu.js, for the little menus and the new-task form), cursor (to put the keyboard on the list after a search), model() (the drawer),
// openUrl(url), ask(actionId, row), afterLink(row).

import { t } from "../i18n/index.js";
import { actionLabel } from "../core/format.js";
import { createActions } from "./hoy/actions.js";
import { createForms } from "./hoy/forms.js";
import { createPlace } from "./procesos/place.js";
import { procesosView, rowViewOf, filterId, FILTERS, ORDERS } from "./procesos/view.js";
import { createProcesos } from "./procesos/render.js";
import { drawerContent } from "./procesos/drawer-content.js";

// The order of the piles is frozen for the whole session of the page (a new page load forgets it), and so is what he chose to look at.
const place = createPlace();
const DEFAULTS = Object.freeze({ filter: "all", order: "stage", query: "", mode: "field", showClosed: false });
const memory = { ...DEFAULTS };

/** Forgets the frozen order and the choices: what a new load of the page does (tests, and the lab between scenarios). */
export function resetSession() {
  place.reset();
  Object.assign(memory, DEFAULTS);
}

let S = null;

/** A card of the snapshot by its key, wherever it is (a column of the board, the proposals, the stuck ones), or null. */
function findCard(model, key) {
  if (!model) return null;
  const snap = model.snap;
  for (const col of snap.board || []) for (const c of col.cards) if (c.key === key) return c;
  for (const c of [...((snap.awaiting_approval || {}).cards || []), ...((snap.needs_gonza || {}).cards || [])]) if (c.key === key) return c;
  return null;
}

function mount(host, ctx) {
  if (S) unmount();
  const openUrl = ctx.openUrl || ((url) => window.open(url, "_blank", "noopener,noreferrer"));
  const me = { ctx, host, model: null, armed: null, first: true, retry: null };

  me.actions = createActions({
    session: ctx.session,
    sound: ctx.sound,
    announce: ctx.announce,
    t,
    open: (key, rowId) => ctx.open(key, rowId),
    openUrl,
    ask: ctx.ask || (ctx.menu ? (id, row) => me.forms.ask(id, row) : null),
    toast: typeof ctx.toast === "function" ? ctx.toast : null,   // a skip, an answer and a withdrawal leave a "Deshacer"
    afterLink: ctx.afterLink || null,
    onChange: (id) => { if (S === me) { me.syncArmed(id); me.ui.repaint(id); } },
  });

  const ui = {
    get: () => memory,
    set(patch) {
      const next = { ...patch };
      if ("filter" in next) next.filter = filterId(next.filter);
      if ("order" in next && !ORDERS.includes(next.order)) delete next.order;
      if ("mode" in next && next.mode !== "table") next.mode = "field";
      if ("query" in next) next.query = String(next.query || "").slice(0, 60);
      Object.assign(memory, next);
      me.redraw();
      if ("filter" in next && me.view && ctx.announce) {
        const f = me.view.filters.find((x) => x.active);
        if (f) ctx.announce(`${f.label} · ${f.n}`);
      }
    },
    stateOf: (id) => ({ armed: me.actions.armedOf(id), pending: me.actions.isPending(id) }),
    retry: () => { if (me.retry) me.retry(); else me.redraw(); },
    setRetry: (fn) => { me.retry = fn; },
    toList: () => { if (ctx.cursor && ctx.cursor.key == null) ctx.cursor.move(1); },
  };
  me.set = ui.set;
  me.ui = createProcesos({ ui, run: (id, view) => me.actions.run(id, view), reduced: ctx.reduced });
  host.append(me.ui.el);
  me.forms = ctx.menu ? createForms({ menu: ctx.menu, session: ctx.session, sound: ctx.sound, announce: ctx.announce, now: ctx.now, cardOf: (key) => findCard(me.model, key), rowEl: (id) => me.ui.el.querySelector(`[data-row="${CSS.escape(id)}"]`) }) : null;

  // what a pressed-once verb says in the foot, and Esc taking it back before anything else hears the Esc
  me.syncArmed = (id) => {
    const verb = me.actions.armedOf(id);
    if (verb) me.armed = { id, verb };
    else if (me.armed && me.armed.id === id) me.armed = null;
    me.ui.armed(me.armed ? t("procesos.armed", { action: actionLabel(me.armed.verb) }) : null);
  };
  me.onEsc = (e) => {
    if (e.key !== "Escape" || !me.armed) return;
    e.stopPropagation();
    me.actions.disarm();
  };
  document.addEventListener("keydown", me.onEsc);

  me.redraw = () => {
    if (S !== me || !me.model) return;
    try {
      const view = procesosView(me.model, { nowMs: ctx.now(), place, ...memory });
      me.ui.clearError();
      me.ui.patch(view, { first: me.first });
      me.first = false;
      me.view = view;
    } catch (e) {
      console.error(e);
      me.ui.error(() => me.redraw());
    }
  };
  S = me;
}

function update(model, ctx) {
  if (!S || !model) return;
  S.model = model;
  S.ctx = ctx || S.ctx;
  S.redraw();
}

function onKey(actionId, row, event) {
  if (!S) return false;
  const stop = () => { if (event && event.preventDefault) event.preventDefault(); };
  if (actionId === "filter") {
    const n = Number(event && event.key);
    if (!(n >= 1 && n <= FILTERS.length)) return false;
    stop();
    S.set({ filter: FILTERS[n - 1].id });
    return true;
  }
  if (actionId === "find") { stop(); S.ui.focusFind(); return true; }
  if (actionId === "field") { stop(); S.set({ mode: memory.mode === "table" ? "field" : "table" }); return true; }
  if (actionId === "newtask") { // a key of the stop, not of a row
    if (!S.forms) return false;
    stop();
    S.forms.newTask(row || S.ui.el.querySelector(".head"));
    return true;
  }
  const view = row && row.dataset && row.dataset.row ? S.ui.rowView(row.dataset.row) : null;
  if (!view || !view.actions.some((a) => a.id === actionId)) return false;
  stop();
  S.actions.run(actionId, view);
  return true;
}

function arrive(opts) {
  if (opts && opts.filter) memory.filter = filterId(opts.filter);
  if (S) S.redraw();
}

/** The content of the drawer for an application: PROCESOS' own page of it, whose buttons run the same verbs as the keys of the list. */
function drawer(detail, card, ctx) {
  return drawerContent(detail, card, ctx, {
    run: async (id, opts) => {
      if (!S) return undefined;
      if (id === "newtask") { if (S.forms) S.forms.newTask(S.ui.el.querySelector(".head")); return undefined; }
      // the row the list has drawn, else the one the model gives: the drawer draws its buttons from the model, whatever the filter or the search hides
      const view = S.ui.rowView("card:" + card.key) || rowViewOf(S.model, card.key, (ctx || S.ctx).now());
      if (!view) return undefined;
      const result = await S.actions.run(id, view, opts);
      // the drawer is not redrawn by a refresh of the snapshot: what a verb changed (the trail, the next step) is read again
      if (result === "fired" && ctx && ctx.drawer && typeof ctx.drawer.reload === "function") ctx.drawer.reload();
      return result;
    },
  });
}

function unmount() {
  if (!S) return;
  const me = S;
  S = null;
  document.removeEventListener("keydown", me.onEsc);
  me.actions.destroy();
  me.ui.destroy();
  me.host.replaceChildren();
}

export const id = "procesos";
export { mount, update, unmount, onKey, arrive, drawer };
export default { id, mount, update, unmount, onKey, arrive, drawer };
