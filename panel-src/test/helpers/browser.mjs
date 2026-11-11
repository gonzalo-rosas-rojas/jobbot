// One call to build a page or lab, serve it under the real CSP on a free port and open it in a private Chrome (tools/cdp.mjs).
//   const t = await openBuilt({ args: ["lab", "f0-stage"], file: "dist/lab-f0-stage.html", query: "?clock=9", width: 1920, height: 1080 });
//   ...  t.page.eval(...)  ...  await t.close();
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { launch } from "../../tools/cdp.mjs";
import { startServer } from "../../tools/serve.mjs";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

/**
 * args:  what follows `node build.mjs` (["lab", "f0-stage"], ["demo", "--out", "dist/x.html"], ["--entry", "dist/e.js", "--out", "dist/x.html"]).
 * file:  the built file to open, relative to panel-src ("dist/lab-f0-stage.html"). media: { reducedMotion, dark }.
 */
export async function openBuilt({ args = [], file, query = "", width = 1920, height = 1080, media = null } = {}) {
  try {
    execFileSync(process.execPath, ["build.mjs", ...args], { cwd: root, stdio: "pipe" });
  } catch (e) {
    throw new Error(`node build.mjs ${args.join(" ")} failed:\n${e.stderr || e.stdout || e.message}`);
  }
  const srv = await startServer(0);
  let browser;
  try {
    browser = await launch({ width, height });
    const page = await browser.newPage();
    if (media) await page.media(media);
    const url = `http://127.0.0.1:${srv.port}/${file.replace(/\\/g, "/")}${query}`;
    await page.goto(url);
    return { page, browser, url, port: srv.port, launchId: srv.launchId, close: async () => { await browser.close(); srv.close(); } };
  } catch (e) {
    await browser?.close();
    srv.close();
    throw e;
  }
}
