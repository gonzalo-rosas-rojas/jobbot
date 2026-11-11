import test from "node:test";
import assert from "node:assert/strict";
import { openBuilt } from "./helpers/browser.mjs";

const open = (opts = {}) => openBuilt({ args: ["lab", "f0-hoy"], file: "dist/lab-f0-hoy.html", ...opts });
const ev = (t, expr) => t.page.eval(expr);
const NOON = "Date.parse('2026-10-07T12:00:00Z')";
// the fonts are binary assets that arrive after the first paint and move the rows by a pixel: measure only once they are in
const load = async (t, name = "rich") => {
  await ev(t, `window.__lab.reduced(true), window.__lab.load(${JSON.stringify(name)}, 9), window.__lab.now(${NOON}), window.__lab.visit(0)`);
  await t.page.waitFor(`document.fonts.status === "loaded"`);
};
// tinykeys reads event.code for a letter or a digit, so a real key needs its code: "a" is KeyA and "3" is Digit3. Enter carries its "\r" text:
// without it Chrome does not press the focused button nor submit the form, which is what a real Enter does.
const key = (t, k) => t.page.key(k, k.length === 1 ? { code: /[0-9]/.test(k) ? "Digit" + k : "Key" + k.toUpperCase(), text: k } : k === "Enter" ? { text: "\r" } : {});
const settled = (t) => t.page.waitFor(`document.querySelector(".menu") && document.querySelector(".menu").getAnimations().every((a) => a.playState === "finished")`);
const acts = (t) => ev(t, `window.__lab.state().acts`);
const headline = async (t) => (await ev(t, `document.querySelector(".hoy .mega").textContent`)).split(/\s+/).filter(Boolean).join(" ");
const menu = (t) => ev(t, `(() => { const m = document.querySelector(".menu"); if (!m) return null; return { title: m.querySelector(".menu-title").textContent, items: [...m.querySelectorAll(".menu-item")].map((b) => [b.dataset.key, b.querySelector(".menu-label").textContent]),
  local: m.hasAttribute("data-local-keys"), role: m.getAttribute("role"), focus: document.activeElement.dataset.key || document.activeElement.tagName }; })()`);

test("s asks until when beside the row, the menu owns the keyboard, and a digit chooses", async () => {
  const t = await open();
  try {
    await load(t);
    await ev(t, `window.__lab.cursorTo("c:fx:12")`);
    await key(t, "s");
    const m = await menu(t);
    assert.equal(m.title, "Posponer hasta cuándo");
    assert.deepEqual(m.items, [["1", "Mañana"], ["2", "En tres días"], ["3", "En una semana"], ["4", "En dos semanas"]]);
    assert.deepEqual([m.local, m.role, m.focus], [true, "menu", "1"], "a box with keys of its own, focus on the first item");
    await settled(t); // it fades in with a 4px slide: measure where it stays
    const box = await ev(t, `(() => { const r = document.querySelector(".menu").getBoundingClientRect(); const row = document.querySelector('[data-card="fx:12"]').getBoundingClientRect(); return { top: r.top, rowTop: row.top, rowBottom: row.bottom, inside: r.right <= innerWidth && r.bottom <= innerHeight && r.left >= 0 && r.top >= 0 }; })()`);
    assert.ok(Math.abs(box.top - (box.rowBottom + 6)) <= 2 || box.top + 1 < box.rowTop, `beside the row (below it, or above when there is no room): ${JSON.stringify(box)}`);
    assert.equal(box.inside, true, "and inside the window");
    await key(t, "a"); // the global shortcuts are quiet inside the box: this would have armed approve
    assert.deepEqual(await ev(t, `window.__lab.state().said`), [], "a is not a verb while a menu is open");
    await key(t, "3");
    await t.page.waitFor(`window.__lab.state().acts.length === 1`);
    assert.deepEqual((await acts(t))[0], { path: "/api/snooze", body: { key: "fx:12", until: "7d" } });
    assert.equal(await ev(t, `document.querySelector(".menu")`), null, "choosing closes it");
    await t.page.waitFor(`!document.querySelector('[data-card="fx:12"]')`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("Esc closes a menu without choosing and gives the focus back to its row; the arrows and Enter also choose; a click outside closes", async () => {
  const t = await open();
  try {
    await load(t);
    await ev(t, `window.__lab.cursorTo("c:fx:12")`);
    await key(t, "s");
    await key(t, "Escape");
    assert.equal(await ev(t, `document.querySelector(".menu")`), null);
    assert.equal(await ev(t, `document.activeElement.dataset.row`), "card:fx:12", "the keyboard is back on the row it came from");
    assert.deepEqual(await acts(t), [], "nothing was sent");
    await key(t, "s");
    await key(t, "ArrowDown");
    assert.equal((await menu(t)).focus, "2");
    await key(t, "ArrowUp");
    await key(t, "ArrowUp");
    assert.equal((await menu(t)).focus, "4", "it wraps");
    await key(t, "Enter");
    await t.page.waitFor(`window.__lab.state().acts.length === 1`);
    assert.equal((await acts(t))[0].body.until, "14d");
    await ev(t, `window.__lab.load("rich", 9); window.__lab.cursorTo("c:fx:12")`);
    await key(t, "s");
    await t.page.click(".eyebrow");
    assert.equal(await ev(t, `document.querySelector(".menu")`), null, "a click outside closes it");
  } finally { await t.close(); }
});

test("x withdraws with the reason he picks as the note; r logs the answer the server allows; a row that cannot do it ignores the key", async () => {
  const t = await open();
  try {
    await load(t);
    await ev(t, `window.__lab.cursorTo("c:fx:12")`);
    await key(t, "x");
    assert.equal((await menu(t)).title, "Retirar esta postulación");
    await key(t, "2");
    await t.page.waitFor(`window.__lab.state().acts.length === 1`);
    assert.deepEqual((await acts(t))[0], { path: "/api/mark", body: { key: "fx:12", state: "withdrawn", note: "Ya no me interesa" } });
    await t.page.waitFor(`!document.querySelector('[data-card="fx:12"]')`);
    await ev(t, `window.__lab.cursorTo("c:fx:7")`); // the offer
    await key(t, "r");
    const m = await menu(t);
    assert.equal(m.title, "Registrar una respuesta");
    assert.ok(m.items.length >= 1 && m.items.every(([, label]) => label), JSON.stringify(m.items));
    await key(t, "1");
    await t.page.waitFor(`window.__lab.state().acts.length === 2`);
    const second = (await acts(t))[1];
    assert.equal(second.path, "/api/mark");
    assert.deepEqual([second.body.key, second.body.note], ["fx:7", null]);
    assert.notEqual(second.body.state, "withdrawn");
    await ev(t, `window.__lab.cursorTo("t:101")`); // a task has neither verb
    await key(t, "x");
    await key(t, "r");
    assert.equal(await ev(t, `document.querySelector(".menu")`), null);
    assert.equal((await acts(t)).length, 2);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("n opens the new-task form (typing keeps the shortcuts quiet), an empty text is refused aloud, a good one is sent with its deadline and shows up", async () => {
  const t = await open();
  try {
    await load(t);
    assert.equal(await headline(t), "Te tocan trece.");
    await key(t, "n");
    const m = await menu(t);
    assert.equal(m.title, "Nueva tarea");
    assert.equal(m.role, "dialog");
    assert.equal(m.focus, "INPUT", "the text field has the keyboard");
    await key(t, "Enter");
    assert.equal(await ev(t, `document.querySelector(".menu-input").getAttribute("aria-invalid")`), "true");
    assert.deepEqual(await ev(t, `window.__lab.state().said`), ["Escribe qué hay que hacer."], "refused aloud");
    assert.deepEqual(await acts(t), []);
    await ev(t, `(() => { const i = document.querySelector(".menu-input"); i.value = "Llamar a la agencia"; document.querySelector(".menu-select").value = "2h"; })()`);
    await key(t, "Enter");
    await t.page.waitFor(`window.__lab.state().acts.length === 1`);
    assert.deepEqual((await acts(t))[0], { path: "/api/task/add", body: { text: "Llamar a la agencia", due_at: "2026-10-07T14:00:00Z" } });
    assert.equal(await ev(t, `document.querySelector(".menu")`), null);
    await t.page.waitFor(`document.querySelector(".hoy .mega").textContent.split(/\s+/).filter(Boolean).join(" ") === "Te tocan catorce."`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("after o on a form the bot could not finish, coming back to the tab asks 'did you do it?': c says yes at once, Esc says not yet, and a row that is not a manual one asks nothing", async () => {
  const t = await open();
  try {
    await load(t);
    await ev(t, `window.__lab.cursorTo("c:fx:9")`); // handoff: o is its main verb
    await key(t, "o");
    assert.equal((await ev(t, `window.__lab.state().links`)).length, 1);
    assert.equal(await ev(t, `document.querySelector(".prompt")`), null, "nothing is asked while he is away");
    await ev(t, `window.__lab.visibility(true)`);
    assert.equal(await ev(t, `document.querySelector(".prompt")`), null, "nor while the tab is hidden");
    await ev(t, `window.__lab.visibility(false)`);
    const p = await ev(t, `(() => { const e = document.querySelector(".prompt"); return e && { text: e.querySelector(".prompt-text").textContent, buttons: [...e.querySelectorAll("button")].map((b) => b.textContent), first: e === document.querySelector(".hoy").firstElementChild }; })()`);
    assert.ok(p.text.startsWith("¿Lo hiciste? "), p.text);
    assert.deepEqual(p.buttons, ["cYa la envié", "EscTodavía no"]);
    assert.equal(p.first, true);
    await key(t, "Escape");
    assert.equal(await ev(t, `document.querySelector(".prompt")`), null);
    assert.deepEqual(await acts(t), [], "not yet sends nothing");
    // again, and this time yes: the question is the confirmation, so one press of c does it (on its own, c would only arm it)
    await ev(t, `window.__lab.cursorTo("c:fx:9")`);
    await key(t, "o");
    await ev(t, `window.__lab.visibility(true); window.__lab.visibility(false)`);
    await key(t, "c");
    await t.page.waitFor(`window.__lab.state().acts.length === 1`);
    assert.deepEqual((await acts(t))[0], { path: "/api/confirm", body: { key: "fx:9" } });
    await t.page.waitFor(`!document.querySelector('[data-card="fx:9"]')`);
    await ev(t, `window.__lab.cursorTo("c:fx:12")`); // a proposal: its posting is a look, not something to do by hand
    await key(t, "o");
    await ev(t, `window.__lab.visibility(true); window.__lab.visibility(false)`);
    assert.equal(await ev(t, `document.querySelector(".prompt")`), null);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the question goes away when what it asks about is no longer on the list (it was settled elsewhere): there is nothing left to confirm", async () => {
  const t = await open();
  try {
    await load(t);
    await ev(t, `window.__lab.cursorTo("c:fx:9")`);
    await key(t, "o");
    await ev(t, `window.__lab.visibility(true); window.__lab.visibility(false)`);
    assert.notEqual(await ev(t, `document.querySelector(".prompt")`), null, "it is asked on the way back");
    await ev(t, `window.__lab.refresh((s) => { s.needs_gonza.cards = s.needs_gonza.cards.filter((c) => c.key !== "fx:9"); s.needs_gonza.total -= 1; })`);
    assert.equal(await ev(t, `document.querySelector('[data-card="fx:9"]')`), null);
    assert.equal(await ev(t, `document.querySelector(".prompt")`), null, "the line left with the row");
    await key(t, "c"); // nothing is waiting for a yes any more: this is the plain verb, on whatever the cursor is on
    assert.deepEqual(await acts(t), []);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("a menu opened for a row that is out of the window (a phone, the page scrolled elsewhere) still lands inside the window", async () => {
  const t = await open({ width: 390, height: 844 });
  try {
    await load(t);
    // the last row of the list: since HOY leads with what to do (2026-10-24) the first rows are on the first screen even on a phone
    const last = await ev(t, `[...document.querySelectorAll(".hoy [data-card]")].at(-1).dataset.card`);
    await ev(t, `window.__lab.cursorTo("c:" + ${JSON.stringify(last)})`);
    assert.ok(await ev(t, `document.querySelector('[data-card="' + ${JSON.stringify(last)} + '"]').getBoundingClientRect().top > innerHeight`), `the row ${last} is below the fold`);
    await key(t, "s");
    await settled(t);
    const box = await ev(t, `(() => { const r = document.querySelector(".menu").getBoundingClientRect(); return { top: Math.round(r.top), bottom: Math.round(r.bottom), left: Math.round(r.left), right: Math.round(r.right), h: innerHeight, w: innerWidth }; })()`);
    assert.ok(box.top >= 0 && box.bottom <= box.h && box.left >= 0 && box.right <= box.w, `inside the window: ${JSON.stringify(box)}`);
    await key(t, "Escape");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});
