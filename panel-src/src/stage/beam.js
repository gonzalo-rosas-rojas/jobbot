// The light of the moment (spec §4.3, A17). One beam per frame, born from a body you can see (the sun at the hub of the
// dial, a lit window, the horizon) and never drawn over the sheet: the polygon used to prove it is clipped at the
// sheet's right edge and drawBeam paints nothing left of the same x (and eases the light in over the first pixels past it). Which body is the nearest one in TIME on the 24 h ring:
// at 09:00 the sun, at 00:26 a window, at 18:00 the horizon, and in between whichever is closer. In work mode the hour is
// the real one, so the light does not change when you travel between stops.

import { shortestDelta } from "../scene/poses.js";

export const FREE_MIN = 120;                               // narrower than this right of the sheet there is no room for a beam
const DOCKED = 120, DOCK_FULL = 320;                    // the dial box of a docked dial (92 to 120 px) and the one from which the sun's beam is the full one
export const BODY_HOURS = { sun: 9, window: 26 / 60, horizon: 18 };
const KINDS = ["sun", "window", "horizon"];
const cssa = (c, a) => `rgba(${c[0]},${c[1]},${c[2]},${Math.max(0, a).toFixed(3)})`;

/** The body whose hour is nearest to `hora` on the 24 h ring; a tie goes to the earlier one of sun, window, horizon. */
export function bodyKind(hora) {
  let best = KINDS[0], bestD = Infinity;
  for (const kind of KINDS) {
    const d = Math.abs(shortestDelta(hora, BODY_HOURS[kind]));
    if (d < bestD - 1e-9) { best = kind; bestD = d; }
  }
  return best;
}

/** Sutherland-Hodgman against the half plane x >= minX. */
export function clipPolygon(polygon, minX) {
  const out = [];
  for (let i = 0; i < polygon.length; i++) {
    const a = polygon[i], b = polygon[(i + 1) % polygon.length];
    const aIn = a[0] >= minX, bIn = b[0] >= minX;
    if (aIn) out.push(a);
    if (aIn !== bIn) {
      const k = (minX - a[0]) / (b[0] - a[0]);
      out.push([minX, a[1] + (b[1] - a[1]) * k]);
    }
  }
  return out;
}

const project = (points, ax, ay) => {
  let min = Infinity, max = -Infinity;
  for (const [x, y] of points) { const v = x * ax + y * ay; if (v < min) min = v; if (v > max) max = v; }
  return { min, max };
};

/** True when a convex polygon and a rectangle overlap with positive area (sharing an edge is not a hit). */
export function polygonHitsRect(polygon, rect) {
  if (polygon.length < 3) return false;
  const corners = [[rect.left, rect.top], [rect.right, rect.top], [rect.right, rect.bottom], [rect.left, rect.bottom]];
  const axes = [[1, 0], [0, 1]];
  for (let i = 0; i < polygon.length; i++) {
    const [x1, y1] = polygon[i], [x2, y2] = polygon[(i + 1) % polygon.length];
    if (x1 !== x2 || y1 !== y2) axes.push([-(y2 - y1), x2 - x1]);
  }
  for (const [ax, ay] of axes) {
    const p = project(polygon, ax, ay), r = project(corners, ax, ay);
    if (p.max <= r.min + 1e-9 || r.max <= p.min + 1e-9) return false;
  }
  return true;
}

/** The area the light covers, as a convex quad: a little wider than the core at the source, 90 % of the beam's width at the far end. */
export function coneOf(beam) {
  const ux = Math.cos(beam.ang), uy = Math.sin(beam.ang), nx = -uy, ny = ux;
  const r0 = Math.max(beam.core || 0, 6), hw = beam.wid * 0.45;
  const fx = beam.x + ux * beam.len, fy = beam.y + uy * beam.len;
  return [[beam.x + nx * r0, beam.y + ny * r0], [fx + nx * hw, fy + ny * hw], [fx - nx * hw, fy - ny * hw], [beam.x - nx * r0, beam.y - ny * r0]];
}

// Of the lit windows right of the sheet, the one nearest to 55 % of the free width (none: no light at all).
function windowFor(windows, clipX, w) {
  const target = clipX + (w - clipX) * 0.55;
  let best = null;
  for (const win of windows) {
    if (win.x < clipX + 60 || win.x > w - 40) continue;
    if (!best || Math.abs(win.x - target) < Math.abs(best.x - target)) best = win;
  }
  return best;
}

/**
 * The beam of the moment, or null. sheet = { left, right, top, bottom } in window px; hub = { x, y, size } the centre of the dial's
 * pose and the side of its box, where the sun is born (spec §3.1: the sun comes out of the dial's hub, not off the tip of its hand,
 * so the hand never points at a light that is somewhere else; the sun's disc is at most 5.5 % of the box, so on a small dial it
 * stays off the digits of the time); windows = lit windows of the near layer as { x, y, a, layer } (skyline.litWindows).
 */
export function beamFor({ hora, w, h, sheet, hub, windows = [] }) {
  const clipX = sheet.right;
  if (w - clipX < FREE_MIN) return null;
  const kind = bodyKind(hora);
  let beam;
  if (kind === "sun") {
    if (!hub || hub.x < clipX + 24 || hub.x > w || hub.y < 0 || hub.y > h) return null;
    // Where it reaches the sheet's edge: most of the way from the hub's own height to 52 % of the window's, so a sun that sits low (the
    // sunk dial of HOY) throws a light that lies nearly flat across the sky instead of a streak that climbs towards the sheet.
    const ty = hub.y + (h * 0.52 - hub.y) * 0.3;
    // A docked dial is a badge of about 100 px and the same light over a 1100 px beam turned into a white comet (the sum of the glow and
    // the rays reaches 0.9 over the darker sky of the day): the beam gets thinner and warmer as the dial shrinks, and is the full one
    // from `DOCK_FULL` px of dial box up (T24; k = 1 gives the old beam back).
    const k = Math.min(1, Math.max(0, ((hub.size || Infinity) - DOCKED) / (DOCK_FULL - DOCKED)));
    const warm = (lo, hi) => lo.map((v, i) => Math.round(v + (hi[i] - v) * k));
    beam = { x: hub.x, y: hub.y, ang: Math.atan2(ty - hub.y, clipX - hub.x), len: Math.hypot(hub.x - sheet.left, hub.y - ty) * 1.02, wid: h * 0.42, color: warm([255, 156, 80], [255, 186, 120]), a: 0.5 * (0.55 + 0.45 * k), core: Math.max(14, Math.min(h * 0.03, (hub.size || Infinity) * 0.055)), coreColor: [255, 240, 205] };
  } else if (kind === "window") {
    const win = windowFor(windows, clipX, w);
    if (!win) return null;
    const ty = h * 0.45;
    beam = { x: win.x, y: win.y, ang: Math.atan2(ty - win.y, clipX - win.x), len: Math.hypot(win.x - clipX, win.y - ty) * 1.15, wid: h * 0.3, color: [255, 205, 130], a: 0.42, core: Math.max(6, h * 0.008), coreColor: [255, 236, 190] };
  } else {
    const x0 = clipX + (w - clipX) * 0.08;
    beam = { x: x0, y: h * 0.96, ang: -0.1, len: (w - x0) * 1.05, wid: h * 0.34, color: [255, 150, 70], a: 0.46, core: h * 0.05, coreColor: [255, 196, 120] };
  }
  return { beam, polygon: clipPolygon(coneOf(beam), clipX), kind };
}

/** How far from the sheet's edge the light takes to come to full strength, at most; less when the source is closer than twice that. */
export const FADE_PX = 72;
/** The width of the fade-in for this beam: half the distance from the sheet to the source at most, so the fade never reaches the body. */
export const fadeWidth = (b, clipLeft) => (clipLeft > 0 ? Math.max(0, Math.min(FADE_PX, (b.x - clipLeft) * 0.5)) : 0);

let scratch = null;
/** A cleared 2D layer of w x h device pixels (one canvas, kept and reused) as { g, image }, or null where no canvas can be made (Node). */
function scratchLayer(w, h) {
  if (!scratch) {
    if (typeof OffscreenCanvas === "function") scratch = new OffscreenCanvas(w, h);
    else if (typeof document !== "undefined") scratch = document.createElement("canvas");
    else return null;
  }
  if (scratch.width !== w || scratch.height !== h) { scratch.width = w; scratch.height = h; }   // resizing clears it
  const g = scratch.getContext("2d");
  g.setTransform(1, 0, 0, 1, 0, 0);
  g.globalCompositeOperation = "source-over";
  g.clearRect(0, 0, w, h);
  return { g, image: scratch };
}

// The light itself, additively, in the current transform and clip. Its two parts can be painted apart (b.part = "bloom" or "rays"):
// the bloom is the wide soft glow (three layered ellipses), the rays are three thin rays and, with `core`, a disc with a halo. The
// stage puts its dark pool between them so the pool dims the wide glow over the dial's labels but not the sun and its rays.
// Both parts are centred near the source (a fifth and a quarter of the length), so the light is brightest at the body and thins
// out towards the sheet: a light whose peak sat half way would look like a patch of its own, cut off from the sun.
function paintBeam(g, b) {
  const bloom = b.part !== "rays", rays = b.part !== "bloom";
  g.save();
  g.globalCompositeOperation = "lighter";
  g.translate(b.x, b.y);
  g.rotate(b.ang);
  if (bloom) for (const [lk, wk, ak] of [[1, 1, 0.5], [0.72, 0.6, 0.75], [0.46, 0.32, 1]]) {
    g.save();
    g.translate(b.len * lk * 0.2, 0);
    g.scale(b.len * lk * 0.55, b.wid * wk * 0.5);
    const rg = g.createRadialGradient(0, 0, 0, 0, 0, 1);
    rg.addColorStop(0, cssa(b.color, b.a * ak)); rg.addColorStop(0.38, cssa(b.color, b.a * ak * 0.42)); rg.addColorStop(1, cssa(b.color, 0));
    g.fillStyle = rg; g.fillRect(-1.2, -1.2, 2.4, 2.4);
    g.restore();
  }
  if (rays) for (const [off, lk, a] of [[-0.03, 1.18, 0.5], [0.024, 1.06, 0.34], [0.07, 0.9, 0.18]]) {
    g.save();
    g.rotate(off);
    g.translate(b.len * lk * 0.28, 0);
    g.scale(b.len * lk * 0.5, b.wid * 0.085);
    const rg = g.createRadialGradient(0, 0, 0, 0, 0, 1);
    rg.addColorStop(0, cssa(b.color, b.a * a)); rg.addColorStop(1, cssa(b.color, 0));
    g.fillStyle = rg; g.fillRect(-1.2, -1.2, 2.4, 2.4);
    g.restore();
  }
  if (rays && b.core) {
    const cc = b.coreColor || [255, 243, 214];
    for (const [r, a] of [[b.core * 7, 0.16], [b.core * 2.8, 0.42], [b.core * 1.3, 0.85], [b.core, 1]]) {
      const rg = g.createRadialGradient(0, 0, 0, 0, 0, r);
      rg.addColorStop(0, cssa(cc, b.a * a)); rg.addColorStop(1, cssa(cc, 0));
      g.fillStyle = rg; g.beginPath(); g.arc(0, 0, r, 0, Math.PI * 2); g.fill();
    }
  }
  g.restore();
}

/** Restricts the clip of `g` to what is outside the rectangles `holes` ({ left, top, right, bottom } in CSS px; `ratio` scales them to the transform of `g`). */
function clipOutside(g, holes, ratio = 1) {
  if (!holes || !holes.length) return;
  g.beginPath();
  g.rect(-4, -4, 100000, 100000);
  for (const r of holes) g.rect(r.left * ratio, r.top * ratio, (r.right - r.left) * ratio, (r.bottom - r.top) * ratio);
  g.clip("evenodd");
}

/**
 * Paints one light on a 2D context. Nothing is painted left of `clipLeft` (the sheet's right edge; 0 for no clip), and the light
 * comes in over the first `fadeWidth` px right of it instead of being cut off at full strength: that strip is painted on a layer
 * of its own, multiplied by a ramp from 0 to 1 and added to the canvas. Where `layer` gives nothing (no canvas to make) the
 * beam is cut at the edge as before. `holes` are rectangles it is not painted in at all (the drawer is a second glass: the light
 * must not show through it any more than through the sheet).
 */
export function drawBeam(g, b, clipLeft = 0, layer = scratchLayer, holes = []) {
  const fade = fadeWidth(b, clipLeft);
  const matrix = typeof g.getTransform === "function" ? g.getTransform() : null;
  const ratio = matrix && matrix.a > 0 ? matrix.a : 1;
  const x0 = Math.ceil(clipLeft * ratio - 1e-6);                 // the strip starts on a whole device pixel
  const wPx = Math.round(fade * ratio);
  const hCss = g.canvas && g.canvas.height > 0 ? g.canvas.height / ratio : Math.ceil(b.y + b.len + b.wid);
  const strip = wPx >= 2 ? layer(wPx, Math.ceil(hCss * ratio)) : null;
  g.save();
  if (clipLeft > 0) { g.beginPath(); g.rect(strip ? (x0 + wPx) / ratio : clipLeft, -2, 100000, 100000); g.clip(); }
  clipOutside(g, holes);
  paintBeam(g, b);
  g.restore();
  if (!strip) return;
  const s = strip.g, h = Math.ceil(hCss * ratio);
  s.setTransform(ratio, 0, 0, ratio, -x0, 0);                    // the layer's first pixel is the device pixel x0 of the canvas
  paintBeam(s, b);
  s.setTransform(1, 0, 0, 1, 0, 0);
  s.globalCompositeOperation = "destination-in";
  const ramp = s.createLinearGradient(0, 0, wPx, 0);
  for (const [at, a] of [[0, 0], [0.25, 0.28], [0.5, 0.62], [0.75, 0.9], [1, 1]]) ramp.addColorStop(at, `rgba(0,0,0,${a})`);
  s.fillStyle = ramp; s.fillRect(0, 0, wPx, h);
  g.save();
  g.setTransform(1, 0, 0, 1, 0, 0);
  clipOutside(g, holes, ratio);
  g.globalCompositeOperation = "lighter";
  g.drawImage(strip.image, x0, 0);
  g.restore();
}
