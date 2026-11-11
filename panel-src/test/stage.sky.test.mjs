import test from "node:test";
import assert from "node:assert/strict";
import { tokensOf, contrast, over, rgb } from "./helpers/color.mjs";
import { STOPS, LIT, skyAt, starsGate, track, createSky } from "../src/stage/sky.js";
import { recorder } from "./helpers/canvas.mjs";

const close = (a, b, eps = 1) => a.every((v, i) => Math.abs(v - b[i]) <= eps);

test("ten stops of colour plus the closing copy of 18:00 at nightT 24", () => {
  assert.equal(STOPS.length, 11);
  assert.deepEqual(STOPS.map((s) => s[0]), [0, 1.2, 2.6, 10, 12.6, 14, 15, 16, 19, 23, 24]);
  assert.deepEqual(STOPS[10].slice(1), STOPS[0].slice(1), "the cycle closes on the 18:00 stop");
  assert.deepEqual(STOPS[7].slice(1, 4), ["#1a2a52", "#53699f", "#ffd7a8"]);   // 10:00
  assert.deepEqual(STOPS[8].slice(1, 4), ["#1f3363", "#5675b0", "#ffe8c4"]);   // 13:00
  assert.deepEqual(STOPS[9].slice(1, 4), ["#1c2750", "#58628f", "#ffbf85"]);   // 17:00
});

test("skyAt gives the stop's colours on the stop, blends between, and has no jump at 18:00", () => {
  assert.deepEqual(skyAt(18).top, [11, 18, 38]);
  assert.deepEqual(skyAt(9).top, [20, 28, 60]);
  assert.deepEqual(skyAt(9).hor, [88, 83, 127]);
  assert.ok(close(skyAt(17.999).top, skyAt(18).top, 1) && close(skyAt(17.999).hor, skyAt(18).hor, 1));
  const mid = skyAt(13.5);                                    // between 13:00 and 17:00
  assert.ok(mid.top[2] < 102 && mid.top[2] > 88);
  for (const h of [0, 3, 6, 9, 12, 15, 18, 21, 23.999]) {
    const s = skyAt(h);
    for (const c of [s.top, s.mid, s.hor, s.warm]) assert.ok(c.length === 3 && c.every((v) => Number.isInteger(v) && v >= 0 && v <= 255), `${h}: ${c}`);
    assert.ok(s.warmA >= 0 && s.warmA <= 1 && s.stars >= 0 && s.stars <= 1 && s.lit >= 0 && s.lit <= 1);
  }
});

test("stars only between 19:30 and 06:30, ramping inside, whatever the stop table says", () => {
  assert.deepEqual([12, 17, 19, 19.5, 6.5, 7, 9].map(starsGate), [0, 0, 0, 0, 0, 0, 0]);
  assert.deepEqual([21, 23, 0, 3, 5].map(starsGate), [1, 1, 1, 1, 1]);
  assert.ok(starsGate(20) > 0 && starsGate(20) < 1 && starsGate(5.75) > 0 && starsGate(5.75) < 1);
  assert.equal(starsGate(23.99), 1, "continuous across midnight");
  assert.equal(starsGate(0), 1);
  for (const h of [9, 12, 17, 18]) assert.equal(skyAt(h).stars, 0, `no stars at ${h}`);
  assert.ok(skyAt(23).stars > 0.9 && skyAt(0.43).stars > 0.9);
});

test("windows: lit at 18:00, off in the daytime, back on towards evening", () => {
  assert.equal(skyAt(18).lit, 0.55);
  assert.equal(skyAt(13).lit, 0);
  assert.ok(skyAt(19.5).lit > 0.5, "the evening is bright with windows");
  assert.ok(skyAt(0.43).lit > 0.1 && skyAt(0.43).lit < 0.25, "at 00:26 most of the city sleeps, a few windows stay lit");
  assert.ok(skyAt(17.5).lit > skyAt(17).lit);
  assert.equal(track(LIT, -5), LIT[0][1]);
  assert.equal(track(LIT, 99), LIT.at(-1)[1]);
  assert.equal(LIT.at(-1)[0], 24);
});

test("A18: --text and --text-dim keep 4.5:1 over glass, glow and horizon at every one of the ten stops (the worst is 09:00)", () => {
  const t = tokensOf(":root {\n  color-scheme: dark;");
  const glass = [8, 12, 22];
  let worst = Infinity;
  for (const [, , hor, warm, warmA] of STOPS.slice(0, 10)) {
    const bg = over(glass, over(rgb(warm), rgb(hor), warmA), 0.72);
    for (const token of ["--text", "--text-dim"]) {
      const ratio = contrast(t[token], bg);
      assert.ok(ratio >= 4.5, `${token} over ${hor}+${warm}@${warmA} is ${ratio.toFixed(2)}:1`);
      if (token === "--text-dim") worst = Math.min(worst, ratio);
    }
  }
  assert.ok(worst >= 5.6 && worst < 5.8, `the tightest stop is ${worst.toFixed(2)}:1 (09:00)`);
});

test("A18 also holds between the stops: every minute of the 24 hours keeps --text-dim at 4.5:1 over glass and the blended sky", () => {
  const t = tokensOf(":root {\n  color-scheme: dark;");
  for (let m = 0; m < 24 * 60; m++) {
    const s = skyAt(m / 60);
    const bg = over([8, 12, 22], over(s.warm, s.hor, s.warmA), 0.72);
    const ratio = contrast(t["--text-dim"], bg);
    assert.ok(ratio >= 4.5, `--text-dim at minute ${m} is ${ratio.toFixed(2)}:1`);
  }
});

function fakeCanvas(w = 1920, h = 1080) {
  const rec = recorder();
  return { canvas: { clientWidth: w, clientHeight: h, width: 0, height: 0, getContext: () => rec.ctx }, rec };
}
function build(over = {}) {
  const { canvas, rec } = fakeCanvas(over.w, over.h);
  const seen = { skyline: [], beams: [] };
  let n = 12345;
  const rand = () => { n = (n * 16807) % 2147483647; return n / 2147483647; };
  const sky = createSky(canvas, {
    skyline: { draw: (g, args) => seen.skyline.push(args) },
    drawBeam: (g, beam, clipLeft) => seen.beams.push([beam, clipLeft]),
    rand, dpr: () => 1.25,
  });
  return { sky, canvas, rec, seen };
}

test("resize sizes the backing store with a pixel ratio of at most 1.5, and size() reports css pixels", () => {
  const { sky, canvas } = build();
  sky.resize();
  assert.deepEqual(sky.size(), { w: 1920, h: 1080 });
  assert.equal(canvas.width, 2400);
  assert.equal(canvas.height, 1350);
});

test("paint: gradient, stars at night and none by day, the beams with their clip, then the skyline with the sky's own colours", () => {
  const { sky, rec, seen } = build();
  sky.resize();
  sky.paint({ hora: 12, beams: [], clipLeft: 0 });
  assert.equal(rec.count("arc"), 0, "no star at noon");
  rec.calls.length = 0;
  const beam = { x: 1, y: 2 };
  sky.paint({ hora: 23, shift: [1, 2, 3], beams: [beam], clipLeft: 1200, ghost: true, warmX: 0.8 });
  const arcs = rec.count("arc");
  assert.ok(arcs > 150 && arcs <= 280, `${arcs} stars`);
  assert.deepEqual(seen.beams, [[beam, 1200]]);
  const s = seen.skyline.at(-1);
  assert.deepEqual([s.w, s.h, s.shift, s.ghost], [1920, 1080, [1, 2, 3], true]);
  assert.deepEqual(s.top, skyAt(23).top);
  assert.equal(s.lit, skyAt(23).lit);
  assert.ok(rec.count("createLinearGradient") === 1 && rec.count("createRadialGradient") === 1);
  const radial = rec.calls.find((c) => c[0] === "createRadialGradient");
  assert.equal(radial[1], 1920 * 0.8, "the warm band follows warmX");
});

test("B18: a window dragged to a monitor of another pixel ratio, with no resize, gets its backing store at the next paint", () => {
  const { canvas, rec } = fakeCanvas();
  let ratio = 1;
  const sky = createSky(canvas, { skyline: { draw() {} }, drawBeam() {}, rand: () => 0.5, dpr: () => ratio });
  sky.resize();
  sky.paint({ hora: 12 });
  assert.equal(canvas.width, 1920);
  ratio = 1.5;
  sky.paint({ hora: 12 });
  assert.deepEqual([canvas.width, canvas.height], [2880, 1620], "same css size, new pixel ratio");
  rec.calls.length = 0;
  sky.paint({ hora: 12 });
  assert.deepEqual([canvas.width, canvas.height], [2880, 1620]);
  assert.equal(rec.calls.find((c) => c[0] === "setTransform")[1], 1.5, "and it paints at that ratio");
});

test("paint twice draws the same stars (they are seeded once, not per frame)", () => {
  const { sky, rec } = build();
  sky.resize();
  sky.paint({ hora: 23 });
  const first = rec.calls.filter((c) => c[0] === "arc").map((c) => c.slice(1, 4).join());
  rec.calls.length = 0;
  sky.paint({ hora: 23 });
  assert.deepEqual(rec.calls.filter((c) => c[0] === "arc").map((c) => c.slice(1, 4).join()), first);
});

test("with a front canvas the sky keeps the far layers and the front canvas takes the rest, cleared at every frame, at the same size and pixel ratio", () => {
  const { canvas } = fakeCanvas();
  const frontRec = recorder();
  const front = { width: 0, height: 0, getContext: () => frontRec.ctx };
  const draws = [];
  const sky = createSky(canvas, {
    skyline: { draw: (g, args) => draws.push([g === frontRec.ctx ? "front" : "sky", args.from, args.to]) },
    drawBeam() {}, rand: () => 0.5, dpr: () => 1.25, front, frontFrom: 1,
  });
  sky.resize();
  assert.deepEqual([front.width, front.height], [canvas.width, canvas.height]);
  sky.paint({ hora: 12 });
  assert.deepEqual(draws, [["sky", undefined, 1], ["front", 1, undefined]]);
  assert.equal(frontRec.count("clearRect"), 1);
  assert.deepEqual(frontRec.calls.find((c) => c[0] === "setTransform").slice(1), [1.25, 0, 0, 1.25, 0, 0]);
  sky.paint({ hora: 12 });
  assert.equal(frontRec.count("clearRect"), 2, "the front canvas is wiped before each frame");
  const lone = build();
  lone.sky.resize();
  lone.sky.paint({ hora: 12 });
  assert.deepEqual([lone.seen.skyline.length, lone.seen.skyline[0].from, lone.seen.skyline[0].to], [1, undefined, undefined], "without a front canvas the whole city is painted at once, as before");
});

test("the holes of a frame are handed to every light", () => {
  const { canvas } = fakeCanvas();
  const seen = [];
  const sky = createSky(canvas, { skyline: { draw() {} }, drawBeam: (g, beam, clipLeft, holes) => seen.push([clipLeft, holes]), rand: () => 0.5, dpr: () => 1 });
  sky.resize();
  const holes = [{ left: 1300, top: 78, right: 1700, bottom: 1040 }];
  sky.paint({ hora: 9, beams: [{}, {}], clipLeft: 1177, holes });
  sky.paint({ hora: 9, beams: [{}], clipLeft: 1177 });
  assert.deepEqual(seen, [[1177, holes], [1177, holes], [1177, []]]);
});

test("with somewhere to paint a picture the stars are painted once on a transparent canvas and a frame only copies it; a new alpha, size or ratio paints them again", () => {
  const { canvas, rec } = fakeCanvas();
  const made = [];
  const makeCanvas = (width, height) => { const r = recorder(); const c = { width, height, rec: r, getContext: () => r.ctx }; made.push(c); return c; };
  const sky = createSky(canvas, { skyline: { draw() {} }, drawBeam() {}, rand: () => 0.5, dpr: () => 1, makeCanvas });
  sky.resize();
  sky.paint({ hora: 22 });
  assert.equal(made.length, 1, "one picture");
  assert.deepEqual([made[0].width, made[0].height], [canvas.width, canvas.height], "with the pixels of the canvas");
  assert.equal(made[0].rec.count("arc"), 280, "all the stars are on it");
  assert.equal(rec.count("arc"), 0, "and none is painted on the sky itself");
  assert.equal(rec.calls.filter((c) => c[0] === "drawImage" && c[1] === made[0]).length, 1);
  const painted = made[0].rec.calls.length;
  sky.paint({ hora: 22, shift: [1, 2, 3], warmX: 0.2, beams: [{}] });
  sky.paint({ hora: 22.0001 });
  assert.equal(made.length, 1, "a frame of a trip or a hair of the hour does not paint them again");
  assert.equal(made[0].rec.calls.length, painted);
  assert.equal(rec.calls.filter((c) => c[0] === "drawImage").length, 3, "each frame copies the picture");
  sky.paint({ hora: 21 });
  assert.equal(made.length, 2, "another alpha of the stars is another picture (21:00 is dimmer)");
  sky.paint({ hora: 12 });
  assert.equal(made.length, 2, "by day there are no stars and no picture");
  assert.equal(rec.count("arc"), 0);
});

test("without somewhere to paint a picture the stars are painted on the sky on every frame", () => {
  const { sky, rec } = build();
  sky.resize();
  sky.paint({ hora: 22 });
  assert.equal(rec.count("arc"), 280);
  sky.paint({ hora: 22 });
  assert.equal(rec.count("arc"), 560);
});
