// Runs scripts/demo/demo.js outside a browser, in a vm with the few globals it touches, so a Node test can ask the demo's
// in-memory "server" for a snapshot. Each api() call waits ~90 ms, as in the page.
import vm from "node:vm";
import { readFileSync } from "node:fs";

export const DEMO_SOURCE = readFileSync(new URL("../../../scripts/demo/demo.js", import.meta.url), "utf8");

export function loadDemo(opts = {}) {
  return loadDemoPage(opts).demo;
}

/** The demo and the <html> element it reads its language from: setting `page.lang` is the language toggle of the page. */
export function loadDemoPage({ search = "", lang = "es" } = {}) {
  const win = {};
  const page = { lang };
  const context = vm.createContext({
    window: win, URL, URLSearchParams, setTimeout, clearTimeout, console,
    location: { search, href: "http://127.0.0.1:47213/" + search },
    document: {
      currentScript: { nonce: "test" }, documentElement: page, addEventListener() {}, head: { append() {} }, body: { prepend() {} },
      createElement: () => ({ getContext: () => ({}), setAttribute() {} }),
    },
    MutationObserver: class { observe() {} },
    localStorage: { getItem: () => null, setItem() {} },
  });
  vm.runInContext(DEMO_SOURCE, context, { filename: "demo.js" });
  return { demo: win.__DEMO__, page };
}

export const snapshotOf = async (opts) => (await loadDemo(opts).api("/api/snapshot")).data;
