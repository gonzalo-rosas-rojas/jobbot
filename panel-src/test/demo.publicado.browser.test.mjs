// T35 (demo-publicado-sin-marcadores), in Chrome: the folder that build.mjs demo --publish writes is served as it is under /demo/, by a static
// server that sends no policy header and knows nothing of the panel, as GitHub Pages serves jobbot-showcase's site/demo. The five faces load
// from fonts/ beside the page with no 404 and no CSP violation, and the demo's data script runs. The same file edited by hand after the build
// (as jobbot-showcase 82cc16b had to fix) loses its data script to the policy, which is what publishedProblems catches without a browser.
// Opened from the disk (file://), the same folder shows its five faces too.
import test from "node:test";
import assert from "node:assert/strict";
import http from "node:http";
import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { launch } from "../tools/cdp.mjs";
import * as G from "../guard.mjs";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const FONTS = ["barlow-condensed-700.woff2", "geist-mono-400.woff2", "instrument-serif-400italic.woff2", "inter-400.woff2", "inter-600.woff2"];
const TYPES = { ".html": "text/html; charset=utf-8", ".woff2": "font/woff2", ".txt": "text/plain; charset=utf-8" };
const tmp = mkdtempSync(path.join(os.tmpdir(), "panel-t35b-"));
process.on("exit", () => rmSync(tmp, { recursive: true, force: true }));
const PUB = path.join(tmp, "pub");
execFileSync(process.execPath, ["build.mjs", "demo", "--publish", "--out", path.join(PUB, "index.html")], { cwd: root, stdio: "pipe" });
const EDITED = path.join(tmp, "edited");
cpSync(PUB, EDITED, { recursive: true });
// A hand edit of the demo's data script, the second inline script (the head's picks theme and language, the last is the page): one comment.
const built = readFileSync(path.join(EDITED, "index.html"), "utf8");
const second = built.indexOf("<script>", built.indexOf("<script>") + 1) + "<script>".length;
const edited = built.slice(0, second) + "\n/* edited by hand */" + built.slice(second);
writeFileSync(path.join(EDITED, "index.html"), edited);

/** A static host: /demo/<path> is a file of `dir`, nothing else exists, and no header carries a policy. */
async function host(dir) {
  const seen = [];
  const server = http.createServer((req, res) => {
    const rel = decodeURIComponent(new URL(req.url, "http://x").pathname);
    if (rel === "/favicon.ico") { res.writeHead(204); return res.end(); }   // the browser's own request, not the page's (tools/serve.mjs does the same)
    const file = rel.startsWith("/demo/") ? path.join(dir, rel.slice("/demo/".length)) : null;
    const ok = file && file.startsWith(dir + path.sep) && existsSync(file) && statSync(file).isFile();
    seen.push({ path: rel, status: ok ? 200 : 404 });
    if (!ok) { res.writeHead(404, { "content-type": "text/plain" }); return res.end("not found"); }
    res.writeHead(200, { "content-type": TYPES[path.extname(file)] || "application/octet-stream" });
    res.end(readFileSync(file));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return { url: `http://127.0.0.1:${server.address().port}/demo/index.html?clock=9`, seen, close: () => server.close() };
}

test("served as a plain folder under /demo/: the five faces load from fonts/, nothing is missing, nothing breaks the policy, and the demo runs", async () => {
  const site = await host(PUB);
  const browser = await launch({ width: 1440, height: 900 });
  try {
    const page = await browser.newPage();
    await page.goto(site.url);
    await page.waitFor(`window.__DEMO__ && window.__DEMO__.stats.total > 0`, { timeout: 15000 });
    const faces = await page.eval(`Promise.all([...document.fonts].map((f) => f.load().then(() => f.family + " " + f.weight + " " + f.style + " " + f.status, () => f.family + " error")))`);
    assert.equal(faces.length, 5, JSON.stringify(faces));
    assert.ok(faces.every((f) => / loaded$/.test(f)), `every face loaded: ${JSON.stringify(faces)}`);
    await page.wait(300);
    for (const f of FONTS) assert.ok(site.seen.some((s) => s.path === `/demo/fonts/${f}` && s.status === 200), `${f} was fetched from fonts/ beside the page`);
    assert.deepEqual(site.seen.filter((s) => s.status !== 200), [],"nothing the page asked for is missing");
    assert.deepEqual(page.problems(), [], "no console error and no CSP violation");
  } finally { await browser.close(); site.close(); }
});

test("the same file edited by hand after the build: the policy refuses the demo's data script, and publishedProblems says so without a browser", async () => {
  assert.equal(typeof G.publishedProblems, "function", "guard.mjs exports publishedProblems(page)");
  assert.ok(G.publishedProblems(edited).length > 0, "caught without a browser");
  const site = await host(EDITED);
  const browser = await launch({ width: 1440, height: 900 });
  try {
    const page = await browser.newPage();
    await page.goto(site.url);
    await page.wait(1500);
    assert.equal(await page.eval(`typeof window.__DEMO__`), "undefined", "the data script did not run");
    assert.ok(page.problems().some((p) => /CSP violation: script-src/.test(p.text)), `the browser says it was the policy (${JSON.stringify(page.problems())})`);
  } finally { await browser.close(); site.close(); }
});

test("opened from the disk (file://), with no server at all: the five faces load from fonts/ and the demo runs, with nothing in the console", async () => {
  const browser = await launch({ width: 1440, height: 900 });
  try {
    const page = await browser.newPage();
    await page.goto(pathToFileURL(path.join(PUB, "index.html")).href + "?clock=9");
    await page.waitFor(`window.__DEMO__ && window.__DEMO__.stats.total > 0`, { timeout: 15000 });
    const faces = await page.eval(`Promise.all([...document.fonts].map((f) => f.load().then(() => f.family + " " + f.status, () => f.family + " error")))`);
    assert.equal(faces.length, 5, JSON.stringify(faces));
    assert.ok(faces.every((f) => / loaded$/.test(f)), `every face loaded from the disk: ${JSON.stringify(faces)}`);
    await page.wait(300);
    assert.deepEqual(page.problems(), [], "no console error and no CSP violation");
  } finally { await browser.close(); }
});
