// Which moment sits under the glass sheet, and how solid the sheet is, at a position of the camera (spec §3.3).
// At rest and on a trip of one floor that is a function of the position alone (poses.js: the sheet swaps at the half of a floor and is
// gone between two stops). On a LONG trip it must not be: "Salto largo: directo, sin poses intermedias". The camera still travels past the
// stops in between, but the sheet leaves the stop it was on and arrives at the one it is going to; the moments of the stops between are never
// mounted, because mounting one is the most expensive thing that happens in a frame (175-320 ms at CPU x4 for each, built and thrown away).

import { sheetIndex, sheetOpacity } from "./poses.js";

const ONE_FLOOR = 1.0001;   // a floor, with a hair of slack for the rounding of px to floors: a trip this short is read from the position

/**
 * { index, opacity } of the sheet. `job` is the tween in flight (engine.S.jump) or null; `pos` the camera in floors; `floorHeight` in px.
 * A glide to the nearest stop, a trip of one floor or less, and calm motion read the sheet from the position as ever.
 */
export function sheetView({ pos, job, floorHeight, reduced = false }) {
  const plain = { index: sheetIndex(pos), opacity: sheetOpacity(pos, reduced) };
  if (reduced || !job || job.snap) return plain;
  const fromFloor = job.from / floorHeight, toFloor = job.to / floorHeight;
  if (Math.abs(toFloor - fromFloor) <= ONE_FLOOR) return plain;
  const fromStop = sheetIndex(fromFloor), toStop = sheetIndex(toFloor);
  const distFrom = Math.abs(pos - fromStop), distTo = Math.abs(pos - toStop);
  // Only the two ends exist: the nearer one is shown, as solid as a stop of one floor is at that distance, and nothing in the middle.
  const distance = Math.min(distFrom, distTo);
  return { index: distFrom <= distTo ? fromStop : toStop, opacity: distance >= 0.5 ? 0 : sheetOpacity(distance) };
}
