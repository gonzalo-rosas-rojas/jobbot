// Takes the four screenshots the README shows, from the panel's demo mode: made-up data, no server, nothing from the
// real database. Run it from anywhere:
//
//   node scripts/readme-shots.mjs [--out docs/img] [--theme dark|light] [--clock 10] [--scenario <name>]
//
// What it does: builds the published demo into a temporary folder (`node panel-src/build.mjs demo --publish`), serves
// that folder on 127.0.0.1 at a port the system picks, starts its own headless Chrome with a temporary profile and a
// debugging port the system picks, opens each stop of the panel in a 1440x900 window and saves a JPEG of it. It needs
// Node 22 or later (the global WebSocket and fetch) and Chrome or Chromium: set CHROME to its path when it is not in the
// usual place. It installs nothing and never touches the browser or the profile of the person running it.
//
// The shots: panel-today.jpg (HOY), panel-queue.jpg (COLA: the first proposal with how its score was made),
// panel-applications.jpg (PROCESOS) and panel-system.jpg (SISTEMA).

import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const argv = process.argv.slice(2);
const option = (name, fallback) => (argv.includes(name) ? argv[argv.indexOf(name) + 1] : fallback);

const OUT = path.resolve(option("--out", path.join(root, "docs", "img")));
const THEME = option("--theme", "light"); // the light sheet keeps its contrast when GitHub scales the image down
const CLOCK = option("--clock", "10");
const SCENARIO = option("--scenario", "");
const WIDTH = 1440;
const HEIGHT = 900;
const LIMIT = 300 * 1024; // a shot must stay under this many bytes
const QUALITIES = [82, 78, 74, 70]; // tried in turn until the file fits

// What a reader must never see in a shot: a value the page failed to fill.
const UNFILLED = /\b(?:null|undefined|NaN)\b|\[object Object\]/g;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function chromePath() {
  const candidates = [
    process.env.CHROME,
    "C:/Program Files/Google/Chrome/Application/chrome.exe",
    "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe",
    "/usr/bin/google-chrome",
    "/usr/bin/google-chrome-stable",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  ];
  const found = candidates.find((candidate) => candidate && existsSync(candidate));
  if (!found) throw new Error("no Chrome or Chromium found: set CHROME to its path");
  return found;
}

/** The published demo, built into `dir`: index.html and the fonts beside it. */
function buildDemo(dir) {
  execFileSync(process.execPath, ["build.mjs", "demo", "--publish", "--out", path.join(dir, "index.html")], {
    cwd: path.join(root, "panel-src"),
    stdio: "pipe",
  });
}

const TYPES = { ".html": "text/html; charset=utf-8", ".woff2": "font/woff2", ".txt": "text/plain; charset=utf-8", ".svg": "image/svg+xml" };

/** A static server for `dir` on 127.0.0.1, port chosen by the system. */
function serve(dir) {
  const server = http.createServer((req, res) => {
    const name = decodeURIComponent(new URL(req.url, "http://127.0.0.1").pathname).replace(/^\/+/, "") || "index.html";
    const file = path.resolve(dir, name);
    if (!file.startsWith(dir + path.sep) || !existsSync(file) || !statSync(file).isFile()) {
      res.writeHead(404, { "content-type": "text/plain" });
      return res.end("not found");
    }
    res.writeHead(200, { "content-type": TYPES[path.extname(file)] || "application/octet-stream", "cache-control": "no-store" });
    res.end(readFileSync(file));
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

/** Starts Chrome with a temporary profile and returns { cdp(), close() } for its first page. */
async function startChrome(profile) {
  const child = spawn(
    chromePath(),
    [
      "--headless=new",
      "--remote-debugging-port=0",
      `--user-data-dir=${profile}`,
      `--window-size=${WIDTH},${HEIGHT}`,
      "--no-first-run",
      "--no-default-browser-check",
      "--disable-extensions",
      "--hide-scrollbars",
      "--force-color-profile=srgb",
      "--mute-audio",
      "--enable-unsafe-swiftshader",
      "about:blank",
    ],
    { stdio: "ignore" },
  );
  // Chrome writes the port it was given into the profile folder.
  const portFile = path.join(profile, "DevToolsActivePort");
  let port = 0;
  for (let i = 0; i < 150 && !port; i++) {
    if (existsSync(portFile)) port = Number(readFileSync(portFile, "utf8").split("\n")[0]);
    else await sleep(100);
  }
  if (!port) {
    child.kill();
    throw new Error("Chrome did not start");
  }
  const target = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json();
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    ws.onopen = resolve;
    ws.onerror = reject;
  });
  let id = 0;
  const waiting = new Map();
  const problems = [];
  ws.onmessage = (event) => {
    const message = JSON.parse(event.data);
    if (message.id && waiting.has(message.id)) {
      const { resolve, reject } = waiting.get(message.id);
      waiting.delete(message.id);
      if (message.error) reject(new Error(message.error.message));
      else resolve(message.result);
    } else if (message.method === "Runtime.exceptionThrown") {
      problems.push(message.params.exceptionDetails.exception?.description || message.params.exceptionDetails.text);
    } else if (message.method === "Runtime.consoleAPICalled" && message.params.type === "error") {
      problems.push(message.params.args.map((arg) => arg.value ?? arg.description ?? "").join(" "));
    }
  };
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const n = ++id;
      waiting.set(n, { resolve, reject });
      ws.send(JSON.stringify({ id: n, method, params }));
    });
  await send("Page.enable");
  await send("Runtime.enable");
  await send("Emulation.setDeviceMetricsOverride", { width: WIDTH, height: HEIGHT, deviceScaleFactor: 1, mobile: false });
  await send("Emulation.setEmulatedMedia", {
    features: [{ name: "prefers-reduced-motion", value: "no-preference" }],
  });
  return {
    problems,
    send,
    async evaluate(expression) {
      const result = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
      if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
      return result.result.value;
    },
    async waitFor(expression, timeout = 15000) {
      const end = Date.now() + timeout;
      while (Date.now() < end) {
        if (await this.evaluate(expression)) return;
        await sleep(100);
      }
      throw new Error(`timed out waiting for ${expression}`);
    },
    close() {
      ws.close();
      child.kill();
    },
  };
}

/** One JPEG of the window as it is, under LIMIT bytes. */
async function shoot(chrome, file) {
  for (const quality of QUALITIES) {
    const { data } = await chrome.send("Page.captureScreenshot", { format: "jpeg", quality });
    const bytes = Buffer.from(data, "base64");
    if (bytes.length < LIMIT) {
      writeFileSync(file, bytes);
      return { quality, size: bytes.length };
    }
  }
  throw new Error(`${path.basename(file)} is over ${LIMIT} bytes even at quality ${QUALITIES.at(-1)}`);
}

const work = mkdtempSync(path.join(os.tmpdir(), "readme-shots-"));
let server;
let chrome;
try {
  const site = path.join(work, "site");
  buildDemo(site);
  server = await serve(site);
  const base = `http://127.0.0.1:${server.address().port}/index.html`;
  chrome = await startChrome(path.join(work, "profile"));
  mkdirSync(OUT, { recursive: true });

  const query = new URLSearchParams({ lang: "en", theme: THEME, clock: CLOCK });
  if (SCENARIO) query.set("scenario", SCENARIO);
  const open = async (hash) => {
    await chrome.send("Page.navigate", { url: `${base}?${query}${hash}` });
    await chrome.waitFor(`!!document.querySelector("#app h1")`);
    await chrome.evaluate("document.fonts.ready.then(() => true)");
    await sleep(2500); // the camera travels to the stop and the page settles
  };

  const shots = [
    ["panel-today.jpg", "#hoy"],
    ["panel-queue.jpg", "#cola"],
    ["panel-applications.jpg", "#procesos"],
    ["panel-system.jpg", "#sistema"],
  ];
  for (const [name, hash] of shots) {
    await open(hash);
    const unfilled = (await chrome.evaluate("document.body.innerText")).match(UNFILLED);
    if (unfilled) throw new Error(`${name} shows ${unfilled.join(", ")}`);
    const { quality, size } = await shoot(chrome, path.join(OUT, name));
    console.log(`${name}: ${(size / 1024).toFixed(0)} KB at quality ${quality}`);
  }
  if (chrome.problems.length) console.log(`the page reported:\n${chrome.problems.map((line) => ` - ${line}`).join("\n")}`);
} finally {
  chrome?.close();
  server?.close();
  // Chrome may hold a file for a moment after it is killed.
  await sleep(500);
  try {
    rmSync(work, { recursive: true, force: true });
  } catch {
    /* a temporary folder left behind is harmless */
  }
}
