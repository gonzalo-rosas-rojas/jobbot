// The command palette on the whole page (Ctrl K, T25): it opens over the stage as a labelled combobox dialog with the page behind it inert, goes to a
// stop or opens an application's drawer or changes a setting, and never changes anything on the server. Driven with real key events in a private
// Chrome at 1920, 1440 and 390 px; what it must be to read is measured, not assumed (the two themes, the type, the contrast, the thumb).
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, mkdirSync } from "node:fs";
import path from "node:path";
import { openPage } from "./helpers/page.mjs";

const rich = JSON.parse(readFileSync(new URL("./fixtures/snapshot.rich.json", import.meta.url), "utf8"));
const SHOTS = process.env.PALETTE_SHOTS || "";
const SIZES = [[1920, 1080], [1440, 900], [390, 844]];
const ev = (t, expr) => t.page.eval(expr);

/** One real key press (CDP): printable keys carry their text, so a focused field takes them; `modifiers` 2 is Ctrl, 8 is Shift. */
async function press(t, k, { modifiers = 0, code, text } = {}) {
  const c = code || (/^\d$/.test(k) ? "Digit" + k : k.length === 1 ? "Key" + k.toUpperCase() : k);
  const txt = text !== undefined ? text : k.length === 1 && !(modifiers & 6) ? k : k === "Enter" ? "\r" : undefined;
  await t.page.send("Input.dispatchKeyEvent", { type: txt ? "keyDown" : "rawKeyDown", key: k, code: c, text: txt, modifiers });
  await t.page.send("Input.dispatchKeyEvent", { type: "keyUp", key: k, code: c, modifiers });
}
const ctrlK = (t) => press(t, "k", { modifiers: 2 });
const type = (t, text) => t.page.send("Input.insertText", { text });
const wait = (t, ms) => t.page.wait(ms);
const isOpen = (t) => ev(t, `!!document.querySelector('[role=dialog].palette')`);
const open = (t) => ctrlK(t).then(() => t.page.waitFor(`!!document.getElementById("palette-input") && document.activeElement === document.getElementById("palette-input")`));
const closed = (t) => t.page.waitFor(`!document.querySelector(".palette")`);
const moment = (t) => ev(t, `document.getElementById("app").dataset.moment`);

/** What the palette shows right now. */
const view = (t) => ev(t, `(() => {
  const q = (s, r = document) => r.querySelector(s), all = (s, r = document) => [...r.querySelectorAll(s)];
  const dialog = q('[role=dialog].palette'), input = q("#palette-input"), list = q("#palette-list");
  const options = all('[role=option]').map((o) => ({ id: o.id, selected: o.getAttribute("aria-selected"), label: q(".palette-label", o)?.textContent ?? null, sub: q(".palette-sub", o)?.textContent ?? null,
    trail: q(".palette-trail", o)?.textContent ?? null, marks: all("mark", o).map((m) => m.textContent), group: o.closest('[role=group]')?.getAttribute("aria-labelledby") ?? null, text: o.textContent }));
  const note = q(".palette-note");
  return { open: !!dialog, role: dialog?.getAttribute("role"), modal: dialog?.getAttribute("aria-modal"), name: dialog?.getAttribute("aria-label") || (dialog?.getAttribute("aria-labelledby") && document.getElementById(dialog.getAttribute("aria-labelledby"))?.textContent) || "",
    combo: { role: input?.getAttribute("role"), expanded: input?.getAttribute("aria-expanded"), controls: input?.getAttribute("aria-controls"), auto: input?.getAttribute("aria-autocomplete"), active: input?.getAttribute("aria-activedescendant"), value: input?.value, label: input?.getAttribute("aria-label") },
    list: { role: list?.getAttribute("role"), hidden: !list || list.hidden || getComputedStyle(list).display === "none" },
    options, groups: all('[role=group]').map((g) => ({ id: g.getAttribute("aria-labelledby"), title: document.getElementById(g.getAttribute("aria-labelledby"))?.textContent.trim() ?? null, n: all('[role=option]', g).length })),
    titles: all(".palette-group-title").map((x) => x.textContent.trim()),
    note: note && !note.hidden ? note.textContent.trim() : null, empty: q(".palette-empty") && !q(".palette-empty").hidden ? q(".palette-empty").textContent.trim() : null,
    status: q('.palette [role=status]')?.textContent.trim() ?? null, text: dialog?.textContent ?? "", inert: document.getElementById("doc").inert, foot: q(".palette-foot") ? getComputedStyle(q(".palette-foot")).display : null };
})()`);
const selectedOf = (v) => v.options.findIndex((o) => o.selected === "true");

// ---- opening, the shape of the dialog, the focus --------------------------------------------------------------------------------------------

for (const [w, h] of SIZES) {
  test(`${w}: Ctrl K opens a labelled combobox dialog with the page behind it inert; the focus is in the field, Tab never leaves, Esc closes and gives the focus back`, async () => {
    const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: w, height: h });
    try {
      await ev(t, `document.querySelector(".hud-search").focus()`);
      assert.equal(await isOpen(t), false);
      await open(t);
      const v = await view(t);
      assert.deepEqual([v.role, v.modal], ["dialog", "true"]);
      assert.ok(v.name.length > 2, "the dialog has a name");
      assert.deepEqual([v.combo.role, v.combo.expanded, v.combo.auto, v.combo.controls], ["combobox", "true", "list", "palette-list"]);
      assert.ok(v.combo.label && v.combo.label.length > 2, "the field has a name");
      assert.equal(v.list.role, "listbox");
      assert.equal(v.inert, true, "the page behind is inert while it is open");
      assert.equal(v.combo.active, v.options[0].id, "the first row is the active one");
      assert.equal(v.options[0].selected, "true");
      assert.equal(v.options.filter((o) => o.selected === "true").length, 1);
      // with nothing typed it is a menu: the stops with their number, then the settings; applications wait for a word
      assert.deepEqual(v.options.slice(0, 6).map((o) => o.label), ["Anoche", "Hoy", "Procesos", "Cola", "Aprender", "Sistema"]);
      assert.deepEqual(v.options.slice(0, 6).map((o) => o.trail.replace(/[^0-9]|aquí/g, "")), ["0", "1", "2", "3", "4", "5"], "each stop with its number");
      assert.equal(v.options.length, 11);
      assert.equal(v.titles.length, 3, "three groups: " + v.titles.join(" | "));
      assert.match(v.titles[0], /ir a/i);
      assert.match(v.titles[1], /postulaciones/i);
      assert.match(v.titles[2], /preferencias y ayuda/i);
      assert.equal(v.groups.length, 2, "only the groups that have rows are groups of the list (the applications wait for a word)");
      assert.doesNotMatch(v.text, /null|undefined|NaN|\[object|⟦/);
      // Tab and Shift+Tab stay inside
      const inside = [];
      for (let i = 0; i < 6; i++) { await press(t, "Tab", { code: "Tab" }); inside.push(await ev(t, `!!document.activeElement.closest(".palette")`)); }
      for (let i = 0; i < 3; i++) { await press(t, "Tab", { code: "Tab", modifiers: 8 }); inside.push(await ev(t, `!!document.activeElement.closest(".palette")`)); }
      assert.deepEqual(inside, Array(9).fill(true), "the focus is trapped in the dialog");
      await press(t, "3");                                           // a digit is a letter of the query here, not a trip to COLA
      await wait(t, 300);
      assert.equal(await moment(t), "hoy", "the digits travel nowhere while the palette is open");
      await press(t, "Escape");
      await closed(t);
      assert.equal(await ev(t, `document.getElementById("doc").inert`), false);
      assert.equal(await ev(t, `document.activeElement === document.querySelector(".hud-search")`), true, "the focus goes back to what had it");
      assert.equal(await moment(t), "hoy");
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}

test("1440: the search button of the HUD opens it, the close button and a click outside close it, Ctrl K again keeps it open with the focus in the field, and it starts empty each time", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: 1440, height: 900 });
  try {
    await t.page.click(".hud-search");
    await t.page.waitFor(`!!document.getElementById("palette-input")`);
    await type(t, "kes");
    await ev(t, `document.querySelector(".palette-close").focus()`);
    await ctrlK(t);
    await t.page.waitFor(`document.activeElement === document.getElementById("palette-input")`);
    assert.equal((await view(t)).combo.value, "kes", "the same palette, not a second one");
    assert.equal(await ev(t, `document.querySelectorAll(".palette").length`), 1);
    await ev(t, `document.querySelector(".palette-close").click()`);
    await closed(t);
    assert.equal(await ev(t, `document.activeElement === document.querySelector(".hud-search")`), true);
    await ctrlK(t);
    await t.page.waitFor(`!!document.getElementById("palette-input")`);
    assert.equal((await view(t)).combo.value, "", "it starts empty");
    await t.page.send("Input.dispatchMouseEvent", { type: "mousePressed", x: 4, y: 4, button: "left", clickCount: 1 });
    await t.page.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: 4, y: 4, button: "left", clickCount: 1 });
    await closed(t);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("a slash is not the palette's: PROCESOS keeps it for its own finder, and Ctrl K works from inside a text field", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: 1440, height: 900 });
  try {
    await press(t, "2");
    await t.page.waitFor(`location.hash === "#procesos" && !!document.querySelector("#app button.cell, #app [data-card]")`, { timeout: 10000 });   // arrived: the keyboard lands on the heading when the trip ends
    await wait(t, 300);
    await press(t, "/", { code: "Slash", text: "/" });
    await wait(t, 300);
    assert.equal(await isOpen(t), false, "/ does not open the palette");
    assert.equal(await ev(t, `document.activeElement?.tagName`), "INPUT", "the finder of PROCESOS has the slash");
    await ctrlK(t);
    await t.page.waitFor(`!!document.querySelector(".palette")`);
    assert.equal(await ev(t, `document.activeElement === document.getElementById("palette-input")`), true, "Ctrl K opens the palette even while a field has the keyboard");
    await press(t, "Escape");
    await closed(t);
    assert.equal(await ev(t, `document.activeElement?.tagName`), "INPUT", "and the focus goes back to that field");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

// ---- typing, the arrows, Enter --------------------------------------------------------------------------------------------------------------

for (const [w, h] of SIZES) {
  test(`${w}: typing filters, the match is in <mark> nodes, the arrows move the active row, and Enter opens the drawer of that application`, async () => {
    const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: w, height: h });
    try {
      await open(t);
      await type(t, "kestrel");
      await t.page.waitFor(`[...document.querySelectorAll("#palette-list [role=option]")].some((o) => /kestrel/i.test(o.textContent))`);
      let v = await view(t);
      assert.equal(v.combo.value, "kestrel");
      assert.ok(v.options.length >= 1 && v.options.length <= 8, `${v.options.length} rows`);
      assert.equal(v.options[0].label, "Kestrel Labs", "the application is the first row");
      assert.equal(v.options[0].sub, "Staff Backend Engineer");
      assert.deepEqual(v.options[0].marks, ["Kestrel"], "the match is a mark of text");
      assert.equal(selectedOf(v), 0);
      assert.match(v.status, /\d+ resultado/);
      assert.equal(v.combo.active, v.options[0].id);
      await type(t, " ");                                            // still the same word, a blank after it
      // the arrows: down moves, up comes back, and past either end it wraps
      await press(t, "ArrowDown", { code: "ArrowDown" });
      v = await view(t);
      assert.equal(selectedOf(v), v.options.length > 1 ? 1 : 0);
      assert.equal(v.combo.active, v.options[selectedOf(v)].id, "aria-activedescendant follows");
      await press(t, "ArrowUp", { code: "ArrowUp" });
      assert.equal(selectedOf(await view(t)), 0);
      await press(t, "ArrowUp", { code: "ArrowUp" });
      v = await view(t);
      assert.equal(selectedOf(v), v.options.length - 1, "up from the first row wraps to the last");
      await press(t, "ArrowDown", { code: "ArrowDown" });
      assert.equal(selectedOf(await view(t)), 0, "down from the last wraps to the first");
      // Enter on the application
      await press(t, "Enter");
      await closed(t);
      await t.page.waitFor(`document.getElementById("detail").classList.contains("open") && /kestrel/i.test(document.querySelector("#detail h2")?.textContent || "")`);
      const after = await ev(t, `({ hash: location.hash, moment: document.getElementById("app").dataset.moment, inert: document.getElementById("doc").inert, inDrawer: !!document.activeElement.closest("#detail"), detailInert: document.getElementById("detail").inert })`);
      assert.match(after.hash, /^#hoy\/demo(:|%3A)/i, "the address names the application on the stop he is on");
      assert.deepEqual([after.moment, after.inert, after.inDrawer, after.detailInert], ["hoy", false, true, false], "the page is alive again and the focus is in the drawer");
      await press(t, "Escape");
      await t.page.waitFor(`!document.getElementById("detail").classList.contains("open")`);
      assert.equal(await ev(t, `location.hash`), "#hoy");
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}

test("390: from the prologue or SISTEMA an application opens on PROCESOS, where the applications live", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: 390, height: 844 });
  try {
    await t.page.click("#tab-sistema");
    await t.page.waitFor(`document.getElementById("app").dataset.moment === "sistema"`);
    await open(t);
    await type(t, "alba robot");
    await t.page.waitFor(`/alba robotics/i.test(document.querySelector("#palette-list [role=option]")?.textContent || "")`);
    await press(t, "Enter");
    await closed(t);
    await t.page.waitFor(`document.getElementById("app").dataset.moment === "procesos" && document.getElementById("detail").classList.contains("open")`, { timeout: 10000 });
    assert.match(await ev(t, `location.hash`), /^#procesos\/demo(:|%3A)/i);
    assert.match(await ev(t, `document.querySelector("#detail h2")?.textContent || ""`), /alba robotics/i);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

for (const [w, h] of [[1440, 900], [390, 844]]) {
  test(`${w}: a stop is found by its name and Enter travels there; digits typed in the field are text`, async () => {
    const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: w, height: h });
    try {
      await open(t);
      await type(t, "apren");
      await t.page.waitFor(`document.querySelector("#palette-list [role=option] .palette-label")?.textContent === "Aprender"`);
      const v = await view(t);
      assert.deepEqual(v.options[0].marks, ["Apren"]);
      assert.equal(v.options[0].group, v.groups[0].id);
      await press(t, "Enter");
      await closed(t);
      await t.page.waitFor(`location.hash === "#aprender"`, { timeout: 10000 });
      assert.equal(await moment(t), "aprender");
      await open(t);
      await press(t, "4");
      await press(t, "2");
      assert.equal((await view(t)).combo.value, "42");
      await press(t, "Escape");
      await closed(t);
      assert.equal(await moment(t), "aprender");
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}

// ---- the settings ---------------------------------------------------------------------------------------------------------------------------

test("1440: the settings do what they say, the page keeps its stop, and the palette speaks the language it was just given", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: 1440, height: 900 });
  try {
    const choose = async (word, expectLabel) => {
      await open(t);
      await type(t, word);
      await t.page.waitFor(`[...document.querySelectorAll("#palette-list [role=option] .palette-label")].some((l) => ${expectLabel}.test(l.textContent))`);
      await ev(t, `(() => { const o = [...document.querySelectorAll("#palette-list [role=option]")].find((x) => ${expectLabel}.test(x.querySelector(".palette-label").textContent)); o.dispatchEvent(new MouseEvent("mousemove", { bubbles: true })); })()`);
      await t.page.waitFor(`${expectLabel}.test(document.querySelector('#palette-list [aria-selected="true"] .palette-label')?.textContent || "")`);
      await press(t, "Enter");
      await closed(t);
    };
    await choose("tema", "/^Tema: claro/");
    await t.page.waitFor(`document.documentElement.getAttribute("data-theme") === "light"`);
    await choose("tema", "/^Tema: oscuro/");
    await t.page.waitFor(`!document.documentElement.getAttribute("data-theme")`);
    await choose("sonido", "/^Sonido: encender/");
    await t.page.waitFor(`__app.sound.enabled === true`);
    await choose("sonido", "/^Sonido: apagar/");
    await t.page.waitFor(`__app.sound.enabled === false`);
    await choose("movimiento", "/^Movimiento: calmo/");
    await t.page.waitFor(`__app.settings.get("motion") === "calm" && document.documentElement.dataset.motion === "calm"`);
    await choose("movimiento", "/^Movimiento: según el sistema/");
    await t.page.waitFor(`__app.settings.get("motion") === "auto"`);
    await choose("atajos", "/^Atajos/");
    await t.page.waitFor(`!!document.querySelector(".help")`);
    assert.equal(await ev(t, `document.getElementById("doc").inert`), true, "the help is a modal of its own");
    await press(t, "Escape");
    await t.page.waitFor(`!document.querySelector(".help")`);
    assert.equal(await moment(t), "hoy", "no setting moved the page");
    await choose("english", "/^Idioma: English/");
    await t.page.waitFor(`document.documentElement.lang === "en"`);
    await open(t);
    const v = await view(t);
    assert.deepEqual(v.options.slice(0, 6).map((o) => o.label), ["Last night", "Today", "Processes", "Queue", "Learn", "System"]);
    assert.match(v.titles.join(" | "), /go to.*applications.*preferences and help/i);
    assert.equal(v.combo.label.length > 2, true);
    await press(t, "Escape");
    await closed(t);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

// ---- nothing that changes the server --------------------------------------------------------------------------------------------------------

/** The rich snapshot with one card's words replaced (everywhere it is listed). */
function withCard(key, patch) {
  const copy = JSON.parse(JSON.stringify(rich));
  const lists = [...copy.board.map((c) => c.cards), copy.needs_gonza.cards, copy.awaiting_approval.cards, copy.snoozed.cards];
  for (const cards of lists) for (const card of cards) if (card.key === key) Object.assign(card, patch);
  return copy;
}

const HOSTILE = { company: `<img src=x onerror="window.__pwned=1"> & Co`, title: `</mark><script>window.__pwned=2</script> Engineer` };

test("1440, the page as Rust serves it: words that sound like an action show an honest line and run nothing; a hostile company is only letters; Enter asks for that application's detail", async () => {
  const snapshot = withCard("fx:12", HOSTILE);
  const t = await openPage({
    kind: "real", query: "?t=tok-1&debug=1&clock=9&lang=es", width: 1440, height: 900,
    answer: async (p) => (p.startsWith("/api/snapshot") ? { status: 200, body: snapshot } : undefined),
  });
  try {
    await t.page.waitFor(`!!window.__app.model()`);
    // an action word: the line says where it is done, the stop is a row, nothing is sent
    for (const [word, where] of [["aprobar", /cola/i], ["stop", /sistema/i], ["posponer", /hoy/i], ["detener", /sistema/i]]) {
      await open(t);
      await type(t, word);
      await t.page.waitFor(`!!document.querySelector(".palette-note") && !document.querySelector(".palette-note").hidden`);
      const v = await view(t);
      assert.match(v.note, where, `${word}: ${v.note}`);
      assert.match(v.note, /se hace desde/i, "it says it is done from the stop, honestly");
      assert.ok(v.options.some((o) => where.test(o.label)), `${word}: the stop is one of the rows`);
      await press(t, "Escape");
      await closed(t);
    }
    // Enter on the honest line's stop goes there, and still nothing is sent
    await open(t);
    await type(t, "aprobar");
    await t.page.waitFor(`document.querySelector("#palette-list [role=option] .palette-label")?.textContent === "Cola"`);
    await press(t, "Enter");
    await closed(t);
    await t.page.waitFor(`document.getElementById("app").dataset.moment === "cola"`, { timeout: 10000 });
    assert.deepEqual(t.api.filter((r) => r.method !== "GET").map((r) => r.method + " " + r.path), [], "the palette sent nothing that changes the server");
    // a hostile company, and a hostile query
    await open(t);
    await type(t, "img src");
    await t.page.waitFor(`[...document.querySelectorAll("#palette-list [role=option]")].some((o) => o.textContent.includes("<img"))`);
    const v = await view(t);
    assert.equal(v.options[0].label, HOSTILE.company, "the company is the letters it was written with");
    assert.equal(v.options[0].sub, HOSTILE.title);
    const hostile = await ev(t, `({ nodes: document.querySelectorAll(".palette img, .palette script, .palette-back img, .palette-back script, .palette b, .palette a").length, pwned: window.__pwned === undefined,
      marks: [...document.querySelectorAll(".palette mark")].every((m) => m.children.length === 0), field: document.getElementById("palette-input").value })`);
    assert.deepEqual([hostile.nodes, hostile.pwned, hostile.marks, hostile.field], [0, true, true, "img src"]);
    await press(t, "Escape");
    await closed(t);
    await open(t);
    await type(t, `<b onclick="window.__pwned=3">x</b>`);
    await wait(t, 250);
    assert.equal(await ev(t, `document.querySelectorAll(".palette b").length + (window.__pwned === undefined ? 0 : 1)`), 0, "a query is letters too");
    await press(t, "Escape");
    await closed(t);
    // Enter on that application asks the server for its detail (a GET), by its key
    await open(t);
    await type(t, "img src");
    await t.page.waitFor(`[...document.querySelectorAll("#palette-list [role=option]")].some((o) => o.textContent.includes("<img"))`);
    await press(t, "Enter");
    await closed(t);
    await t.page.waitFor(`true`);
    await wait(t, 500);
    assert.ok(t.api.some((r) => r.method === "GET" && r.path.startsWith("/api/job?key=" + encodeURIComponent("fx:12"))), "the detail of that application was asked for: " + t.api.map((r) => r.path).join(" "));
    assert.deepEqual(t.api.filter((r) => r.method !== "GET"), [], "and still nothing that changes the server");
    const noise = t.page.problems().filter((p) => !p.text.includes("/api/job?") && !p.text.includes("/api/shot?") && !/404/.test(p.text));
    assert.deepEqual(noise, []);
  } finally { await t.close(); }
});

// ---- layers and the empty page --------------------------------------------------------------------------------------------------------------

test("1440: Esc closes the palette first and leaves the drawer behind it open; the palette is above the drawer", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: 1440, height: 900 });
  try {
    await open(t);
    await type(t, "kestrel");
    await t.page.waitFor(`/kestrel/i.test(document.querySelector("#palette-list [role=option]")?.textContent || "")`);
    await press(t, "Enter");
    await t.page.waitFor(`document.getElementById("detail").classList.contains("open") && !!document.querySelector("#detail h2")`);
    await open(t);
    const z = await ev(t, `({ palette: +getComputedStyle(document.querySelector(".palette-back")).zIndex, drawer: +getComputedStyle(document.getElementById("detail")).zIndex, hit: document.elementFromPoint(720, 400)?.closest(".palette-back") !== null })`);
    assert.ok(z.palette > z.drawer && z.palette >= 60, JSON.stringify(z));
    assert.equal(z.hit, true, "the palette is what is under the pointer");
    await press(t, "Escape");
    await closed(t);
    assert.equal(await ev(t, `document.getElementById("detail").classList.contains("open")`), true, "one layer per Esc: the drawer is still there");
    assert.equal(await ev(t, `document.getElementById("doc").inert`), false);
    await press(t, "Escape");
    await t.page.waitFor(`!document.getElementById("detail").classList.contains("open")`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

for (const [w, h] of [[1920, 1080], [390, 844]]) {
  test(`${w}: an empty install: the palette still opens, says what is missing in words and never prints null`, async () => {
    const t = await openPage({ query: "?debug=1&clock=9&lang=es&scenario=empty", width: w, height: h });
    try {
      await open(t);
      let v = await view(t);
      assert.equal(v.options.length, 11, "the stops and the settings are there with no applications at all");
      assert.doesNotMatch(v.text, /null|undefined|NaN|\[object|⟦/);
      assert.match(v.titles.join(" | "), /postulaciones/i);
      await type(t, "a");
      await wait(t, 150);
      assert.doesNotMatch((await view(t)).text, /null|undefined|NaN|\[object|⟦/);
      await type(t, "zzzqq");
      await t.page.waitFor(`!!document.querySelector(".palette-empty") && !document.querySelector(".palette-empty").hidden`);
      v = await view(t);
      assert.match(v.empty, /nada coincide/i);
      assert.equal(v.options.length, 0);
      assert.equal(v.combo.expanded, "false", "no list is open when there is nothing to choose");
      assert.equal(v.combo.active || "", "");
      assert.doesNotMatch(v.text, /null|undefined|NaN|\[object|⟦/);
      await press(t, "Enter");                                       // nothing to run: nothing happens
      await wait(t, 200);
      assert.equal(await isOpen(t), true);
      assert.equal(await moment(t), "hoy");
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}

// ---- the skin -------------------------------------------------------------------------------------------------------------------------------

/** In the page: the box, the type and the contrast of every kind of text of the palette, worst case (the glass over a white sky). */
const measure = (t) => ev(t, `(() => {
  const cv = document.createElement("canvas"); cv.width = cv.height = 1; const cx = cv.getContext("2d", { willReadFrequently: true });
  const rgba = (css) => { cx.clearRect(0, 0, 1, 1); cx.fillStyle = "rgba(0,0,0,0)"; cx.fillStyle = css; cx.fillRect(0, 0, 1, 1); const d = cx.getImageData(0, 0, 1, 1).data; return [d[0], d[1], d[2], d[3] / 255]; };
  const over = (top, under) => top.slice(0, 3).map((c, i) => c * top[3] + under[i] * (1 - top[3]));
  const lum = ([r, g, b]) => { const f = (c) => { c /= 255; return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); }; return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b); };
  const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
  const box = document.querySelector(".palette"); const bs = getComputedStyle(box);
  const ground = over(rgba(bs.backgroundColor), [255, 255, 255, 1]);                         // the glass over the brightest thing the sky could be
  const r = box.getBoundingClientRect();
  const pick = (sel) => [...document.querySelectorAll(sel)].filter((e) => e.getClientRects().length > 0);
  const roles = { input: "#palette-input", label: ".palette-label", sub: ".palette-sub", trail: ".palette-trail", group: ".palette-group-title", mark: ".palette mark", note: ".palette-note", foot: ".palette-foot", close: ".palette-close", kbd: ".palette kbd", empty: ".palette-empty" };
  const contrast = {};
  for (const [name, sel] of Object.entries(roles)) {
    for (const el of pick(sel)) { const c = rgba(getComputedStyle(el).color); const bg = el.matches("mark") ? over(rgba(getComputedStyle(el).backgroundColor), ground) : ground; contrast[name] = Math.min(contrast[name] ?? 99, ratio(over(c, bg), bg)); }
  }
  const input = document.getElementById("palette-input"); const ph = getComputedStyle(input, "::placeholder");
  if (input.value === "") contrast.placeholder = ratio(over(rgba(ph.color), ground), ground);
  const small = [];
  for (const el of document.querySelectorAll(".palette *")) {
    if (![...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim()) || el.getClientRects().length === 0) continue;
    const cs = getComputedStyle(el); if (cs.fontFamily.toLowerCase().includes("mono") && parseFloat(cs.fontSize) < 11) small.push(el.className + " " + cs.fontSize);
  }
  const rows = pick("#palette-list [role=option]").map((o) => Math.round(o.getBoundingClientRect().height));
  const back = getComputedStyle(document.querySelector(".palette-back"));
  return { x: Math.round(r.left), y: Math.round(r.top), w: Math.round(r.width), h: Math.round(r.height), vw: innerWidth, vh: innerHeight, bgAlpha: rgba(bs.backgroundColor)[3], ground: ground.map(Math.round), contrast, small, rows,
    radius: bs.borderTopLeftRadius, shadow: bs.boxShadow, z: back.zIndex, scroll: document.documentElement.scrollWidth - innerWidth, foot: pick(".palette-foot").length, theme: document.documentElement.getAttribute("data-theme") || "tinta" };
})()`);

for (const [w, h] of SIZES) {
  for (const theme of ["tinta", "papel"]) {
    test(`${w} ${theme}: a dark glass box over the stage, readable (A18) in both themes, mono text from 11 px, a thumb-sized row on a phone`, async () => {
      const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: w, height: h });
      try {
        if (theme === "papel") { await press(t, "t"); await t.page.waitFor(`document.documentElement.getAttribute("data-theme") === "light"`); }
        await open(t);
        await type(t, "e");
        await t.page.waitFor(`document.querySelectorAll("#palette-list [role=option]").length > 3`);
        let m = await measure(t);
        const flat = w < 760;
        if (flat) {
          assert.ok(m.w >= m.vw - 20 && m.h >= m.vh * 0.6, `almost the whole screen: ${m.w}x${m.h} of ${m.vw}x${m.vh}`);
          assert.ok(m.rows.length > 0 && m.rows.every((x) => x >= 44), `every row is a thumb: ${m.rows.join(",")}`);
          assert.equal(m.foot, 0, "the hint of keys is for a keyboard");
        } else {
          assert.ok(m.w >= 520 && m.w <= 760, `a modal of a width to read: ${m.w}`);
          assert.ok(Math.abs(m.x + m.w / 2 - m.vw / 2) <= 2, "centred");
          assert.ok(m.y >= 40 && m.y <= m.vh * 0.25, `a third of the way down at most: ${m.y}`);
          assert.ok(m.h <= m.vh * 0.8, `it never fills the window: ${m.h}`);
          assert.equal(m.foot, 1, "the keys are said on the screen");
        }
        assert.ok(m.bgAlpha >= 0.85 && m.bgAlpha < 1, `glass, dense enough to read: ${m.bgAlpha}`);
        assert.equal(m.radius, "0px", "no rounded card");
        assert.equal(m.shadow, "none", "no shadow");
        assert.ok(Number(m.z) >= 60);
        assert.equal(m.scroll, 0, "no sideways scroll");
        assert.deepEqual(m.small, [], "mono text below 11 px");
        for (const [role, c] of Object.entries(m.contrast)) assert.ok(c >= 4.5, `${role} is ${c.toFixed(2)}:1 on the glass over white, below 4.5:1 (${theme})`);
        for (const need of ["input", "label", "sub", "group", "mark", "close", "placeholder"]) if (need !== "placeholder") assert.ok(need in m.contrast, `${need} was measured`);
        // with the field emptied the placeholder is measured too
        await ev(t, `(() => { const i = document.getElementById("palette-input"); i.value = ""; i.dispatchEvent(new Event("input", { bubbles: true })); })()`);
        await wait(t, 150);
        m = await measure(t);
        assert.ok(m.contrast.placeholder >= 4.5, `the placeholder is ${m.contrast.placeholder?.toFixed(2)}:1`);
        if (SHOTS) {
          mkdirSync(SHOTS, { recursive: true });
          await type(t, w < 760 ? "kes" : "kestrel");
          await wait(t, 300);
          await t.page.shot(path.join(SHOTS, `palette-${w}-${theme}.png`));
        }
        assert.deepEqual(t.page.problems(), []);
      } finally { await t.close(); }
    });
  }
}

test("1440: with reduced motion the palette appears with no animation at all", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: 1440, height: 900, media: { reducedMotion: true } });
  try {
    await open(t);
    const a = await ev(t, `[...document.querySelectorAll(".palette-back, .palette, .palette-item")].flatMap((el) => [parseFloat(getComputedStyle(el).animationDuration) * 1000, parseFloat(getComputedStyle(el).transitionDuration) * 1000, ...el.getAnimations().map((x) => x.effect.getTiming().duration)]).filter((x) => x > 1)`);
    assert.deepEqual(a, [], "nothing lasts more than a millisecond");
    await press(t, "Escape");
    await closed(t);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});
