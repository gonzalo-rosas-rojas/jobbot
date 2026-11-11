import test from "node:test";
import assert from "node:assert/strict";
import { readLaunch } from "../src/core/dom.js";
import { useStorage } from "../src/core/store.js";

const shelf = () => { const m = new Map(); return { m, getItem: (k) => (m.has(k) ? m.get(k) : null), setItem: (k, v) => m.set(k, String(v)), removeItem: (k) => m.delete(k) }; };
const env = (search, extra = {}) => { const e = { location: { search, pathname: "/", hash: "" }, history: { replaceState() {} }, ...extra }; e.top = e; return e; };

test("the launch token is kept for the tab: a reload without ?t= finds it, and a new one replaces it", () => {
  const session = shelf();
  useStorage({ local: shelf(), session });
  assert.equal(readLaunch(env("?t=abc")).token, "abc");
  assert.equal(readLaunch(env("")).token, "abc", "a reload comes back without the token");
  assert.equal(readLaunch(env("?t=new")).token, "new");
  assert.equal(readLaunch(env("")).token, "new");
  assert.ok([...session.m.keys()].every((k) => k.startsWith("panel.")));
  useStorage(null);
});

test("without a token anywhere the launch has none, and broken storage never breaks the start", () => {
  useStorage({ local: shelf(), session: shelf() });
  assert.equal(readLaunch(env("")).token, "");
  const broken = { getItem() { throw new Error("blocked"); }, setItem() { throw new Error("blocked"); }, removeItem() { throw new Error("blocked"); } };
  useStorage({ local: broken, session: broken });
  assert.equal(readLaunch(env("?t=abc")).token, "abc");
  assert.equal(readLaunch(env("")).token, "abc", "kept in memory for the life of the page");
  useStorage({ local: broken, session: null });
  assert.equal(readLaunch(env("?t=q")).token, "q");
  useStorage(null);
});

test("the public demo saves nothing", () => {
  const session = shelf();
  useStorage({ local: shelf(), session });
  assert.equal(readLaunch(env("?t=abc", { __DEMO__: {} })).token, "abc");
  assert.equal(session.m.size, 0);
  assert.equal(readLaunch(env("", { __DEMO__: {} })).token, "");
  useStorage(null);
});
