import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { installedPageProblem } from "../tools/installed-page.mjs";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");

// The page the binary embeds must be what the sources build. A change under src/ that was not followed by `node build.mjs --install` leaves
// everything else green while the binary serves the old page.
test("crates/jobbot-panel/src/page.html is what panel-src builds today", () => {
  const problem = installedPageProblem();
  assert.equal(problem, null, problem || "");
});

test("the comparison passes on a page equal to a fresh build and fails, saying how to regenerate, on a different or missing one", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "panel-installed-test-"));
  try {
    const fresh = path.join(dir, "fresh.html");
    execFileSync(process.execPath, ["build.mjs", "--out", fresh], { cwd: root, encoding: "utf8" });
    assert.equal(installedPageProblem(fresh), null);
    // a checkout with core.autocrlf gives the committed page back with CRLF: the same page, not a stale one
    const crlf = path.join(dir, "crlf.html");
    writeFileSync(crlf, readFileSync(fresh, "utf8").replace(/\n/g, "\r\n"));
    assert.equal(installedPageProblem(crlf), null);
    const stale = path.join(dir, "stale.html");
    writeFileSync(stale, readFileSync(fresh, "utf8") + "<!-- stale -->");
    assert.match(installedPageProblem(stale), /node build\.mjs --install/);
    assert.match(installedPageProblem(path.join(dir, "missing.html")), /does not exist/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
