// What the painters of the stage (sky, skyline, beam) share: the two CSS colour strings and the offscreen canvas factory.

export const css = (c) => `rgb(${c[0]},${c[1]},${c[2]})`;
export const cssa = (c, a) => `rgba(${c[0]},${c[1]},${c[2]},${Math.max(0, a).toFixed(3)})`;

/** (w, h) -> a canvas that is not on the page; null where there is none (Node), and then every frame paints everything. */
export const offscreen = typeof OffscreenCanvas === "function" ? (w, h) => new OffscreenCanvas(w, h) : null;
