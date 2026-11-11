// SISTEMA, the machine room (spec §5.5): "¿Está vivo el motor y con qué cuenta corre?". This file only joins the parts and speaks the moment
// contract of spec §5; the work is in ./sistema/:
//   view.js     the page as data (the engine told from the snapshot alone, nothing invented, hostile text cleaned)       pure, tested in Node
//   actions.js  what `p` does: STOP at once, quitting STOP in two presses, undo, and the page's answer before the server's   no DOM, tested in Node
//   render.js   the page, built once and patched
//
// The contract: default export { id, mount(host, ctx), update(model, ctx), unmount(), onKey(actionId, row, event) }.
//   mount     builds the page into `host` (a skeleton until the first `update`)
//   update    called with every new model while SISTEMA is on screen, and once after `mount`; patches, never rebuilds
//   onKey     the one row/stop key SISTEMA has, `engine` (`p`): stops the engine, or asks twice to quit STOP; returns true when it was SISTEMA's
// What it takes from `ctx`: session.act, sound.play, announce, open(jobKey, rowId), now() and, optionally, toast(text, ok, { undo }) (the
// toast of the shell; with it a STOP leaves a "Deshacer"; without it the sentence is read through `announce`), settings.get("batchMin")
// (the batch threshold, the day something stores it; "sin dato" until then).

import { t, lang } from "../i18n/index.js";
import { sistemaView, SOURCES_SHOWN, ERRORS_SHOWN } from "./sistema/view.js";
import { createActions } from "./sistema/actions.js";
import { createSistema } from "./sistema/render.js";

let S = null;

const batchOf = (ctx) => (ctx.settings && typeof ctx.settings.get === "function" ? ctx.settings.get("batchMin") : null);

// The sheet holds the page without scrolling (the wheel belongs to the camera, spec §3.5). What has no fixed size is the two lists, boards and
// errors; when the page does not fit, they give way, one row at a time, and the rest is said as "y N más". Boards first, down to three, then
// errors down to two, then the keys of the foot (they are also in the palette), then both lists to the floor. The count in the title of the
// errors stays the whole one.
const FLOOR = { sources: 2, errors: 1 };
function shrunk(now) {
  if (now.sources > 3) return { ...now, sources: now.sources - 1 };
  if (now.errors > 2) return { ...now, errors: now.errors - 1 };
  if (now.foot) return { ...now, foot: false };
  if (now.sources > FLOOR.sources) return { ...now, sources: now.sources - 1 };
  if (now.errors > FLOOR.errors) return { ...now, errors: now.errors - 1 };
  return null;
}
const overflows = (host) => host.scrollHeight > host.clientHeight + 1;

/**
 * What decides how much room the lists need: the language, how much there is to list, what makes the left column taller or shorter (the warning
 * of the account, STOP, an error of the run) and the epoch, which moves when the window or the fonts change. While it is the same, a refresh
 * starts from the listing that fitted last time and writes nothing: the lists are not built up and cut down again at every poll.
 */
const fitKey = (me, full) => [lang(), full.sources.rows.length, full.sources.moreLabel, full.errors.total, full.account.anchor ? full.account.anchor.state : "", full.account.state,
  full.stop.on, full.run.result.tone, me.epoch].join("|");

/** Works the view out again and draws it: from `update` (a new model), from the actions (the intent or the arming changed) and from a resize. */
function paint(me) {
  if (!me.model) return;
  const state = me.actions.state();
  const base = { nowMs: me.ctx.now(), intent: state.intent, batchMin: batchOf(me.ctx) };
  const max = { sources: SOURCES_SHOWN, errors: ERRORS_SHOWN, foot: true };
  const full = sistemaView(me.model, { ...base, show: max });
  const key = fitKey(me, full);
  let show = me.fit && me.fit.key === key ? me.fit.show : max;
  for (;;) {
    me.view = show === max ? full : sistemaView(me.model, { ...base, show });
    me.ui.patch(me.view, { ...state, foot: show.foot });
    // the next step starts from what was really listed: a smaller number than the list has changes nothing and would only measure again
    const next = overflows(me.host) ? shrunk({ sources: me.view.sources.rows.length, errors: me.view.errors.rows.length, foot: show.foot }) : null;
    if (!next) break;
    show = next;
  }
  me.fit = { key, show };
}

function mount(host, ctx) {
  if (S) unmount();
  const me = { ctx, host, model: null, view: null, fit: null, epoch: 0 };
  me.actions = createActions({
    session: ctx.session,
    sound: ctx.sound,
    announce: ctx.announce,
    t,
    toast: typeof ctx.toast === "function" ? ctx.toast : null,
    onChange: () => { if (S === me) paint(me); },
  });
  me.ui = createSistema({
    run: () => me.actions.run(me.view && me.view.stop),
    open: (key, rowId) => ctx.open(key, rowId),
  });
  host.append(me.ui.el);
  // a resize changes how much room there is; the fonts arriving change how much the text takes: both fit the lists again
  me.refit = () => {
    if (me.frame || typeof requestAnimationFrame !== "function") return;
    me.frame = requestAnimationFrame(() => { me.frame = 0; me.epoch += 1; if (S === me) paint(me); });
  };
  if (typeof window !== "undefined") window.addEventListener("resize", me.refit);
  // a face that arrives after the first paint (fonts load when the text first uses them) changes how much room the text takes. `ready` is only worth
  // waiting for while something is loading: when everything has loaded it is already settled, and asking it anyway made a second pass of the fit in the
  // frame after EVERY mount, a trip's swap included (a long frame at CPU x4: ACEPTACION A7). A face that loads later still fires `loadingdone`.
  if (typeof document !== "undefined" && document.fonts) {
    if (document.fonts.ready && document.fonts.status !== "loaded") document.fonts.ready.then(me.refit);
    if (typeof document.fonts.addEventListener === "function") document.fonts.addEventListener("loadingdone", me.refit);
  }
  S = me;
}

function update(model, ctx) {
  if (!S || !model) return;
  S.ctx = ctx;
  S.model = model;
  S.actions.settle();
  paint(S);
}

function onKey(actionId, row, event) {
  if (!S || actionId !== "engine" || !S.view) return false;
  if (event && event.preventDefault) event.preventDefault();
  S.actions.run(S.view.stop);
  return true;
}

function unmount() {
  if (!S) return;
  const me = S;
  S = null;
  if (typeof window !== "undefined") window.removeEventListener("resize", me.refit);
  if (typeof document !== "undefined" && document.fonts && typeof document.fonts.removeEventListener === "function") document.fonts.removeEventListener("loadingdone", me.refit);
  if (me.frame && typeof cancelAnimationFrame === "function") cancelAnimationFrame(me.frame);
  me.actions.destroy();
  me.ui.destroy();
  me.ui.el.remove();                                  // only its own section: the shell may have mounted the next moment in the same host
}

export const id = "sistema";
export { mount, update, unmount, onKey };
export default { id, mount, update, unmount, onKey };
