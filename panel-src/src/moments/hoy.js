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
import { createActions, unsnoozeRowOf } from "./hoy/actions.js";
import { createHoy } from "./hoy/render.js";
import { createForms } from "./hoy/forms.js";
import { createPrompt } from "./hoy/prompt.js";
import { openInNewTab } from "./shared/util.js";
import { indexCards } from "./shared/cards.js";

// "Since your last visit" means since the last time the page was open BEFORE this one: the shell saves a new `lastVisit` whenever
// the tab is hidden, so it is the visit boot held (core/diff.js `holdVisit`; read on the first mount when nothing held one) and kept for the whole session of the page.
let sinceMs = null;
function visitMs() {
  if (sinceMs == null) sinceMs = holdVisit();
  return sinceMs;
}

let S = null;

function mount(host, ctx) {
  if (S) unmount();
  const openUrl = ctx.openUrl || openInNewTab;
  const state = { ctx, first: true, host };
  state.actions = createActions({
    session: ctx.session,
    sound: ctx.sound,
    announce: ctx.announce,
    t,
    open: (key, rowId) => ctx.open(key, rowId),
    openUrl,
    // without a menu HOY still works, only `s` is "until tomorrow" and `x`, `r` do nothing (actions.js)
    ask: ctx.ask || (ctx.menu ? (id, row) => state.forms.ask(id, row) : null),
    toast: typeof ctx.toast === "function" ? ctx.toast : null,   // a skip, an answer and a withdrawal leave a "Deshacer"
    afterLink: ctx.afterLink || ((row) => { if (state.prompt) state.prompt.afterLink(row); }),
    onChange: (id) => { if (S === state) state.ui.repaint(id); },
  });
  state.ui = createHoy({
    actions: state.actions,
    run: (id, view) => state.actions.run(id, view),
    more: () => ctx.go("procesos", { filter: "turn" }),
    unsnooze: (key) => state.actions.run("unsnooze", unsnoozeRowOf(key)),
    reduced: ctx.reduced,
  });
  host.append(state.ui.el);
  // the little menus (postpone, withdraw, register, new task) need the shell's menu
  // the form reads the card the row was drawn from (hoy/view.js indexes the cards the same way), so a key in two places is one card for both
  const cardOf = (key) => (state.model ? indexCards(state.model.snap).get(key) || null : null);
  const rowEl = (id) => state.ui.el.querySelector(`[data-row="${CSS.escape(id)}"]`);
  state.forms = ctx.menu ? createForms({ menu: ctx.menu, session: ctx.session, sound: ctx.sound, announce: ctx.announce, now: ctx.now, cardOf, rowEl }) : null;
  state.prompt = createPrompt({ host: state.ui.el, run: (id, row, opts) => state.actions.run(id, row, opts), announce: ctx.announce });
  S = state;
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
  const state = S;
  S = null;
  state.actions.destroy();
  state.prompt.destroy();
  state.ui.destroy();
  // the whole host: the shell gives it to one moment at a time (ui/shell.js unmountMoment runs this unmount and empties the host before mountMoment
  // puts the next moment in), so there is no other moment's section to spare here
  state.host.replaceChildren();
}

export const id = "hoy";
export { mount, update, unmount, onKey };
export default { id, mount, update, unmount, onKey };
