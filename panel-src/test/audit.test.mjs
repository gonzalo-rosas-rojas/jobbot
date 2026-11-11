// tools/audit-deps.mjs greps what the dependencies do. The real packages must match the table of audited hits; a
// synthetic package shows the scanner sees network, dynamic code and markup sinks, and ignores comments.
import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { scan, judge, KNOWN, TARGETS } from "../tools/audit-deps.mjs";

test("the installed dependencies do exactly what was audited", async () => {
  const hits = await scan();
  assert.deepEqual(judge(hits), []);
  assert.ok(Object.keys(TARGETS).length >= 3);
  for (const h of hits) assert.notEqual(h.kind, "net", `${h.pkg} reaches out: ${h.text}`);
});

test("none of the packages that reach the page can fetch, open sockets or build code from strings", async () => {
  const hits = await scan();
  const sinks = hits.filter((h) => h.kind === "code" || (h.kind === "net"));
  assert.deepEqual(sinks.map((h) => `${h.pkg} ${h.pattern}`), []);
});

test("a hit that is not in the table fails, and so does a table entry that vanished", () => {
  const extra = [{ pkg: "lenis", pattern: "fetch(", kind: "net", file: "x.js", text: "fetch('https://example.invalid')" }];
  assert.match(judge(extra).join("\n"), /lenis: 1 x fetch\( \(audited: 0\)/);
  assert.match(judge([]).join("\n"), /lenis: audited 1 x location but found 0/);
  assert.ok(KNOWN.lenis.location.why.length > 40, "every audited hit explains itself");
});

test("the scanner sees network, dynamic code and markup sinks, and skips comments", async () => {
  const root = mkdtempSync(path.join(os.tmpdir(), "audit-"));
  try {
    mkdirSync(path.join(root, "fake"));
    writeFileSync(path.join(root, "fake", "a.js"), [
      "// see https://example.invalid/docs and fetch(url) and eval(x)",
      "/* innerHTML new Function */",
      "export function go(u, el, s) {",
      "  fetch(u); new WebSocket(u); eval(s); new Function(s); el.innerHTML = s; navigator.sendBeacon(u, s); return import(u);",
      "}",
    ].join("\n"));
    const hits = await scan({ fake: ["a.js"] }, root);
    const seen = new Set(hits.map((h) => h.pattern));
    for (const want of ["fetch(", "WebSocket", "eval", "new Function", "innerHTML", "sendBeacon", "import("]) assert.ok(seen.has(want), want);
    assert.ok(!seen.has("url"), "a URL in a comment is not a hit");
    assert.equal(hits.filter((h) => h.pattern === "eval").length, 1, "the eval in the comment is not counted");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
