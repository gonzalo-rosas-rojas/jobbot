// Lab "f0-frame": the real stage behind the real HUD, stops bar and bottom bar, with a fake sheet that has the real measures.
// Driven through window.__lab by test/ui.frame.browser.test.mjs and by hand for screenshots.
//   __lab.go(id, hora)           moves to a stop (any of the six) at an hour: stops bar, HUD name, sky, dial and clock
//   __lab.counts({...})          the numbers of the stops bar        __lab.status(snap)   the HUD's window and run
//   __lab.texts()                { text, cls, color, rect }[] of every run of text of the HUD and the bar     __lab.notext(on)  makes them transparent
//   __lab.seen()                 { picked: ids chosen by click or arrow, help: n presses of "? atajos", search: n presses of Ctrl K }
import { createStage, clockText } from "../src/stage/stage.js";
import { mulberry32 } from "../src/stage/skyline.js";
import { createHud } from "../src/ui/hud.js";
import { createStops } from "../src/ui/stops.js";
import { createBar } from "../src/ui/bar.js";
import { createSoundUi } from "../src/ui/sound-ui.js";
import { createPlayer } from "../src/sound/player.js";
import { createPrefs } from "../src/sound/prefs.js";
import { sound as engine } from "../src/sound/index.js";
import { icon } from "../src/ui/icons.js";
import { STOP_IDS } from "../src/scene/poses.js";
import { t, setLang } from "../src/i18n/index.js";

const NOW = Date.parse("2026-10-07T12:00:00Z");
const seen = { picked: [], help: 0, search: 0 };

const canvas = document.getElementById("sky");
const sheetEl = document.getElementById("app");
const stage = createStage({ canvas, dialHost: document.getElementById("dial"), t, getSheetRect: () => sheetEl.getBoundingClientRect(), rand: mulberry32(20261007) });
// the sound's indicator is part of the HUD, so A18 and the geometry are measured on it too (off is its dimmest state: --text-dim)
const soundUi = createSoundUi({ player: createPlayer({ engine, prefs: createPrefs({ store: { get: () => null, set() {} } }) }), toast() {}, onToggle() {} });
const hud = createHud({ onSearch: () => { seen.search += 1; }, extra: soundUi.indicator });
const stops = createStops({ onSelect: (id) => { seen.picked.push(id); go(id, hora); } });
const bar = createBar({ onHelp: () => { seen.help += 1; } });
hud.center.append(stops.el);
document.getElementById("shell").append(hud.el, bar.el);

let hora = 9;
function go(id, at = hora) {
  hora = at;
  stops.setActive(id);
  hud.setStop(id);
  hud.setClock(clockText(at));
  stage.resize();
  stage.render({ pos: Math.max(0, STOP_IDS.indexOf(id)), hora: at });
}
window.addEventListener("resize", () => go(stops.el.dataset.at, hora));

// Every run of text of the HUD and the bottom bar with its colour and its box in window px, for the contrast test (review M3): the ground
// is measured on a capture with the text made transparent (`notext`), so it is exactly what the text is painted on.
function texts() {
  const out = [];
  for (const root of [hud.el, bar.el]) {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const text = node.textContent.trim();
      const el = node.parentElement;
      const cs = getComputedStyle(el);
      if (!text || cs.display === "none" || cs.visibility === "hidden") continue;
      const range = document.createRange();
      range.selectNodeContents(node);
      const r = range.getBoundingClientRect();
      if (r.width < 1 || r.height < 1) continue;
      out.push({ text, cls: el.className.baseVal === undefined ? el.className : "", color: cs.color, rect: { left: r.left, top: r.top, right: r.right, bottom: r.bottom } });
    }
  }
  return out;
}
const notext = (on) => document.body.classList.toggle("lab-notext", on);

window.__lab = {
  texts, notext,
  go,
  counts: (c) => stops.setCounts(c),
  status: (snap) => hud.setStatus(snap, NOW),
  lang: (code) => { setLang(code); hud.relabel(); stops.relabel(); bar.relabel(); },
  seen: () => ({ ...seen, picked: [...seen.picked] }),
  icon: (name) => { const el = icon(name, "icon-lg"); document.body.append(el); const box = el.querySelector("use").getBBox(); el.remove(); return { w: box.width, h: box.height }; },
};
go("hoy", 9);
