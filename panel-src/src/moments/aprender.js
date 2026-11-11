// APRENDER, el escritorio de noche (spec §5.4): "¿Qué no supo contestar el robot y qué debo repasar esta noche?". This file only joins the parts
// and speaks the moment contract of spec §5; the work is in ./aprender/:
//   data.js     the questions that block forms, what was learned, the eve of an interview, the patterns of the rejections                 pure
//   view.js     the page as data (every figure from the model, every word from i18n, "sin dato" for what the snapshot does not carry)      pure
//   actions.js  what `u` does (give the forms of a question back to the queue, in two presses)                                            no DOM
//   drill.js    the review: one question at a time, the answer when he asks, a grade 0-5                                                  no DOM
//   render.js   the page, built once and patched      drillui.js   the box of the review
//
// The contract: default export { id, mount(host, ctx), update(model, ctx), unmount(), onKey(actionId, row, event) }.
//   mount     builds the page into `host` (a skeleton until the first `update`)
//   update    called with every new model while APRENDER is on screen, and once after `mount`; patches, never rebuilds
//   onKey     a key of §3.4 with the row the cursor is on (the element, or null). `open` (Enter) on no row starts the review (or opens the eve's
//             dossier when nothing is due); `resolve` (u) on no row acts on the first question; returns true when the key was APRENDER's
// What it takes from `ctx`: session.act / api / said, sound.play, announce, open(jobKey, rowId), reduced(), now() and, optionally, toast(text, ok)
// (without it a failure is read through `announce`) and openUrl(url) (tests and the lab).
//
// What this moment does NOT do, on purpose: save an answer. `POST /api/answers` does not exist; the answer bank (profile/answers.toml) is private
// and read by the engine alone. The page says how he gives an answer today ("how"), and does not pretend to save one.

import { t } from "../i18n/index.js";
import { aprenderView } from "./aprender/view.js";
import { eveOf, parseDossier } from "./aprender/data.js";
import { createActions } from "./aprender/actions.js";
import { createDrill } from "./aprender/drill.js";
import { createDrillUi } from "./aprender/drillui.js";
import { createPage } from "./aprender/render.js";

const RETRY_MS = 60 * 1000;          // a dossier that failed to load is asked for again after this, at the next refresh
const REFRESH_MS = 10 * 60 * 1000;   // and one that loaded is read again after this (the engine may have written a better one)

let S = null;

/** The dossier of the card of the eve, fetched once and kept: key -> { state: "loading" | "ok" | "error" | "none", parsed, why, at }. */
function ensureDossier(me, nowMs) {
  const eve = eveOf(me.model.snap, nowMs);
  if (!eve || !eve.card.has_dossier) return;
  const key = eve.card.key;
  const have = me.dossiers.get(key);
  if (have && (have.state === "loading" || nowMs - have.at < (have.state === "error" ? RETRY_MS : REFRESH_MS))) return;
  const api = me.ctx.session && me.ctx.session.api;
  if (typeof api !== "function") { me.dossiers.set(key, { state: "error", why: t("aprender.nodata"), at: nowMs }); return; }
  // a reload keeps what was on screen until the new text arrives: the entry stays "ok" while it is asked for again
  me.dossiers.set(key, have && have.state === "ok" ? { ...have, at: nowMs } : { state: "loading", at: nowMs });
  Promise.resolve(api.call(me.ctx.session, "/api/job?key=" + encodeURIComponent(key))).then((r) => {
    if (S !== me) return;
    const at = me.ctx.now();
    if (!r || !r.ok || !r.data) me.dossiers.set(key, { state: "error", why: me.ctx.session.said ? me.ctx.session.said(r || {}, t("err.job")) : t("err.job"), at });
    else if (typeof r.data.dossier !== "string" || !r.data.dossier.trim()) me.dossiers.set(key, { state: "none", at });
    else me.dossiers.set(key, { state: "ok", parsed: parseDossier(r.data.dossier), at });
    paint(me);
  }, () => {
    if (S !== me) return;
    me.dossiers.set(key, { state: "error", why: t("err.network"), at: me.ctx.now() });
    paint(me);
  });
}

/** Works the view out again and draws it: from `update` (a new model), the actions (an armed or pending row), the review and the dossier. */
function paint(me) {
  if (!me.model) return;
  const nowMs = me.ctx.now();
  ensureDossier(me, nowMs);
  const view = aprenderView(me.model, { nowMs, dossiers: me.dossiers, expanded: me.expanded });
  me.view = view;
  me.page.patch(view);
  me.drillUi.paint();
}

function mount(host, ctx) {
  if (S) unmount();
  const me = { ctx, host, model: null, view: null, expanded: false, dossiers: new Map() };
  const session = ctx.session;
  const openUrl = ctx.openUrl || ((url) => window.open(url, "_blank", "noopener,noreferrer"));
  const toast = (text, ok) => { if (typeof ctx.toast === "function") ctx.toast(text, ok); else ctx.announce(text); };
  me.actions = createActions({
    session,
    sound: ctx.sound,
    announce: ctx.announce,
    t,
    open: (key, rowId) => ctx.open(key, rowId),
    openUrl,
    onChange: (id) => { if (S === me && me.page) me.page.repaint(id); },
  });
  me.drill = createDrill({
    api: (path) => session.api(path),
    act: (path, body, opts) => session.act(path, body, opts),
    said: (r, fallback) => (typeof session.said === "function" ? session.said(r, fallback) : fallback),
    toast,
    t,
    sound: ctx.sound,
    onChange: () => { if (S === me) paint(me); },
  });
  me.drillUi = createDrillUi({ drill: me.drill, t, reduced: ctx.reduced, onLeave: () => leave(me) });
  me.page = createPage({
    actions: me.actions,
    drill: me.drill,
    drillUi: me.drillUi,
    t,
    onOpen: (id, view) => me.actions.run(id, view),
    onMore: () => { me.expanded = !me.expanded; paint(me); },
    onTopic: (topic) => begin(me, topic),
    onStart: () => begin(me, ""),
  });
  host.append(me.page.el);
  S = me;
}

/** Opens the review (of one topic, or of whatever is due) and puts the keyboard on the first question once it is there. */
async function begin(me, topic) {
  await me.drill.start(topic);
  if (S === me && me.drill.state.active) me.drillUi.focusStart();
}

/** Leaves the review: the box closes, and the keyboard goes back to the button that opened it, or to the headline. */
function leave(me) {
  me.drill.leave();
  if (S !== me) return;
  const back = me.page.el.querySelector("button[data-ap='start']:not([hidden]), .ap-topic") || me.page.el.querySelector("#aprender-title");
  if (back) back.focus({ preventScroll: true });
}

function update(model, ctx) {
  if (!S || !model) return;
  S.ctx = ctx;
  S.model = model;
  paint(S);
}

function onKey(actionId, row, event) {
  if (!S) return false;
  const me = S;
  const rowEl = row && row.dataset && row.dataset.row ? row : null;
  const view = rowEl ? me.page.rowView(rowEl.dataset.row) : null;
  const handled = (fn) => { if (event && event.preventDefault) event.preventDefault(); fn(); return true; };
  if (actionId === "open" && !rowEl) {
    // Enter with the cursor on no row: the review, and once it is open Enter shows the answer (the box has its own keys too)
    if (me.drill.state.active) return me.drill.state.card && !me.drill.state.shown ? handled(() => me.drillUi.reveal && me.drillUi.reveal()) : false;
    if (me.view && me.view.drill.state === "due") return handled(() => begin(me, ""));
    const eve = me.view && me.view.eve.row;
    if (eve) return handled(() => me.actions.run("open", eve));
    return false;
  }
  const target = view || (actionId === "resolve" ? me.page.firstQuestion() : null);
  if (!target || !target.actions.some((a) => a.id === actionId)) return false;
  return handled(() => me.actions.run(actionId, target));
}

function unmount() {
  if (!S) return;
  const me = S;
  S = null;
  me.drill.leave();
  me.actions.destroy();
  me.page.destroy();
  me.page.el.remove();                                // only its own section: the shell may have mounted the next moment in the same host
}

export const id = "aprender";
export { mount, update, unmount, onKey };
export default { id, mount, update, unmount, onKey };
