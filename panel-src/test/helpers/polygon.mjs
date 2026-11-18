// test/helpers/polygon.mjs — the oracle of "the light never touches the sheet" (stage/beam.js clips its polygon at the sheet's edge; the tests ask
// this independent separating-axis check whether any of it still reaches the sheet). Only the tests use it.

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
