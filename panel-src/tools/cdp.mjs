// A tiny Chrome DevTools Protocol driver, so that every agent (and the Node tests) can open pages, run code in
// them, take screenshots, throttle the CPU and collect console messages and CSP violations WITHOUT sharing one
// browser. Each launch() starts its own headless Chrome with its own profile folder and debugging port, and
// close() kills it and deletes the folder. Node 22+ (global WebSocket and fetch); no npm packages.
//
//   import { launch } from "./tools/cdp.mjs";
//   const b = await launch({ width: 1440, height: 900 });
//   const page = await b.newPage();
//   await page.goto("http://127.0.0.1:8791/dist/lab-world.html");
//   console.log(await page.eval("document.title"));          // runs in the page, returns JSON-able values
//   await page.shot("out/world.png");
//   console.log(page.problems());                              // console errors, exceptions, CSP violations
//   await b.close();

import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync, mkdirSync } from "node:fs";
import os from "node:os";
import path from "node:path";

const CHROME = process.env.CHROME || "C:/Program Files/Google/Chrome/Application/chrome.exe";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export async function launch({ width = 1440, height = 900, args = [] } = {}) {
  const dir = mkdtempSync(path.join(os.tmpdir(), "panel-cdp-"));
  const port = 9300 + Math.floor(Math.random() * 600);
  const proc = spawn(
    CHROME,
    [
      "--headless=new",
      `--remote-debugging-port=${port}`,
      `--user-data-dir=${dir}`,
      `--window-size=${width},${height}`,
      "--no-first-run",
      "--no-default-browser-check",
      "--disable-extensions",
      "--hide-scrollbars",
      "--force-color-profile=srgb",
      "--mute-audio", // the sound engine's tests switch sound on; a headless Chrome would play it through the real speakers
      "--enable-unsafe-swiftshader", // WebGL still works on a machine without a usable GPU
      ...args,
      "about:blank",
    ],
    { stdio: "ignore" },
  );
  let version = null;
  for (let i = 0; i < 100 && !version; i++) {
    try {
      version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
    } catch {
      await sleep(100);
    }
  }
  if (!version) {
    proc.kill();
    throw new Error("Chrome did not start (set CHROME to its path)");
  }
  const pages = [];
  return {
    version,
    async newPage() {
      const target = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })).json();
      const page = await connect(target.webSocketDebuggerUrl, width, height);
      pages.push(page);
      return page;
    },
    async close() {
      for (const p of pages) p.ws.close();
      proc.kill();
      await sleep(300);
      try {
        rmSync(dir, { recursive: true, force: true });
      } catch {
        /* Chrome may still hold a file for a moment; the temp folder is harmless */
      }
    },
  };
}

async function connect(url, width, height) {
  const ws = new WebSocket(url);
  await new Promise((resolve, reject) => {
    ws.onopen = resolve;
    ws.onerror = reject;
  });
  let id = 0;
  const waiting = new Map();
  const listeners = new Map();
  const log = [];
  ws.onmessage = (event) => {
    const msg = JSON.parse(event.data);
    if (msg.id && waiting.has(msg.id)) {
      const { resolve, reject } = waiting.get(msg.id);
      waiting.delete(msg.id);
      msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result);
    } else if (msg.method) {
      for (const fn of listeners.get(msg.method) || []) fn(msg.params);
    }
  };
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const n = ++id;
      waiting.set(n, { resolve, reject });
      ws.send(JSON.stringify({ id: n, method, params }));
    });
  const on = (method, fn) => listeners.set(method, [...(listeners.get(method) || []), fn]);

  on("Runtime.consoleAPICalled", (p) => {
    const text = p.args.map((a) => a.value ?? a.description ?? "").join(" ");
    log.push({ kind: p.type, text });
  });
  on("Runtime.exceptionThrown", (p) => log.push({ kind: "exception", text: p.exceptionDetails.exception?.description || p.exceptionDetails.text }));
  on("Log.entryAdded", (p) => log.push({ kind: p.entry.level, text: p.entry.text + (p.entry.url ? " " + p.entry.url : "") }));
  await send("Page.enable");
  await send("Runtime.enable");
  await send("Log.enable");
  await send("Network.enable");
  await send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
  // A Content-Security-Policy violation is an event inside the page; this turns each one into a log line.
  await send("Page.addScriptToEvaluateOnNewDocument", {
    source: `document.addEventListener("securitypolicyviolation", function (e) { console.error("CSP violation: " + e.violatedDirective + " " + (e.blockedURI || "") + " " + (e.sample || "")); });`,
  });

  const page = {
    ws,
    send,
    log,
    /** Navigates and waits for the load event. */
    async goto(target, { timeout = 15000 } = {}) {
      const loaded = new Promise((resolve) => on("Page.loadEventFired", resolve));
      await send("Page.navigate", { url: target });
      await Promise.race([loaded, sleep(timeout)]);
      await sleep(50);
    },
    /** Evaluates an expression in the page (promises are awaited) and returns its value. Throws on errors. */
    async eval(expression) {
      const r = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true, userGesture: true });
      if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text);
      return r.result.value;
    },
    async shot(file, { fullPage = false } = {}) {
      mkdirSync(path.dirname(path.resolve(file)), { recursive: true });
      const r = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: fullPage });
      writeFileSync(file, Buffer.from(r.data, "base64"));
      return file;
    },
    async viewport(w, h, { dpr = 1, mobile = false } = {}) {
      await send("Emulation.setDeviceMetricsOverride", { width: w, height: h, deviceScaleFactor: dpr, mobile });
    },
    /** features: { reducedMotion: true, dark: true } */
    async media({ reducedMotion, dark } = {}) {
      const features = [];
      if (reducedMotion !== undefined) features.push({ name: "prefers-reduced-motion", value: reducedMotion ? "reduce" : "no-preference" });
      if (dark !== undefined) features.push({ name: "prefers-color-scheme", value: dark ? "dark" : "light" });
      await send("Emulation.setEmulatedMedia", { features });
    },
    /** rate 4 = a CPU four times slower. */
    async throttle(rate) {
      await send("Emulation.setCPUThrottlingRate", { rate });
    },
    async click(selector) {
      const box = await page.eval(
        `(() => { const el = document.querySelector(${JSON.stringify(selector)}); if (!el) return null; el.scrollIntoView({block:"center"}); const r = el.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`,
      );
      if (!box) throw new Error("no element for " + selector);
      for (const type of ["mouseMoved", "mousePressed", "mouseReleased"]) {
        await send("Input.dispatchMouseEvent", { type, x: box.x, y: box.y, button: "left", clickCount: 1 });
      }
    },
    async key(key, { code = key, text } = {}) {
      await send("Input.dispatchKeyEvent", { type: "keyDown", key, code, text });
      await send("Input.dispatchKeyEvent", { type: "keyUp", key, code });
    },
    async wait(ms) {
      await sleep(ms);
    },
    /** Polls a page expression until it is truthy. */
    async waitFor(expression, { timeout = 8000, every = 50 } = {}) {
      const end = Date.now() + timeout;
      while (Date.now() < end) {
        if (await page.eval(expression)) return true;
        await sleep(every);
      }
      throw new Error("timed out waiting for " + expression);
    },
    /** Console errors, uncaught exceptions and CSP violations seen so far. */
    problems() {
      return log.filter((l) => ["error", "exception"].includes(l.kind) || /CSP violation/.test(l.text));
    },
    async metrics() {
      const { metrics } = await send("Performance.getMetrics");
      return Object.fromEntries(metrics.map((m) => [m.name, m.value]));
    },
  };
  await send("Performance.enable");
  return page;
}
