// What only a browser sees of the demo's line about itself (docs/encargos/nota-del-demo-en-hoy.md): as the showcase opens it (no hash, the
// demo lands on HOY), the line, the note and the two links are on screen at a desktop and a phone size, in es and en, nothing paints over them, the
// keyboard reaches the links, the key l says them in the other language, and on a phone the drawer covers the note as it covers the sheet.
import test from "node:test";
import assert from "node:assert/strict";
import { openPage } from "./helpers/page.mjs";
import { press } from "./helpers/acceptance.mjs";

const ev = (t, expr) => t.page.eval(expr);
const REPO = "https://github.com/gonzalo-rosas-rojas/jobbot";
const SHOWCASE = "https://gonzalo-rosas-rojas.github.io/jobbot-showcase/";
const LINE = {
  es: "jobbot busca avisos, adapta el CV y llena el formulario de noche; tú apruebas cada envío.",
  en: "jobbot finds postings, tailors the CV and fills the form overnight; you approve every send.",
};
const WORDS = { es: ["Código", "Cómo funciona"], en: ["Code", "How it works"] };

/** The note as drawn: its box, whether each of its links is the element the page paints at its centre, and where Tab puts them. */
const LOOK = `(() => {
  const note = document.querySelector("#app .hoy > :first-child");
  if (!note) return null;
  const box = (el) => { const r = el.getBoundingClientRect(); return { top: r.top, bottom: r.bottom, left: r.left, right: r.right }; };
  const links = [...note.querySelectorAll("a")].map((a) => { const r = a.getBoundingClientRect(); const x = r.left + r.width / 2, y = r.top + r.height / 2;
    const hit = document.elementFromPoint(x, y); return { href: a.getAttribute("href"), text: a.textContent.trim(), target: a.target, rel: a.rel, on: !!hit && (hit === a || a.contains(hit)), ...box(a) }; });
  const cs = getComputedStyle(note);
  return { text: note.textContent.replace(/\\s+/g, " ").trim(), box: box(note), position: cs.position, visible: note.getClientRects().length > 0 && cs.visibility !== "hidden" && parseFloat(cs.opacity) > 0.5,
    links, vw: innerWidth, vh: innerHeight, hscroll: document.documentElement.scrollWidth - innerWidth };
})()`;

for (const [width, height] of [[1440, 900], [1280, 800], [390, 844]]) {
  for (const lang of ["es", "en"]) {
    test(`${width}x${height} ${lang}: as the showcase opens the demo, the line, the note and both links are on screen in HOY's sheet and nothing covers them`, async () => {
      const t = await openPage({ query: `?scenario=urgent&theme=dark&lang=${lang}&debug=1`, width, height });
      try {
        await t.page.waitFor(`document.getElementById("app").dataset.moment === "hoy" && !!document.querySelector("#app .hoy > :first-child a")`);
        await t.page.wait(900);                                    // the camera has come to rest on HOY
        const v = await ev(t, LOOK);
        assert.ok(v, "HOY has a first element");
        assert.ok(v.text.startsWith(LINE[lang]), `it starts with the line: «${v.text}»`);
        assert.ok(v.visible, "it is painted");
        assert.equal(v.position, "static", "in the flow of the sheet");
        assert.ok(v.box.top >= 0 && v.box.bottom <= v.vh && v.box.left >= 0 && v.box.right <= v.vw, `inside the window: ${JSON.stringify(v.box)} in ${v.vw}x${v.vh}`);
        assert.deepEqual(v.links.map((l) => [l.href, l.text, l.target]), [[REPO, WORDS[lang][0], "_blank"], [SHOWCASE, WORDS[lang][1], "_blank"]]);
        for (const l of v.links) {
          assert.ok(l.rel.split(/\s+/).includes("noopener"), `${l.href}: rel noopener`);
          assert.ok(l.on, `${l.href}: the page paints the link itself at its centre, nothing over it`);
          assert.ok(l.top >= 0 && l.bottom <= v.vh, `${l.href}: on screen`);
        }
        assert.equal(v.hscroll, 0, "no sideways scroll");
        assert.deepEqual(t.page.problems(), []);
      } finally { await t.close(); }
    });
  }
}

test("1440x900: Tab reaches the two links, in their order, and the key l says the line and the links in English", async () => {
  const t = await openPage({ query: "?scenario=urgent&lang=es&debug=1", width: 1440, height: 900 });
  try {
    await t.page.waitFor(`!!document.querySelector("#app .hoy > :first-child a")`);
    await t.page.wait(900);
    const order = await ev(t, `(() => { const all = [...document.querySelectorAll("a[href], button:not([disabled]), [tabindex]:not([tabindex='-1'])")].filter((el) => !el.closest("[inert]") && el.getClientRects().length);
      const note = [...document.querySelectorAll("#app .hoy > :first-child a")]; return note.map((a) => all.indexOf(a)); })()`);
    assert.ok(order.every((i) => i >= 0), `both links are in the order of Tab: ${order}`);
    assert.ok(order[0] < order[1], "the code before the showcase");
    await press(t, "l");
    await t.page.waitFor(`document.documentElement.lang === "en" && (document.querySelector("#app .hoy > :first-child") || {}).textContent?.includes(${JSON.stringify(LINE.en)})`);
    const v = await ev(t, LOOK);
    assert.deepEqual(v.links.map((l) => l.text), WORDS.en, "the links in English too");
    assert.ok(!v.text.includes(LINE.es), "and no Spanish left");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("390x844: the drawer, opened, covers the note like the rest of the sheet, and no badge is anywhere", async () => {
  const t = await openPage({ query: "?scenario=urgent&lang=es&debug=1", width: 390, height: 844 });
  try {
    await t.page.waitFor(`!!document.querySelector("#app .hoy > :first-child a") && !!document.querySelector("#app [data-card]")`);
    assert.equal(await ev(t, `document.querySelectorAll(".demo-badge").length`), 0, "no badge");
    const key = await ev(t, `document.querySelector("#app [data-card]").dataset.card`);
    await ev(t, `__app.drawer.open(${JSON.stringify(key)})`);
    await t.page.waitFor(`document.getElementById("detail").classList.contains("open") && getComputedStyle(document.getElementById("detail")).opacity === "1"`);
    await t.page.wait(400);
    const hits = await ev(t, `[...document.querySelectorAll("#app .hoy > :first-child a")].map((a) => { const r = a.getBoundingClientRect(); const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2); return !!hit && !!hit.closest("#detail"); })`);
    assert.deepEqual(hits, [true, true], "where the links are, the drawer is what the page paints");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the real panel (no __DEMO__): HOY has no line, no note and no link to the code or the showcase", async () => {
  const { fixture, answerWith } = await import("./helpers/acceptance.mjs");
  const t = await openPage({ kind: "real", query: "?t=tok&debug=1&clock=9", width: 1440, height: 900, answer: answerWith(fixture("rich")) });
  try {
    const v = await ev(t, `({ text: document.body.textContent, links: [...document.querySelectorAll("a")].map((a) => a.getAttribute("href")) })`);
    for (const w of [...Object.values(LINE), "Demo con datos ficticios", "Demo with fictional data"]) assert.ok(!v.text.includes(w), `the real page says «${w}»`);
    assert.ok(!v.links.includes(REPO) && !v.links.includes(SHOWCASE), "and links to neither");
  } finally { await t.close(); }
});
