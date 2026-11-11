// What he sees and touches of the sound: the small indicator in the HUD that says in a word what the sound is doing now (off, actions, actions and
// background, silent until 21:40, mute for working hours), and the sheet "Escuchar los sonidos", where every sound of the vocabulary has a button and
// a key to hear it WHEN HE ASKS, and says which action or event it belongs to, so that he learns to tie one to the other. The same sheet holds the
// controls (the sound, the two buses, the volume, the silences, the working hours) so that muting is never more than a click or `m` away.
//
//   const ui = createSoundUi({ player, settings, chooseSound, toast, onToggle, root });
//   ui.indicator            the HUD button (main.js puts it in the HUD)
//   ui.sheet                { open(), close(), toggle(), isOpen() }       ui.entries()   the palette's entries about the sound
//   ui.controls             { master(), bus(id), mute(kind), work(), volume(), listen() }, what the palette's entries and the sheet's buttons call
//   ui.refresh()            works the state out again (the minute passed, the language changed)
//
// The sheet is a modal dialog like the help and the palette: the page behind is inert (`onToggle(open)` is how the shell does that), Tab stays inside,
// Esc or a click outside closes it and the focus goes back to where it was. Its box carries `data-local-keys`, so the page's own keys are quiet
// while it is open and every letter and digit is the key of one sound. Pressing a sound with the sound OFF turns it on first: that click is the
// gesture the browser asks for, and he asked to listen.

import { h } from "../core/dom.js";
import { t, fmtDate, onLang } from "../i18n/index.js";
import { icon } from "./icons.js";
import { CUES, GROUPS } from "../sound/cues.js";
import { LEVEL_IDS } from "../sound/player.js";
import { soundEntries, stateWords } from "./sound-entries.js";

const KEYCAPS = "123456789abcdefghijklmnopqrstuvwxyz";           // one key for each of up to 35 sounds; the list in cues.js is held to that
const HOUR = 3600000;
const REFRESH_MS = 15000;                                          // the end of a silence and the turn of the working day change the word without anyone touching anything
const PLAYING_MS = 450;
const time = (ms) => fmtDate(ms, "time");
const nameOf = (cue) => t(`sound.cue.${cue.id}.name`);
const FOCUSABLE = "button:not([disabled]), [href], input, select, [tabindex]:not([tabindex='-1'])";

export function createSoundUi({ player, settings = null, chooseSound = null, toast = () => {}, onToggle = () => {}, root = () => document.body, timers = { setInterval: (f, ms) => setInterval(f, ms), clearInterval: (id) => clearInterval(id), setTimeout: (f, ms) => setTimeout(f, ms) } }) {
  // ---- what the controls do (the palette calls these too)
  const say = (key, vars) => toast(t(key, vars), true);

  function enable(on) {
    if (chooseSound) chooseSound(on);
    else {
      player.setEnabled(on);
      if (settings) settings.set("sound", player.enabled);
    }
  }

  const controls = {
    master: () => enable(!player.enabled),
    bus(id) {
      const next = !player.status()[id];
      player.setBus(id, next);
      say(`sound.toast.${id}.${next ? "on" : "off"}`);
    },
    mute(kind) {
      if (kind === "off") { player.unmute(); say("sound.toast.unmute"); return; }
      const until = kind === "tomorrow" ? player.muteUntilTomorrow() : player.muteFor(HOUR);
      say("sound.toast.mute", { until: time(until) });
    },
    work() {
      const allow = player.status().work.on;
      player.setWork(!allow);
      say(allow ? "sound.toast.work.allow" : "sound.toast.work.silence");
    },
    volume() {
      const level = player.cycleLevel();
      say("sound.toast.volume", { level: t("sound.volume." + level).toLowerCase() });
      player.preview("switch");
    },
    listen: () => sheet.open(),
  };

  // ---- the indicator of the HUD
  const indicatorText = h("span", { class: "hud-sound-text" });
  const indicatorIcon = h("span", { class: "hud-sound-icon" });
  const indicator = h("button", { class: "hud-sound label", type: "button", "aria-haspopup": "dialog", onclick: () => sheet.toggle() }, indicatorIcon, indicatorText);
  let shownKey = "";

  function paintIndicator() {
    const s = player.status();
    const words = stateWords(s, time);
    const key = `${s.mode}|${words}|${s.volume}`;
    if (key === shownKey) return;
    shownKey = key;
    indicator.dataset.mode = s.mode;
    indicatorText.textContent = words;
    indicatorIcon.replaceChildren(icon(s.mode === "work" ? "clock" : s.mode === "off" || s.mode === "muted" || s.mode === "none" ? "volume-x" : "volume-2"));
    indicator.setAttribute("aria-label", s.mode === "work" ? t("sound.hud.aria.work", { until: s.work.to }) : t("sound.hud.aria", { state: words }));
    indicator.title = t("sound.hud.title", { state: words });
  }

  // ---- the sheet "Escuchar los sonidos"
  let box = null;
  let dialog = null;
  let from = null;
  let heard = null;
  let note = null;
  let syncControls = () => {};
  const flipped = new Map();                                       // a cue with two directions plays one, then the other
  let playingTimer = null;

  const isOpen = () => box !== null;

  function toggleButton(label, flag, run) {
    const state = h("span", { class: "sound-state" });
    const button = h("button", { class: "sound-switch", type: "button", role: "switch", onclick: run }, h("span", { class: "sound-switch-label" }, label), state);
    const sync = () => {
      const on = flag();
      button.setAttribute("aria-checked", String(on));
      state.textContent = t(on ? "sound.ctl.on" : "sound.ctl.off");
    };
    return { button, sync };
  }

  function choice(label, run, isOn) {
    const button = h("button", { class: "sound-choice", type: "button", onclick: run }, label);
    return { button, sync: () => button.setAttribute("aria-pressed", String(isOn())) };
  }

  function buildControls() {
    const status = () => player.status();
    const master = toggleButton(t("sound.ctl.master"), () => player.enabled, () => controls.master());
    const actions = toggleButton(t("sound.ctl.actions"), () => status().actions, () => controls.bus("actions"));
    const background = toggleButton(t("sound.ctl.background"), () => status().background, () => controls.bus("background"));
    const levels = LEVEL_IDS.map((id) => choice(t("sound.volume." + id), () => { player.setLevel(id); player.preview("switch"); }, () => status().level === id));
    const hour = h("button", { class: "sound-choice", type: "button", onclick: () => controls.mute("hour") }, t("sound.mute.hour"));
    const tomorrow = h("button", { class: "sound-choice", type: "button", onclick: () => controls.mute("tomorrow") }, t("sound.mute.tomorrow"));
    const unmute = h("button", { class: "sound-choice", type: "button", onclick: () => controls.mute("off") }, t("sound.mute.off"));
    const muteLine = h("p", { class: "sound-line", role: "status" });
    const workLine = h("p", { class: "sound-line" });
    const workButton = h("button", { class: "sound-choice", type: "button", onclick: () => controls.work() });
    const levelBox = h("div", { class: "sound-radios", role: "group", "aria-label": t("sound.ctl.volume") }, levels.map((l) => l.button));

    syncControls = () => {
      const s = status();
      for (const c of [master, actions, background, ...levels]) c.sync();
      unmute.hidden = !s.muted;
      hour.hidden = tomorrow.hidden = s.muted;
      muteLine.textContent = s.muted ? t("sound.state.muted", { until: time(s.muteUntil) }) : "";
      muteLine.hidden = !s.muted;
      workLine.textContent = t(s.work.on ? "sound.work.on" : "sound.work.off", { from: s.work.from, to: s.work.to });
      workButton.textContent = t(s.work.on ? "sound.work.allow" : "sound.work.silence");
      note.textContent = s.mode === "off" ? t("sound.sheet.note.off") : s.mode === "muted" ? t("sound.sheet.note.muted", { until: time(s.muteUntil) }) : s.mode === "work" ? t("sound.sheet.note.work", { until: s.work.to }) : "";
      note.hidden = note.textContent === "";
    };

    const row = (title, ...kids) => h("div", { class: "sound-ctl-row" }, h("span", { class: "sound-ctl-title label" }, title), h("div", { class: "sound-ctl-body" }, ...kids));
    return h("section", { class: "sound-ctl", "aria-label": t("sound.sheet.title") },
      row(t("sound.ctl.master"), master.button),
      row(t("sound.ctl.buses"), actions.button, background.button),
      row(t("sound.ctl.volume"), levelBox),
      row(t("sound.ctl.mute"), hour, tomorrow, unmute, muteLine),
      row(t("sound.ctl.work"), workLine, workButton));
  }

  /** Plays one sound because he asked: the sound goes on first if it was off (this click is the gesture the browser wants). */
  function listen(cue, rowEl) {
    if (!player.enabled) {
      player.setEnabled(true);
      if (settings) settings.set("sound", player.enabled);
    }
    const alt = cue.alt ? !flipped.get(cue.id) : false;
    if (cue.alt) flipped.set(cue.id, alt);
    const ok = player.preview(cue.id, { alt });
    const said = ok ? t("sound.sheet.heard", { name: nameOf(cue) }) : t("sound.sheet.failed");
    heard.textContent = said;
    if (rowEl) {
      for (const other of dialog.querySelectorAll("[data-playing]")) other.removeAttribute("data-playing");
      rowEl.setAttribute("data-playing", "true");
      if (typeof rowEl.scrollIntoView === "function") rowEl.scrollIntoView({ block: "nearest" });
      timers.setTimeout(() => rowEl.removeAttribute("data-playing"), PLAYING_MS);
    }
    syncControls();
    paintIndicator();
  }

  function cueRow(cue, cap) {
    const id = `sound-when-${cue.id.replace(/\./g, "-")}`;
    const name = nameOf(cue);
    return h("li", {}, h("button", {
      class: "sound-row", type: "button", "data-cue": cue.id, "aria-keyshortcuts": cap, "aria-label": t("sound.sheet.play", { name }), "aria-describedby": id,
      onclick: (e) => listen(cue, e.currentTarget),
    },
    h("span", { class: "sound-text" }, h("span", { class: "sound-name" }, name), h("span", { id, class: "sound-when" }, t(`sound.cue.${cue.id}.when`))),
    h("span", { class: "sound-go" }, icon("play"), h("kbd", { class: "k" }, cap))));
  }

  function buildLists() {
    return Object.entries(GROUPS).map(([bus, groups]) => h("section", { class: "sound-bus", "data-bus": bus },
      h("h4", { class: "sound-bus-title label" }, t(`sound.bus.${bus}`)),
      h("p", { class: "sound-bus-hint" }, t(`sound.bus.${bus}.hint`)),
      groups.map((group) => h("div", { class: "sound-group", role: "group", "aria-labelledby": `sound-g-${group}` },
        h("h5", { id: `sound-g-${group}`, class: "sound-group-title label" }, t(`sound.group.${group}`)),
        h("ul", { class: "sound-list" }, CUES.filter((c) => c.bus === bus && c.group === group).map((cue) => cueRow(cue, KEYCAPS[CUES.indexOf(cue)])))))));
  }

  function close() {
    if (!box) return;
    box.remove();
    box = dialog = heard = note = null;
    syncControls = () => {};
    onToggle(false);
    player.play("close");
    if (from && from.isConnected && from.focus) from.focus({ preventScroll: true });
    from = null;
  }

  function onKey(e) {
    if (e.isComposing) return;
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); close(); return; }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") { close(); return; }        // the palette opens over the page, not over this sheet
    if (e.key === "Tab") {
      const items = Array.from(dialog.querySelectorAll(FOCUSABLE)).filter((el) => el.getClientRects().length > 0);
      if (items.length === 0) { e.preventDefault(); return; }
      const at = items.indexOf(document.activeElement);
      const next = e.shiftKey ? (at <= 0 ? items.length - 1 : at - 1) : (at < 0 || at === items.length - 1 ? 0 : at + 1);
      e.preventDefault();
      items[next].focus();
      return;
    }
    if (e.ctrlKey || e.metaKey || e.altKey || e.key.length !== 1) return;
    const at = KEYCAPS.indexOf(e.key.toLowerCase());
    const cue = at >= 0 ? CUES[at] : null;
    if (!cue) return;
    e.preventDefault();
    listen(cue, dialog.querySelector(`[data-cue="${cue.id}"]`));
  }

  function open() {
    if (box) { dialog.focus(); return; }
    from = document.activeElement;
    note = h("p", { class: "sound-note", role: "status", hidden: true });
    heard = h("p", { class: "sound-heard label", role: "status", "aria-live": "polite" }, t("sound.sheet.hint"));
    const closeButton = h("button", { class: "sound-close label", type: "button", onclick: () => close() }, t("common.close"));
    const controlsEl = buildControls();
    dialog = h("div", { class: "sound-sheet", role: "dialog", "aria-modal": "true", "aria-labelledby": "sound-title", tabindex: -1, "data-local-keys": "", "data-lenis-prevent": "", onkeydown: onKey },
      h("div", { class: "sound-head" }, h("h3", { id: "sound-title" }, t("sound.sheet.title")), closeButton),
      h("p", { class: "sound-intro" }, t("sound.sheet.intro")),
      note,
      controlsEl,
      ...buildLists(),
      heard);
    box = h("div", { class: "sound-back", "data-lenis-prevent": "", onmousedown: (e) => { if (e.target === box) close(); } }, dialog);
    // the wheel is the sheet's or nobody's: over the veil it must not move the page behind (the stage's camera)
    box.addEventListener("wheel", (e) => { if (!dialog.contains(e.target)) e.preventDefault(); }, { passive: false });
    root().append(box);
    syncControls();
    player.play("open");
    onToggle(true);
    const first = dialog.querySelector(".sound-switch");
    (first || dialog).focus({ preventScroll: true });
  }

  const sheet = { open, close, isOpen, toggle: () => (box ? close() : open()) };

  // ---- keeping it true: the player tells when a choice changed; the clock tells the rest
  function refresh() {
    paintIndicator();
    syncControls();
  }
  player.onChange(refresh);
  timers.setInterval(refresh, REFRESH_MS);
  onLang(() => {
    shownKey = "";
    if (box) close();
    refresh();
  });
  paintIndicator();

  return {
    indicator,
    sheet,
    controls,
    refresh,
    entries: () => soundEntries({ status: player.status(), run: controls, time }),
  };
}
