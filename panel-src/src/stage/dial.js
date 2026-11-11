// The 24-hour dial (spec §3.3, §6.1): the one clock of the page. Midnight is at the top, so the bot's window (18:00 to
// 09:00) sweeps over the top and the hours Gonza is busy (09:00 to 18:00) are hatched at the bottom. The hand is always
// the real hour. Six marks on the ring stand for the six stops; the active one glows and, while you travel, the glow
// follows the short way between marks. The geometry is pure (and tested in Node); createDial only draws it once with
// sv() and then moves a few attributes. The look is in styles/dial.css.

import { sv } from "../core/dom.js";
import { MARKS, STOP_IDS } from "../scene/poses.js";
import { inWindow, nightT } from "../scene/clock.js";

export const CENTER = 300, R_RING = 262;
export const VIEW = { min: -60, size: 720 };                 // viewBox "-60 -60 720 720": room for the numerals outside the ring
const WS = 18, WE = 9;                                       // the bot's window, in hours of the day
const MARK_R = R_RING - 6;
const f = (n) => n.toFixed(2);

// The dial is drawn in SVG units and scaled, so a text of 13 units is 13 x k pixels on the screen, k = box / 720. Below 11 px a mono
// text is a smudge (spec §4.2), so the dial gives up detail instead of shrinking it: the micro labels (13 units) go first, then
// the numerals that are not the four cardinal hours, then the cardinal ones grow to the next sizes of the type scale (--fs-4, --fs-5).
export const LOD_MIN_PX = 11;
/** "full" every text | "mid" no micro labels | "compact" 00 06 12 18 at 22 units | "tiny" the same at 34 | "bare" no labels but the time. */
export function detailOf(k) {
  if (k * 13 >= LOD_MIN_PX) return "full";
  if (k * 15 >= LOD_MIN_PX) return "mid";
  if (k * 22 >= LOD_MIN_PX) return "compact";
  if (k * 34 >= LOD_MIN_PX) return "tiny";
  return "bare";
}

export const angleOf = (hora) => ((((hora % 24) + 24) % 24) / 24) * 360;
export function polar(r, deg) {
  const a = (deg * Math.PI) / 180;
  return [CENTER + r * Math.sin(a), CENTER - r * Math.cos(a)];
}
export function arcPath(r, d0, d1) {
  const large = d1 - d0 > 180 ? 1 : 0;
  const [x0, y0] = polar(r, d0), [x1, y1] = polar(r, d1);
  return `M${f(x0)} ${f(y0)} A${r} ${r} 0 ${large} 1 ${f(x1)} ${f(y1)}`;
}
/** Annular sector between two radii, clockwise from d0 to d1. */
export function sectorPath(r0, r1, d0, d1) {
  const large = d1 - d0 > 180 ? 1 : 0;
  const [x0, y0] = polar(r1, d0), [x1, y1] = polar(r1, d1), [x2, y2] = polar(r0, d1), [x3, y3] = polar(r0, d0);
  return `M${f(x0)} ${f(y0)} A${r1} ${r1} 0 ${large} 1 ${f(x1)} ${f(y1)} L${f(x2)} ${f(y2)} A${r0} ${r0} 0 ${large} 0 ${f(x3)} ${f(y3)}Z`;
}

/** Everything fixed about the dial, as data: ticks, numerals, the hourly runs of the window, the six marks, the busy band. */
export function dialSpec() {
  const ticks = [], numerals = [], runs = [], marks = [];
  for (let hh = 0; hh < 24; hh++) {
    const win = hh >= WS || hh < WE, major = hh % 3 === 0, deg = angleOf(hh);
    const [x1, y1] = polar(R_RING, deg), [x2, y2] = polar(R_RING + (major ? 20 : 12), deg);
    ticks.push({ hh, win, major, quarter: false, x1, y1, x2, y2 });
    if (win) {
      for (let q = 1; q < 4; q++) {
        const [a, b] = polar(R_RING, deg + q * 3.75), [c, d] = polar(R_RING + 6, deg + q * 3.75);
        ticks.push({ hh, win: true, major: false, quarter: true, x1: a, y1: b, x2: c, y2: d });
      }
    }
    if (major) { const [x, y] = polar(R_RING + 42, deg); numerals.push({ hh, x, y: y + 5, text: String(hh).padStart(2, "0") }); }
  }
  for (let k = 0; k <= 15; k++) { const [x, y] = polar(R_RING - 22, angleOf((WS + k) % 24)); runs.push({ k, x, y, big: k % 6 === 0 }); }
  for (const id of STOP_IDS) { const angle = angleOf(MARKS[id]); const [x, y] = polar(MARK_R, angle); marks.push({ id, hora: MARKS[id], angle, x, y }); }
  const b0 = angleOf(WE), b1 = angleOf(WS);
  const [lx0, ly0] = polar(R_RING - 22, b1), [lx1, ly1] = polar(R_RING - 22, b0);
  return {
    ticks, numerals, runs, marks,
    busy: sectorPath(R_RING - 52, R_RING, b0, b1),
    busyHair: arcPath(R_RING - 52, b0, b1),
    busyLabel: `M${f(lx0)} ${f(ly0)} A${R_RING - 22} ${R_RING - 22} 0 0 0 ${f(lx1)} ${f(ly1)}`,
    rail: arcPath(R_RING, angleOf(WS), angleOf(WS) + 225),
  };
}

/** What moves with the hour: the lit part of the rail, the sweep behind the hand, how many runs are done and which is 'now'. */
export function railAt(hora) {
  const inside = inWindow(hora);
  const n = inside ? nightT(hora) : 15;                      // the night is over: the rail stays full until 18:00 starts another
  const start = angleOf(WS), end = start + n * 15;
  const passed = Math.floor(n + 1e-6);
  return {
    nightT: n,
    lit: n > 0.01 ? arcPath(R_RING, start, end) : "",
    sweep: inside && n > 0.01 ? sectorPath(30, R_RING - 54, Math.max(start, end - 70), end) : "",
    passed,
    now: inside ? passed : -1,
    hand: angleOf(hora),
  };
}

/** Where the tip of the hand is, in window pixels, for a box whose top-left corner is (x, y) and whose side is `size`. */
export function tipOf(x, y, size, hora) {
  const [tx, ty] = polar(R_RING - 6, angleOf(hora));
  return { x: x + ((tx - VIEW.min) / VIEW.size) * size, y: y + ((ty - VIEW.min) / VIEW.size) * size };
}

/** A pose (poses.js) at an hour, for a box that is `base` px wide in CSS: the transform, the ring's diameter and the tip. */
export function layoutOf(pose, hora, base) {
  const { size, cx, cy, docked } = pose;
  const x = cx - size / 2, y = cy - size / 2;
  return { x, y, cx, cy, size, scale: size / base, ring: (2 * R_RING * size) / VIEW.size, tip: tipOf(x, y, size, hora), docked, detail: detailOf(size / VIEW.size) };
}

export function createDial(host, { t }) {
  const spec = dialSpec();
  const ring = (cls, r) => sv("circle", { class: cls, cx: CENTER, cy: CENTER, r });
  const line = (cls, k) => sv("line", { class: cls, x1: f(k.x1), y1: f(k.y1), x2: f(k.x2), y2: f(k.y2) });

  const busyText = sv("textPath", { href: "#dial-busy-path", startOffset: "50%" }, t("dial.window"));
  // The time sits 146 units under the hub (and the zone label 180): the sun is born at the hub (stage/beam.js), and its disc and
  // halo would otherwise touch the top of the digits and wash them out.
  const zone = sv("text", { class: "dial-micro", x: CENTER, y: CENTER + 180, "text-anchor": "middle" }, t("dial.local"));
  const time = sv("text", { class: "dial-time", x: CENTER, y: CENTER + 146, "text-anchor": "middle" }, "");
  const lit = sv("path", { class: "rail-lit", d: "" });
  const sweep = sv("path", { class: "sweep", d: "" });
  const runs = spec.runs.map((r) => sv("circle", { class: r.big ? "run run-big" : "run", cx: f(r.x), cy: f(r.y), r: r.big ? 6.5 : 3.2 }));
  const hand = sv("g", { class: "hand", transform: `rotate(${angleOf(WS)} ${CENTER} ${CENTER})` },
    sv("line", { class: "hand-line", x1: CENTER, y1: CENTER + 18, x2: CENTER, y2: CENTER - (R_RING - 6) }),
    sv("circle", { class: "hand-tip", cx: CENTER, cy: CENTER - (R_RING - 6), r: 5.5 }));
  const glowEl = sv("g", { class: "mark-glow", transform: `rotate(0 ${CENTER} ${CENTER})`, opacity: 0 },
    sv("circle", { cx: CENTER, cy: CENTER - MARK_R, r: 11 }));

  const svg = sv("svg", { class: "dial-svg", viewBox: `${VIEW.min} ${VIEW.min} ${VIEW.size} ${VIEW.size}`, role: "img", "aria-label": t("dial.aria") },
    sv("defs", null,
      sv("pattern", { id: "dial-hatch", width: 7, height: 7, patternUnits: "userSpaceOnUse", patternTransform: "rotate(45)" }, sv("line", { class: "hatch-line", x1: 0, y1: 0, x2: 0, y2: 7 })),
      sv("radialGradient", { id: "dial-face", cx: "50%", cy: "42%", r: "62%" },
        sv("stop", { class: "face-stop face-stop-0", offset: "0%" }), sv("stop", { class: "face-stop face-stop-1", offset: "70%" }), sv("stop", { class: "face-stop face-stop-2", offset: "100%" })),
      sv("path", { id: "dial-busy-path", d: spec.busyLabel, fill: "none" })),
    sv("circle", { class: "face", cx: CENTER, cy: CENTER, r: R_RING, fill: "url(#dial-face)" }),
    ...[196, 132, 70].map((r) => ring("ring-inner", r)),
    sv("line", { class: "cross", x1: CENTER - R_RING, y1: CENTER, x2: CENTER + R_RING, y2: CENTER }),
    sv("line", { class: "cross", x1: CENTER, y1: CENTER - R_RING, x2: CENTER, y2: CENTER + R_RING }),
    sv("path", { class: "busy", d: spec.busy, fill: "url(#dial-hatch)" }),
    sv("path", { class: "hair", d: spec.busyHair }),
    sv("text", { class: "dial-micro busy-label", "text-anchor": "middle" }, busyText),
    sv("path", { class: "rail", d: spec.rail }), lit, sweep,
    ...spec.ticks.map((k) => line(["tick", k.win && "tick-win", k.quarter && "tick-q"].filter(Boolean).join(" "), k)),
    ...spec.numerals.map((n) => sv("text", { class: n.hh % 6 === 0 ? "dial-num dial-cardinal" : "dial-num", "data-hh": n.hh, x: f(n.x), y: f(n.y), "text-anchor": "middle" }, n.text)),
    ...runs,
    ...spec.marks.map((m) => sv("circle", { class: "mark", "data-stop": m.id, cx: f(m.x), cy: f(m.y), r: 3.5 })),
    glowEl, hand, sv("circle", { class: "hub", cx: CENTER, cy: CENTER, r: 7 }), time, zone);
  host.append(svg);

  let baseSize = 600, lastRail = "", lastText = null, lastTransform = "", lastDocked = null, lastDetail = null, lastGlow = "";

  return {
    host, svg,
    /** The side of the CSS box in px: the biggest pose. layout() only ever scales it down. */
    base(px) { baseSize = px; host.style.setProperty("--dial", px.toFixed(1) + "px"); },
    /** hora: 0..24, the hand; text: what the middle reads ("00:26"). */
    set(hora, text) {
      const r = railAt(hora);
      const key = `${r.hand.toFixed(3)}|${r.passed}|${r.now}`;
      if (key !== lastRail) {
        lastRail = key;
        hand.setAttribute("transform", `rotate(${f(r.hand)} ${CENTER} ${CENTER})`);
        lit.setAttribute("d", r.lit);
        sweep.setAttribute("d", r.sweep);
        runs.forEach((dot, k) => { dot.classList.toggle("run-done", k <= r.passed); dot.classList.toggle("run-now", k === r.now); });
      }
      if (text !== lastText) { lastText = text; time.textContent = text; }
    },
    /** pose: from poses.js. Moves and scales the box; returns the geometry (the hub, cx and cy, is where the sun is born, stage/beam.js). */
    layout(pose, hora) {
      const g = layoutOf(pose, hora, baseSize);
      const transform = `translate3d(${g.x.toFixed(1)}px, ${g.y.toFixed(1)}px, 0) scale(${g.scale.toFixed(4)})`;
      if (transform !== lastTransform) { lastTransform = transform; host.style.transform = transform; }
      if (g.docked !== lastDocked) { lastDocked = g.docked; host.classList.toggle("is-docked", g.docked); }
      if (g.detail !== lastDetail) { lastDetail = g.detail; host.dataset.detail = g.detail; host.classList.toggle("is-compact", g.detail !== "full"); }
      return g;
    },
    /** { angle, strength } from markGlow(pos): where the lit mark is and how much. */
    glow({ angle, strength }) {
      const key = `${angle.toFixed(2)}|${strength.toFixed(2)}`;
      if (key === lastGlow) return;
      lastGlow = key;
      glowEl.setAttribute("transform", `rotate(${f(angle)} ${CENTER} ${CENTER})`);
      glowEl.setAttribute("opacity", strength.toFixed(2));
    },
    /** After a language change. */
    relabel() {
      svg.setAttribute("aria-label", t("dial.aria"));
      busyText.textContent = t("dial.window");
      zone.textContent = t("dial.local");
    },
  };
}
