// COLA's capture plane and the drawer share the same place: the drawer opens right of the sheet (spec §3.6) and shows the SAME capture of the
// dry run (its "Captura de la prueba en seco", with Ampliar), so the plane at the sheet's right steps back while the drawer is open instead
// of staying half covered (cut by the drawer, showing through its glass) and comes back, whole and without being fetched again, when the
// drawer closes. This runs in the REAL shell over the demo's fictional data, not in the lab: the marker is the drawer's own (`#detail.open`).
import test from "node:test";
import assert from "node:assert/strict";
import { openBuilt } from "./helpers/browser.mjs";

const PAGE = "dist/demo-cola-drawer.html";
const ev = (t, expr) => t.page.eval(expr);

async function openCola(width, height) {
  const t = await openBuilt({ args: ["demo", "--out", PAGE], file: PAGE, width, height });
  await t.page.wait(2200);
  await t.page.key("3", { code: "Digit3", text: "3" });
  await t.page.waitFor(`document.querySelector(".cola .plane-wrap")?.dataset.state === "ok" && document.querySelector(".cola .plane img.shot")?.complete === true`, 8000);
  await t.page.wait(700); // the arrival of the sheet is done
  return t;
}

const plane = (t) => ev(t, `(() => {
  const wrap = document.querySelector(".cola .plane-wrap"), img = wrap && wrap.querySelector("img.shot"), cs = wrap && getComputedStyle(wrap), r = wrap && wrap.getBoundingClientRect();
  const drawer = document.querySelector("#detail"), d = drawer && drawer.getBoundingClientRect();
  return { display: cs && cs.display, visibility: cs && cs.visibility, opacity: cs && Number(cs.opacity), src: img ? img.src : null, loaded: img ? img.complete && img.naturalWidth > 0 : null,
    drawerOpen: !!drawer && drawer.classList.contains("open"), drawerLeft: d && Math.round(d.left), planeLeft: r && Math.round(r.left), planeRight: r && Math.round(r.right), viewW: innerWidth };
})()`);

async function round(width, height) {
  const t = await openCola(width, height);
  try {
    const before = await plane(t);
    assert.equal(before.display, "grid", `${width}: the plane is there at the right of the sheet`);
    assert.equal(before.visibility, "visible");
    assert.equal(before.opacity, 1);
    assert.equal(before.loaded, true, "with its capture");
    assert.equal(before.drawerOpen, false);
    assert.ok(before.planeRight <= before.viewW, `${width}: the plane is inside the window`);

    await t.page.key("Enter", { code: "Enter" });
    await t.page.waitFor(`document.querySelector("#detail")?.classList.contains("open")`, 6000);
    await t.page.wait(450); // the drawer's own slide is 220 ms
    const open = await plane(t);
    assert.equal(open.drawerOpen, true);
    assert.ok(open.drawerLeft < before.planeRight && open.planeLeft < open.drawerLeft + 460, `${width}: the drawer does sit over the plane's place (that is the case)`);
    assert.equal(open.visibility, "hidden", `${width}: the plane steps back while the drawer shows the same capture`);
    assert.equal(open.opacity, 0, `${width}: no half-covered plane through the glass`);
    assert.equal(open.src, before.src, "it is not thrown away, only put aside");

    await t.page.key("Escape", { code: "Escape" });
    await t.page.waitFor(`!document.querySelector("#detail")?.classList.contains("open")`, 6000);
    await t.page.wait(450);
    const after = await plane(t);
    assert.equal(after.drawerOpen, false);
    assert.equal(after.visibility, "visible", `${width}: the plane is back when the drawer closes`);
    assert.equal(after.opacity, 1);
    assert.equal(after.src, before.src, "the same capture, not fetched again");
    assert.equal(after.loaded, true);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
}

test("a wide screen (1920 x 1080): the plane steps back while the drawer is open and returns, the same capture, when it closes", () => round(1920, 1080));
test("an ultrawide (3440 x 1440): the same, with room for both it does not show the capture twice", () => round(3440, 1440));
test("a laptop (1440 x 900): the same, with the plane narrowed", () => round(1440, 900));

test("a laptop narrower than 1300 px has no plane to hide: the drawer opens over a sheet that never had one beside it", async () => {
  const t = await openBuilt({ args: ["demo", "--out", PAGE], file: PAGE, width: 1280, height: 800 });
  try {
    await t.page.wait(2200);
    await t.page.key("3", { code: "Digit3", text: "3" });
    await t.page.waitFor(`!!document.querySelector(".cola .plane-wrap")`, 8000);
    await t.page.key("Enter", { code: "Enter" });
    await t.page.waitFor(`document.querySelector("#detail")?.classList.contains("open")`, 6000);
    assert.equal((await plane(t)).display, "none");
    await t.page.key("Escape", { code: "Escape" });
    await t.page.waitFor(`!document.querySelector("#detail")?.classList.contains("open")`, 6000);
    assert.equal((await plane(t)).display, "none");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});
