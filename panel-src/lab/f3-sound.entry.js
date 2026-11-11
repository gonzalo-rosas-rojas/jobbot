// Lab "f3-sound": the real HUD with the real sound indicator and the sheet "Escuchar los sonidos", on the real engine (the test Chrome is muted) and a
// player with a clock the test moves. Driven through window.__lab by test/ui.sound.browser.test.mjs and by hand for screenshots.
//   __lab.hour(h)       the working-day hour the player sees (default 21: outside working hours)     __lab.advance(ms)   the clock moves on
//   __lab.played()      the ids of the sounds that started                                              __lab.toasts        what the controls said
//   __lab.opens         how many times the sheet told the page to make itself inert (true/false)         __lab.ui / player / prefs   the objects
import { createHud } from "../src/ui/hud.js";
import { createSoundUi } from "../src/ui/sound-ui.js";
import { createPlayer } from "../src/sound/player.js";
import { createPrefs } from "../src/sound/prefs.js";
import { sound as engine } from "../src/sound/index.js";
import { setLang } from "../src/i18n/index.js";

const params = new URLSearchParams(location.search);
if (params.get("lang")) setLang(params.get("lang"));
if (params.get("theme") === "light") document.documentElement.setAttribute("data-theme", "light");
const memory = new Map();
const prefs = createPrefs({ store: { get: (k) => memory.get(k) ?? null, set: (k, v) => memory.set(k, v) } });
let clock = Date.parse("2026-10-07T21:00:00Z");
let hour = Number(params.get("hour") || 21);
const player = createPlayer({ engine, prefs, now: () => clock, hora: () => hour, reduced: () => params.get("motion") === "calm", hidden: () => false });
const toasts = [];
const opens = [];
const doc = document.getElementById("doc");
const ui = createSoundUi({
  player, settings: { set() {} }, toast: (text, ok) => toasts.push(text),
  onToggle: (open) => { opens.push(open); doc.inert = open; },
  chooseSound: (on) => { player.setEnabled(on); toasts.push(on ? "on" : "off"); },
  root: () => document.body,
});
const hud = createHud({ onSearch: () => {}, extra: ui.indicator });
document.getElementById("shell").prepend(hud.el);
hud.setStop("hoy");
hud.setClock("21:00");
document.body.addEventListener("keydown", (e) => { if (e.key === "m" && !e.target.closest("[data-local-keys]")) ui.controls.master(); });

window.__lab = {
  ui, player, prefs, toasts, opens,
  hour: (h) => { hour = h; ui.refresh(); },
  advance: (ms) => { clock += ms; ui.refresh(); },
  played: () => player.history().map((h) => h.id),
  engine, setLang,
};
