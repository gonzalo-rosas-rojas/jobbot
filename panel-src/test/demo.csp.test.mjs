// The demo that is published (build.mjs demo --publish, docs/demo/index.html) is a static file with no server behind it, so the nonces of the real page
// are gone and no header carries a policy: the guard's list of strings would be its only defence (review M5). The build therefore writes a meta CSP into
// it, with the SHA-256 of each inline script and style instead of a nonce (the demo keeps its rule of no nonces), and `connect-src 'none'`, because the
// demo draws its own data and never asks the network. These tests build a published demo from a tiny entry, check the policy against the bytes of the
// file, and open the file in Chrome from a server that sends no CSP header, as GitHub Pages does.
import test from "node:test";
import assert from "node:assert/strict";
import http from "node:http";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { launch } from "../tools/cdp.mjs";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
mkdirSync(path.join(root, "dist", "demo-csp"), { recursive: true });
writeFileSync(path.join(root, "dist", "demo-csp-entry.js"), 'document.body.dataset.ok = "1";');
const FILE = path.join(root, "dist", "demo-csp", "index.html");
const build = (...extra) => execFileSync(process.execPath, ["build.mjs", "demo", "--entry", "dist/demo-csp-entry.js", ...extra], { cwd: root, encoding: "utf8", stdio: "pipe" });
const sha = (text) => "'sha256-" + createHash("sha256").update(text, "utf8").digest("base64") + "'";

build("--publish", "--out", "dist/demo-csp/index.html");
const html = readFileSync(FILE, "utf8");
const meta = /<meta http-equiv="Content-Security-Policy" content="([^"]*)">/.exec(html);
const policy = Object.fromEntries((meta ? meta[1] : "").split(";").map((d) => d.trim()).filter(Boolean).map((d) => [d.split(/\s+/)[0], d.split(/\s+/).slice(1)]));

test("the published demo has no nonce and one meta CSP, before any script", () => {
  assert.ok(!html.includes("__NONCE__") && !/\snonce=/.test(html), "no nonces");
  assert.equal(html.split('http-equiv="Content-Security-Policy"').length - 1, 1);
  assert.ok(html.indexOf("Content-Security-Policy") < html.indexOf("<script"), "the policy comes first");
  assert.ok(html.indexOf("<meta charset") < html.indexOf("Content-Security-Policy"), "after the charset, which must be in the first 1024 bytes");
});

test("the policy: nothing by default, no network, no frames, no base or form, Trusted Types for scripts, and no 'unsafe' source anywhere", () => {
  assert.deepEqual(policy["default-src"], ["'none'"]);
  assert.deepEqual(policy["connect-src"], ["'none'"]);
  assert.deepEqual(policy["base-uri"], ["'none'"]);
  assert.deepEqual(policy["form-action"], ["'none'"]);
  assert.deepEqual(policy["require-trusted-types-for"], ["'script'"]);
  assert.ok(!("frame-ancestors" in policy), "frame-ancestors does not work in a meta: it would be a line that looks like a protection and is not");
  assert.ok(!/unsafe-/.test(meta[1]), "no 'unsafe-inline', no 'unsafe-eval'");
  assert.ok(!/https?:|\*/.test(meta[1]), "no host and no wildcard");
});

test("script-src names the SHA-256 of every inline script of the file and nothing else; style-src that of its one style, and nothing else (the demo makes no style while it runs)", () => {
  const scripts = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)].map((m) => sha(m[1]));
  const styles = [...html.matchAll(/<style>([\s\S]*?)<\/style>/g)].map((m) => sha(m[1]));
  assert.equal(scripts.length, 3, "the head script, the demo and the page");
  assert.equal(styles.length, 1);
  assert.deepEqual([...policy["script-src"]].sort(), [...scripts].sort());
  assert.deepEqual(policy["style-src"], styles, "the style of the page and nothing else: the demo's line about itself is drawn by HOY, with the page's own style");
  assert.equal(html.split("<script").length - 1, 3, "every script tag is one of those");
});

test("the demo that runs under the dev server (nonces, header CSP) carries no meta: the server's policy is the policy", () => {
  build("--out", "dist/demo-csp/dev.html");
  const dev = readFileSync(path.join(root, "dist", "demo-csp", "dev.html"), "utf8");
  assert.ok(!dev.includes("Content-Security-Policy"));
  assert.ok(dev.includes('<script nonce="__NONCE__">'));
});

test("in Chrome, from a server that sends no policy header: the page runs, a fetch is refused by the meta, and nothing else complains", async () => {
  const server = http.createServer((req, res) => { res.writeHead(200, { "content-type": "text/html; charset=utf-8" }); res.end(readFileSync(FILE)); });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const browser = await launch({ width: 1000, height: 700 });
  try {
    const page = await browser.newPage();
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    assert.equal(await page.eval(`document.body.dataset.ok`), "1", "the three inline scripts ran: their hashes are right");
    assert.deepEqual(page.problems(), []);
    const refused = await page.eval(`fetch(location.href).then(() => "went through", (e) => "refused: " + e.name)`);
    assert.match(refused, /^refused/, "connect-src 'none' stops a fetch to the page's own address");
    await page.wait(200);
    assert.ok(page.problems().some((p) => /connect-src/.test(p.text)), `and the browser says it was the policy (${JSON.stringify(page.problems())})`);
  } finally { await browser.close(); server.close(); }
});
