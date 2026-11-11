// What the stage looks like at each stop (spec §3.3, §6.1): where the dial stands and how big it is, how far the three
// skyline layers have panned, which mark of the ring glows, and how opaque the sheet is. Pure functions of the scroll
// position in stops (`pos`, 0..5) and of the window; the sky itself does not live here: in work mode it follows the real hour.

export const STOP_IDS = ["prologo", "hoy", "procesos", "cola", "aprender", "sistema"];
// Hour of each stop's mark on the 24 h ring (spec §2.4). Never closer than 30 degrees; the closest pair, 03:15 and 05:30, is 33.75.
export const MARKS = { prologo: 3.25, hoy: 9, procesos: 26 / 60, cola: 18, aprender: 21, sistema: 5.5 };
export const PAN_PX = 600;                 // one stop of travel pans the world this much...
export const PARALLAX = [0.04, 0.1, 0.2];  // ...and the far, mid and near layers take this fraction of it

const HUD = 56;
const DOCK_GAP = 14;
const SCENE_MIN = 260;                     // narrower than this right of the sheet, the dial docks even in HOY
// Where the hero dial's centre stands, as a share of the window's height. HOY sinks it (relay2, juicio-arte: "el hero sube a 0,665"): the
// ring dips behind the city, which the stage paints in front of the dial. 0.64 is the deepest where the numerals 09 and 15 (the lowest
// that must stay readable: 220 units under the hub) still end above the highest roof of the city (0.838 of the height; scene.poses.test
// holds it); at 0.70 the near buildings hid the 15 and the caption of the busy arc, and A18 saw a label with no ink. The 12 and the
// caption may go behind the roofs, as in relay2. SISTEMA and, in a very wide window, PROCESOS and APRENDER keep the hero at mid
// height: nothing of the city stands in front of it there.
export const HERO_SUNK = 0.64, HERO_MID = 0.5;
export const WIDE_MIN = 2400;              // from this width on, PROCESOS and APRENDER keep the hero dial too: the sky right of the sheet is a third of the window
const clamp = (x, a, b) => Math.min(b, Math.max(a, x));
const lerp = (a, b, t) => a + (b - a) * t;

export const smoothstep = (a, b, x) => { const t = clamp((x - a) / (b - a), 0, 1); return t * t * (3 - 2 * t); };

/**
 * The sheet of the stop you are on is solid; between two stops it is gone. 1 within 0.04 of a stop, 0 from 0.28 away.
 * Under reduced motion there is no fade: the engine does not snap a page that rests between two stops (the browser scrolls it
 * as it likes), so a sheet that depended on the position would stay invisible there; it swaps at the half (sheetIndex) instead.
 */
export function sheetOpacity(pos, reduced = false) {
  if (reduced) return 1;
  const u = pos - Math.floor(pos);
  const d = Math.min(u, 1 - u);
  return 1 - smoothstep(0.04, 0.28, d);
}

/** Which stop's moment is mounted: the nearest one. */
export const sheetIndex = (pos) => clamp(Math.floor(pos + 0.5), 0, STOP_IDS.length - 1);

/** Pixels each skyline layer (far, mid, near) is displaced at `pos`; still under reduced motion. */
export const skylineShift = (pos, reduced = false) => PARALLAX.map((k) => (reduced ? 0 : PAN_PX * k * pos));

/** Signed hours (-12, 12] from `fromHora` to `toHora` the short way round a 24 h ring. */
export function shortestDelta(fromHora, toHora) {
  let d = (((toHora - fromHora) % 24) + 24) % 24;
  if (d > 12) d -= 24;
  return d;
}

/** The CSS of tokens.css worked out: gutter clamp(24px, 3.3vw, 64px), sheet clamp(960px, 58vw, 1400px). */
export function sheetGeometry({ vw }) {
  const gutter = clamp(0.033 * vw, 24, 64);
  const sheet = clamp(0.58 * vw, 960, 1400);
  return { gutter, sheet, sheetRight: gutter + sheet };
}

const dockSize = (vw) => clamp(0.06 * vw, 92, 120);

/**
 * Where the dial's box stands at a stop. g = { vw, vh, sheetRight, gutter, hud = 56 }.
 * `size` is the side of the SVG box (the ring is about 73 % of it); (cx, cy) its centre in window pixels.
 */
export function dialPoseOf(id, g) {
  const { vw, vh, sheetRight, gutter, hud = HUD } = g;
  const dock = () => { const size = dockSize(vw); return { size, cx: vw - gutter - size / 2, cy: hud + DOCK_GAP + size / 2, docked: true }; };
  // The prologue's dial is the biggest thing on the page and still sinks its ring behind the city, but at 0.62 the roofs cut the hour and hid
  // "hora local" (2026-10-24); at 0.55 both clear the buildings in front of them from 1366x768 to 3440x1440 (scene.poses.test holds it).
  if (id === "prologo") return { size: Math.min(0.94 * vh, 0.54 * vw), cx: 0.72 * vw, cy: 0.55 * vh, docked: false };
  // The hero dial stands in the sky right of the sheet, as big as the sky allows (and docks where there is no room for it).
  const hero = (cy) => {
    const scene = vw - sheetRight - gutter;
    if (scene < SCENE_MIN) return dock();
    return { size: Math.min(0.6 * vh, 0.92 * scene), cx: sheetRight + scene / 2, cy: cy * vh, docked: false };
  };
  if (id === "hoy") return hero(HERO_SUNK);
  if (id === "sistema") return hero(HERO_MID);                               // relay2 keeps the hero in the machine room
  if ((id === "procesos" || id === "aprender") && vw >= WIDE_MIN) return hero(HERO_MID);
  return dock();                                                             // COLA has the capture in that sky; a laptop has no room
}

/** The pose at a scroll position: the two neighbouring stops blended with a smoothstep. */
export function dialPoseAt(pos, g) {
  const p = clamp(pos, 0, STOP_IDS.length - 1);
  const i = Math.min(Math.floor(p), STOP_IDS.length - 2);
  const u = smoothstep(0, 1, p - i);
  const a = dialPoseOf(STOP_IDS[i], g), b = dialPoseOf(STOP_IDS[i + 1], g);
  return { size: lerp(a.size, b.size, u), cx: lerp(a.cx, b.cx, u), cy: lerp(a.cy, b.cy, u), docked: u > 0.6 ? b.docked : a.docked };
}

/** Which mark of the ring glows (angle in degrees, 0 up, clockwise) and how strongly; it travels the short way between marks. */
export function markGlow(pos) {
  const p = clamp(pos, 0, STOP_IDS.length - 1);
  const i = Math.min(Math.floor(p), STOP_IDS.length - 2);
  const u = p - i;
  const from = MARKS[STOP_IDS[i]], to = MARKS[STOP_IDS[i + 1]];
  const hora = from + shortestDelta(from, to) * u;
  return { angle: ((((hora % 24) + 24) % 24) / 24) * 360, strength: 0.55 + 0.45 * Math.abs(2 * u - 1) };
}
