// What only a page shows of the encargo docs/encargos/boton-de-accion-unico.md: the one action button looks the same wherever it is. The primary button of
// HOY, COLA, APRENDER, SISTEMA and the drawer has the same computed size, weight, height, decoration, border and background, on a desk and on a phone; in
// the drawer every button is that button, and a link is what is underlined (with its exit icon), never a button. ui.act.test.mjs guards the code.
import test from "node:test";
import assert from "node:assert/strict";
import { openPage } from "./helpers/page.mjs";

const ev = (t, expr) => t.page.eval(expr);
// the computed look that makes a button a button (colour is left out: the drawer, the stops and Papel each have their ink)
const LOOK = `(el) => { const c = getComputedStyle(el), l = el.querySelector(".act-label");
  return { fontSize: c.fontSize, fontWeight: c.fontWeight, minHeight: c.minHeight, decoration: c.textDecorationLine, border: c.borderTopStyle + " " + c.borderTopWidth,
    background: c.backgroundColor, label: l ? getComputedStyle(l).fontWeight : null }; }`;
const visible = `(e) => e.getClientRects().length > 0`;

async function primaries(t) {
  const out = {};
  for (const [stop, selector] of [["hoy", "#app .hoy .act.primary"], ["cola", "#app .cola .act.primary"], ["aprender", "#app .aprender .act.primary"], ["sistema", '#app .sistema [data-action="engine"]']]) {
    await ev(t, `window.__app.go(${JSON.stringify(stop)})`);
    await t.page.waitFor(`document.getElementById("app").dataset.moment === ${JSON.stringify(stop)} && [...document.querySelectorAll(${JSON.stringify(selector)})].some(${visible})`);
    out[stop] = await ev(t, `(${LOOK})([...document.querySelectorAll(${JSON.stringify(selector)})].find(${visible}))`);
  }
  await ev(t, `window.__app.go("hoy")`);
  await t.page.waitFor(`document.getElementById("app").dataset.moment === "hoy" && !!document.querySelector("#app [data-card]")`);
  await ev(t, `__app.drawer.open(document.querySelector("#app [data-card]").dataset.card)`);
  await t.page.waitFor(`document.getElementById("detail").classList.contains("open") && !!document.querySelector("#detail .d-actions button")`);
  const drawer = await ev(t, `(() => { const D = document.getElementById("detail"), look = ${LOOK}, first = D.querySelector(".d-actions button"), a = [...D.querySelectorAll("a[target=_blank]")].find(${visible});
    return { primary: look(first), isAct: first.matches("button.act.primary"), notAct: [...D.querySelectorAll("button")].filter((b) => !b.matches(".act")).map((b) => b.className || b.textContent.trim()),
      buttons: [...new Set([...D.querySelectorAll("button.act, button")].filter(${visible}).map((b) => getComputedStyle(b).textDecorationLine))],
      link: a ? { decoration: getComputedStyle(a).textDecorationLine, icon: !!a.querySelector('use[href="#i-external-link"]') } : null }; })()`);
  return { stops: out, drawer };
}

function judge(seen) {
  const { stops, drawer } = seen;
  for (const stop of ["cola", "aprender", "sistema"]) assert.deepEqual(stops[stop], stops.hoy, `${stop}'s primary button looks like HOY's`);
  assert.deepEqual(drawer.primary, stops.hoy, "the drawer's primary button looks like the stops'");
  assert.ok(drawer.isAct, "the drawer's primary button is the panel's action button (button.act.primary)");
  assert.deepEqual(drawer.notAct, [], "every button of the drawer is the action button");
  assert.ok(drawer.link, "the drawer has a link that opens another tab");
  assert.match(drawer.link.decoration, /underline/, "a link is underlined");
  assert.ok(drawer.link.icon, "and carries the exit icon");
  assert.deepEqual(drawer.buttons, ["none"], "no button of the drawer is underlined: a button never looks like a link");
}

test("on a desk, the primary button of HOY, COLA, APRENDER, SISTEMA and the drawer look the same, and in the drawer only a link is underlined", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: 1440, height: 900 });
  try {
    judge(await primaries(t));
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("on a phone, the same: one look for every primary button, at least 44 px tall (review B7) and with no key shown", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=en", width: 390, height: 844 });
  try {
    const seen = await primaries(t);
    judge(seen);
    assert.ok(parseFloat(seen.stops.hoy.minHeight) >= 44, `a finger needs the height: ${seen.stops.hoy.minHeight}`);
    assert.equal(await ev(t, `[...document.querySelectorAll("#detail button.act .k, #app button.act .k")].filter(${visible}).length`), 0, "no key on a phone");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});
