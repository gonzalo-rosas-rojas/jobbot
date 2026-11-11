// The stage (spec §3.3, §4.3): the one place that knows the sky, the skyline, the dial and the beam, and puts them in the
// same frame. render({ pos, hora }) draws what the page looks like at scroll position `pos` (0..5 stops) and hour `hora`:
// the dial's pose and hand, the lit mark, the sky of that hour, the skyline panned by the parallax, and ONE light born from
// the nearest visible body and clipped at the sheet's right edge (A17). It schedules nothing: the scene engine (scene/engine.js)
// calls it on every frame that has work, and once a minute at rest.

import { createSky, skyAt } from "./sky.js";
import { createSkyline } from "./skyline.js";
import { createDial } from "./dial.js";
import { beamFor, bodyKind, drawBeam } from "./beam.js";
import { STOP_IDS, dialPoseOf, dialPoseAt, markGlow, skylineShift, sheetGeometry, sheetOpacity, smoothstep } from "../scene/poses.js";

const HUD = 56;
// The layers of the city from this one on (0 far, 1 mid, 2 near) are painted on a canvas in front of the dial: the dial of HOY stands
// half sunk behind two layers of the city, as in relay2 (spec §2.2, veredicto §3.1). Reversible: 3 puts the whole city behind the dial.
export const FRONT_FROM = 1;

// The dial's text is --text-dim (and --text) on the bare sky. The sky of the day is light at mid height and pale at the horizon
// (--text-dim measures 1.4 to 3.2:1 there between 09:00 and 17:00), so the stage paints a soft pool of --night-1 under the dial
// on the canvas, before the light: the sun and its beam add to it, so they stay as bright as they were, while every label of the
// dial reads over a dark ground (>= 4.5:1, measured on real captures by test/stage.lab.browser.test.mjs at seven window sizes from 1366x657 to
// 3440x1440 and at 09:00, 11:00, 12:00, 13:30 and 17:00; other sizes and hours are not measured). The sky, the palette and
// the tokens are untouched. POOL.rgb is --night-1 (test/stage.stage.test.mjs keeps the two equal); `alpha` is how dark the pool is
// out to `solid` of its radius, which is `reach` times the half side of the dial's box (the numerals sit at 87 % of that half side),
// and from there it eases away on a smoothstep: with a straight ramp over the last fifth the pool had a rim and, by day, read as an
// eclipse (T24; reach 1.08 and solid 0.8 give that disc back). `day` is the share of `alpha` that stays under the sky of the day
// (poolAlpha): at night the pool cannot be seen, but by day a full one is a dark patch, which §4.8 rules out; the labels carry
// their own halo (dial.css), so the pool only has to take the glare of the sun out from behind them. Reversible: POOL.day = 1.
export const POOL = { rgb: [10, 14, 25], alpha: 0.76, day: 0.55, solid: 0.55, reach: 1.3 };

const lumaOf = ([r, g, b]) => {
  const lin = (c) => ((c /= 255) <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
};

/** How dark the pool is at an hour: POOL.alpha while the sky is dark, POOL.day of it once the middle of the sky is pale (luminance 0.06 and up; 0.02 and under is night; the sky of the day is a deep blue since T24, so its middle is about 0.07). */
export function poolAlpha(hora) {
  const pale = smoothstep(0.02, 0.06, lumaOf(skyAt(hora).mid));
  return POOL.alpha * (1 - pale * (1 - POOL.day));
}

/**
 * Soft patch of POOL.rgb at (x, y), radius r: `pool.alpha` out to POOL.solid of the radius, then eased away to nothing at the edge. Like the
 * light it is not painted left of `clipLeft` (the sheet's edge): the sheet covers that side, and the rim of a pool that is wide and low (the sunk
 * dial of HOY) would otherwise tint the skyline under the sheet and change the picture there from stop to stop.
 */
function drawPool(g, pool, clipLeft = 0) {
  const [r, gr, b] = POOL.rgb;
  const rg = g.createRadialGradient(pool.x, pool.y, 0, pool.x, pool.y, pool.r);
  rg.addColorStop(0, `rgba(${r},${gr},${b},${pool.alpha})`);
  rg.addColorStop(POOL.solid, `rgba(${r},${gr},${b},${pool.alpha})`);
  // From `solid` out the shadow eases away on a smoothstep instead of a straight ramp, so its edge has no rim to read as a disc.
  for (const k of [0.2, 0.4, 0.6, 0.8]) {
    rg.addColorStop(POOL.solid + (1 - POOL.solid) * k, `rgba(${r},${gr},${b},${(pool.alpha * (1 - k * k * (3 - 2 * k))).toFixed(3)})`);
  }
  rg.addColorStop(1, `rgba(${r},${gr},${b},0)`);
  g.save();
  g.globalCompositeOperation = "source-over";
  g.fillStyle = rg;
  const x0 = Math.max(pool.x - pool.r, clipLeft);
  if (x0 < pool.x + pool.r) g.fillRect(x0, pool.y - pool.r, pool.x + pool.r - x0, pool.r * 2);
  g.restore();
}

// The sky hands each entry of `beams` to its light painter before it draws the city; the stage uses that slot for the pool too:
// the wide glow of the beam first, then the pool over it, then the rays and the sun's core on top, so the pool dims the glow where
// the dial's labels are while the sun, its rays and the beam's far end stay as bright as the beam rule made them.
const paintLight = (g, light, clipLeft, holes) => (light.pool ? drawPool(g, light, clipLeft) : drawBeam(g, light, clipLeft, undefined, holes));
const lights = (beam, layout, alpha) => [
  ...(beam ? [{ ...beam, part: "bloom" }] : []),
  { pool: true, x: layout.cx, y: layout.cy, r: (layout.size / 2) * POOL.reach, alpha },
  ...(beam ? [{ ...beam, part: "rays" }] : []),
];

/** "HH:MM" for a decimal hour, to the nearest minute, round the clock (0.43 reads 00:26). */
export function clockText(hora) {
  const total = Math.round((((hora % 24) + 24) % 24) * 60) % 1440;
  return `${String(Math.floor(total / 60)).padStart(2, "0")}:${String(total % 60).padStart(2, "0")}`;
}

/** The transparent canvas that goes right over the dial (and under the shell) and takes the near layers of the city; null where there is no DOM. */
function frontCanvasOf(canvas, dialHost) {
  const doc = canvas && canvas.ownerDocument;
  if (!doc || typeof doc.createElement !== "function" || !dialHost || typeof dialHost.after !== "function") return null;
  const el = doc.createElement("canvas");
  el.className = "sky-front";
  el.setAttribute("aria-hidden", "true");
  dialHost.after(el);
  return el;
}

/** The drawer is a second glass beside the sheet: while it is open its rectangle is one the light is not painted in. */
function drawerHoles(doc) {
  const el = doc && typeof doc.getElementById === "function" ? doc.getElementById("detail") : null;
  if (!el || !el.classList || !el.classList.contains("open")) return [];
  const r = el.getBoundingClientRect();
  return r.width > 0 && r.height > 0 ? [{ left: r.left, top: r.top, right: r.right, bottom: r.bottom }] : [];
}

/**
 * canvas: the sky's canvas; dialHost: the dial's box; t: i18n; getSheetRect(): the live rectangle of the sheet in window px;
 * getHoles(): the other rectangles the light is not painted in (the drawer's, by default). The stage adds a canvas of its own right
 * over the dial for the near layers of the city (FRONT_FROM). parts = { dial, skyline, dpr, front } swaps a piece for a test without a
 * DOM (front: null for no second canvas).
 */
export function createStage({ canvas, dialHost, t, getSheetRect = () => null, getHoles = () => drawerHoles(canvas && canvas.ownerDocument), rand, parts = {} }) {
  const skyline = parts.skyline || createSkyline();
  const front = "front" in parts ? parts.front : frontCanvasOf(canvas, dialHost);
  const sky = createSky(canvas, { skyline, drawBeam: paintLight, rand, dpr: parts.dpr, front, frontFrom: FRONT_FROM });
  const dial = parts.dial || createDial(dialHost, { t });
  let dead = false;
  let last = null;                                  // the last frame asked for, painted again when the drawer opens or closes
  let watching = null;

  function geometry() {
    const { w, h } = sky.size();
    const { gutter, sheetRight } = sheetGeometry({ vw: w });
    return { vw: w, vh: h, gutter, sheetRight, hud: HUD };
  }

  /** The canvas changed size (or the first time): the dial's box takes the size of the biggest pose. */
  function resize() {
    if (dead) return;
    sky.resize();
    const g = geometry();
    dial.base(Math.max(...STOP_IDS.map((id) => dialPoseOf(id, g).size)));
  }

  /** One frame. Returns what was painted ({ beam, layout, sheet }) for tests and captures, or null once destroyed or with no canvas. */
  function render({ pos, hora, reduced = false, noBeam = false }) {
    if (dead) return null;
    const { w, h } = sky.size();
    if (!w || !h) return null;
    const g = geometry();
    dial.set(hora, clockText(hora));
    dial.glow(markGlow(pos));
    const layout = dial.layout(dialPoseAt(pos, g), hora);
    const live = getSheetRect();
    const sheet = live && live.right > live.left ? live : { left: g.gutter, right: g.sheetRight, top: HUD + 22, bottom: h - 40 };
    const shift = skylineShift(pos, reduced);
    // On a stop the light is cut at the sheet's right edge (A17). While the camera travels the sheet fades out and the light would hang
    // in mid sky with a cut edge and nothing to be cut by: its edge follows the sheet's own opacity back to the sheet's left side.
    const edge = sheet.right - (sheet.right - sheet.left) * (1 - sheetOpacity(pos, reduced));
    const holes = getHoles();
    // Only the light of a window needs the list of lit windows; the sun and the horizon do not look at the city. minA 0.2: at 02:00-04:30
    // the city sleeps and a window is drawn at a quarter strength or less (skyline.js, LIT), so the default 0.5 left the small hours with no light at all.
    const windows = bodyKind(hora) === "window" ? skyline.litWindows(w, h, shift, skyAt(hora).lit, { minX: sheet.right + 60, maxX: w - 40, minA: 0.2 }) : [];
    const found = beamFor({ hora, w, h, sheet: { left: sheet.left, right: edge, top: sheet.top, bottom: sheet.bottom }, hub: { x: layout.cx, y: layout.cy, size: layout.size }, windows });
    sky.paint({
      hora, shift,
      beams: lights(found && !noBeam ? found.beam : null, layout, poolAlpha(hora)),
      clipLeft: edge, holes,
      ghost: !reduced && Math.abs(pos - Math.round(pos)) > 0.04,
      warmX: found ? Math.min(0.95, Math.max(0.05, found.beam.x / w)) : 0.5,
    });
    if (!noBeam) { last = { pos, hora, reduced }; watchDrawer(); }
    return { beam: found, layout, sheet, holes };
  }

  /** The drawer opening or closing is a new picture of the light, and nothing else asks the stage for one while the page rests. */
  function watchDrawer() {
    const el = watching === null && canvas && canvas.ownerDocument && typeof canvas.ownerDocument.getElementById === "function" ? canvas.ownerDocument.getElementById("detail") : null;
    if (!el || typeof MutationObserver !== "function") return;
    watching = new MutationObserver(() => { if (last && !dead) render(last); });
    watching.observe(el, { attributes: true, attributeFilter: ["class"] });
  }

  return { resize, render, destroy() { dead = true; if (watching) watching.disconnect(); if (front && typeof front.remove === "function") front.remove(); } };
}
