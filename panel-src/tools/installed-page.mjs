// Does the page the Rust binary embeds (crates/jobbot-panel/src/page.html) match what the sources build today?
// `node build.mjs --install` writes that file, so a build with the same options to a file of its own must give the same bytes
// (the build is deterministic). Used by test/build.installed.test.mjs, which runs in `npm run check`.

import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
export const INSTALLED = path.join(ROOT, "..", "crates", "jobbot-panel", "src", "page.html");

/** null when `installed` equals a fresh build of the real page; otherwise a message that says how to regenerate it. */
export function installedPageProblem(installed = INSTALLED) {
  const regenerate = "run `node build.mjs --install` in panel-src and commit crates/jobbot-panel/src/page.html";
  if (!existsSync(installed)) return `${installed} does not exist: ${regenerate}`;
  const dir = mkdtempSync(path.join(tmpdir(), "panel-installed-"));
  try {
    const out = path.join(dir, "page.html");
    const run = spawnSync(process.execPath, ["build.mjs", "--out", out], { cwd: ROOT, encoding: "utf8" });
    if (run.status !== 0) return `the build itself failed:\n${run.stderr || run.stdout}`;
    // The build writes LF (build.mjs reads every source with CRLF folded). A checkout with core.autocrlf hands the committed page back
    // with CRLF, so the installed side is folded the same way before comparing (the Rust page tests do the same).
    const fresh = readFileSync(out);
    const have = Buffer.from(readFileSync(installed, "utf8").replace(/\r\n/g, "\n"), "utf8");
    if (fresh.equals(have)) return null;
    return `crates/jobbot-panel/src/page.html is not what panel-src builds today (${have.length} bytes installed, ${fresh.length} built): ${regenerate}`;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}
