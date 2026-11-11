// The wiring of the encargo docs/encargos/lote-acciones-que-se-entienden.md that only a page shows: the shell tells the moments and the help whether the
// flat page (a phone) is in charge. On a phone HOY's hint over the rows and the help name no key; on a desk the help says how the page is used before
// its keys; and a window that crosses 760 px changes the hint without a reload. lote.acciones.test.mjs proves the words on the pure views.
import test from "node:test";
import assert from "node:assert/strict";
import { openPage } from "./helpers/page.mjs";

const ev = (t, expr) => t.page.eval(expr);
const KEY_WORDS = /\b(?:teclas?|atajos?|letras?)\b/i;
const hint = (t) => ev(t, `(document.querySelector(".hoy .rest-hint") || {}).textContent || ""`);
const onHoy = async (t) => {
  await ev(t, `window.__app.go("hoy")`);
  await t.page.waitFor(`document.getElementById("app").dataset.moment === "hoy" && !!document.querySelector(".hoy .rest-hint")?.textContent`);
};
const openHelp = async (t) => {
  await t.page.click(".bar-help");
  await t.page.waitFor(`!!document.querySelector(".help")`);
  return ev(t, `(() => { const box = document.querySelector(".help"); const usage = box.querySelector(".help-usage"); const group = box.querySelector(".help-group");
    return { usage: [...box.querySelectorAll(".help-usage li")].map((li) => li.textContent), keys: box.querySelectorAll("kbd").length,
      usageFirst: !!usage && (!group || !!(usage.compareDocumentPosition(group) & Node.DOCUMENT_POSITION_FOLLOWING)) }; })()`);
};

test("on a phone, HOY's hint over the rows and the help say how to use the page by touch and name no key", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: 390, height: 844 });
  try {
    await onHoy(t);
    const h = await hint(t);
    assert.doesNotMatch(h, KEY_WORDS, `the hint: "${h}"`);
    assert.match(h, /\btoca\b/i, `the hint says to tap: "${h}"`);
    const sheet = await openHelp(t);
    assert.equal(sheet.keys, 0, "the help of a phone lists no key");
    assert.ok(sheet.usage.length > 0 && sheet.usage.every((line) => !KEY_WORDS.test(line)), `it says how to use the page: ${JSON.stringify(sheet.usage)}`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("on a desk, the help says how the page is used before its keys, and a window that crosses 760 px changes HOY's hint without a reload", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: 1440, height: 900 });
  try {
    await onHoy(t);
    assert.match(await hint(t), /\bclic\b/i, "a desk is told what a click does");
    const sheet = await openHelp(t);
    assert.equal(sheet.usage.length, 3, `three lines of use: ${JSON.stringify(sheet.usage)}`);
    assert.ok(sheet.keys > 0, "then the keys");
    assert.ok(sheet.usageFirst, "the use comes before the keys");
    await t.page.key("Escape");
    await t.page.waitFor(`!document.querySelector(".help")`);
    await ev(t, `window.__kept = 7`);
    await t.page.viewport(390, 844);
    await t.page.waitFor(`window.__app.mode() === "flat"`);
    await t.page.waitFor(`/\\btoca\\b/i.test(document.querySelector(".hoy .rest-hint")?.textContent || "")`);
    assert.equal(await ev(t, `window.__kept`), 7, "the same page, not a reload");
    assert.doesNotMatch(await hint(t), KEY_WORDS);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});
