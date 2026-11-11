// Which moment sits under the glass sheet, and how solid the sheet is, at a position of the camera (spec §3.3).
// At rest and on a trip of one floor that is a function of the position alone (poses.js: the sheet swaps at the half of a floor and is
// gone between two stops). On a LONG trip it must not be: "Salto largo: directo, sin poses intermedias". The camera still travels past the
// stops in between, but the sheet leaves the stop it was on and arrives at the one it is going to; the moments of the stops between are never
// mounted, because mounting one is the most expensive thing that happens in a frame (175-320 ms at CPU x4 for each, built and thrown away).

import { sheetIndex, sheetOpacity } from "./poses.js";

/**
 * { index, opacity } of the sheet. `job` is the tween in flight (engine.S.jump) or null; `pos` the camera in floors; `floorHeight` in px.
 * A glide to the nearest stop, a trip of one floor or less, and calm motion read the sheet from the position as ever.
 */
export function sheetView({ pos, job, floorHeight, reduced = false }) {
  const plain = { index: sheetIndex(pos), opacity: sheetOpacity(pos, reduced) };
  if (reduced || !job || job.snap) return plain;
  const a = job.from / floorHeight, b = job.to / floorHeight;
  if (Math.abs(b - a) <= 1.0001) return plain;
  const from = sheetIndex(a), to = sheetIndex(b);
  const away = Math.abs(pos - from), near = Math.abs(pos - to);
  // Only the two ends exist: the nearer one is shown, as solid as a stop of one floor is at that distance, and nothing in the middle.
  const d = Math.min(away, near);
  return { index: away <= near ? from : to, opacity: d >= 0.5 ? 0 : sheetOpacity(d) };
}
