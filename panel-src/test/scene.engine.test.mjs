import test from "node:test";
import assert from "node:assert/strict";
import { createEngine, DT_CAP, SLEEP_AFTER, TRIP_DT_CAP } from "../src/scene/engine.js";

const STOPS = ["prologo", "hoy", "procesos", "cola", "aprender", "sistema"];

function fakeLenis(limit = 5000) {
  const l = {
    animatedScroll: 0, limit, isScrolling: false, writes: [], rafs: [], stopped: false,
    raf(t) { l.rafs.push(t); },
    scrollTo(y, opts = {}) { l.writes.push({ y, opts }); l.animatedScroll = y; },
    stop() { l.stopped = true; }, start() { l.stopped = false; }, resize() {},
  };
  return l;
}
function fakeLoop() {
  let t = 1000, queued = null, ids = 0;
  return {
    now: () => t,
    raf: (f) => { queued = f; return ++ids; },
    caf: () => { queued = null; },
    pending: () => queued !== null,
    tick(ms = 16) { t += ms; const f = queued; queued = null; if (f) f(t); return f !== null; },
    run(ms, step = 16) { let spent = 0; while (spent < ms && this.pending()) { this.tick(step); spent += step; } return spent; },
  };
}
function setup(over = {}) {
  const lenis = over.lenis || fakeLenis();
  const loop = fakeLoop();
  const arrivals = [];
  const frames = [];
  const engine = createEngine({
    lenis, stops: STOPS, floorHeight: () => 1000, now: loop.now, raf: loop.raf, caf: loop.caf,
    reduced: over.reduced || (() => false), onFrame: over.onFrame || ((v) => frames.push(v)), onArrive: (id, info) => arrivals.push({ id, ...info, at: loop.now() }),
    ...(over.canSnap ? { canSnap: over.canSnap } : {}),
  });
  engine.measure();
  return { engine, lenis, loop, arrivals, frames };
}

test("measure fixes the marks at i x floor height and the max at the end of the page", () => {
  const { engine } = setup();
  assert.deepEqual(engine.S.marks, [0, 1000, 2000, 3000, 4000, 5000]);
  assert.equal(engine.S.max, 5000);
});

test("an adjacent jump lands in 420 +- 30 ms, writes only immediate+force, ends exactly on the mark, and arrives once", () => {
  const { engine, lenis, loop, arrivals } = setup();
  loop.run(100);
  const t0 = loop.now();
  assert.equal(engine.jumpTo("hoy"), true);
  loop.run(1000);
  assert.equal(arrivals.length, 1);
  assert.deepEqual({ id: arrivals[0].id, cut: arrivals[0].cut, push: arrivals[0].push, manual: arrivals[0].manual }, { id: "hoy", cut: false, push: true, manual: false });
  assert.ok(Math.abs(arrivals[0].at - t0 - 420) <= 30, `arrived after ${arrivals[0].at - t0} ms`);
  assert.ok(lenis.writes.length > 10);
  for (const w of lenis.writes) assert.deepEqual(w.opts, { immediate: true, force: true });
  assert.equal(lenis.writes.at(-1).y, 1000);
  assert.equal(engine.current(), "hoy");
});

test("N floors take min(800, 420 + 120 (N - 1))", () => {
  for (const [to, want] of [["procesos", 540], ["cola", 660], ["aprender", 780], ["sistema", 800]]) {
    const { engine, loop, arrivals } = setup();
    loop.run(50);
    const t0 = loop.now();
    engine.jumpTo(to, { push: false });
    loop.run(1500);
    assert.ok(Math.abs(arrivals[0].at - t0 - want) <= 30, `${to}: ${arrivals[0].at - t0} vs ${want}`);
    assert.equal(arrivals[0].push, false);
  }
});

test("while a jump runs p grows monotonically, view.jumping is true, and a frame never advances more than DT_CAP", () => {
  const { engine, lenis, loop, frames } = setup();
  engine.jumpTo("aprender");
  loop.run(100);
  loop.tick(900); // the tab stalled for almost a second
  const rafs = lenis.rafs;
  for (let i = 1; i < rafs.length; i++) assert.ok(rafs[i] - rafs[i - 1] <= DT_CAP + 1e-9, `step ${rafs[i] - rafs[i - 1]}`);
  loop.run(2000);
  const during = frames.filter((v) => v.jumping);
  assert.ok(during.length > 5);
  for (let i = 1; i < frames.length; i++) assert.ok(frames[i].y >= frames[i - 1].y - 1e-9);
  assert.ok(frames.every((v) => v.p >= 0 && v.p <= 1));
});

test("cancelJump mid-way stops the writes at once (no brusque move), arrives nowhere, and 150 ms later the page glides to the nearest stop", () => {
  const { engine, lenis, loop, arrivals } = setup();
  engine.jumpTo("cola");
  loop.run(200);
  const y = lenis.animatedScroll;
  assert.ok(y > 0 && y < 3000);
  engine.cancelJump();
  const writes = lenis.writes.length;
  loop.run(100);
  assert.equal(lenis.writes.length, writes);
  assert.equal(lenis.animatedScroll, y);
  loop.run(1000);
  assert.equal(lenis.animatedScroll, Math.round(y / 1000) * 1000, "the nearest mark");
  assert.deepEqual(arrivals.filter((a) => !a.manual), []);
});

function keyEvent(key, target) {
  const e = new Event("keydown");
  Object.defineProperty(e, "key", { value: key });
  Object.defineProperty(e, "target", { value: target });
  return e;
}

test("a wheel, a touch, a pointer or a navigation key cancels the jump; a key typed in a field does not", () => {
  for (const make of [() => new Event("wheel"), () => new Event("touchstart"), () => new Event("pointerdown"), () => keyEvent("ArrowDown", {})]) {
    const { engine, loop, lenis } = setup();
    const win = new EventTarget();
    const detach = engine.bind(win, { typing: () => false });
    engine.jumpTo("sistema");
    loop.run(150);
    const at = lenis.writes.length;
    win.dispatchEvent(make());
    loop.run(100);
    assert.equal(lenis.writes.length, at, "writes after the cancelling event");
    detach();
  }
  const { engine, loop, lenis } = setup();
  const win = new EventTarget();
  engine.bind(win, { typing: (el) => el.field === true });
  engine.jumpTo("sistema");
  loop.run(150);
  win.dispatchEvent(keyEvent("ArrowDown", { field: true }));
  loop.run(2000);
  assert.equal(lenis.animatedScroll, 5000, "typing in a field must not cancel the trip");
});

test("jumping to another stop while one is in flight retargets from where the page is; only the last arrives", () => {
  const { engine, loop, arrivals, lenis } = setup();
  engine.jumpTo("sistema");
  loop.run(200);
  const y = lenis.animatedScroll;
  engine.jumpTo("hoy");
  loop.run(1500);
  assert.deepEqual(arrivals.map((a) => a.id), ["hoy"]);
  assert.ok(y > 0);
  assert.equal(lenis.animatedScroll, 1000);
});

test("reduced motion cuts: one write, one arrival, flagged cut", () => {
  const { engine, loop, arrivals, lenis } = setup({ reduced: () => true });
  engine.jumpTo("cola");
  loop.run(100);
  assert.equal(lenis.writes.filter((w) => w.y === 3000).length, 1);
  assert.deepEqual(arrivals.map((a) => [a.id, a.cut]), [["cola", true]]);
  assert.equal(engine.current(), "cola");
});

test("jumping to where you already are does not travel and does not push history", () => {
  const { engine, loop, arrivals } = setup();
  engine.jumpTo("hoy"); loop.run(1000);
  arrivals.length = 0;
  engine.jumpTo("hoy", { push: true });
  loop.run(100);
  assert.deepEqual(arrivals.map((a) => [a.id, a.push, a.manual]), [["hoy", false, false]]);
});

test("an unknown id changes nothing", () => {
  const { engine, lenis } = setup();
  assert.equal(engine.jumpTo("nope"), false);
  assert.deepEqual(lenis.writes, []);
});

test("place() puts the page on a mark at once and arrives nowhere", () => {
  const { engine, loop, arrivals, lenis } = setup();
  engine.place("aprender");
  loop.run(100);
  assert.equal(lenis.animatedScroll, 4000);
  assert.equal(engine.current(), "aprender");
  assert.deepEqual(arrivals, []);
});

test("a manual scroll that rests for 150 ms settles on the nearest stop, once", () => {
  const { engine, lenis, loop, arrivals } = setup();
  loop.run(100);
  lenis.animatedScroll = 1980;     // the user dragged the scrollbar
  loop.run(100);
  assert.deepEqual(arrivals, []);  // still moving a moment ago
  loop.run(400);
  assert.deepEqual(arrivals.map((a) => [a.id, a.manual, a.push]), [["procesos", true, false]]);
  loop.run(600);
  assert.equal(arrivals.length, 1);
});

test("p and pos follow the page; current is the stop nearest to pos", () => {
  const { engine, lenis, loop } = setup();
  lenis.animatedScroll = 2400; loop.run(50);
  assert.equal(engine.S.pos, 2.4);
  assert.equal(engine.S.p, 0.48);
  assert.equal(engine.current(), "procesos");
  lenis.animatedScroll = 2600; loop.run(50);
  assert.equal(engine.current(), "cola");
});

test("the loop sleeps after SLEEP_AFTER idle frames, wakes on wake(), and a sleeping engine paints nothing", () => {
  const { engine, loop, frames } = setup();
  loop.run(2000);
  assert.equal(loop.pending(), false, "the engine must stop asking for frames");
  const painted = frames.length;
  loop.tick(16); loop.tick(16);
  assert.equal(frames.length, painted);
  engine.wake();
  assert.equal(loop.pending(), true);
  loop.tick(16);
  assert.ok(frames.length >= painted);
  assert.ok(SLEEP_AFTER >= 3 && SLEEP_AFTER <= 20);
});

test("measure() after a resize recomputes the marks and keeps a dirty flag for one repaint", () => {
  let height = 1000;
  const lenis = fakeLenis(5000);
  const loop = fakeLoop();
  const frames = [];
  const engine = createEngine({ lenis, stops: STOPS, floorHeight: () => height, now: loop.now, raf: loop.raf, caf: loop.caf, onFrame: (v) => frames.push(v) });
  engine.measure(); loop.run(500);
  height = 800; lenis.limit = 4000;
  engine.measure();
  assert.deepEqual(engine.S.marks, [0, 800, 1600, 2400, 3200, 4000]);
  assert.equal(engine.S.dirty, true);
  const painted = frames.length;
  loop.tick(16);
  assert.equal(frames.length, painted + 1);
});

// The three tests below come from reading Lenis 1.3.26 (node_modules/lenis/dist/lenis.mjs), which the fakes above cannot show.
test("a wheel, a touch or a pointer wakes a sleeping engine: Lenis only advances when the engine feeds it time", () => {
  for (const name of ["wheel", "touchstart", "pointerdown"]) {
    const { engine, loop } = setup();
    const win = new EventTarget();
    engine.bind(win, { typing: () => false });
    loop.run(2000);
    assert.equal(loop.pending(), false, "asleep");
    win.dispatchEvent(new Event(name));
    assert.equal(loop.pending(), true, `${name} must wake the loop`);
  }
});

test("inside onArrive the engine already reports the stop it arrived at (a landing and a cut)", () => {
  for (const [reduced, stop] of [[false, "aprender"], [true, "cola"]]) {
    const lenis = fakeLenis();
    const loop = fakeLoop();
    const seen = [];
    const engine = createEngine({
      lenis, stops: STOPS, floorHeight: () => 1000, now: loop.now, raf: loop.raf, caf: loop.caf, reduced: () => reduced,
      onArrive: (id) => seen.push({ id, current: engine.current(), y: engine.S.y, pos: engine.S.pos }),
    });
    engine.measure();
    engine.jumpTo(stop);
    loop.run(1500);
    const mark = STOPS.indexOf(stop) * 1000;
    assert.deepEqual(seen, [{ id: stop, current: stop, y: mark, pos: mark / 1000 }], `reduced=${reduced}`);
  }
});

test("a window resize re-aligns a settled page to its stop; a page in the middle of a scroll stays where it is", () => {
  let height = 1000;
  const lenis = fakeLenis(5000);
  const loop = fakeLoop();
  const arrivals = [];
  const engine = createEngine({ lenis, stops: STOPS, floorHeight: () => height, now: loop.now, raf: loop.raf, caf: loop.caf, onArrive: (id) => arrivals.push(id) });
  const win = new EventTarget();
  engine.bind(win, { typing: () => false });
  engine.measure();
  engine.place("cola"); loop.run(300);
  assert.equal(lenis.animatedScroll, 3000);
  height = 800; lenis.limit = 4000;
  win.dispatchEvent(new Event("resize"));
  loop.run(300);
  assert.deepEqual(engine.S.marks, [0, 800, 1600, 2400, 3200, 4000]);
  assert.equal(lenis.animatedScroll, 2400, "the sheet must not stay half faded after the window changed");
  assert.equal(engine.current(), "cola");
  lenis.animatedScroll = 1234;      // the user is dragging the scrollbar: the browser fires scroll events
  win.dispatchEvent(new Event("scroll"));
  loop.run(32);
  height = 1000; lenis.limit = 5000;
  win.dispatchEvent(new Event("resize"));
  loop.run(32);
  assert.equal(lenis.animatedScroll, 1234, "a resize in the middle of a manual scroll leaves the page alone");
  assert.deepEqual(arrivals, [], "re-aligning is not an arrival");
});

test("destroy cancels the frame and the listeners", () => {
  const { engine, loop, lenis } = setup();
  const win = new EventTarget();
  engine.bind(win, { typing: () => false });
  engine.destroy();
  assert.equal(loop.pending(), false);
  win.dispatchEvent(new Event("wheel"));
  assert.equal(loop.pending(), false);
  assert.deepEqual(lenis.writes, []);
});

// ---- the findings of the independent review (REVIEW-F0-front.md): A1, M1, M2, B1, B2, B4 ----

function wheelEvent(deltaY, target = {}) {
  const e = new Event("wheel");
  Object.defineProperty(e, "deltaY", { value: deltaY });
  Object.defineProperty(e, "deltaX", { value: 0 });
  Object.defineProperty(e, "target", { value: target });
  return e;
}

test("A1: a scroll that rests between two stops glides to the nearest one in at most 400 ms, arrives once, and writes only immediate+force", () => {
  for (const [y, mark] of [[1180, 1000], [1620, 2000], [2499, 2000], [2501, 3000], [4930, 5000]]) {
    const { loop, lenis, arrivals, engine } = setup();
    loop.run(100);
    lenis.animatedScroll = y;                 // the user stopped here: the scrollbar, a wheel notch, the end of a flick
    const t0 = loop.now();
    loop.run(150 + 400 + 200);
    assert.equal(lenis.animatedScroll, mark, `from ${y}`);
    assert.equal(engine.current(), STOPS[mark / 1000]);
    assert.equal(arrivals.length, 1, "the stop is announced once, not again when the glide ends");
    assert.deepEqual([arrivals[0].manual, arrivals[0].push], [true, false]);
    assert.ok(arrivals[0].at - t0 <= 150 + 80, `announced after ${arrivals[0].at - t0} ms`);
    for (const w of lenis.writes) assert.deepEqual(w.opts, { immediate: true, force: true });
    loop.run(1000);
    assert.equal(loop.pending(), false, "and the loop goes back to sleep");
  }
});

test("A1: a page already on a mark does not move, and a gesture during the glide cancels it and snaps again after the next 150 ms of quiet", () => {
  const rest = setup();
  rest.loop.run(100); rest.lenis.animatedScroll = 3000; rest.loop.run(1000);
  assert.deepEqual(rest.lenis.writes, []);
  const { loop, lenis, engine } = setup();
  const win = new EventTarget();
  engine.bind(win, { typing: () => false });
  loop.run(100);
  lenis.animatedScroll = 1300; loop.run(300);                // the glide to 1000 has started
  const y = lenis.animatedScroll;
  assert.ok(y < 1300 && y > 1000, `mid-glide at ${y}`);
  win.dispatchEvent(wheelEvent(100));
  const writes = lenis.writes.length;
  loop.run(100);
  assert.equal(lenis.writes.length, writes, "the wheel stopped the glide");
  lenis.animatedScroll = 2290;                               // and the wheel carried the page to the other side
  loop.run(900);
  assert.equal(lenis.animatedScroll, 2000);
});

test("A1: no snap under reduced motion, with canSnap() false (a drawer is open) or while a field has the focus; engine.snap() does it when you ask", () => {
  for (const make of [() => setup({ reduced: () => true }), () => setup({ canSnap: () => false })]) {
    const { loop, lenis, arrivals, engine } = make();
    loop.run(100); lenis.animatedScroll = 1300; loop.run(1500);
    assert.equal(lenis.animatedScroll, 1300, "the page stays where it is");
    assert.deepEqual(arrivals.map((a) => a.id), ["hoy"], "but the stop it rests nearest to is still announced");
    assert.equal(engine.snap(), false, "engine.snap() obeys the same holds");
  }
  const typing = setup();
  const win = new EventTarget();
  win.document = { activeElement: { field: true } };
  typing.engine.bind(win, { typing: (el) => !!(el && el.field) });
  typing.loop.run(100); typing.lenis.animatedScroll = 1300; typing.loop.run(1500);
  assert.equal(typing.lenis.animatedScroll, 1300);
  win.document.activeElement = null;
  assert.equal(typing.engine.snap(), true);
  typing.loop.run(900);
  assert.equal(typing.lenis.animatedScroll, 1000);
  const open = { on: true };
  const held = setup({ canSnap: () => !open.on });
  held.loop.run(100); held.lenis.animatedScroll = 1300; held.loop.run(500);
  open.on = false;
  assert.equal(held.engine.snap(), true, "the drawer closed: the shell asks for the snap");
  held.loop.run(900);
  assert.equal(held.lenis.animatedScroll, 1000);
});

test("A1: while a finger is down the page is not quiet, so it does not snap under the hand; it snaps 150 ms after the finger leaves", () => {
  const { engine, loop, lenis } = setup();
  const win = new EventTarget();
  engine.bind(win, { typing: () => false });
  loop.run(100);
  lenis.animatedScroll = 2300;
  const down = new Event("touchstart"); Object.defineProperty(down, "touches", { value: [{}] });
  win.dispatchEvent(down);
  loop.run(1000);
  assert.equal(lenis.animatedScroll, 2300, "the finger is still on the glass");
  const up = new Event("touchend"); Object.defineProperty(up, "touches", { value: [] });
  win.dispatchEvent(up);
  loop.run(700);
  assert.equal(lenis.animatedScroll, 2000);
});

test("M1: the inertia tail of a wheel does not cancel a trip started by the keyboard; a wheel on a list never cancels one; a firm notch does", () => {
  const run = (events) => {
    const { engine, loop, lenis, arrivals } = setup();
    const win = new EventTarget();
    engine.bind(win, { typing: () => false });
    loop.run(100);
    for (const dy of [90, 100, 100, 80]) { win.dispatchEvent(wheelEvent(dy)); loop.run(16); }    // the flick that was running
    engine.jumpTo("sistema");
    loop.run(16);
    for (const ev of events) { win.dispatchEvent(ev); loop.run(16); }
    loop.run(1500);
    return { y: lenis.animatedScroll, arrivals };
  };
  const tail = Array.from({ length: 40 }, (_, i) => wheelEvent(Math.max(1, 70 - i * 2)));       // 70 down to 1, every frame
  const a = run(tail);
  assert.equal(a.y, 5000, "the trip landed");
  assert.deepEqual(a.arrivals.map((x) => [x.id, x.manual]), [["sistema", false]]);
  const list = run(Array.from({ length: 10 }, () => wheelEvent(120, { closest: (sel) => (sel === "[data-lenis-prevent]" ? {} : null) })));
  assert.equal(list.y, 5000, "a wheel that scrolls a list is not a wish to leave the trip");
  const small = run(Array.from({ length: 10 }, () => wheelEvent(3)));
  assert.equal(small.y, 5000, "a few pixels of wheel are not a gesture");
  const firm = setup();
  const win = new EventTarget();
  firm.engine.bind(win, { typing: () => false });
  firm.engine.jumpTo("sistema"); firm.loop.run(150);
  const at = firm.lenis.writes.length;
  win.dispatchEvent(wheelEvent(100)); firm.loop.run(100);
  assert.equal(firm.lenis.writes.length, at, "a firm notch stops the trip (the glide to a stop comes 150 ms later)");
  assert.ok(firm.lenis.animatedScroll < 1000);
});

test("M2: a navigation key whose default was already prevented (Home/End in the tab list, which makes its own jump) does not cancel that jump", () => {
  for (const [key, id, y] of [["End", "sistema", 5000], ["Home", "prologo", 0], ["ArrowDown", "cola", 3000]]) {
    const { engine, loop, lenis } = setup();
    const win = new EventTarget();
    engine.bind(win, { typing: () => false });
    engine.place("cola"); loop.run(50);
    const e = new Event("keydown", { cancelable: true });
    Object.defineProperty(e, "key", { value: key });
    Object.defineProperty(e, "target", { value: {} });
    e.preventDefault();                       // the tab list handled the key (stops.js) before it reached the window
    engine.jumpTo(id);                        // ...and started its own trip
    win.dispatchEvent(e);
    loop.run(1500);
    assert.equal(lenis.animatedScroll, y, `${key}`);
  }
});

test("B1: a resize in the middle of a trip re-aims it at the new mark of the same stop and announces that stop", () => {
  let height = 1000;
  const lenis = fakeLenis(5000);
  const loop = fakeLoop();
  const arrivals = [];
  const engine = createEngine({ lenis, stops: STOPS, floorHeight: () => height, now: loop.now, raf: loop.raf, caf: loop.caf, onArrive: (id, info) => arrivals.push({ id, ...info }) });
  const win = new EventTarget();
  engine.bind(win, { typing: () => false });
  engine.measure();
  engine.jumpTo("aprender", { push: false });
  loop.run(200);
  height = 700; lenis.limit = 3500;
  win.dispatchEvent(new Event("resize"));
  loop.run(1500);
  assert.deepEqual(engine.S.marks, [0, 700, 1400, 2100, 2800, 3500]);
  assert.equal(lenis.animatedScroll, 2800, "the new mark of APRENDER, not the old 4000 clamped");
  assert.deepEqual(arrivals.map((a) => [a.id, a.push]), [["aprender", false]]);
});

test("B4: an exception in onFrame (or in onArrive) is reported and the loop keeps running", () => {
  const seen = [];
  const real = console.error;
  console.error = (...a) => seen.push(a.map(String).join(" "));
  try {
    let n = 0;
    const { engine, loop, lenis } = setup({ onFrame: () => { if (++n === 3) throw new Error("boom"); } });
    engine.jumpTo("procesos");
    loop.run(1500);
    assert.equal(lenis.animatedScroll, 2000, "the trip still finished");
    assert.ok(seen.some((line) => line.includes("boom")), "and the error was reported");
    const lenis2 = fakeLenis(); const loop2 = fakeLoop();
    const e2 = createEngine({ lenis: lenis2, stops: STOPS, floorHeight: () => 1000, now: loop2.now, raf: loop2.raf, caf: loop2.caf, onArrive: () => { throw new Error("arrive boom"); } });
    e2.measure(); e2.jumpTo("hoy"); loop2.run(1000);
    assert.equal(lenis2.animatedScroll, 1000);
    e2.jumpTo("cola"); loop2.run(1500);
    assert.equal(lenis2.animatedScroll, 3000, "and the next trip works");
  } finally { console.error = real; }
});

test("B2: without Lenis the engine drives the native scroll through the window (jumps, marks and the snap still work)", () => {
  const win = new EventTarget();
  win.innerHeight = 1000; win.scrollY = 0; win.document = { documentElement: { scrollHeight: 6000 } };
  win.scrollTo = (x, y) => { win.scrollY = Math.max(0, Math.min(5000, y)); };
  const loop = fakeLoop();
  const arrivals = [];
  const engine = createEngine({ lenis: null, win, stops: STOPS, floorHeight: () => 1000, now: loop.now, raf: loop.raf, caf: loop.caf, onArrive: (id, info) => arrivals.push({ id, ...info }) });
  engine.bind(win, { typing: () => false });
  engine.measure();
  assert.deepEqual(engine.S.marks, [0, 1000, 2000, 3000, 4000, 5000]);
  engine.jumpTo("cola"); loop.run(1500);
  assert.equal(win.scrollY, 3000);
  assert.deepEqual(arrivals.map((a) => a.id), ["cola"]);
  win.scrollY = 3300; win.dispatchEvent(new Event("scroll")); loop.run(900);
  assert.equal(win.scrollY, 3000, "snapped back");
});

test("A1: the long exponential tail of Lenis's smoothing (lerp 0.1, a pixel every few frames for a second) counts as rest: the stop is announced and reached in about a second", () => {
  const lenis = fakeLenis();
  lenis.target = 900; lenis.animatedScroll = 900;
  lenis.raf = (t) => { lenis.rafs.push(t); lenis.animatedScroll += (lenis.target - lenis.animatedScroll) * 0.1; lenis.isScrolling = Math.abs(lenis.target - lenis.animatedScroll) > 0.001; };
  lenis.scrollTo = (y, opts = {}) => { lenis.writes.push({ y, opts }); lenis.animatedScroll = lenis.target = y; };
  const { loop, arrivals, engine } = setup({ lenis });
  engine.measure();
  loop.run(100);
  const t0 = loop.now();
  lenis.target = 1100;                          // two wheel notches: 200 px of tail
  loop.run(2000);
  assert.equal(arrivals.length, 1);
  assert.ok(arrivals[0].at - t0 < 900, `announced after ${arrivals[0].at - t0} ms`);
  assert.equal(lenis.animatedScroll, 1000, "and the page is on the mark");
  assert.ok(lenis.writes.length > 0 && lenis.writes.at(-1).y === 1000);
});

// ---- review of the motion (ACEPTACION A3 and A7, and the settling time of a wheel): trips on the wall clock, the rest decided by where the wheel is going ----

test("A3/A7: a trip lasts what the formula says on the wall clock, even if a frame in the middle took 230 ms (a moment was mounted inside it)", () => {
  for (const [to, want] of [["procesos", 540], ["cola", 660], ["sistema", 800]]) {
    const { engine, loop, arrivals } = setup();
    loop.run(50);
    const t0 = loop.now();
    engine.jumpTo(to, { push: false });
    loop.run(100);
    loop.tick(230);                                   // the moment of the stop in the middle was built in this frame
    loop.tick(130);                                   // and the browser laid it out in the next
    loop.run(1500);
    assert.equal(arrivals.length, 1);
    assert.ok(arrivals[0].at - t0 >= want - 1 && arrivals[0].at - t0 <= want + 30, `${to}: arrived after ${arrivals[0].at - t0} ms, the formula says ${want}`);
  }
});

test("a tab that stalled for a second does not fast-forward a trip by more than TRIP_DT_CAP, nor the smoothing of the wheel by more than DT_CAP", () => {
  assert.ok(TRIP_DT_CAP >= 420 && TRIP_DT_CAP <= 500 && TRIP_DT_CAP > DT_CAP, "above the longest frame of a trip (a mount at CPU x4 is 420 ms), well under a second");
  const { engine, lenis, loop } = setup();
  engine.jumpTo("sistema");                           // 800 ms
  loop.run(100);
  const before = lenis.rafs.length;
  loop.tick(1000);
  assert.ok(engine.S.jump, "the trip is still on its way after the stall");
  assert.ok(lenis.rafs[before] - lenis.rafs[before - 1] <= DT_CAP + 1e-9, "Lenis got at most DT_CAP");
  loop.run(100);
  assert.ok(engine.S.jump, "and still has 130 ms to go 100 ms later");
});

// A Lenis with the smoothing of the real one (1.3.26: lerp 0.1 per 60 Hz frame, a wheel moves `targetScroll` at once, an immediate scrollTo cancels it)
function smoothLenis(limit = 5000) {
  const l = fakeLenis(limit);
  l.targetScroll = 0;
  l.time = undefined;
  l.raf = (t) => {
    l.rafs.push(t);
    const dt = l.time === undefined ? 0 : t - l.time;
    l.time = t;
    l.animatedScroll += (l.targetScroll - l.animatedScroll) * (1 - Math.exp(-dt * 0.0063));
    if (Math.abs(l.targetScroll - l.animatedScroll) < 0.01) l.animatedScroll = l.targetScroll;
    l.isScrolling = l.animatedScroll !== l.targetScroll;
  };
  l.scrollTo = (y, opts = {}) => { l.writes.push({ y, opts }); l.animatedScroll = l.targetScroll = y; l.isScrolling = false; };
  l.roll = (dy) => { l.targetScroll = Math.min(l.limit, Math.max(0, l.targetScroll + dy)); l.isScrolling = true; };
  return l;
}

/** A person rolls the wheel: `notches` of 100 px, one every `every` ms, on a page that rests on `stop`. Returns the clock at the last notch. */
function rollWheel({ stop = "hoy", notches, every = 16, over = {} }) {
  const lenis = smoothLenis();
  const s = setup({ lenis, ...over });
  const win = new EventTarget();
  s.engine.bind(win, { typing: () => false });
  s.engine.place(stop); s.loop.run(100);
  const log = [];
  for (let i = 0; i < notches; i++) { lenis.roll(100); win.dispatchEvent(wheelEvent(100)); s.loop.run(every); }
  const last = s.loop.now();
  return { ...s, win, last, lenis, log };
}

test("a wheel gesture decides its stop from where it is going (Lenis's targetScroll) 220 ms after its last notch, and the sheet is solid in under 700 ms", () => {
  for (const [notches, want, label] of [[8, 2000, "800 px: PROCESOS"], [14, 2000, "1400 px: PROCESOS"], [3, 1000, "300 px: back to HOY"], [18, 3000, "1800 px: COLA"]]) {
    const { engine, lenis, loop, arrivals, last } = rollWheel({ notches });
    let solidAt = null;
    for (let t = 0; t < 1500 && solidAt === null; t += 16) { loop.tick(16); if (Math.abs(lenis.animatedScroll - want) < 40 && !engine.S.jump && loop.now() - last > 220) solidAt = loop.now() - last; }
    assert.ok(solidAt !== null, `${label}: never settled (y ${lenis.animatedScroll})`);
    assert.ok(solidAt <= 700, `${label}: solid after ${solidAt} ms`);
    loop.run(600);
    assert.equal(lenis.animatedScroll, want, label);
    const announced = arrivals.map((a) => a.id);
    assert.deepEqual(announced, want === 1000 ? [] : [STOPS[want / 1000]], `${label}: the stop is announced once (and not at all when the page came back to where it was)`);
    for (const w of lenis.writes) assert.deepEqual(w.opts, { immediate: true, force: true });
  }
});

test("the glide that follows a wheel gesture carries the speed the page had: no stop and no kick at the moment it takes over", () => {
  const { lenis, loop, engine } = rollWheel({ notches: 8 });
  const ys = [];
  for (let i = 0; i < 80; i++) { loop.tick(16); ys.push(lenis.animatedScroll); }
  const first = lenis.writes.length ? ys.findIndex((y, i) => i > 0 && lenis.writes.some((w) => w.y === y)) : -1;
  assert.ok(first > 1, "the glide wrote");
  const steps = ys.map((y, i) => (i ? y - ys[i - 1] : 0));
  const before = steps[first - 1], at = steps[first], after = steps[first + 1];
  assert.ok(before > 3, `Lenis was moving at ${before} px a frame`);
  assert.ok(at > 0.4 * before && at < 2.2 * before, `the glide's first step is ${at} px after a frame of ${before}`);
  assert.ok(after <= at * 1.6 + 1, `and it does not take off (${after} after ${at})`);
  assert.equal(engine.S.jump === null || engine.S.jump.snap, true);
});

test("a gesture is not over while the wheel still turns: slow notches 150 ms apart keep accumulating towards the next stop", () => {
  const { lenis, loop } = rollWheel({ notches: 7, every: 150 });
  assert.equal(lenis.writes.length, 1, "nothing but the place() of the beginning: the page was carried, never aimed");
  loop.run(2000);
  assert.equal(lenis.animatedScroll, 2000, "700 px is past the half: PROCESOS");
});

test("a wheel on a list (data-lenis-prevent) is not a gesture of the page, and a page that has not moved is not aimed", () => {
  const lenis = smoothLenis();
  const { engine, loop } = setup({ lenis });
  const win = new EventTarget();
  engine.bind(win, { typing: () => false });
  engine.place("cola"); loop.run(100);
  for (let i = 0; i < 6; i++) { win.dispatchEvent(wheelEvent(120, { closest: (sel) => (sel === "[data-lenis-prevent]" ? {} : null) })); loop.run(16); }
  loop.run(1500);
  assert.deepEqual(lenis.writes.map((w) => w.y), [3000], "only the place() of the beginning");
});

test("when snapping is not allowed (a drawer is open, reduced motion) the wheel keeps the old rule: the stop is decided where the page comes to rest", () => {
  for (const over of [{ canSnap: () => false }, { reduced: () => true }]) {
    const { lenis, loop, arrivals } = rollWheel({ notches: 8, over });
    loop.run(2500);
    assert.equal(Math.round(lenis.animatedScroll), 1800, "the page stays where the wheel left it");
    assert.deepEqual(arrivals.map((a) => [a.id, a.manual]), [["procesos", true]]);
  }
});
