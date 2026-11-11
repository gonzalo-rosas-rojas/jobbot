// Development server for panel-src/dist with the SAME headers the Rust panel sends (spec §9.5 and §9.6), so a CSP violation
// shows up while building and not later. Pages that contain __NONCE__ get a fresh nonce per request, and __ASSETS__ becomes
// /a/<launch-id>, exactly as crates/jobbot-panel/src/http.rs does.
//
//   node tools/serve.mjs [port]      default 8791, 127.0.0.1 only
//
// Routes: /dist/<file> (built pages and labs) and /a/<launch-id>/<name> (the fonts of assets/fonts and the files of
// ../crates/jobbot-panel/src/assets, by exact name). Everything else, /api/* included, is a 404.
import http from "node:http";
import { readFileSync, existsSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
import { randomBytes } from "node:crypto";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(here, "..");
const ASSET_DIRS = [path.join(root, "assets", "fonts"), path.join(root, "..", "crates", "jobbot-panel", "src", "assets")];
const TYPES = { ".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".json": "application/json", ".css": "text/css", ".png": "image/png", ".woff2": "font/woff2", ".svg": "image/svg+xml" };
const COMMON = { "x-content-type-options": "nosniff", "cross-origin-resource-policy": "same-origin", "referrer-policy": "no-referrer" };

export const csp = (nonce) => [
  "default-src 'none'", `script-src 'nonce-${nonce}'`, `style-src 'nonce-${nonce}'`, "connect-src 'self'", "img-src 'self' blob:",
  "font-src 'self'", "base-uri 'none'", "form-action 'none'", "frame-ancestors 'none'", "require-trusted-types-for 'script'",
].join("; ");

/** name -> absolute path of every file the page may name under /a/<id>/: this table is the whole of what can be fetched. */
function assetTable() {
  const table = new Map();
  for (const dir of ASSET_DIRS) {
    if (!existsSync(dir)) continue;
    for (const name of readdirSync(dir)) if (/\.(woff2|svg)$/.test(name)) table.set(name, path.join(dir, name));
  }
  return table;
}

function notFound(res) {
  res.writeHead(404, { "content-type": "text/plain" });
  res.end("not found");
}

// Like crates/jobbot-panel/src/http.rs: the Host must be this server's own socket by name (127.0.0.1 or localhost) and port, or any page on the
// web could reach it through a name of its own that resolves to 127.0.0.1 (DNS rebinding). No Host is a refusal too.
const hostOk = (host, port) => typeof host === "string" && [`127.0.0.1:${port}`, `localhost:${port}`].includes(host.toLowerCase());
const isFile = (file) => { try { return statSync(file).isFile(); } catch (e) { return false; } };   // a directory exists, and reading it throws

export function startServer(port = 0) {
  const launchId = randomBytes(8).toString("hex");
  const assets = assetTable();
  const server = http.createServer((req, res) => {
    // An exception in a handler must never take the process down: every test of every agent talks to this one server.
    try { handle(req, res); } catch (e) {
      if (!res.headersSent) res.writeHead(500, { "content-type": "text/plain" });
      res.end("error");
    }
  });
  function handle(req, res) {
    if (!hostOk(req.headers.host, server.address().port)) {
      res.writeHead(403, { "content-type": "text/plain" });
      return res.end("Host no permitido");
    }
    const url = new URL(req.url, "http://localhost");
    let rel;
    try { rel = decodeURIComponent(url.pathname).replace(/^\/+/, ""); } catch (e) { return notFound(res); }
    if (rel === "favicon.ico") { // the Rust server answers 204 here; a 404 would be logged as a console error now that img-src allows 'self'
      res.writeHead(204, COMMON);
      return res.end();
    }
    const asset = /^a\/([0-9a-f]{16})\/([a-z0-9._-]+)$/.exec(rel);
    if (asset) {
      const file = asset[1] === launchId ? assets.get(asset[2]) : null;
      if (!file) return notFound(res);
      res.writeHead(200, { ...COMMON, "content-type": TYPES[path.extname(file)], "cache-control": "public, max-age=31536000, immutable" });
      return res.end(readFileSync(file));
    }
    const file = path.normalize(path.join(root, rel));
    if (!rel.startsWith("dist/") || !file.startsWith(path.join(root, "dist") + path.sep) || !isFile(file)) return notFound(res);
    const type = TYPES[path.extname(file)] || "application/octet-stream";
    let body = readFileSync(file);
    const headers = { ...COMMON, "content-type": type, "cache-control": "no-store" };
    if (type.startsWith("text/html")) {
      const nonce = randomBytes(16).toString("hex");
      body = Buffer.from(body.toString("utf8").replaceAll("__NONCE__", nonce).replaceAll("__ASSETS__", `/a/${launchId}`));
      headers["content-security-policy"] = csp(nonce);
      headers["x-frame-options"] = "DENY";
    }
    res.writeHead(200, headers);
    res.end(body);
  }
  return new Promise((resolve) => server.listen(port, "127.0.0.1", () => resolve({ server, port: server.address().port, launchId, close: () => server.close() })));
}

export async function main(argv) {
  const srv = await startServer(Number(argv[0]) || 8791);
  console.log(`panel-src dev server on http://127.0.0.1:${srv.port}/dist/page.html (assets under /a/${srv.launchId}/)`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main(process.argv.slice(2));
