// Lab "f0-engine": the real scene engine on the real Lenis (autoRaf off, the options of spec §3.2) over the real document map
// (a sticky 100svh stage and five spacers), with a sheet whose opacity is the one the stage will use (poses.js sheetOpacity).
// The unit tests of the engine use a fake Lenis; the inertia, the immediate writes and the native scroll events are only real
// here. Driven through window.__lab by test/scene.engine.browser.test.mjs.
//   __lab.state()        { y, pos, opacity, current, jumping }   what a person would see: the page's own scrollY and the sheet's computed opacity
//   __lab.jumpTo(id)     the engine's jump                       __lab.arrivals()   [{ id, cut, push, manual }] since the last clear
//   __lab.hold(on)       canSnap() answers !on (a drawer is open) __lab.snap()      engine.snap()
// A role=tablist of five buttons does what ui/stops.js does with the keys: ArrowRight/Left/Home/End jump and prevent the default.
import Lenis from "lenis";
import { createEngine } from "../src/scene/engine.js";
import { STOP_IDS, sheetOpacity } from "../src/scene/poses.js";
import { typingIn } from "../src/ui/keyfilter.js";

const sheet = document.getElementById("sheet");
const list = document.getElementById("list");
const tabs = document.getElementById("tabs");
const reducedQuery = matchMedia("(prefers-reduced-motion: reduce)");
const arrivals = [];
let hold = false;

for (let i = 0; i < 100; i++) { const row = document.createElement("p"); row.textContent = "row " + i; list.append(row); }

const lenis = new Lenis({ autoRaf: false, lerp: 0.1, smoothWheel: true, syncTouch: false });
const engine = createEngine({
  lenis,
  stops: STOP_IDS,
  reduced: () => reducedQuery.matches,
  canSnap: () => !hold,
  onFrame: (v) => sheet.style.setProperty("opacity", String(sheetOpacity(v.pos, reducedQuery.matches))),
  onArrive: (id, info) => arrivals.push({ id, cut: info.cut, push: info.push, manual: info.manual }),
});
engine.bind(window, { typing: typingIn });
engine.measure();

const TABS = STOP_IDS.slice(1);
for (const id of TABS) {
  const tab = document.createElement("button");
  tab.type = "button";
  tab.id = "tab-" + id;
  tab.setAttribute("role", "tab");
  tab.textContent = id;
  tab.addEventListener("click", () => engine.jumpTo(id));
  tabs.append(tab);
}
tabs.addEventListener("keydown", (e) => {
  const at = TABS.findIndex((id) => document.getElementById("tab-" + id) === document.activeElement);
  if (at < 0) return;
  const next = e.key === "ArrowRight" ? (at + 1) % TABS.length : e.key === "ArrowLeft" ? (at + TABS.length - 1) % TABS.length : e.key === "Home" ? 0 : e.key === "End" ? TABS.length - 1 : -1;
  if (next < 0) return;
  e.preventDefault();
  engine.jumpTo(TABS[next]);
  document.getElementById("tab-" + TABS[next]).focus();
});

window.__lab = {
  engine,
  state: () => ({
    y: window.scrollY,
    pos: window.scrollY / window.innerHeight,
    opacity: Number(getComputedStyle(sheet).opacity),
    current: engine.current(),
    jumping: !!engine.S.jump,
  }),
  jumpTo: (id, opts) => engine.jumpTo(id, opts),
  arrivals: () => arrivals.map((a) => ({ ...a })),
  clearArrivals: () => { arrivals.length = 0; },
  hold: (on) => { hold = !!on; },
  snap: () => engine.snap(),
  focusTab: (id) => document.getElementById("tab-" + id).focus(),
  reduced: () => reducedQuery.matches,
};
