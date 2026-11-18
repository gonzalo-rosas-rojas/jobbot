// The sound's own face without a browser (test/helpers/fake-dom.mjs, a fake player): the indicator of the HUD says what the sound is doing, and the
// sheet "Escuchar los sonidos" opens, drives the player, plays a sound on its key, closes (Esc, the veil, a new language) and gives the focus back.
import test from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
const { h } = await import("../src/core/dom.js");
const { setLang, t } = await import("../src/i18n/index.js");
const { createSoundUi } = await import("../src/ui/sound-ui.js");
const { CUES } = await import("../src/sound/cues.js");

function fakePlayer() {
  const state = { enabled: true, actions: true, background: true, level: "mid", muted: false, work: { on: false, from: "09:00", to: "18:00" }, calls: [] };
  const listeners = [];
  const player = {
    get enabled() { return state.enabled; },
    status: () => ({
      enabled: state.enabled, mode: !state.enabled ? "off" : state.actions && state.background ? "all" : "none",
      actions: state.actions, background: state.background, muted: state.muted, muteUntil: 0, work: { ...state.work, now: false }, level: state.level, volume: 0.5,
    }),
    setEnabled(on) { state.enabled = on; state.calls.push(["setEnabled", on]); listeners.forEach((f) => f()); },
    setBus(id, on) { state[id] = on; state.calls.push(["setBus", id, on]); listeners.forEach((f) => f()); },
    setLevel(id) { state.level = id; listeners.forEach((f) => f()); },
    cycleLevel() { return state.level; },
    muteFor() { return 0; }, muteUntilTomorrow() { return 0; }, unmute() {}, setWork() {},
    preview(id, opts) { state.calls.push(["preview", id, !!(opts && opts.alt)]); return true; },
    play(name) { state.calls.push(["play", name]); return true; },
    onChange(fn) { listeners.push(fn); },
  };
  return { player, state };
}

function rig({ chooseSound = undefined } = {}) {
  dom.reset();
  setLang("es");
  const { player, state } = fakePlayer();
  const log = { toggles: [], toasts: [], chosen: [] };
  const settings = { set: (k, v) => log.chosen.push([k, v]) };
  const opener = h("button", { id: "opener" }, "open");
  dom.document.body.append(opener);
  const ui = createSoundUi({
    player, settings, chooseSound, toast: (text, ok) => log.toasts.push([text, ok]), onToggle: (open) => log.toggles.push(open), root: () => dom.document.body,
    timers: { setInterval() {}, clearInterval() {}, setTimeout() { return 0; } },
  });
  dom.document.body.append(ui.indicator);
  return { ui, player, state, log, opener };
}

const key = (k) => ({ type: "keydown", key: k, bubbles: true, preventDefault() {}, stopPropagation() {} });

test("the indicator says what the sound does now and follows the player", () => {
  const { ui, state } = rig();
  assert.equal(ui.indicator.dataset.mode, "all");
  assert.equal(ui.indicator.getAttribute("aria-haspopup"), "dialog");
  state.enabled = false;
  ui.refresh();
  assert.equal(ui.indicator.dataset.mode, "off");
  assert.equal(ui.indicator.querySelector(".hud-sound-text").textContent, t("sound.state.off"));
});

test("the sheet opens as a modal dialog on its first switch, drives the player, and Esc gives the focus back", () => {
  const { ui, state, log, opener } = rig();
  opener.focus();
  ui.sheet.open();
  assert.equal(ui.sheet.isOpen(), true);
  assert.deepEqual(log.toggles, [true]);
  const dialog = dom.document.body.querySelector(".sound-sheet");
  assert.equal(dialog.getAttribute("aria-modal"), "true");
  assert.equal(dom.document.activeElement, dialog.querySelector(".sound-switch"));
  const actions = dialog.querySelectorAll(".sound-switch")[1];
  assert.equal(actions.getAttribute("aria-checked"), "true");
  actions.click();
  assert.deepEqual(state.calls.filter((c) => c[0] === "setBus"), [["setBus", "actions", false]]);
  assert.equal(dialog.querySelectorAll(".sound-switch")[1].getAttribute("aria-checked"), "false", "the switch follows the player");
  dialog.dispatchEvent(key("Escape"));
  assert.equal(ui.sheet.isOpen(), false);
  assert.equal(dom.document.body.querySelector(".sound-sheet"), null);
  assert.deepEqual(log.toggles, [true, false]);
  assert.equal(dom.document.activeElement, opener);
});

test("a press on the veil closes the sheet; a new language closes it too", () => {
  const { ui } = rig();
  ui.sheet.open();
  const veil = dom.document.body.querySelector(".sound-back");
  veil.dispatchEvent({ type: "mousedown", bubbles: true, target: veil });
  assert.equal(ui.sheet.isOpen(), false);
  ui.sheet.open();
  setLang("en");
  assert.equal(ui.sheet.isOpen(), false);
  setLang("es");
});

test("the key of a sound plays it and says which; with the sound off it goes on first, straight to the player and the settings", () => {
  const { ui, state, log } = rig();
  state.enabled = false;
  ui.sheet.open();
  const dialog = dom.document.body.querySelector(".sound-sheet");
  dialog.dispatchEvent(key("1"));
  assert.deepEqual(state.calls.filter((c) => c[0] === "setEnabled"), [["setEnabled", true]]);
  assert.deepEqual(log.chosen, [["sound", true]], "remembered in the settings");
  assert.deepEqual(state.calls.filter((c) => c[0] === "preview"), [["preview", CUES[0].id, false]]);
  assert.equal(dialog.querySelector(".sound-heard").textContent, t("sound.sheet.heard", { name: t("sound.cue." + CUES[0].id + ".name") }));
  assert.equal(dialog.querySelector('[data-cue="' + CUES[0].id + '"]').getAttribute("data-playing"), "true");
});

test("the master control goes through chooseSound when the shell gave one, else straight to the player", () => {
  const chosen = [];
  const a = rig({ chooseSound: (on) => chosen.push(on) });
  a.ui.controls.master();
  assert.deepEqual(chosen, [false]);
  assert.deepEqual(a.state.calls, []);
  const b = rig();
  b.ui.controls.master();
  assert.deepEqual(b.state.calls, [["setEnabled", false]]);
  assert.deepEqual(b.log.chosen, [["sound", false]]);
});

test("the palette's entries and the controls speak through the toast", () => {
  const { ui, log } = rig();
  ui.controls.bus("background");
  assert.deepEqual(log.toasts, [[t("sound.toast.background.off"), true]]);
  assert.ok(ui.entries().length >= 1);
});
