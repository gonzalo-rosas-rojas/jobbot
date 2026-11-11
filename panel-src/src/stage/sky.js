// The sky (spec §4.3): a vertical gradient, a warm band at the horizon, seeded stars, the light of the moment (beams, drawn by
// stage/beam.js) and the skyline in front. Everything is a function of the hour on a 24 h ring; in work mode the hour is the
// server's wall clock, so the colour does not change when you travel between stops.

import { nightT } from "../scene/clock.js";

// [nightT (hours since 18:00), top, horizon, warm glow, warm alpha, stars alpha]. Seven night stops from the story's sky.js,
// three day stops of §4.3 (10:00, 13:00, 17:00), and a copy of 18:00 at 24 so the ring closes without a jump. The day is a deep
// cobalt with a mauve horizon, not the pale haze of the first table (09:00 was #243050 / #8c8da0): in T24 the pale sky read as
// washed out beside board/relay2 (HOY there is #0a1024 / #4a4572), turned the sun's beam greenish-white and made the dark pool under the
// dial an eclipse. Darker only helps A18. Reversible: the first table is in the history of this file (commit before T24).
export const STOPS = [
  [0.0, "#0b1226", "#222c50", "#ff9440", 0.42, 0.12],
  [1.2, "#090e1d", "#1c2441", "#d9703a", 0.26, 0.5],
  [2.6, "#05080f", "#0e1630", "#3a4a7c", 0.1, 0.9],
  [10.0, "#04060b", "#0a1224", "#27386a", 0.07, 1.0],
  [12.6, "#070b17", "#18203a", "#5a5a8a", 0.14, 0.75],
  [14.0, "#101830", "#3a4468", "#ffb27a", 0.42, 0.3],
  [15.0, "#141c3c", "#58537f", "#ffb98a", 0.5, 0.05],
  [16.0, "#1a2a52", "#53699f", "#ffd7a8", 0.3, 0],
  [19.0, "#1f3363", "#5675b0", "#ffe8c4", 0.18, 0],
  [23.0, "#1c2750", "#58628f", "#ffbf85", 0.35, 0],
  [24.0, "#0b1226", "#222c50", "#ff9440", 0.42, 0.12],
];

// Fraction of lit windows by nightT: the story's night (people go to bed, then wake up), then off in the daytime and on again
// towards 18:00. The values from 16 on are this plan's, not the spec's: T24 judges them with Gonza.
export const LIT = [[0, 0.55], [1, 0.62], [3, 0.46], [5.5, 0.2], [8, 0.07], [11, 0.04], [12.8, 0.12], [14, 0.3], [15, 0.22], [16, 0.08], [19, 0], [23, 0.04], [24, 0.55]];

const clamp = (x, a = 0, b = 1) => Math.min(b, Math.max(a, x));
const hexRgb = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
const mix = (a, b, t) => a.map((v, i) => Math.round(v + (b[i] - v) * t));
const css = (c) => `rgb(${c[0]},${c[1]},${c[2]})`;
const cssa = (c, a) => `rgba(${c[0]},${c[1]},${c[2]},${Math.max(0, a).toFixed(3)})`;
const PARSED = STOPS.map(([t, top, hor, warm, warmA, stars]) => ({ t, top: hexRgb(top), hor: hexRgb(hor), warm: hexRgb(warm), warmA, stars }));

/** Piecewise-linear value of `points` ([x, y] pairs, x ascending) at `x`, held flat past both ends. */
export function track(points, x) {
  if (x <= points[0][0]) return points[0][1];
  for (let i = 1; i < points.length; i++) {
    if (x <= points[i][0]) {
      const [x0, y0] = points[i - 1], [x1, y1] = points[i];
      return y0 + ((y1 - y0) * (x - x0)) / (x1 - x0);
    }
  }
  return points.at(-1)[1];
}

function sample(n) {
  let i = 0;
  while (i < PARSED.length - 2 && n > PARSED[i + 1].t) i++;
  const a = PARSED[i], b = PARSED[i + 1];
  const k = clamp((n - a.t) / (b.t - a.t));
  return {
    top: mix(a.top, b.top, k), hor: mix(a.hor, b.hor, k), warm: mix(a.warm, b.warm, k),
    warmA: a.warmA + (b.warmA - a.warmA) * k, stars: a.stars + (b.stars - a.stars) * k,
  };
}

/** 0 outside 19:30 to 06:30, ramping up from 19:30 to 21:00 and down from 05:00 to 06:30, 1 in between. */
export function starsGate(hora) {
  const h = ((hora % 24) + 24) % 24;
  const up = (x, a, b) => clamp((x - a) / (b - a));
  return h >= 12 ? up(h, 19.5, 21) : 1 - up(h, 5, 6.5);
}

/** The sky at an hour: gradient colours, glow, star alpha (already gated) and the fraction of lit windows. */
export function skyAt(hora) {
  const n = nightT(hora);
  const s = sample(n);
  return { top: s.top, mid: mix(s.top, s.hor, 0.5), hor: s.hor, warm: s.warm, warmA: s.warmA, stars: s.stars * starsGate(hora), lit: track(LIT, n) };
}

/**
 * Paints the sky on `canvas`. `skyline` draws the city, `drawBeam(g, beam, clipLeft, holes)` one light, `rand` seeds the 280 stars
 * once. `frame = { hora, shift, beams, clipLeft, holes, ghost, warmX }`; `warmX` is the fraction of the width where the warm band
 * is centred (the stage puts it under the light's source); `holes` are rectangles of the window the light is not painted in.
 * `front` is an optional second canvas (transparent, over the dial) that takes the layers of the city from `frontFrom` on: the
 * sky canvas keeps the far ones, and the dial stands between the two.
 */
const offscreen = typeof OffscreenCanvas === "function" ? (w, h) => new OffscreenCanvas(w, h) : null;      // null where there is none (Node): then everything is painted on every frame
const STAR_QUANT = 200;            // the stars' alpha is rounded to 1/200 of its scale: a step of it is a tenth of a level on the brightest star

export function createSky(canvas, { skyline, drawBeam, rand, dpr = () => Math.min(globalThis.devicePixelRatio || 1, 1.5), front = null, frontFrom = 1, makeCanvas = offscreen }) {
  const g = canvas.getContext("2d", { alpha: false });
  const fg = front ? front.getContext("2d") : null;
  const stars = Array.from({ length: 280 }, () => ({ x: rand(), y: Math.pow(rand(), 1.4) * 0.9, r: 0.35 + rand() * rand() * 1.5, a: 0.35 + rand() * 0.65 }));
  let w = 0, h = 0, ratio = 1;
  let starPicture = null;           // the 280 stars on a transparent canvas of their own: { key, picture }; they only change with the hour, by a hair, and not on a trip

  /** The stars are painted once per (size, ratio, alpha) and a frame copies them; with nowhere to paint them they are painted on every frame, as before. */
  function paintStars(alpha) {
    const q = Math.round(alpha * STAR_QUANT) / STAR_QUANT;
    const draw = (to, level) => {
      for (const st of stars) {
        const a = st.a * level;
        if (a < 0.02) continue;
        to.fillStyle = `rgba(236,232,221,${a.toFixed(3)})`;
        to.beginPath();
        to.arc(st.x * w, st.y * h, st.r, 0, Math.PI * 2);
        to.fill();
      }
    };
    const key = `${w}|${h}|${ratio}|${q}`;
    if (!makeCanvas) { draw(g, alpha); return; }
    if (!starPicture || starPicture.key !== key) {
      const picture = makeCanvas(canvas.width, canvas.height);
      const sg = picture.getContext("2d");
      sg.setTransform(ratio, 0, 0, ratio, 0, 0);
      draw(sg, q);
      starPicture = { key, picture };
    }
    g.save();
    g.setTransform(1, 0, 0, 1, 0, 0);               // the picture has the pixels of the canvas, one to one
    g.drawImage(starPicture.picture, 0, 0);
    g.restore();
  }

  function resize() {
    ratio = dpr();
    w = canvas.clientWidth;
    h = canvas.clientHeight;
    canvas.width = Math.round(w * ratio);
    canvas.height = Math.round(h * ratio);
    if (front) { front.width = canvas.width; front.height = canvas.height; }
  }

  function paint({ hora, shift = [0, 0, 0], beams = [], clipLeft = 0, holes = [], ghost = false, warmX = 0.5 }) {
    if (dpr() !== ratio) resize();                 // a window moved to a monitor of another pixel ratio fires no resize (review B18)
    const s = skyAt(hora);
    g.setTransform(ratio, 0, 0, ratio, 0, 0);
    g.globalCompositeOperation = "source-over";
    const lg = g.createLinearGradient(0, 0, 0, h);
    lg.addColorStop(0, css(s.top));
    lg.addColorStop(0.62, css(s.mid));
    lg.addColorStop(0.9, css(s.hor));
    lg.addColorStop(1, css(mix(s.hor, s.warm, 0.35 + s.warmA * 0.5)));
    g.fillStyle = lg;
    g.fillRect(0, 0, w, h);
    const cx = w * warmX;
    const hg = g.createRadialGradient(cx, h * 1.02, 0, cx, h * 1.02, Math.max(w * 0.75, h * 0.9));
    hg.addColorStop(0, cssa(s.warm, s.warmA));
    hg.addColorStop(0.5, cssa(s.warm, s.warmA * 0.28));
    hg.addColorStop(1, cssa(s.warm, 0));
    g.fillStyle = hg;
    g.fillRect(0, 0, w, h);
    if (s.stars > 0.02) paintStars(s.stars);
    for (const beam of beams) drawBeam(g, beam, clipLeft, holes);
    const city = { w, h, shift, lit: s.lit, top: s.top, hor: s.hor, ghost, ratio };
    if (!fg) { skyline.draw(g, city); return; }
    skyline.draw(g, { ...city, to: frontFrom });
    fg.setTransform(ratio, 0, 0, ratio, 0, 0);
    fg.clearRect(0, 0, w, h);
    skyline.draw(fg, { ...city, from: frontFrom });
  }

  return { resize, size: () => ({ w, h }), paint, ctx: g };
}
