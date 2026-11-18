// The scene engine (spec §3.2, R7 §2): one variable `p`, two writers of the scroll, one reader.
//   writer 1  the native scroll, smoothed by Lenis (the engine only feeds it time: lenis.raf(virtual time))
//   writer 2  a cancelable tween (scene/jump.js) written with lenis.scrollTo(y, { immediate: true, force: true })
//   reader    frame(): the only requestAnimationFrame of the page
// Time is virtual, with two clocks. Lenis's smoothing gets at most DT_CAP ms a frame, so a stalled tab does not fast-forward it. The
// tweens (trips and glides) follow the wall clock with a ceiling of TRIP_DT_CAP ms a frame: a trip lasts what §3.3 says even if a frame in
// the middle took 230 ms because the moment of the stop was mounted in it (the acceptance matrix A3 and A7), and a stall of a second still
// moves it by no more than 450 ms (the longest frame measured on a trip, mounting SISTEMA at CPU x4, is 420). The loop sleeps after SLEEP_AFTER quiet frames and wakes on any input. bind(),
// fieldFocused() and the defaults (window, performance, requestAnimationFrame, the native scroller) touch the DOM; Lenis, the clock and
// the frame scheduler are injected, so the tests drive the rest with fakes.
// A page never rests between two stops: 150 ms after a scroll comes to rest the engine glides to the nearest one (R7 "Snap
// propio"; the sheet is gone between stops, so a page left there would show nothing). Not under reduced motion, not with a drawer
// open (`canSnap`), not while a field has the focus or a finger is down. `snap()` asks for it by hand.
// A wheel gesture does not wait for the page to come to rest: Lenis knows where it is going (`targetScroll`), so 220 ms after the last
// notch the engine glides to the stop nearest to THAT, carrying the speed the page has. The smoothing of the wheel takes most of a second
// to arrive at under a pixel, and a sheet that is invisible between two stops cannot wait for that.
// Do not listen to `scrollend` for arrivals: every immediate write of the jump makes Lenis 1.3.26 fire about two of them.

import { clamp } from "../core/math.js";
import { planJump, planSnap, stepJump } from "./jump.js";

export const DT_CAP = 50;
export const TRIP_DT_CAP = 450;
export const SLEEP_AFTER = 8;
const NAV_KEYS = new Set(["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End", " ", "Spacebar"]);
const MOVED_EPSILON_PX = 0.01; // a frame that moved the page by less than this did not move it (rounding of the smoothing)
const WROTE_TOLERANCE_PX = 0.5; // the page this near the y the engine wrote is where the engine put it, not where a person took it
const REST_SPEED = 0.05;      // px per ms (under a pixel a frame at 60 Hz): slower than this the page is not being scrolled
const WHEEL_INTENT_PX = 40;   // less than this is the end of an inertia tail or a nudge, not a wish to leave the trip
export const WHEEL_STREAM_MS = 120;  // wheel events closer than this belong to one gesture
const WHEEL_QUIET_MS = 220;   // a gesture is over this long after its last event (a slow roll, a notch every 300 ms, is several gestures)
const WHEEL_LINE_PX = 16;     // pixels in one line of a wheel that reports lines
const WHEEL_LIVE_MS = 1200;   // a wheel that moved nothing (the end of the page) is forgotten after this
const REST_QUIET_MS = 150;    // a page that no one has touched for this long goes to rest on its stop
const FIRST_FRAME_MS = 16.7;  // the first frame has no earlier one to measure against: it counts as one frame of a 60 Hz screen

/** The few members of Lenis that the engine uses, over the browser's own scroll: the page still works if Lenis does not build. */
function nativeScroller(win) {
  return {
    get animatedScroll() { return win.scrollY; },
    get limit() { return Math.max(0, win.document.documentElement.scrollHeight - win.innerHeight); },
    isScrolling: false,
    raf() {},
    resize() {},
    scrollTo(y) { win.scrollTo(0, y); },
  };
}

export function createEngine({
  lenis,
  win = typeof window === "undefined" ? null : window,
  stops,
  floorHeight = () => win.innerHeight,
  now = () => performance.now(),
  raf = (f) => requestAnimationFrame(f),
  caf = (id) => cancelAnimationFrame(id),
  reduced = () => false,
  canSnap = () => true,
  onFrame = () => {},
  onArrive = () => {},
  quietMs = REST_QUIET_MS,
  wheelQuietMs = WHEEL_QUIET_MS,
}) {
  if (!lenis) lenis = nativeScroller(win);
  const S = { y: 0, p: 0, pos: 0, max: 0, marks: [], current: 0, jump: null, dirty: true };
  let rafId = 0;
  let lastT = 0;
  let virtual = 0;
  let trip = 0;              // the clock of the tweens
  let vel = 0;               // px per ms the page moved in the last frame (signed)
  let wheelAt = null;        // when the last wheel event that moves the page arrived
  let idle = 0;
  let dead = false;
  let wrote = null;          // the y this engine last wrote: movement that does not match it came from a person
  let settled = true;
  let quietSince = 0;
  let lastArrived = null;
  let detachInput = () => {};
  let typingIn = () => false;      // set by bind(): is this element a field?
  let host = win;                  // the window bind() listens on (its document says which element has the focus)
  let fingers = 0;                 // touches on the screen (Chrome sends no pointer event for the page's own scrollbar, so a mouse is not tracked)
  let stream = null;               // the wheel gesture in progress: { begin, t, mag }
  let tripAt = -Infinity;          // when the last trip began, to tell the tail of an earlier flick from a new wheel

  const idxOf = (pos) => clamp(Math.floor(pos + 0.5), 0, stops.length - 1);

  function measure() {
    if (typeof lenis.resize === "function") lenis.resize();
    const floor = floorHeight();
    S.max = Math.max(0, Number(lenis.limit) || 0);
    S.marks = stops.map((_, i) => (S.max > 0 ? Math.min(i * floor, S.max) : i * floor));
    S.dirty = true;
    wake();
  }

  function ensureMarks() {
    if (!S.marks.length) measure();
  }

  function wake() {
    idle = 0;
    if (!rafId && !dead) { lastT = 0; rafId = raf(frame); }
  }

  // A callback that throws must not stop the loop: it is rearmed at the end of the frame, after the callbacks.
  function arrive(id, info) {
    lastArrived = id;
    settled = true;
    try { onArrive(id, info); } catch (err) { console.error("scene engine: onArrive failed", err); }
  }

  function write(y) {
    lenis.scrollTo(y, { immediate: true, force: true });
    wrote = y;
  }

  // The one place that turns the scroll position into the engine's numbers, so a callback that runs in the middle of a
  // frame (onArrive) or right after a cut sees the same S that the next onFrame will.
  function read() {
    const y = lenis.animatedScroll;
    S.y = y;
    S.pos = clamp(y / floorHeight(), 0, stops.length - 1);
    S.p = S.max > 0 ? clamp(y / S.max, 0, 1) : 0;
    S.current = idxOf(S.pos);
    return y;
  }

  // One step of the trip in progress, written to the page. The job that arrived on this step, or null.
  function advanceJump() {
    const job = S.jump;
    if (!job) return null;
    const step = stepJump(job, trip);
    write(step.y);
    if (!step.done) return null;
    S.jump = null;
    return job;
  }

  // The page moved by itself or by a person: only what is not the engine's own write counts as someone scrolling.
  function noticeMovement(y, moved, dt) {
    if (wrote !== null && Math.abs(y - wrote) <= WROTE_TOLERANCE_PX) return;
    // Lenis's smoothing is exponential: the last 10 px take another half second at under a pixel a frame. That is rest.
    if (settled || moved > REST_SPEED * dt) quietSince = virtual;
    settled = false;
    wrote = null;
  }

  // A glide that put the page on a mark it had already announced does not announce it again; one aimed at a stop the page had not reached does.
  function announceLanding(landed) {
    if (!landed) return;
    if (!landed.snap) arrive(landed.id, { cut: false, push: landed.push, manual: false });
    else if (landed.id !== lastArrived) arrive(landed.id, { cut: false, push: false, manual: true });
  }

  // When the page has stopped moving, or a wheel gesture is over, it settles on a stop.
  function restOrGlide() {
    if (!settled && fingers > 0) quietSince = virtual;   // a finger on the screen is not rest
    if (settled && wheelAt !== null && now() - wheelAt > WHEEL_LIVE_MS) wheelAt = null;
    if (!settled && !S.jump && wheelGestureOver()) {
      wheelAt = null;
      rest(nearestStop(lenis.targetScroll));
    } else if (!settled && !S.jump && virtual - quietSince >= quietMs) {
      wheelAt = null;
      rest(stops[S.current]);
    }
  }

  function frame() {
    rafId = 0;
    if (dead) return;
    const t = now();
    const wall = lastT ? clamp(t - lastT, 0, TRIP_DT_CAP) : FIRST_FRAME_MS;
    const dt = Math.min(wall, DT_CAP);
    lastT = t;
    virtual += dt;
    trip += wall;
    lenis.raf(virtual);

    const before = S.y;
    let worked = S.dirty || !!S.jump;
    const landed = advanceJump();

    const y = read();
    const moved = Math.abs(y - before);
    vel = dt > 0 ? (y - before) / dt : 0;
    if (moved > MOVED_EPSILON_PX) {
      worked = true;
      noticeMovement(y, moved, dt);
    }
    announceLanding(landed);
    restOrGlide();

    const active = worked || !!S.jump || !settled || !!lenis.isScrolling;
    if (active) {
      idle = 0;
      try { onFrame({ y, p: S.p, pos: S.pos, max: S.max, jumping: !!S.jump, dt }); } catch (err) { console.error("scene engine: onFrame failed", err); }
      S.dirty = false;
    } else idle += 1;
    if (active || idle < SLEEP_AFTER) rafId = raf(frame);
  }

  function jumpTo(id, { push = true } = {}) {
    const i = stops.indexOf(id);
    if (i < 0) return false;
    ensureMarks();
    S.jump = null;
    tripAt = now();
    const to = S.marks[i];
    const plan = planJump({ from: lenis.animatedScroll, to, floorHeight: floorHeight(), reduced: reduced(), now: trip });
    if (!plan) arrive(id, { cut: false, push: false, manual: false });
    else if (plan.cut) { write(to); read(); S.dirty = true; arrive(id, { cut: true, push, manual: false }); }
    else S.jump = { ...plan, id, push };
    wake();
    return true;
  }

  const fieldFocused = () => {
    const el = host && host.document && host.document.activeElement;
    return !!el && typingIn(el);
  };
  const canGlide = () => !reduced() && canSnap() && !fieldFocused();
  const wheelGestureOver = () => wheelAt !== null && fingers === 0 && now() - wheelAt >= wheelQuietMs && canGlide() && Number.isFinite(lenis.targetScroll);

  /** The stop whose mark is nearest to the scroll position `y`. */
  function nearestStop(y) {
    let best = 0;
    for (let i = 1; i < S.marks.length; i++) if (Math.abs(S.marks[i] - y) < Math.abs(S.marks[best] - y)) best = i;
    return stops[best];
  }

  /**
   * The scroll has come to rest, or is as good as at rest because a wheel gesture has ended: settle on `id`. The stop the page is nearest to
   * is announced at once (the sheet already shows it); a stop the page has still to reach is announced when the glide gets it there.
   */
  function rest(id) {
    settled = true;
    const here = stops[S.current];
    if (id === here && id !== lastArrived) arrive(here, { cut: false, push: false, manual: true });
    if (!S.jump) glide(id);
  }

  /** Glide to the mark of `id` unless something forbids it. True if the page is, or is on its way, on that mark. */
  function glide(id) {
    if (!canGlide()) return false;
    const i = stops.indexOf(id);
    if (i < 0) return false;
    ensureMarks();
    const moving = Math.abs(vel) > REST_SPEED && !S.jump;
    const plan = planSnap({ from: lenis.animatedScroll, to: S.marks[i], floorHeight: floorHeight(), now: trip, speed: moving ? vel : 0 });
    if (plan) { S.jump = { ...plan, id, push: false, snap: true }; wake(); }
    return true;
  }

  /** Put a page that rests between two stops on the nearest one (the shell calls it when a drawer closes). False if something forbids it. */
  function snap() {
    if (S.jump) return false;
    return glide(stops[S.current]);
  }

  function place(id) {
    const i = stops.indexOf(id);
    if (i < 0) return;
    ensureMarks();
    S.jump = null;
    write(S.marks[i]);
    read();
    lastArrived = id;
    settled = true;
    S.dirty = true;
    wake();
  }

  function cancelJump() {
    if (!S.jump) return;
    S.jump = null;
    settled = false;
    quietSince = virtual;
    S.dirty = true;
    wake();
  }

  // A wheel cancels a trip only when it means to: not when a list under the pointer takes it (data-lenis-prevent), not when it is
  // a few pixels, and not when it is the tail of a flick that was already running when the trip began (the events of one gesture
  // arrive less than 120 ms apart and shrink). The glide of A1 puts the page right whatever is left of the tail.
  // `readWheel` only reads: where the event falls in the gesture in progress (`stream`) and whether it cancels. `onWheel` then
  // records it as the latest of that gesture, which is what the next event is read against.
  const takenByAList = (e) => !!(e.target && typeof e.target.closest === "function" && e.target.closest("[data-lenis-prevent]"));

  function readWheel(e) {
    const t = now();
    const unit = e.deltaMode === 1 ? WHEEL_LINE_PX : e.deltaMode === 2 ? floorHeight() : 1;
    const mag = Math.max(Math.abs(e.deltaY), Math.abs(e.deltaX)) * unit;     // NaN when the event carries no delta: counts as a wish
    const flowing = !!stream && t - stream.t < WHEEL_STREAM_MS;
    const tail = flowing && stream.begin < tripAt && mag < stream.mag;
    return { begin: flowing ? stream.begin : t, t, mag, cancels: !(mag < WHEEL_INTENT_PX) && !tail };
  }

  function bind(target, { typing = () => false } = {}) {
    detachInput();
    typingIn = typing;
    host = target;
    // Lenis smooths a wheel by itself but only advances when the engine feeds it time, so an input must also wake the loop.
    const touch = () => { cancelJump(); wake(); };
    const onWheel = (e) => {
      if (!takenByAList(e)) {
        wheelAt = now();
        const { cancels, ...latest } = readWheel(e);
        stream = latest;
        if (cancels) cancelJump();
      }
      wake();
    };
    const onTouch = (e) => { fingers = e.touches ? e.touches.length : 1; if (e.type === "touchstart") cancelJump(); wake(); };
    const onTouchEnd = (e) => { fingers = e.touches ? e.touches.length : 0; wake(); };
    const onBlur = () => { fingers = 0; wake(); };
    // Home and End in the tab list make their own trip and prevent the default before the event reaches the window.
    const onKey = (e) => { if (!e.defaultPrevented && NAV_KEYS.has(e.key) && !typing(e.target)) cancelJump(); wake(); };
    // A settled page keeps its stop when the window changes size (otherwise it rests between two stops, with the sheet half faded).
    const onResize = () => {
      const job = S.jump;
      const keep = settled && !job ? stops[S.current] : null;
      measure();
      if (keep) place(keep);
      else if (job) { if (job.snap) glide(job.id); else jumpTo(job.id, { push: job.push }); }   // the marks moved: aim again
    };
    const poke = () => wake();
    const listeners = [
      ["wheel", onWheel, { passive: true }], ["touchstart", onTouch, { passive: true }], ["touchmove", onTouch, { passive: true }],
      ["touchend", onTouchEnd, { passive: true }], ["touchcancel", onTouchEnd, { passive: true }],
      ["pointerdown", touch, { passive: true }], ["blur", onBlur], ["keydown", onKey], ["scroll", poke, { passive: true }], ["resize", onResize],
    ];
    for (const [name, fn, opts] of listeners) target.addEventListener(name, fn, opts);
    const doc = target.document;
    if (doc && doc.addEventListener) doc.addEventListener("visibilitychange", poke);
    detachInput = () => {
      for (const [name, fn, opts] of listeners) target.removeEventListener(name, fn, opts);
      if (doc && doc.removeEventListener) doc.removeEventListener("visibilitychange", poke);
      fingers = 0;
      detachInput = () => {};
    };
    return detachInput;
  }

  function destroy() {
    dead = true;
    detachInput();
    if (rafId) caf(rafId);
    rafId = 0;
    S.jump = null;
  }

  return { S, jumpTo, place, cancelJump, snap, measure, wake, current: () => stops[S.current], bind, destroy };
}
