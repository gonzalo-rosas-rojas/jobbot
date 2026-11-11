// The sound's indicator and the sheet "Escuchar los sonidos" on their own (lab f3-sound: the real HUD, the real engine in a MUTED Chrome and a player whose clock the
// test moves). The whole page, with the shell around them, is page.sound.browser.test.mjs; this file holds what does not need the shell: the words of the indicator for
// every state and how they follow the clock, the sheet's keys, focus and controls, and what a phone is given.
import test from "node:test";
import assert from "node:assert/strict";
import { openBuilt } from "./helpers/browser.mjs";
import { CUES, GROUPS } from "../src/sound/cues.js";

const open = (opts = {}) => openBuilt({ args: ["lab", "f3-sound"], file: "dist/lab-f3-sound.html", ...opts });
const ev = (t, expr) => t.page.eval(expr);
const press = async (t, k, { modifiers = 0 } = {}) => {
  const code = /^\d$/.test(k) ? "Digit" + k : k.length === 1 ? "Key" + k.toUpperCase() : k;
  const text = k.length === 1 && !(modifiers & 6) ? k : k === "Enter" ? "\r" : undefined;
  await t.page.send("Input.dispatchKeyEvent", { type: text ? "keyDown" : "rawKeyDown", key: k, code, text, modifiers });
  await t.page.send("Input.dispatchKeyEvent", { type: "keyUp", key: k, code, modifiers });
};
const word = (t) => ev(t, `document.querySelector(".hud-sound-text").textContent`);
const aria = (t) => ev(t, `document.querySelector(".hud-sound").getAttribute("aria-label")`);
const played = (t) => ev(t, `__lab.played()`);

test("the indicator says the state in words and in the shape of its icon, for every state, and follows the clock", async () => {
  const t = await open({ query: "?lang=es" });
  try {
    assert.equal(await word(t), "apagado");
    assert.match(await aria(t), /^Sonido: apagado\. Abrir la lista de sonidos\.$/);
    assert.equal(await ev(t, `document.querySelector(".hud-sound use").getAttribute("href")`), "#i-volume-x");
    await ev(t, `__lab.ui.controls.master()`);
    assert.equal(await word(t), "acciones y fondo");
    assert.equal(await ev(t, `document.querySelector(".hud-sound use").getAttribute("href")`), "#i-volume-2");
    await ev(t, `__lab.ui.controls.bus("background")`);
    assert.equal(await word(t), "acciones");
    await ev(t, `__lab.ui.controls.bus("actions")`);
    assert.equal(await word(t), "sin canales", "both buses off is said, not hidden");
    await ev(t, `__lab.ui.controls.bus("background")`);
    assert.equal(await word(t), "solo fondo");
    await ev(t, `__lab.ui.controls.bus("actions")`);
    // a silence of an hour: the words carry the end, and the end passes with the clock
    await ev(t, `__lab.ui.controls.mute("hour")`);
    assert.equal(await word(t), "silencio hasta 21:00".replace("21:00", await ev(t, `new Date(__lab.player.status().muteUntil).toLocaleTimeString("es-CL", { hour: "2-digit", minute: "2-digit", hourCycle: "h23" })`)));
    assert.match(await aria(t), /silencio hasta \d\d:\d\d/);
    await ev(t, `__lab.advance(61 * 60 * 1000)`);
    assert.equal(await word(t), "acciones y fondo", "the silence ended by itself");
    // working hours: mute, in words, and the clock icon
    await ev(t, `__lab.hour(12)`);
    assert.equal(await word(t), "mudo hasta 18:00");
    assert.match(await aria(t), /^Sonido: mudo por horario laboral hasta las 18:00\./);
    assert.equal(await ev(t, `document.querySelector(".hud-sound use").getAttribute("href")`), "#i-clock");
    assert.equal(await ev(t, `__lab.player.play("approve")`), false);
    await ev(t, `__lab.ui.controls.work()`);
    assert.equal(await word(t), "acciones y fondo", "allowing it is a choice");
    await ev(t, `__lab.ui.controls.master()`);
    assert.equal(await word(t), "apagado");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the controls make the same words in English", async () => {
  const t = await open({ query: "?lang=en" });
  try {
    assert.equal(await word(t), "off");
    await ev(t, `__lab.ui.controls.master()`);
    assert.equal(await word(t), "actions and background");
    assert.match(await aria(t), /^Sound: actions and background\. Open the list of sounds\.$/);
    await ev(t, `__lab.ui.controls.mute("tomorrow")`);
    assert.match(await word(t), /^silent until \d\d:\d\d$/);
    assert.match(await ev(t, `__lab.toasts.at(-1)`), /^Silent until \d\d:\d\d$/);
  } finally { await t.close(); }
});

const sheet = (t) => ev(t, `(() => {
  const d = document.querySelector(".sound-sheet");
  if (!d) return null;
  return { role: d.getAttribute("role"), modal: d.getAttribute("aria-modal"), label: document.getElementById(d.getAttribute("aria-labelledby"))?.textContent, inert: document.getElementById("doc").inert,
    local: d.hasAttribute("data-local-keys"), focusIn: d.contains(document.activeElement), active: document.activeElement.className,
    rows: [...d.querySelectorAll(".sound-row")].map((r) => ({ cue: r.dataset.cue, key: r.querySelector("kbd").textContent, name: r.querySelector(".sound-name").textContent, when: r.querySelector(".sound-when").textContent,
      label: r.getAttribute("aria-label"), keys: r.getAttribute("aria-keyshortcuts"), described: document.getElementById(r.getAttribute("aria-describedby")) === r.querySelector(".sound-when") })),
    groups: [...d.querySelectorAll(".sound-bus")].map((b) => ({ bus: b.dataset.bus, title: b.querySelector(".sound-bus-title").textContent, groups: [...b.querySelectorAll(".sound-group")].map((g) => [g.querySelector(".sound-group-title").textContent, g.querySelectorAll(".sound-row").length]) })),
    heard: d.querySelector(".sound-heard").textContent, note: d.querySelector(".sound-note").hidden ? null : d.querySelector(".sound-note").textContent,
    switches: [...d.querySelectorAll(".sound-switch")].map((s) => [s.querySelector(".sound-switch-label").textContent, s.getAttribute("aria-checked"), s.querySelector(".sound-state").textContent]),
    pressed: [...d.querySelectorAll(".sound-choice[aria-pressed=true]")].map((b) => b.textContent.replace(/^\\W+/, "")), buttons: [...d.querySelectorAll("button:not(.sound-row)")].filter((b) => b.getClientRects().length).map((b) => b.textContent.trim()) };
})()`);

test("the sheet lists every sound once, grouped by bus, each with a name, a line on when it sounds, a key and a button; nothing sounds by opening it", async () => {
  const t = await open({ query: "?lang=es" });
  try {
    await ev(t, `__lab.ui.sheet.open()`);
    const v = await sheet(t);
    assert.deepEqual([v.role, v.modal, v.label], ["dialog", "true", "Escuchar los sonidos"]);
    assert.equal(v.inert, true);
    assert.equal(v.local, true);
    assert.equal(v.focusIn, true);
    assert.deepEqual(v.rows.map((r) => r.cue).sort(), CUES.map((c) => c.id).sort(), "every cue, once");
    assert.equal(v.rows.length, CUES.length);
    assert.equal(new Set(v.rows.map((r) => r.key)).size, CUES.length, "a key each");
    assert.ok(v.rows.every((r) => /^[1-9a-z]$/.test(r.key)), "digits and letters only");
    for (const r of v.rows) {
      assert.ok(r.name.length > 3 && r.when.length > 10, r.cue);
      assert.equal(r.described, true, r.cue);
      assert.equal(r.keys, r.key);
      assert.match(r.label, /^Escuchar: /);
    }
    assert.deepEqual(v.groups.map((b) => b.bus), ["actions", "background"]);
    assert.deepEqual(v.groups.map((b) => b.title), ["Acciones", "Fondo"]);
    const expected = Object.entries(GROUPS).map(([bus, groups]) => groups.map((g) => CUES.filter((c) => c.bus === bus && c.group === g).length));
    assert.deepEqual(v.groups.map((b) => b.groups.map((g) => g[1])), expected, "each group has its cues");
    assert.match(v.note, /apagado/);
    assert.deepEqual(v.switches, [["Sonido", "false", "Apagado"], ["Acciones", "true", "Encendido"], ["Fondo", "true", "Encendido"]]);
    assert.deepEqual(v.pressed, ["Medio"]);
    assert.deepEqual(await played(t), [], "opening plays nothing (the sound is off)");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("a sound plays only when asked, by its key or its button; the first one turns the sound on; the keys of the page stay quiet inside", async () => {
  const t = await open({ query: "?lang=es" });
  try {
    await ev(t, `__lab.ui.sheet.open()`);
    await press(t, "1");
    await t.page.waitFor(`__lab.engine.enabled === true`);
    assert.deepEqual(await ev(t, `__lab.player.history().map((h) => [h.id, h.preview])`), [["step", true]], "a preview, once, and the sound is on now");
    assert.match((await sheet(t)).heard, /^Sonó: Moverte por la lista$/);
    assert.equal(await word(t), "acciones y fondo");
    assert.equal((await sheet(t)).note, null);
    // m is a sound's key here, and the page's m (the master switch of the lab's body listener) did nothing
    const m = (await sheet(t)).rows.find((r) => r.key === "m");
    await press(t, "m");
    assert.equal(await ev(t, `__lab.engine.enabled`), true);
    assert.equal(await ev(t, `__lab.player.history().at(-1).id`), m.cue);
    await t.page.click(`.sound-row[data-cue="error"]`);
    assert.equal(await ev(t, `__lab.player.history().at(-1).id`), "error");
    assert.equal(await ev(t, `document.querySelector('.sound-row[data-cue="error"]').dataset.playing`), "true", "the row says it is the one playing");
    // a sound with two directions is heard in the other one the second time
    await ev(t, `window.__opts = []; const p = __lab.player.preview.bind(__lab.player); __lab.player.preview = (n, o) => { __opts.push([n, o && o.alt === true]); return p(n, o); }`);
    for (const id of ["switch", "switch", "theme", "theme", "lang", "lang"]) await t.page.click(`.sound-row[data-cue="${id}"]`);
    // the sheet binds `player.preview` at call time through the object, so the wrapper sees every call
    const opts = await ev(t, `__opts`);
    assert.deepEqual(opts.map((o) => o[0]), ["switch", "switch", "theme", "theme", "lang", "lang"]);
    for (let i = 0; i < opts.length; i += 2) assert.notEqual(opts[i][1], opts[i + 1][1], `${opts[i][0]}: on/off, light/dark, es/en: one, then the other`);
    assert.equal(opts[0][1], true, "a cue with two directions starts with its alternative the first time it is asked for");
    // a sound that does not exist as a key does nothing
    const before = (await played(t)).length;
    await press(t, "?");
    assert.equal((await played(t)).length, before);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("a silence or the working hours do not stop what he asks to hear, and the sheet says so", async () => {
  const t = await open({ query: "?lang=es" });
  try {
    await ev(t, `__lab.ui.controls.master()`);
    await ev(t, `__lab.ui.controls.mute("hour")`);
    await ev(t, `__lab.ui.sheet.open()`);
    assert.match((await sheet(t)).note, /silencio hasta las \d\d:\d\d.*suenan igual/);
    await press(t, "1");
    assert.equal(await ev(t, `__lab.player.history().at(-1).id`), "step");
    await ev(t, `__lab.ui.sheet.close(); __lab.ui.controls.mute("off"); __lab.hour(12); __lab.ui.sheet.open()`);
    assert.match((await sheet(t)).note, /horario laboral.*18:00/i);
    await press(t, "2");
    assert.equal(await ev(t, `__lab.player.history().at(-1).id`), "arrive.hoy");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("Tab stays in the sheet, Esc and the veil close it, Ctrl K closes it for the palette, and the focus goes back to where it was", async () => {
  const t = await open({ query: "?lang=es" });
  try {
    await ev(t, `document.querySelector(".hud-sound").focus()`);
    await press(t, "Enter");
    await t.page.waitFor(`!!document.querySelector(".sound-sheet")`);
    assert.deepEqual(await ev(t, `__lab.opens`), [true]);
    const inside = [];
    for (let i = 0; i < 10; i++) { await press(t, "Tab"); inside.push(await ev(t, `!!document.activeElement.closest(".sound-sheet")`)); }
    for (let i = 0; i < 5; i++) { await press(t, "Tab", { modifiers: 8 }); inside.push(await ev(t, `!!document.activeElement.closest(".sound-sheet")`)); }
    assert.deepEqual(inside, Array(15).fill(true));
    await press(t, "Escape");
    await t.page.waitFor(`!document.querySelector(".sound-sheet")`);
    assert.deepEqual(await ev(t, `__lab.opens`), [true, false]);
    assert.equal(await ev(t, `document.activeElement.classList.contains("hud-sound")`), true, "back on the indicator");
    assert.equal(await ev(t, `document.getElementById("doc").inert`), false);
    // the veil
    await t.page.click(".hud-sound");
    await t.page.waitFor(`!!document.querySelector(".sound-sheet")`);
    await t.page.send("Input.dispatchMouseEvent", { type: "mousePressed", x: 8, y: 400, button: "left", clickCount: 1 });
    await t.page.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: 8, y: 400, button: "left", clickCount: 1 });
    await t.page.waitFor(`!document.querySelector(".sound-sheet")`);
    // Ctrl K
    await t.page.click(".hud-sound");
    await t.page.waitFor(`!!document.querySelector(".sound-sheet")`);
    await press(t, "k", { modifiers: 2 });
    await t.page.waitFor(`!document.querySelector(".sound-sheet")`);
    // the button of the indicator toggles it
    await t.page.click(".hud-sound");
    await t.page.waitFor(`!!document.querySelector(".sound-sheet")`);
    assert.equal(await ev(t, `document.querySelector(".hud-sound").getAttribute("aria-haspopup")`), "dialog");
    await ev(t, `__lab.ui.sheet.toggle()`);
    assert.equal(await ev(t, `!!document.querySelector(".sound-sheet")`), false);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the sheet's controls work on the state, and the sheet stays true while it is open", async () => {
  const t = await open({ query: "?lang=es" });
  try {
    await ev(t, `__lab.ui.sheet.open()`);
    const click = (text) => ev(t, `[...document.querySelectorAll(".sound-sheet button")].find((b) => b.textContent.trim().startsWith(${JSON.stringify(text)})).click()`);
    await click("Sonido");
    assert.deepEqual((await sheet(t)).switches[0], ["Sonido", "true", "Encendido"]);
    await click("Fondo");
    assert.deepEqual((await sheet(t)).switches[2], ["Fondo", "false", "Apagado"]);
    assert.equal(await word(t), "acciones");
    await click("Alto");
    assert.deepEqual((await sheet(t)).pressed, ["Alto"]);
    assert.equal(await ev(t, `__lab.player.status().level`), "high");
    await click("1 hora");
    let v = await sheet(t);
    assert.ok(v.buttons.includes("Quitar el silencio") && !v.buttons.includes("1 hora"), "while it is silent the sheet offers to end it: " + v.buttons);
    assert.match(v.note, /silencio hasta/);
    await ev(t, `__lab.advance(2 * 3600 * 1000)`);
    v = await sheet(t);
    assert.ok(v.buttons.includes("1 hora") && v.note === null, "the sheet follows the clock: the silence ended");
    await click("Permitir");
    assert.ok((await sheet(t)).buttons.includes("Silenciar"));
    await click("Silenciar");
    assert.match(await ev(t, `__lab.toasts.at(-1)`), /calla en horario laboral/);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the language closes the sheet (its words are drawn once) and the indicator changes its words in place", async () => {
  const t = await open({ query: "?lang=es" });
  try {
    await ev(t, `__lab.ui.sheet.open()`);
    assert.equal(await ev(t, `document.querySelector(".sound-sheet h3").textContent`), "Escuchar los sonidos");
    await ev(t, `__lab.setLang("en")`);
    assert.equal(await ev(t, `!!document.querySelector(".sound-sheet")`), false, "a sheet in the old language is not left behind");
    assert.equal(await ev(t, `document.getElementById("doc").inert`), false, "and the page is not left inert");
    assert.equal(await word(t), "off");
    await ev(t, `__lab.ui.sheet.open()`);
    assert.equal(await ev(t, `document.querySelector(".sound-sheet h3").textContent`), "Listen to the sounds");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

// ---- small screens ----------------------------------------------------------------------------------------------------------------------------

test("390: the indicator is a 44 px target with the state in its name; the sheet is the whole screen and scrolls inside; every control is 44 px; no key caps", async () => {
  const t = await open({ query: "?lang=es", width: 390, height: 844 });
  try {
    const box = await ev(t, `(() => { const r = document.querySelector(".hud-sound").getBoundingClientRect(); return [Math.round(r.width), Math.round(r.height)]; })()`);
    assert.ok(box[0] >= 44 && box[1] >= 44, "a thumb fits: " + box);
    assert.equal(await ev(t, `getComputedStyle(document.querySelector(".hud-sound-text")).display`), "none", "icon only");
    assert.match(await aria(t), /Sonido: apagado/);
    await t.page.click(".hud-sound");
    await t.page.waitFor(`!!document.querySelector(".sound-sheet")`);
    const m = await ev(t, `(() => {
      const d = document.querySelector(".sound-sheet");
      const r = d.getBoundingClientRect();
      const small = [...d.querySelectorAll("button")].filter((b) => b.getClientRects().length).map((b) => b.getBoundingClientRect()).filter((x) => x.height < 43.5);
      const font = [...d.querySelectorAll("*")].filter((n) => [...n.childNodes].some((c) => c.nodeType === 3 && c.textContent.trim())).map((n) => parseFloat(getComputedStyle(n).fontSize));
      d.scrollTop = 600;
      return { w: Math.round(r.width), h: Math.round(r.height), scrolls: d.scrollHeight > d.clientHeight && d.scrollTop > 0, small: small.length, minFont: Math.min(...font), side: document.documentElement.scrollWidth <= innerWidth,
        keys: [...d.querySelectorAll(".sound-row kbd")].filter((k) => getComputedStyle(k).display !== "none").length, footer: Math.round(d.querySelector(".sound-heard").getBoundingClientRect().bottom) };
    })()`);
    assert.equal(m.w, 390);
    assert.ok(m.h >= 800 && m.h <= 844, "the sheet fills the screen: " + m.h);
    assert.equal(m.scrolls, true, "the list scrolls inside the sheet");
    assert.equal(m.small, 0, "every control is at least 44 px tall");
    assert.ok(m.minFont >= 11, "no text under 11 px: " + m.minFont);
    assert.equal(m.side, true, "no sideways scroll");
    assert.equal(m.keys, 0, "no key caps without a keyboard");
    assert.ok(m.footer <= 844 && m.footer >= 800, "the status line stays at the bottom while it scrolls: " + m.footer);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("1920: the sheet's text is at least 11 px, every control is reachable with the keyboard, and A18 holds on the pixels of the dialog (both themes)", async () => {
  for (const theme of ["dark", "light"]) {
    const t = await open({ query: `?lang=es&theme=${theme}` });
    try {
      await ev(t, `__lab.ui.sheet.open()`);
      const m = await ev(t, `(() => {
        const d = document.querySelector(".sound-sheet");
        const font = [...d.querySelectorAll("*")].filter((n) => [...n.childNodes].some((c) => c.nodeType === 3 && c.textContent.trim())).map((n) => parseFloat(getComputedStyle(n).fontSize));
        const focusable = [...d.querySelectorAll("button")].every((b) => b.tabIndex >= 0);
        return { minFont: Math.min(...font), focusable, ground: getComputedStyle(d).backgroundColor, text: getComputedStyle(d.querySelector(".sound-when")).color, name: getComputedStyle(d.querySelector(".sound-name")).color };
      })()`);
      assert.ok(m.minFont >= 11, `${theme}: ${m.minFont}`);
      assert.equal(m.focusable, true);
      // the dialog is Tinta in both themes: the same ground and the same inks
      assert.equal(m.ground, "rgb(16, 22, 38)", `${theme}: night-2`);
      assert.equal(m.text, "rgb(158, 172, 190)", `${theme}: text-dim`);
      assert.equal(m.name, "rgb(233, 239, 247)", `${theme}: text`);
    } finally { await t.close(); }
  }
});
