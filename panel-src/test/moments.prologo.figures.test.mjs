// The four figures of the prologue each land under their own name (data-fig), whatever order the view lists them in.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
const { setLang, setTimeZone } = await import("../src/i18n/index.js");
const { model } = await import("../src/core/model.js");
const prologoModule = await import("../src/moments/prologo.js");
const prologo = prologoModule.default;
const { prologoView } = await import("../src/moments/prologo.js");

const snap = JSON.parse(readFileSync(new URL("./fixtures/snapshot.rich.json", import.meta.url), "utf8"));
const nowMs = Date.parse(snap.generated_at);

test("each figure of the view is drawn under the data-fig of its own id", () => {
  setLang("es");
  setTimeZone("America/Santiago");
  dom.reset();
  const host = dom.document.createElement("div");
  dom.document.body.append(host);
  const m = model(snap);
  prologo.mount(host, {});
  try {
    prologo.update(m, { now: () => nowMs });
    const view = prologoView(m, { nowMs });
    for (const fig of view.figures) {
      const node = host.querySelector(`[data-fig="${fig.id}"]`);
      assert.ok(node, fig.id + " has its place");
      assert.equal(node.querySelector(".fig-n").textContent, String(fig.n), fig.id + " shows its own number");
      assert.equal(node.querySelector(".fig-l").textContent, fig.label, fig.id + " shows its own label");
      assert.equal(node.querySelector(".fig-n").dataset.gold, fig.gold ? "true" : undefined, fig.id + " is gold only when the view says so");
    }
  } finally { prologo.unmount(); }
});

test("figures listed in another order, or with an id the page has no place for, still land under their own data-fig", () => {
  dom.reset();
  const dl = dom.document.createElement("dl");
  const { createFigures } = prologoModule;
  createFigures(dl).paint([
    { id: "awaiting", n: 4, label: "D", gold: true },
    { id: "ghost", n: 99, label: "nope" },
    { id: "read", n: 1, label: "A" },
    { id: "complete", n: 3, label: "C" },
    { id: "above", n: 2, label: "B" },
  ]);
  const at = (id) => dl.querySelector(`[data-fig="${id}"]`);
  assert.deepEqual(["read", "above", "complete", "awaiting"].map((id) => at(id).querySelector(".fig-n").textContent), ["1", "2", "3", "4"]);
  assert.deepEqual(["read", "above", "complete", "awaiting"].map((id) => at(id).querySelector(".fig-l").textContent), ["A", "B", "C", "D"]);
  assert.equal(at("awaiting").querySelector(".fig-n").dataset.gold, "true");
  assert.equal(at("read").querySelector(".fig-n").dataset.gold, undefined);
  assert.equal(dl.querySelector('[data-fig="ghost"]'), null);
});
