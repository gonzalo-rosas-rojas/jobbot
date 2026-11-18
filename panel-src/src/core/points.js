// One rule for a number that came from outside (the server, the storage): it is that number, or it is nothing.

/** A value as the number its sender meant, or null when it is anything else (text, a boolean, nothing): never a made-up zero. */
export function pointsOf(value) {
  const n = typeof value === "number" || (typeof value === "string" && value.trim() !== "") ? Number(value) : NaN;
  return Number.isFinite(n) ? n : null;
}
