// Lab "f0-dial": the dial alone on a dark stage, driven through window.__lab by test/stage.dial.browser.test.mjs.
//   __lab.place(pos, hora): the pose at scroll position `pos` (0..5 stops) with the hand at `hora`; returns the geometry
//   __lab.lang("en"): changes the language and relabels
import { createDial } from "../src/stage/dial.js";
import { STOP_IDS, dialPoseAt, dialPoseOf, markGlow, sheetGeometry } from "../src/scene/poses.js";
import { setLang, t } from "../src/i18n/index.js";

const dial = createDial(document.getElementById("dial"), { t });
const clockText = (hora) => `${String(Math.floor(hora)).padStart(2, "0")}:${String(Math.floor((hora % 1) * 60)).padStart(2, "0")}`;

function place(pos, hora) {
  const vw = innerWidth, vh = innerHeight;
  const { gutter, sheetRight } = sheetGeometry({ vw });
  const g = { vw, vh, sheetRight, gutter };
  dial.base(Math.max(...STOP_IDS.map((id) => dialPoseOf(id, g).size)));
  dial.set(hora, clockText(hora));
  dial.glow(markGlow(pos));
  return dial.layout(dialPoseAt(pos, g), hora);
}

window.__lab = { place, lang: (l) => { setLang(l); dial.relabel(); } };
place(1, 9);
