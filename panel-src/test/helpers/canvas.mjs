// test/helpers/canvas.mjs — a 2D context that records every call instead of drawing: { ctx, calls, count(name) }.
export function recorder() {
  const calls = [];
  const ctx = new Proxy({}, {
    get: (_, key) => (...args) => { calls.push([key, ...args]); return String(key).startsWith("create") ? { addColorStop() {} } : undefined; },
    set: (_, key, value) => { calls.push(["set:" + key, value]); return true; },
  });
  return { ctx, calls, count: (name) => calls.filter((c) => c[0] === name).length };
}
