// The city behind the sheet (spec §4.3): three layers of seeded buildings, far to near, whose windows light up with `lit`.
// Pure drawing on a 2D context; the sky decides the colours and the hour, the stage decides the shift (parallax).
// The geometry is the one of the story's sky.js and board/relay/relay.js (makeLayer), at its own seed so the city does not
// repeat the stars' random sequence.

const clamp = (x, a = 0, b = 1) => Math.min(b, Math.max(a, x));
const mix = (a, b, t) => a.map((v, i) => v + (b[i] - v) * t);
const css = (c) => `rgb(${c[0]},${c[1]},${c[2]})`;

export function mulberry32(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Buildings of one layer, left to right from x = -40 to past 4200: { x, w, h, wins: [{ x, y, thr }] }. */
export function makeLayer(rand, minH, maxH, minW, maxW, winSize, gap) {
  const buildings = [];
  let x = -40;
  while (x < 4200) {
    const w = minW + rand() * (maxW - minW);
    const h = minH + rand() * rand() * (maxH - minH) + (maxH - minH) * 0.1 * rand();
    const wins = [];
    for (let wx = 6; wx < w - winSize - 3; wx += winSize + gap) {
      for (let wy = 8; wy < h - winSize - 4; wy += winSize + gap + 2) {
        if (rand() < 0.82) wins.push({ x: wx, y: wy, thr: rand() });
      }
    }
    buildings.push({ x, w, h, wins });
    x += w + (rand() < 0.3 ? 4 + rand() * 18 : 0);
  }
  return buildings;
}

// How each layer is built and drawn: params are makeLayer's (minH, maxH, minW, maxW, winSize, gap); `scale` multiplies the
// window-height unit; `offset` is the layer's base pan; `bodyK` mixes the sky's top and horizon for its colour; `win` is the
// window colour, `winAlpha` how strong, `size` the window side, `litK` how much of `lit` this layer gets; `ghost` the blur copies.
export const LAYER_SPEC = [
  { name: "far", params: [40, 170, 30, 70, 2.2, 3], scale: 0.9, offset: 40, bodyK: 0.55, win: "255,214,150", winAlpha: 0.35, size: 1.8, litK: 0.9, ghost: 0 },
  { name: "mid", params: [50, 230, 40, 90, 3, 4], scale: 1, offset: 140, bodyK: 0.25, win: "255,205,130", winAlpha: 0.6, size: 2.6, litK: 1, ghost: 3 },
  { name: "near", params: [30, 130, 60, 130, 4, 6], scale: 1.05, offset: 300, bodyK: 0, win: "255,196,110", winAlpha: 0.85, size: 3.4, litK: 0.8, ghost: 6 },
];

const offscreen = typeof OffscreenCanvas === "function" ? (w, h) => new OffscreenCanvas(w, h) : null;      // null where there is none (Node): then every frame paints the whole city

// A layer is the same picture on every frame of a trip: only its pan moves (parallax). So a layer is painted once on a strip of its own, wider than the
// window by a third of a window on each side, and a frame copies the slice that is on screen, snapped to a device pixel. The strip is painted again when
// what the picture depends on changes (the window's height, the pixel ratio, the body colour, the lit fraction at 1/400) or the pan leaves it. Measured on
// the continuous scroll at CPU x4 (heavy, 1920x1080) the city was a third of the page's time: about 6 ms of the 11 ms of a frame of the stage.
const QUANT = 400;                 // the lit fraction is rounded to 1/QUANT: a window's alpha ramps over a tenth of it, so a step is invisible
const MARGIN = 0.3;                // the strip reaches this much of the window's width beyond the screen on each side (and never less than 400 px): a stop pans the near layer 120 px

export function createSkyline({ seed = 20261007 ^ 0x5eed, makeCanvas = offscreen } = {}) {
  const rand = mulberry32(seed);
  const layers = LAYER_SPEC.map((spec) => makeLayer(rand, ...spec.params));
  const unitOf = (h) => clamp(h / 900, 0.7, 1.6);

  /** Per layer: the buildings, the scale (window height unit times the layer's), the pan in px and the ground line. */
  function geometry(w, h, shift = [0, 0, 0]) {
    const unit = unitOf(h);
    return {
      unit,
      baseY: h + 2,
      layers: LAYER_SPEC.map((spec, i) => ({ spec, buildings: layers[i], scale: unit * spec.scale, offset: spec.offset + (shift[i] || 0) })),
    };
  }

  // `from` and `to` pick a range of layers (0 far, 1 mid, 2 near): the stage paints the far one on the sky and the others on a canvas
  // of their own in front of the dial, so the dial of HOY stands half sunk in the city (spec §2.2).
  /**
   * The buildings of one layer on `g`, whose left edge is `origin` in the layer's own x and whose ground line is at `baseY`; `span` is the width of `g`
   * (buildings off it are skipped), `level` the lit fraction for this layer, `windows` false for a silhouette (the ghost copies are made of one).
   */
  function paintLayer(g, L, { origin, baseY, span, level, body, ghostBody, ghost, windows = true }) {
    const { spec, scale } = L;
    const winPx = spec.size * scale;
    for (const b of L.buildings) {
      const bx = b.x * scale - origin;
      if (bx > span + 10 || bx + b.w * scale < -10) continue;
      const bh = b.h * scale;
      if (ghost && spec.ghost) {
        g.fillStyle = ghostBody;
        g.fillRect(bx - spec.ghost, baseY - bh, b.w * scale + 0.5, bh + 2);
        g.fillRect(bx + spec.ghost, baseY - bh, b.w * scale + 0.5, bh + 2);
      }
      g.fillStyle = body;
      g.fillRect(bx, baseY - bh, b.w * scale + 0.5, bh + 2);
      if (!windows || level <= 0) continue;
      for (const wn of b.wins) {
        const a = clamp((level - wn.thr) / 0.1) * spec.winAlpha;
        if (a < 0.03) continue;
        g.fillStyle = `rgba(${spec.win},${a.toFixed(3)})`;
        g.fillRect(bx + wn.x * scale, baseY - bh + wn.y * scale, winPx, winPx * 1.15);
      }
    }
  }

  const strips = [];                                       // per layer: { key, x0, cw, ch, picture, silhouette }

  /** The strip of a layer for this frame: the one kept when it still holds the picture and the pan, else painted again. */
  function stripFor(i, L, { w, h, ratio, level, body }) {
    const key = `${h}|${ratio}|${L.scale}|${body}|${level}`;
    let s = strips[i];
    const pad = L.spec.ghost;                                // the ghost copies are cut from the same strip, a few px to each side
    if (!s || s.key !== key || L.offset - pad < s.x0 || L.offset + w + pad > s.x0 + s.cw) {
      const margin = Math.max(400, Math.round(w * MARGIN));
      const x0 = (Math.floor(L.offset * ratio) - Math.round(margin * ratio)) / ratio;    // a whole number of device pixels, so the buildings fall where they would on the window
      const cw = Math.ceil(w) + 2 * margin + 2;
      let tallest = 0;
      for (const b of L.buildings) tallest = Math.max(tallest, b.h * L.scale);
      const chd = Math.ceil((tallest + 6) * ratio), ch = chd / ratio;   // device pixels, so the strip lands on the pixel grid; the ground line (h + 2) is at ch + 2 on it
      const picture = makeCanvas(Math.ceil(cw * ratio), chd);
      const g = picture.getContext("2d");
      g.setTransform(ratio, 0, 0, ratio, 0, 0);
      paintLayer(g, L, { origin: x0, baseY: ch + 2, span: cw, level, body });
      s = strips[i] = { key, x0, cw, ch, chd, picture, silhouette: null, body };
    }
    return s;
  }

  function silhouetteOf(s, L, ratio) {
    if (!s.silhouette) {
      s.silhouette = makeCanvas(s.picture.width, s.picture.height);
      const g = s.silhouette.getContext("2d");
      g.setTransform(ratio, 0, 0, ratio, 0, 0);
      paintLayer(g, L, { origin: s.x0, baseY: s.ch + 2, span: s.cw, level: 0, body: s.body, windows: false });
    }
    return s.silhouette;
  }

  /**
   * `ratio` (device pixels per CSS pixel of `g`) turns the strips on; without it, or where no offscreen canvas exists, every building and window is
   * painted on `g` on every call, which is the same picture to the pixel when the pan is a whole number of device pixels.
   */
  function draw(g, { w, h, shift = [0, 0, 0], lit = 0, top, hor, ghost = false, from = 0, to = LAYER_SPEC.length, ratio = 0 }) {
    const geo = geometry(w, h, shift);
    const cached = ratio > 0 && !!makeCanvas;
    for (let i = from; i < Math.min(to, geo.layers.length); i++) {
      const L = geo.layers[i];
      const { spec, offset } = L;
      const body = css(mix(top, hor, spec.bodyK).map((v) => Math.round(v * 0.5)));
      if (!cached) {
        const ghostBody = body.replace("rgb(", "rgba(").replace(")", ",0.45)");
        paintLayer(g, L, { origin: offset, baseY: geo.baseY, span: w, level: lit * spec.litK, body, ghostBody, ghost });
        continue;
      }
      const s = stripFor(i, L, { w, h, ratio, level: Math.round(lit * spec.litK * QUANT) / QUANT, body });
      const sx = Math.round((offset - s.x0) * ratio);          // the slice that is on screen, in whole device pixels
      const sw = Math.min(Math.ceil(w * ratio), s.picture.width - sx), dw = sw / ratio, dy = (Math.round(h * ratio) - s.chd) / ratio;
      if (ghost && spec.ghost) {
        const sil = silhouetteOf(s, L, ratio), gp = Math.round(spec.ghost * ratio);
        g.globalAlpha = 0.45;
        g.drawImage(sil, sx + gp, 0, sw, s.picture.height, 0, dy, dw, s.ch);
        g.drawImage(sil, sx - gp, 0, sw, s.picture.height, 0, dy, dw, s.ch);
        g.globalAlpha = 1;
      }
      g.drawImage(s.picture, sx, 0, sw, s.picture.height, 0, dy, dw, s.ch);
    }
  }

  /**
   * The lit windows of one layer (the near one by default) whose centre falls between `minX` and `maxX`, left to right, as
   * { x, y, a, layer } in canvas pixels. The beam picks its source among them (stage/beam.js).
   */
  // The lit windows of a layer do not depend on the pan, only their x does (by the pan itself): the whole list is made once per (layer, height, lit
  // fraction at 1/QUANT, minimum alpha), sorted by x in the layer's own coordinates, and a call is a search in it. The beam asks on every frame.
  const windowLists = new Map();
  function litList(geo, layer, level, minA) {
    const key = `${layer}|${geo.baseY}|${geo.layers[layer].scale}|${level}|${minA}`;
    let list = windowLists.get(key);
    if (!list) {
      const L = geo.layers[layer];
      const winPx = L.spec.size * L.scale;
      list = [];
      for (const b of L.buildings) {
        const bh = b.h * L.scale;
        for (const wn of b.wins) {
          const a = clamp((level - wn.thr) / 0.1) * L.spec.winAlpha;
          if (a < minA) continue;
          list.push({ x: b.x * L.scale + wn.x * L.scale + winPx / 2, y: geo.baseY - bh + wn.y * L.scale + (winPx * 1.15) / 2, a });
        }
      }
      list.sort((p, q) => p.x - q.x);
      if (windowLists.size >= 6) windowLists.delete(windowLists.keys().next().value);
      windowLists.set(key, list);
    }
    return list;
  }

  function litWindows(w, h, shift, lit, { minX = 0, maxX = w, layer = 2, minA = 0.5 } = {}) {
    const geo = geometry(w, h, shift);
    const L = geo.layers[layer];
    const list = litList(geo, layer, Math.round(lit * L.spec.litK * QUANT) / QUANT, minA);
    let lo = 0, hi = list.length;                          // the first window at or right of minX
    while (lo < hi) { const mid = (lo + hi) >> 1; if (list[mid].x - L.offset < minX) lo = mid + 1; else hi = mid; }
    const out = [];
    for (let i = lo; i < list.length; i++) {
      const x = list[i].x - L.offset;
      if (x > maxX) break;
      out.push({ x, y: list[i].y, a: list[i].a, layer });
    }
    return out;
  }

  return { layers, geometry, draw, litWindows };
}
