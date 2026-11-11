// The shell of the single page (spec §2.1, §3, §6): it builds the frame inside `#shell` (skip link, HUD with the stops bar, the glass
// sheet `main#app`, the drawer `aside#detail`, the bottom bar, the toast and the one live region), keeps ONE moment mounted in the
// sheet, and wires every snapshot to everything that shows it. It does not move the camera: the stage (a desktop window) or the flat
// page (under 760 px) is a `nav` that the page installs with `setNav`, and the shell asks it to travel (`go`), to hold still while the
// drawer is open (`lock`) and to stop a trip (`cancel`). The stage calls back `showStop` (the moment under the sheet changed) and `arrive`
// (a trip ended, or the scroll came to rest on a stop). A moment never reaches for the stage, the sound or the session on its own: it
// gets `ctx` (moments/registry.js).

import { h, focusMark, restoreFocus, isTypingIn } from "../core/dom.js";
import { t, tn, lang, onLang, getTimeZone } from "../i18n/index.js";
import { stopCounts } from "../core/model.js";
import { humanize, cardIndex } from "../core/humanize.js";
import { saveView } from "../core/store.js";
import { STOP_IDS } from "../scene/poses.js";
import { momentOf, STOPS_META, askOf } from "../moments/registry.js";
import { createHud } from "./hud.js";
import { createStops } from "./stops.js";
import { createBar } from "./bar.js";
import { createRouter } from "./router.js";
import { createToast } from "./toast.js";
import { createAnnouncer } from "./announce.js";
import { createTitle } from "./title.js";
import { createHelp } from "./help.js";
import { createPalette } from "./palette.js";
import { buildEntries, cardsOf, actionVerbs } from "./palette-items.js";
import { createMenu } from "./menu.js";
import { createDrawer } from "./drawer.js";
import { createCursor } from "./cursor.js";
import { createTools } from "./tools.js";
import { createSoundUi } from "./sound-ui.js";
import { createKeyHandlers } from "./shell-keys.js";
import { liveSet } from "./wired.js";
import { installKeys } from "./keys.js";
import { quietFor } from "./keyfilter.js";
import { icon } from "./icons.js";

const NEWTASK_STOPS = ["hoy", "procesos"];                        // the stops whose `n` is a button on the flat page
const SLOW_MS = 3000;                                             // after this long without the first snapshot the loading state adds a second, quiet line
const JOB_STOPS = ["hoy", "procesos", "cola", "aprender"];        // the stops whose rows are applications: the palette opens one's drawer where he is; elsewhere it goes to PROCESOS first
const noNav = { go() {}, lock() {}, settle() {}, cancel() {}, repaint() {}, current: () => null };

/**
 * createShell({ root, view, session, settings, sound, flags, embedded, reduced, now, brand, about }) -> {
 *   ctx, app, host, hud, stops, drawer, help, palette, menu, toast, cursor, router, tools, announcer,
 *   setNav(nav, isFlat), go(id, opts), showStop(id, { fade }), arrive(id, { push, manual }), openJob(key), current(), model,
 *   tick() (a minute passed), setClock(text), setDialBig(on), announceCross(mark), isTyping() }
 * `reduced()` says whether motion is asked to be calm (the system's request or his own setting); `now()` is the page's "now" in ms (the clock
 * of the machine, or the machine's moved to the hour of ?clock= so that the words of a moment agree with the sky).
 */
export function createShell({ root, view, session, settings, sound, flags = {}, embedded = false, reduced = () => false, now = Date.now, brand, about }) {
  let model = null;
  let shown = null;                                              // the id of the moment under the sheet
  let nav = noNav;
  let flat = false;                                              // the flat page (a phone) is in charge: no keys are named to a hand with no keyboard
  let pendingJob = null;                                         // a deep link to an application on a stop the page is still travelling to
  // What the sheet is: "loading" until the first snapshot (or the first failure), "ready" once there is data, "error" when the first read failed. The sky,
  // the HUD, the stops and the dial are painted at once; the sheet says in calm words that it is loading instead of showing the empty skeleton of a moment.
  let phase = "loading";
  let slowTimer = null;

  const announcer = createAnnouncer();
  // What the server says reaches him in his words (core/humanize.js): the company and the role instead of the key, the date as he writes it. Every
  // text of the server goes through this one function on its way to the toast, the live region and the banner, so no moment has to know about it.
  const human = (text) => humanize(text, cardIndex(model && model.snap), { lang: lang(), tz: getTimeZone() });
  const toastBase = createToast({ say: (text) => announcer.say(text) });
  const toast = { ...toastBase, show: (text, ok, opts) => toastBase.show(human(text), ok, opts), polite: (text, ok) => toastBase.polite(human(text), ok) };
  const tools = createTools({ toast, sound, settings });
  // The sound's own face: the indicator of the HUD and the sheet "Escuchar los sonidos" (ui/sound-ui.js). Only a `sound` that is a player (it can say its
  // status) has one; the sheet is a dialog like the help, so the page behind it is inert while it is open.
  const soundUi = typeof sound.status === "function"
    ? createSoundUi({ player: sound, settings, chooseSound: (on) => tools.chooseSound(on), toast: (text, ok) => toast.show(text, ok), onToggle: (open) => { doc.inert = open; if (!open) nav.settle(); } })
    : null;
  const title = createTitle(brand);
  const router = createRouter({ embedded });
  const menu = createMenu({ sound });
  const cursor = createCursor({ view, sound, reduced, root: () => app });

  // ---- the frame
  const hud = createHud({ onSearch: () => palette.open(), extra: soundUi && soundUi.indicator, brand });
  const stops = createStops({ onSelect: (id) => go(id) });
  hud.center.append(stops.el);
  const bar = createBar({ onHelp: () => help.toggle() });
  const newTaskLabel = h("span", {});
  const newTask = h("button", { class: "flat-new label", type: "button", hidden: true, onclick: () => { const m = momentOf(shown); if (m && m.onKey) m.onKey("newtask", null, null); } }, icon("plus"), newTaskLabel);
  const bannerText = h("span", { class: "sheet-banner-text" });
  const retryButton = h("button", { class: "sheet-retry label", type: "button", onclick: () => retry() });
  const banner = h("div", { class: "sheet-banner", role: "alert", hidden: true }, bannerText, retryButton);
  const loadingEyebrow = h("p", { class: "loading-eyebrow label" });
  // a paragraph, not a heading: the h1 of the sheet is the only one of the document, and while it loads the status region says the words
  const loadingTitle = h("p", { class: "loading-title serif", tabindex: -1 });
  const loadingMore = h("p", { class: "loading-more label", hidden: true });
  const loadingEl = h("div", { class: "sheet-loading", role: "status", "aria-live": "polite" }, loadingEyebrow, loadingTitle, loadingMore);
  const host = h("div", { class: "sheet-host", hidden: true });
  const app = h("main", { id: "app", class: "sheet", tabindex: -1, "aria-busy": true }, h("div", { class: "flat-actions" }, newTask), banner, loadingEl, host);
  const detailEl = h("aside", { id: "detail", class: "detail", tabindex: -1, inert: true, "data-lenis-prevent": "" });
  const skip = h("a", { class: "skip", href: "#app", onclick: (e) => { e.preventDefault(); app.focus({ preventScroll: false }); } });
  root.replaceChildren(skip, hud.el, app, detailEl, bar.el, toast.el);
  document.body.append(announcer.el);                            // outside #doc: it must still be heard while the help makes the page inert

  const jobKeys = () => [...new Set(cursor.rows().map((row) => cursor.jobOf(row)).filter(Boolean))];
  const drawer = createDrawer({
    el: detailEl, session, router, stop: () => shown, sound, getCtx: () => ctx, jobKeys, toast: (text, ok) => toast.show(text, ok),
    onShow: (key, origin) => cursor.follow(key, origin),
    onOpenChange: (open) => nav.lock(open),
    // the stop's own page of an application (PROCESOS has one); the other stops leave the common content of ui/detail.js
    content: (detail, card, x) => { const m = momentOf(shown); return m && typeof m.drawer === "function" ? m.drawer(detail, card, x) : null; },
  });

  const handlers = createKeyHandlers({
    stop: () => shown, cursor, drawer, help: { isOpen: () => help.isOpen(), close: () => help.close(), toggle: () => help.toggle() },
    palette: { isOpen: () => palette.isOpen(), close: () => palette.close(), open: () => palette.open() }, menu, tools, sheet: soundUi && soundUi.sheet, go: (id) => go(id), cancelJump: () => nav.cancel(),
  });
  const doc = root.closest("#doc") || root;
  // a modal dialog leaves nothing reachable behind it (WAI-ARIA); the live region and the dialog itself are outside #doc
  const help = createHelp({ root: document.body, sound, getStop: () => shown, live: () => liveSet(shown), flat: () => flat, onToggle: (open) => { doc.inert = open; if (!open) nav.settle(); } });
  // Ctrl K: the same kind of dialog, so the same inert page and the same hand-back to the camera. It is given what it holds each time it opens (the stops, the
  // applications of the last snapshot, the settings as they are now) and what to do with a choice: it never reaches the server by itself.
  const palette = createPalette({
    root: document.body, sound, onToggle: (open) => { doc.inert = open; if (!open) nav.settle(); },
    source: () => ({
      entries: buildEntries({
        stops: STOPS_META, cards: cardsOf(model && model.snap), current: shown,
        state: { theme: tools.themeName() === "light" ? "light" : "dark", lang: lang(), sound: sound.enabled, motion: settings.get("motion") },
        run: { go: (id) => go(id), job: (key) => openApplication(key), theme: () => tools.theme(), lang: () => tools.lang(), sound: () => tools.sound(), motion: () => tools.motion(), help: () => help.open() },
        soundEntries: soundUi ? soundUi.entries() : null,
      }),
      verbs: actionVerbs(STOPS_META),
    }),
  });
  installKeys({ target: window, handlers, stop: () => shown, locked: () => menu.isOpen() || help.isOpen() || palette.isOpen() || !!(soundUi && soundUi.sheet.isOpen()), drawerOpen: () => drawer.isOpen(), typing: quietFor });

  // ---- the context of the moments (spec §2.3)
  const ctx = {
    t, tn, lang, sound, settings, session, flags, cursor, view, menu,
    open: (key, origin) => drawer.open(key, origin),
    go: (id, opts) => go(id, opts),
    reduced,
    flat: () => flat,
    now,
    about: about || null,                                        // the public demo's line about itself (HOY draws it); null on the real panel
    announce: (text) => announcer.say(human(text)),
    toast: (text, ok, opts) => toast.show(text, ok, opts),
    drawer: { open: (key, origin) => drawer.open(key, origin), close: () => drawer.close(), isOpen: () => drawer.isOpen(), step: (dir) => drawer.step(dir), reload: () => drawer.reload(), get key() { return drawer.key; } },
    model: () => model,
    themeName: () => tools.themeName(),
    chooseTheme: (name) => tools.chooseTheme(name),
  };

  // ---- the moment under the sheet
  const failed = (e) => {
    console.error(e);
    if (!host.querySelector("[data-room-error]")) host.prepend(h("div", { class: "sheet-banner", role: "alert", "data-room-error": "" }, t("room.error")));
  };

  /** Lists that scroll by themselves keep the wheel (Lenis ignores `data-lenis-prevent`): the host does when its content is taller than the sheet. */
  function syncScroll() {
    host.toggleAttribute("data-lenis-prevent", host.scrollHeight > host.clientHeight + 1);
  }

  function relabelNewTask() {
    newTaskLabel.textContent = t("keys.newtask");
    newTask.hidden = !NEWTASK_STOPS.includes(shown) || phase !== "ready";
  }

  /** The words of the loading state and of the button of the banner, in the language of the page. */
  function relabelLoading() {
    loadingEyebrow.textContent = shown ? t("tab." + shown) : "";
    loadingTitle.textContent = t("loading.title");
    if (!loadingMore.hidden) loadingMore.textContent = t("loading.slow");
    retryButton.textContent = t("loading.retry");
  }

  function setPhase(next) {
    phase = next;
    clearTimeout(slowTimer);
    slowTimer = null;
    loadingEl.hidden = next !== "loading";
    host.hidden = next !== "ready";
    if (next === "ready") app.removeAttribute("aria-busy"); else app.setAttribute("aria-busy", String(next === "loading"));
    if (next === "loading") {
      loadingMore.hidden = true;
      loadingMore.textContent = "";
      slowTimer = setTimeout(() => { if (phase === "loading") { loadingMore.textContent = t("loading.slow"); loadingMore.hidden = false; } }, SLOW_MS);
    }
    relabelLoading();
    relabelNewTask();
  }

  /** "Reintentar": the banner goes, the sheet is loading again, and the snapshot is read once more (forced). */
  function retry() {
    banner.hidden = true;
    if (!model) setPhase("loading");
    session.refresh({ force: true });
  }

  function mountMoment(id) {
    const moment = momentOf(id);
    app.dataset.moment = id;
    if (id === "prologo") { app.removeAttribute("aria-labelledby"); app.setAttribute("aria-label", t("tab.prologo")); }
    else { app.removeAttribute("aria-label"); app.setAttribute("aria-labelledby", "tab-" + id); }
    try {
      moment.mount(host, ctx);
      if (model) moment.update(model, ctx);
    } catch (e) { failed(e); }
    cursor.apply();
    relabelNewTask();
    syncScroll();
  }

  function unmountMoment() {
    if (!shown) return;
    try { momentOf(shown).unmount(); } catch (e) { console.error(e); }
    host.replaceChildren();
  }

  /** Puts the moment of `id` under the sheet (a no-op when it already is). `fade`: a cut under reduced motion gets a short crossfade (spec §3.9). */
  function showStop(id, { fade = false } = {}) {
    if (id === shown || !momentOf(id)) return;
    if (drawer.isOpen()) drawer.close({ silent: true });
    unmountMoment();
    shown = id;
    document.body.dataset.stop = id;
    view.room = id;
    saveView(view);
    view.cursor = null;
    hud.setStop(id);
    stops.setActive(id);
    loadingEyebrow.textContent = t("tab." + id);
    mountMoment(id);
    if (fade && typeof app.animate === "function") app.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 180, easing: "linear" });
  }

  /** After a trip by key or bar the keyboard would be on nothing (the old moment is gone): it lands on the heading of the new one. */
  function land() {
    const active = document.activeElement;
    const heading = phase === "ready" ? host.querySelector("h1") : phase === "loading" ? loadingTitle : null;
    if (heading && (!active || active === document.body || active === document.documentElement || !document.body.contains(active))) heading.focus({ preventScroll: true });
  }

  /** A trip ended (or the page came to rest on a stop): the address, the history, the announcement and the keyboard. `cut`: it was a cut. */
  function arrive(id, { push = false, manual = false, cut = false } = {}) {
    showStop(id, { fade: cut });                                 // a cut (calm motion) arrives with the short crossfade of spec §3.9
    router.write(id, drawer.key, { push });
    if (!manual) {
      announcer.say(t("nav.announce", { name: t("tab." + id), what: t(askOf(id)) }));
      sound.play("arrive", { room: id });                          // the prologue has no motif: the cue of a room is for the five that are rooms
      land();
    }
    if (pendingJob) { const key = pendingJob; pendingJob = null; drawer.open(key); }
  }

  /** Goes to a stop. `opts` belongs to the moment (HOY's "y M más" asks PROCESOS for a filter); the moment hears it before the page moves. */
  function go(id, opts) {
    if (!STOP_IDS.includes(id)) return;
    const moment = momentOf(id);
    if (opts && moment && typeof moment.arrive === "function") moment.arrive(opts);
    if (id === shown) { if (drawer.isOpen()) drawer.close(); return; }
    if (drawer.isOpen()) drawer.close({ silent: true });
    nav.go(id, { push: true });
  }

  /** The address named an application (`#procesos/<key>`): open it on its stop, once the page is there. */
  function openJob(key, stop = shown) {
    if (!key) return;
    if (stop && stop !== shown) { pendingJob = key; return; }
    drawer.open(key);
  }

  /** An application chosen in the palette: its drawer opens where he is when that stop has applications; from the prologue or SISTEMA it opens on PROCESOS. */
  function openApplication(key) {
    if (JOB_STOPS.includes(shown)) { drawer.open(key); return; }
    pendingJob = key;
    go("procesos");
  }

  // ---- snapshots
  function updateMoment() {
    const moment = shown && momentOf(shown);
    if (!moment || !model) return;
    const mark = focusMark(document.activeElement, host);
    try {
      moment.update(model, ctx);
      host.querySelector("[data-room-error]")?.remove();
    } catch (e) { failed(e); }
    cursor.apply();
    restoreFocus(mark, host);
    syncScroll();
  }

  function onSnapshot(event) {
    model = event.model;
    banner.hidden = true;
    if (phase !== "ready") setPhase("ready");                    // in the same task as the first draw, so no frame shows the empty skeleton between the two
    hud.setStatus(model.snap, now());
    stops.setCounts(stopCounts(model));
    title.update(model);
    // His busy hours are the ones outside the bot's window: no unsolicited sound then (spec §7).
    sound.setBusy(model.snap.controls.window_open === false);
    updateMoment();
    if (event.news && event.news.lines.length) announcer.say(event.news.lines.slice(0, 4).join(". "));
    // the sound of what changed while he was not looking is not decided here: main.js feeds the snapshots to the background detector (sound/background.js)
  }

  session.subscribe((event) => {
    if (event.type === "snapshot") onSnapshot(event);
    else if (event.type === "toast") toast.show(event.toast.message, event.toast.ok);
    else if (event.type === "error") {
      // a refresh that failed once the page has data is a toast and a backoff (session.js); a wrong key is a page that says so, for good
      if (event.error && (event.error.fatal || !model)) {
        bannerText.textContent = human(event.error.message);
        banner.hidden = false;
        if (!model) setPhase("error");
      }
    } else if (event.type === "recovered") banner.hidden = true;
  });

  // ---- the language: every label again, the moment drawn anew where it stands
  onLang(() => {
    hud.relabel();
    stops.relabel();
    bar.relabel();
    title.update(model);
    help.close();
    palette.close();
    menu.close();
    drawer.rerender();
    if (shown) {
      const mark = focusMark(document.activeElement, host);
      unmountMoment();
      mountMoment(shown);
      restoreFocus(mark, host);
    }
    relabelNewTask();
    relabelLoading();
    skip.textContent = t("shell.skip");
  });
  skip.textContent = t("shell.skip");
  relabelNewTask();
  setPhase("loading");

  router.listen(({ stop, job }) => {
    if (stop && stop !== shown) { if (drawer.isOpen()) drawer.close({ silent: true }); if (job) pendingJob = job; nav.go(stop, { push: false }); return; }
    if (job && job !== drawer.key) drawer.open(job);
    else if (!job && drawer.isOpen()) drawer.close({ fromPop: true });
  });

  window.addEventListener("resize", syncScroll);
  title.update(null);

  return {
    ctx, app, host, hud, stops, drawer, help, palette, menu, toast, cursor, router, tools, announcer, soundUi,
    /** `isFlat` says which nav it is: what the moments and the help tell about keys depends on it, so the moment on screen is drawn again when it changes. */
    setNav(next, isFlat = false) {
      nav = next || noNav;
      const changed = flat !== isFlat;
      flat = isFlat;
      if (changed) { updateMoment(); help.refresh(); }
    },
    go, showStop, arrive, openJob, syncScroll,
    /** The camera may glide a page that rests between two stops to the nearest one only while nothing else holds the page (spec A1). */
    canSnap: () => !drawer.isOpen() && !menu.isOpen() && !help.isOpen() && !palette.isOpen() && !(soundUi && soundUi.sheet.isOpen()),
    current: () => shown,
    get model() { return model; },
    /** A minute passed: the words that depend on "now" ("corrida hace 7 h") are worked out again. */
    tick() { if (model) hud.setStatus(model.snap, now()); },
    setClock: (text) => hud.setClock(text),
    /** The dial is big (prologue, HOY) or docked; the one clock of the page is the dial's while it is big and the HUD's when it docks (spec §4.8). */
    setDialBig(on) {
      if (on === null) delete hud.el.dataset.dial;
      else if (hud.el.dataset.dial !== (on ? "big" : "docked")) hud.el.dataset.dial = on ? "big" : "docked";
    },
    announceCross: (mark) => announcer.say(t("shell.cross." + mark)),
    isTyping: () => isTypingIn(app) || menu.isOpen(),
  };
}
