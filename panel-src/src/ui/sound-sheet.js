// The sheet "Escuchar los sonidos", where every sound of the vocabulary has a button and a key to hear it WHEN HE ASKS, and says which action or event it
// belongs to, so that he learns to tie one to the other. The same sheet holds the controls (the sound, the two buses, the volume, the silences, the working
// hours) so that muting is never more than a click or `m` away.
//
// The sheet is a modal dialog like the help and the palette: the page behind is inert (`onToggle(open)` is how the shell does that), Tab stays inside,
// Esc or a click outside closes it and the focus goes back to where it was. Its box carries `data-local-keys`, so the page's own keys are quiet
// while it is open and every letter and digit is the key of one sound. Pressing a sound with the sound OFF turns it on first: that click is the
// gesture the browser asks for, and he asked to listen.
//
//   createSoundSheet({ player, controls, setSoundOn, paintIndicator, onToggle, root, timers }) -> { sheet, sync }
//   sheet   { open(), close(), toggle(), isOpen() }          sync()   shows the player's state again in the open sheet (a no-op when closed)

import { h } from "../core/dom.js";
import { t } from "../i18n/index.js";
import { icon } from "./icons.js";
import { CUES, GROUPS } from "../sound/cues.js";
import { LEVEL_IDS } from "../sound/player.js";
import { clockTime } from "./sound-entries.js";
import { createFocusReturn } from "./focus-return.js";
import { modalVeil, closeOnEscape } from "./modal.js";

const KEYCAPS = "123456789abcdefghijklmnopqrstuvwxyz";           // one key for each of up to 35 sounds; the list in cues.js is held to that
const PLAYING_MS = 450;
const FOCUSABLE = "button:not([disabled]), [href], input, select, [tabindex]:not([tabindex='-1'])";
const nameOf = (cue) => t(`sound.cue.${cue.id}.name`);

/** The line under the sheet's title that says why nothing sounds, or "" when something does. */
function noteWords(s) {
  switch (s.mode) {
    case "off": return t("sound.sheet.note.off");
    case "muted": return t("sound.sheet.note.muted", { until: clockTime(s.muteUntil) });
    case "work": return t("sound.sheet.note.work", { until: s.work.to });
    default: return "";
  }
}

/** Where the focus goes on Tab among `count` stops from `at` (-1: none has it yet): it wraps, and Shift Tab goes the other way. */
function nextFocusIndex(at, count, backwards) {
  if (at < 0) return backwards ? count - 1 : 0;
  return (at + (backwards ? -1 : 1) + count) % count;
}

export function createSoundSheet({ player, controls, setSoundOn, paintIndicator, onToggle, root, timers }) {
  let box = null;
  let dialog = null;
  const focusReturn = createFocusReturn();
  let heard = null;
  let note = null;
  let syncControls = () => {};
  const flipped = new Map();                                       // a cue with two directions plays one, then the other

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
      muteLine.textContent = s.muted ? t("sound.state.muted", { until: clockTime(s.muteUntil) }) : "";
      muteLine.hidden = !s.muted;
      workLine.textContent = t(s.work.on ? "sound.work.on" : "sound.work.off", { from: s.work.from, to: s.work.to });
      workButton.textContent = t(s.work.on ? "sound.work.allow" : "sound.work.silence");
      note.textContent = noteWords(s);
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
  function playCue(cue, rowEl) {
    if (!player.enabled) setSoundOn(true);
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
      onclick: (e) => playCue(cue, e.currentTarget),
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
    focusReturn.restore();
  }

  function onKey(e) {
    if (e.isComposing) return;
    if (closeOnEscape(e, close)) return;
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") { close(); return; }        // the palette opens over the page, not over this sheet
    if (e.key === "Tab") {
      const items = Array.from(dialog.querySelectorAll(FOCUSABLE)).filter((el) => el.getClientRects().length > 0);
      if (items.length === 0) { e.preventDefault(); return; }
      e.preventDefault();
      items[nextFocusIndex(items.indexOf(document.activeElement), items.length, e.shiftKey)].focus();
      return;
    }
    if (e.ctrlKey || e.metaKey || e.altKey || e.key.length !== 1) return;
    const at = KEYCAPS.indexOf(e.key.toLowerCase());
    const cue = at >= 0 ? CUES[at] : null;
    if (!cue) return;
    e.preventDefault();
    playCue(cue, dialog.querySelector(`[data-cue="${cue.id}"]`));
  }

  function open() {
    if (box) { dialog.focus(); return; }
    focusReturn.remember();
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
    box = modalVeil({ cls: "sound-back", dialog, onClose: close, holdsWhenItFits: false }); // over the veil the wheel is nobody's; over the sheet it is the sheet's, even when it fits
    root().append(box);
    syncControls();
    player.play("open");
    onToggle(true);
    const first = dialog.querySelector(".sound-switch");
    (first || dialog).focus({ preventScroll: true });
  }

  return {
    sheet: { open, close, isOpen, toggle: () => (box ? close() : open()) },
    sync: () => syncControls(),
  };
}
