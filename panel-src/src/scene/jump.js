// The jump between floors, as pure functions (R7 §2.2 and §2.3): a tween over the one scroll position, planned here and
// stepped by the page's single requestAnimationFrame, which writes each step into the scroller. Nothing in this file
// touches the DOM, so the lab, the tests and the page can share it.

export const ADJACENT_MS = 420; // one floor
export const STEP_MS = 120;     // each further floor
export const MAX_MS = 800;      // the longest jump
export const SNAP_MAX_MS = 400; // the glide to the nearest stop when a scroll comes to rest between two (R7 "Snap propio")

/** How long a jump that crosses `floors` floors takes: min(800, 420 + 120 * (N - 1)); under one floor counts as one. */
export function jumpDuration(floors) {
  const n = Math.max(1, floors);
  return Math.min(MAX_MS, ADJACENT_MS + STEP_MS * (n - 1));
}

/** Slow in, fast through the middle, slow out; 0 at 0, 1 at 1, symmetric about the half. */
export function easeInOutQuart(u) {
  const x = Math.min(1, Math.max(0, u));
  return x < 0.5 ? 8 * x ** 4 : 1 - (-2 * x + 2) ** 4 / 2;
}

/**
 * What to do to get from `from` to `to` (scroll positions in px) when a floor is `floorHeight` tall:
 * null if already there, `{ cut: true, to }` under reduced motion (arrive at once), otherwise a job for stepJump.
 */
export function planJump({ from, to, floorHeight, reduced = false, now = 0 }) {
  if (Math.abs(to - from) < 0.5) return null;
  if (reduced) return { cut: true, to };
  return { cut: false, from, to, t0: now, ms: jumpDuration(Math.abs(to - from) / floorHeight) };
}

/**
 * Where the job is at `now`, and whether it has arrived. The last step is exactly `to`. A job with `carry` (a glide that began while the page
 * was moving) is a cubic Hermite curve that starts with that much momentum and ends at rest; any other job eases in and out.
 */
export function stepJump(job, now) {
  const u = Math.min(1, Math.max(0, (now - job.t0) / job.ms));
  if (u >= 1) return { y: job.to, done: true };
  if (job.carry === undefined) return { y: job.from + (job.to - job.from) * easeInOutQuart(u), done: false };
  const h10 = u * (1 - u) * (1 - u), h01 = u * u * (3 - 2 * u);
  return { y: job.from + job.carry * h10 + (job.to - job.from) * h01, done: false };
}

/**
 * The short glide that puts a page that came to rest between two stops on the nearest one: null if already there, otherwise a
 * job for stepJump of 140 ms plus 520 ms per floor to cover (half a floor is the longest way, so 400 ms at most). Never a cut:
 * the caller does not snap at all under reduced motion.
 */
export function planSnap({ from, to, floorHeight, now = 0, speed = 0 }) {
  const dist = Math.abs(to - from);
  if (dist < 0.5) return null;
  const ms = Math.round(Math.min(SNAP_MAX_MS, Math.max(160, 140 + 520 * (dist / floorHeight))));
  const job = { cut: false, from, to, t0: now, ms };
  // `speed` (px per ms, signed) is how fast the page is moving as the glide begins. It is carried into the glide, so that the end of a wheel
  // gesture is one movement and not a stop followed by a start; kept within what the curve can take without overshooting the mark
  // (2.5 times the way, which is monotone) and, when it points away from the mark, to a fifth of it, so the page gives way only a little.
  if (Math.abs(speed) > 0.02) {
    const way = to - from;
    const carry = speed * ms;
    job.carry = Math.sign(carry) * Math.min(Math.abs(carry), (carry * way > 0 ? 2.5 : 0.6) * Math.abs(way));
  }
  return job;
}
