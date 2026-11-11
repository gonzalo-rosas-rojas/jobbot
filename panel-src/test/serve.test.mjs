import test from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync, rmSync } from "node:fs";
import { startServer, csp } from "../tools/serve.mjs";

const PAGE = new URL("../dist/serve-test.html", import.meta.url);
mkdirSync(new URL("../dist/", import.meta.url), { recursive: true });
writeFileSync(PAGE, '<style nonce="__NONCE__">a{background:url("__ASSETS__/mark.svg")}</style><p>ok</p>');
const srv = await startServer(0);
test.after(() => { srv.close(); rmSync(PAGE, { force: true }); });
const get = (p) => fetch(`http://127.0.0.1:${srv.port}${p}`);

test("the policy is the one of §9.6, character for character, and the page carries the nonce and the prefix", async () => {
  const r = await get("/dist/serve-test.html");
  const policy = r.headers.get("content-security-policy");
  const nonce = /'nonce-([0-9a-f]+)'/.exec(policy)[1];
  assert.equal(policy, csp(nonce));
  assert.equal(policy, `default-src 'none'; script-src 'nonce-${nonce}'; style-src 'nonce-${nonce}'; connect-src 'self'; img-src 'self' blob:; font-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; require-trusted-types-for 'script'`);
  assert.equal(r.headers.get("x-frame-options"), "DENY");
  assert.equal(r.headers.get("x-content-type-options"), "nosniff");
  const body = await r.text();
  assert.ok(body.includes(`nonce="${nonce}"`) && !body.includes("__NONCE__"));
  assert.ok(body.includes(`url("/a/${srv.launchId}/mark.svg")`) && !body.includes("__ASSETS__"));
  const other = await (await get("/dist/serve-test.html")).headers.get("content-security-policy");
  assert.notEqual(other, policy, "a new nonce at every load");
});

test("a font is served under this launch with its type and an immutable cache; anything else is a 404", async () => {
  const ok = await get(`/a/${srv.launchId}/barlow-condensed-700.woff2`);
  assert.equal(ok.status, 200);
  assert.equal(ok.headers.get("content-type"), "font/woff2");
  assert.equal(ok.headers.get("cache-control"), "public, max-age=31536000, immutable");
  assert.equal(ok.headers.get("x-content-type-options"), "nosniff");
  assert.equal(ok.headers.get("cross-origin-resource-policy"), "same-origin");
  assert.equal((await ok.arrayBuffer()).byteLength > 1000, true);
  for (const bad of [`/a/0000000000000000/barlow-condensed-700.woff2`, `/a/${srv.launchId}/nope.woff2`, `/a/${srv.launchId}/..%2Fpackage.json`, `/a/${srv.launchId}/BARLOW-CONDENSED-700.woff2`, `/a/${srv.launchId}/sub/inter-400.woff2`, "/package.json", "/guard.mjs", "/dist/..%2Fpackage.json"]) {
    assert.equal((await get(bad)).status, 404, bad);
  }
});

test("/api answers 404 on purpose (the labs must not need the Rust server) and /favicon.ico answers 204 like the real one", async () => {
  assert.equal((await get("/api/snapshot")).status, 404);
  assert.equal((await get("/favicon.ico")).status, 204);
});

// ---- review M6: a request to a directory used to kill the process (readFileSync on a directory throws EISDIR), and the Host was not checked ----

import http from "node:http";
import net from "node:net";
import { mkdirSync as mkdir } from "node:fs";

const withHost = (path, host) => new Promise((resolve, reject) => {
  const req = http.request({ host: "127.0.0.1", port: srv.port, path, method: "GET", headers: host === null ? {} : { host } }, (res) => {
    res.resume();
    res.on("end", () => resolve(res.statusCode));
  });
  req.on("error", reject);
  req.end();
});

test("M6: a request to a directory is a 404 and the server stays up for the next one", async () => {
  mkdir(new URL("../dist/serve-test-dir/", import.meta.url), { recursive: true });
  for (const path of ["/dist/serve-test-dir", "/dist/serve-test-dir/", "/dist/", "/dist"]) assert.equal((await get(path)).status, 404, path);
  assert.equal((await get("/dist/serve-test.html")).status, 200, "and the server still answers");
  rmSync(new URL("../dist/serve-test-dir/", import.meta.url), { recursive: true, force: true });
});

test("M6: only 127.0.0.1 and localhost with this server's own port are accepted as Host (DNS rebinding), as the Rust server does", async () => {
  for (const host of [`127.0.0.1:${srv.port}`, `localhost:${srv.port}`, `LOCALHOST:${srv.port}`]) assert.equal(await withHost("/dist/serve-test.html", host), 200, host);
  for (const host of ["evil.example", "evil.example:80", `evil.example:${srv.port}`, `127.0.0.1.evil.example:${srv.port}`, `localhost.evil.com:${srv.port}`, "127.0.0.1", "localhost", `127.0.0.1:${srv.port + 1}`, `[::1]:${srv.port}`]) {
    assert.equal(await withHost("/dist/serve-test.html", host), 403, JSON.stringify(host));
  }
  assert.equal(await withHost(`/a/${srv.launchId}/barlow-condensed-700.woff2`, "evil.example"), 403, "the fonts too");
  assert.equal(await withHost("/favicon.ico", "evil.example"), 403, "and the favicon");
  const raw = await new Promise((resolve, reject) => {                       // HTTP/1.0 with no Host at all: the http client always adds one
    const sock = net.connect(srv.port, "127.0.0.1", () => sock.write("GET /dist/serve-test.html HTTP/1.0" + String.fromCharCode(13, 10, 13, 10)));
    let data = "";
    sock.on("data", (d) => { data += d; });
    sock.on("end", () => resolve(data));
    sock.on("error", reject);
  });
  assert.match(raw, /^HTTP\/1\.[01] 403 /, "no Host is a refusal too");
  assert.equal(await withHost("/dist/serve-test.html", `127.0.0.1:${srv.port}`), 200, "still up");
});
