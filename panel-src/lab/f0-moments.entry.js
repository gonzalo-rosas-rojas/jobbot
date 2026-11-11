// Lab "f0-moments": any of the six moments of the registry on the real sheet over the real stage, with a fake ctx that records what it
// was asked. Driven through window.__lab by test/moments.registry.browser.test.mjs and by hand for screenshots.
//   __lab.show(id, fixture, hour)   unmounts what is on, mounts the moment `id` with "empty" | "rich" | "heavy" and puts the stage at `hour`
//   __lab.lang(code)                the language and an update          __lab.state()   { went, said, sounds, opened }
//   __lab.textBoxes() / __lab.ghost(on)   the measure of A18 over the veil (test/moments.registry.browser.test.mjs)
import { momentOf } from "../src/moments/registry.js";
import { STOP_IDS } from "../src/scene/poses.js";
import { model, stopCounts } from "../src/core/model.js";
import { createStage, clockText } from "../src/stage/stage.js";
import { mulberry32 } from "../src/stage/skyline.js";
import { createHud } from "../src/ui/hud.js";
import { createStops } from "../src/ui/stops.js";
import { createBar } from "../src/ui/bar.js";
import { t, setLang, setTimeZone } from "../src/i18n/index.js";
import empty from "../test/fixtures/snapshot.empty.json";
import rich from "../test/fixtures/snapshot.rich.json";
import heavy from "../test/fixtures/snapshot.heavy.json";

const FIXTURES = { empty, rich, heavy };
setTimeZone("America/Santiago");
const log = { went: [], said: [], sounds: [], opened: [] };
const live = document.getElementById("live");
let snap = null;
let nowMs = 0;
let current = null;
let hora = 9;

const ctx = {
  session: { fresh: new Set(), act: async () => ({ ok: true }), api: () => new Promise(() => {}), blob: async () => null },   // api and blob: what COLA and APRENDER ask of the server; the lab has none, so a detail stays "loading"
  sound: { play: (name) => log.sounds.push(name) },
  announce: (text) => { live.textContent = text; log.said.push(text); },
  open: (key, row) => log.opened.push([key, row]),
  go: (stop, opts) => log.went.push([stop, opts || null]),
  reduced: () => true,
  now: () => nowMs,
  lastVisit: () => 0,
};

const sheetEl = document.getElementById("app");
const stage = createStage({ canvas: document.getElementById("sky"), dialHost: document.getElementById("dial"), t, getSheetRect: () => sheetEl.getBoundingClientRect(), rand: mulberry32(20261007) });
const hud = createHud({ onSearch: () => {} });
const stops = createStops({ onSelect: () => {} });
const bar = createBar({ onHelp: () => {} });
hud.center.append(stops.el);
document.getElementById("shell").append(hud.el, bar.el);

function frame() {
  const id = current ? current.id : "hoy";
  stops.setActive(id);
  hud.setStop(id);
  hud.setClock(clockText(hora));
  stage.resize();
  stage.render({ pos: Math.max(0, STOP_IDS.indexOf(id)), hora });
  if (snap) { hud.setStatus(snap, nowMs); stops.setCounts(stopCounts(model(snap))); }
}
window.addEventListener("resize", frame);

function update() {
  if (current && snap) current.update(model(snap), ctx);
  frame();
}

window.__lab = {
  show(id, name = "rich", at = 9) {
    if (current) current.unmount();
    snap = JSON.parse(JSON.stringify(FIXTURES[name]));
    nowMs = Date.parse("2026-10-07T12:00:00Z");
    hora = at;
    sheetEl.dataset.moment = id;
    current = momentOf(id);
    current.mount(document.getElementById("host"), ctx);
    update();
  },
  lang(code) { setLang(code); hud.relabel(); stops.relabel(); bar.relabel(); update(); },
  state: () => JSON.parse(JSON.stringify(log)),
  // For the contrast test (A18): where each text of the moment is painted, its colour, and a switch that makes the glyphs and the
  // underlines invisible, so a screenshot shows only what they are painted on (the veil over the sky).
  // Only what is painted: each rect is cut to the boxes that clip it (a list that scrolls, the sheet), as the APRENDER lab does, so a
  // label hidden under the bottom of a scroller is not measured against the sky behind it (2026-10-24: "Sin noticias" in PROCESOS).
  textBoxes: () => {
    const clips = (el) => { const out = []; for (let e = el; e && e !== document.body; e = e.parentElement) { const cs = getComputedStyle(e); if (cs.overflowX !== "visible" || cs.overflowY !== "visible") out.push(e.getBoundingClientRect()); } return out; };
    return [...document.querySelectorAll("#host *")].flatMap((el) => [...el.childNodes].filter((n) => n.nodeType === 3 && n.textContent.trim()).map((n) => {
      const range = document.createRange();
      range.selectNodeContents(n);
      const boxes = clips(el);
      const rects = [...range.getClientRects()].map((r) => boxes.reduce((a, b) => ({ left: Math.max(a.left, b.left), top: Math.max(a.top, b.top), right: Math.min(a.right, b.right), bottom: Math.min(a.bottom, b.bottom) }), { left: r.left, top: r.top, right: r.right, bottom: r.bottom }))
        .filter((r) => r.right - r.left > 1 && r.bottom - r.top > 1);
      return { cls: String(el.className), text: n.textContent.trim().slice(0, 40), color: getComputedStyle(el).color, rects };
    }));
  },
  ghost(on) {
    for (const el of document.querySelectorAll("#host *")) {
      if (on) { el.style.setProperty("color", "transparent", "important"); el.style.setProperty("border-color", "transparent", "important"); }
      else { el.style.removeProperty("color"); el.style.removeProperty("border-color"); }
    }
  },
};
frame();
