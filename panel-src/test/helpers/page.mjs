// The whole page, in a private Chrome, under the real CSP (spec A1, A2, A3, A10, A14). Two builds of the same page, each made ONCE per test
// process:
//   demo   `node build.mjs demo`: the page with the made-up server of scripts/demo/demo.js (?scenario=calm|urgent|stop|empty|heavy...), so
//          every action answers in memory and the page can be driven for real.
//   real   the same page with the demo's script cut out, so it talks to /api/* like the one Rust serves. The test answers those requests
//          itself (Fetch interception) with the fixtures and records what the page asked.
// The demo build is used for both because the strict build of the page is held to the byte budgets of the guard (A6), and a test of
// behaviour should not depend on how heavy the dictionaries are.
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { launch } from "../../tools/cdp.mjs";
import { startServer } from "../../tools/serve.mjs";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const built = new Map();
const made = [];                                                  // the files of this process under dist/, gone when it ends
process.once("exit", () => { for (const f of made) { try { rmSync(path.join(root, f), { force: true }); } catch { /* a leftover in dist/ is harmless */ } } });

/** Builds the page once; returns the name of the file under dist/ ("demo" or "real"). */
export function buildPage(kind = "demo") {
  if (built.has(kind)) return built.get(kind);
  mkdirSync(path.join(root, "dist"), { recursive: true });
  const demoFile = `dist/t21-demo-${process.pid}.html`;
  try {
    execFileSync(process.execPath, ["build.mjs", "demo", "--out", demoFile], { cwd: root, stdio: "pipe" });
  } catch (e) {
    throw new Error(`node build.mjs demo failed:\n${e.stderr || e.stdout || e.message}`);
  }
  made.push(demoFile);
  let file = demoFile;
  if (kind === "real") {
    const marker = '<script nonce="__NONCE__">';
    const parts = readFileSync(path.join(root, demoFile), "utf8").split(marker);
    if (parts.length !== 4) throw new Error(`the demo page has ${parts.length - 1} scripts, expected 3`);
    file = `dist/t21-real-${process.pid}.html`;
    made.push(file);
    writeFileSync(path.join(root, file), parts[0] + marker + parts[1] + marker + parts[3]);   // the middle one is the demo's server
  }
  built.set(kind, file);
  return file;
}

/** Fetch answers for the real page: `answer(path, init) -> { status, body } | undefined`; whatever it does not answer is a 404. */
async function intercept(page, answer, log) {
  await page.send("Fetch.enable", { patterns: [{ urlPattern: "*/api/*" }] });
  page.ws.addEventListener("message", async (event) => {
    const msg = JSON.parse(event.data);
    if (msg.method !== "Fetch.requestPaused") return;
    const { requestId, request } = msg.params;
    const url = new URL(request.url);
    const body = request.postData ? request.postData : null;
    log.push({ path: url.pathname + url.search, method: request.method, token: request.headers["X-Panel-Token"] || request.headers["x-panel-token"] || null, body });
    const got = (await answer(url.pathname + url.search, { method: request.method, body })) || { status: 404, body: { message: "no" } };
    await page.send("Fetch.fulfillRequest", {
      requestId, responseCode: got.status || 200,
      responseHeaders: [{ name: "content-type", value: "application/json" }],
      body: Buffer.from(JSON.stringify(got.body)).toString("base64"),
    });
  });
}

/**
 * Opens the page in a window of `width` x `height`. `kind`: "demo" or "real" (then `answer` serves /api/*). `media`: { reducedMotion, dark }.
 * Returns { page, url, requests (every address the page asked for), api (what the page asked of /api/*), close() }.
 */
export async function openPage({ kind = "demo", query = "", width = 1920, height = 1080, media = null, answer = null, wait = true } = {}) {
  const file = buildPage(kind);
  const srv = await startServer(0);
  let browser;
  try {
    browser = await launch({ width, height });
    const page = await browser.newPage();
    const requests = [];
    const api = [];
    page.ws.addEventListener("message", (event) => {
      const msg = JSON.parse(event.data);
      if (msg.method === "Network.requestWillBeSent") requests.push(msg.params.request.url);
    });
    if (media) await page.media(media);
    if (answer) await intercept(page, answer, api);
    const url = `http://127.0.0.1:${srv.port}/${file}${query}`;
    await page.goto(url);
    if (wait) await page.waitFor(`!!document.querySelector("#app h1")`, { timeout: 10000 });
    return { page, url, port: srv.port, launchId: srv.launchId, requests, api, close: async () => { await browser.close(); srv.close(); } };
  } catch (e) {
    await browser?.close();
    srv.close();
    throw e;
  }
}

/** The mouse wheel at a point of the window (CDP). */
export const wheel = (page, x, y, deltaY) => page.send("Input.dispatchMouseEvent", { type: "mouseWheel", x, y, deltaX: 0, deltaY });

/** y of the page on every frame for `ms`: [[t, y], ...]. Start it BEFORE the action; read it with `page.eval("__trace")`. */
export const trace = (page, ms = 900) => page.eval(`window.__trace = []; (() => { const t0 = performance.now(); (function s() { __trace.push([Math.round(performance.now() - t0), Math.round(scrollY)]); if (performance.now() - t0 < ${ms}) requestAnimationFrame(s); })(); })()`);
