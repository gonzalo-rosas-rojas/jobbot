// HOY, 09:00, el relevo (spec §5.1): "¿Qué me toca hoy y qué pasó anoche?". The moment the page opens on in work mode. This file only
// joins the parts and speaks the moment contract of spec §5; the work is in ./hoy/:
//   view.js     the page as data (every count from the model, every word from i18n, hostile text cleaned)       pure, tested in Node
//   actions.js  what the keys and buttons do (the request of each verb, the two steps, the dim row)              no DOM, tested in Node
//   rows.js     one row, patched in place          render.js  the page, built once and patched
//
// The contract: default export { id, mount(host, ctx), update(model, ctx), unmount(), onKey(actionId, row, event) }.
//   mount     builds the page into `host` (a skeleton until the first `update`)
//   update    called with every new model while HOY is on screen, and once after `mount`; patches, never rebuilds
//   onKey     a row key of §3.4 (`a`, `h`, `Enter`…) with the row the cursor is on (the element, or null); acts on that row, or on LO SIGUIENTE
//             when the cursor is on none; returns true when the key was HOY's (the shell then does nothing more with it)
// What it takes from `ctx`: session.act and session.fresh, sound.play, announce, open(jobKey, rowId), go(stopId, opts), reduced(), now()
// and, optionally, menu (ui/menu.js: the little menus of `s`, `x`, `r` and `n`), openUrl(url), lastVisit() (ms; tests and the lab), flat() (true on a
// phone: the hint over the rows names no key; the labs have none), about (the public demo's line about itself, drawn first; absent on the real panel), and ask(actionId, row) and afterLink(row) to replace what forms.js and prompt.js do (tests).

import { t } from "../i18n/index.js";
import { holdVisit } from "../core/diff.js";
import { hoyView } from "./hoy/view.js";
import { createActions } from "./hoy/actions.js";
import { createHoy } from "./hoy/render.js";
import { createForms } from "./hoy/forms.js";
import { createPrompt } from "./hoy/prompt.js";

// "Since your last visit" means since the last time the page was open BEFORE this one: the shell saves a new `lastVisit` whenever
// the tab is hidden, so it is the visit boot held (core/diff.js `holdVisit`; read on the first mount when nothing held one) and kept for the whole session of the page.
let sinceMs = null;
const visitMs = () => (sinceMs == null ? (sinceMs = holdVisit()) : sinceMs);

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
  const me = { ctx, first: true, host };
  me.actions = createActions({
    session: ctx.session,
    sound: ctx.sound,
    announce: ctx.announce,
    t,
    open: (key, rowId) => ctx.open(key, rowId),
    openUrl,
    // without a menu HOY still works, only `s` is "until tomorrow" and `x`, `r` do nothing (actions.js)
    ask: ctx.ask || (ctx.menu ? (id, row) => me.forms.ask(id, row) : null),
    toast: typeof ctx.toast === "function" ? ctx.toast : null,   // a skip, an answer and a withdrawal leave a "Deshacer"
    afterLink: ctx.afterLink || ((row) => { if (me.prompt) me.prompt.afterLink(row); }),
    onChange: (id) => { if (S === me) me.ui.repaint(id); },
  });
  me.ui = createHoy({
    actions: me.actions,
    run: (id, view) => me.actions.run(id, view),
    more: () => ctx.go("procesos", { filter: "turn" }),
    unsnooze: (key) => me.actions.run("unsnooze", { id: "snz:" + key, data: { card: key }, open: null, link: null, actions: [{ id: "unsnooze", permanent: false }] }),
    reduced: ctx.reduced,
  });
  host.append(me.ui.el);
  // the little menus (postpone, withdraw, register, new task) need the shell's menu
  me.forms = ctx.menu ? createForms({ menu: ctx.menu, session: ctx.session, sound: ctx.sound, announce: ctx.announce, now: ctx.now, cardOf: (key) => findCard(me.model, key), rowEl: (id) => me.ui.el.querySelector(`[data-row="${CSS.escape(id)}"]`) }) : null;
  me.prompt = createPrompt({ host: me.ui.el, run: (id, row, opts) => me.actions.run(id, row, opts), announce: ctx.announce });
  S = me;
}

function update(model, ctx) {
  if (!S || !model) return;
  const view = hoyView(model, { nowMs: ctx.now(), lastVisitMs: ctx.lastVisit ? ctx.lastVisit() : visitMs(), flat: !!(ctx.flat && ctx.flat()), about: ctx.about });
  S.model = model;
  S.ui.patch(view, { fresh: ctx.session.fresh || new Set(), first: S.first });
  S.first = false;
  // what he went to do by hand is no longer on the list (settled elsewhere): there is nothing left to ask about
  const asked = S.prompt.pending();
  if (asked && !S.ui.rowView(asked.id)) S.prompt.cancel();
}

function onKey(actionId, row, event) {
  if (!S) return false;
  if (actionId === "newtask") { // a key of the stop, not of a row
    if (!S.forms) return false;
    if (event && event.preventDefault) event.preventDefault();
    S.forms.newTask(row || S.ui.el.querySelector(".next"));
    return true;
  }
  const view = row && row.dataset && row.dataset.row ? S.ui.rowView(row.dataset.row) : S.ui.nextView();
  if (!view || !view.actions.some((a) => a.id === actionId)) return false;
  if (event && event.preventDefault) event.preventDefault();
  S.actions.run(actionId, view);
  return true;
}

function unmount() {
  if (!S) return;
  const me = S;
  S = null;
  me.actions.destroy();
  me.prompt.destroy();
  me.ui.destroy();
  me.host.replaceChildren();
}

export const id = "hoy";
export { mount, update, unmount, onKey };
export default { id, mount, update, unmount, onKey };
