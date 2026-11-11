// The sound on the whole page, in a private Chrome that is MUTED (tools/cdp.mjs passes --mute-audio: the real engine runs, nothing reaches a speaker).
// What he asked for: the sounds are micro-interactions that tie an action to a sound, the background ones are quieter and rarer, and all of it can be
// silenced. Measured here on the mounted page, with real key events:
//   - off when the page opens (no audio context exists), `m` turns it on and off, the indicator in the HUD says which in words
//   - the palette turns it on, silences it for an hour and brings it back (Sonido, Acciones, Fondo, Silenciar, horario laboral)
//   - working hours (09:00 to 18:00) are mute by default: the indicator says so and nothing is created, until he allows it
//   - approving in COLA is one "arm", one "confirm" and one "approve", in that order
//   - a new snapshot with a reply is ONE background sound, the first snapshot is none, and the next minute's replies are dropped
//   - "Escuchar los sonidos": a dialog, the page behind inert, one button and one key for every sound, the focus comes back
//   - nothing leaves the page: no request but its own
// The order of the page's own frames is untouched: Esc closes the sheet first, the focus order of the HUD keeps the indicator between the state and the key.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, mkdirSync } from "node:fs";
import path from "node:path";
import { openPage } from "./helpers/page.mjs";
import { CUES } from "../src/sound/cues.js";

const rich = JSON.parse(readFileSync(new URL("./fixtures/snapshot.rich.json", import.meta.url), "utf8"));
const SHOTS = process.env.SOUND_SHOTS || "";
const ev = (t, expr) => t.page.eval(expr);

async function press(t, k, { modifiers = 0, code, text } = {}) {
  const c = code || (/^\d$/.test(k) ? "Digit" + k : k.length === 1 ? "Key" + k.toUpperCase() : k);
  const txt = text !== undefined ? text : k.length === 1 && !(modifiers & 6) ? k : k === "Enter" ? "\r" : undefined;
  await t.page.send("Input.dispatchKeyEvent", { type: txt ? "keyDown" : "rawKeyDown", key: k, code: c, text: txt, modifiers });
  await t.page.send("Input.dispatchKeyEvent", { type: "keyUp", key: k, code: c, modifiers });
}
const ctrlK = (t) => press(t, "k", { modifiers: 2 });
const type = (t, text) => t.page.send("Input.insertText", { text });

/** What the HUD indicator says: the words, the accessible name and the icon. */
const hud = (t) => ev(t, `(() => {
  const b = document.querySelector(".hud-sound");
  return b ? { text: b.querySelector(".hud-sound-text").textContent, aria: b.getAttribute("aria-label"), mode: b.dataset.mode, title: b.title, icon: b.querySelector("use")?.getAttribute("href") ?? null } : null;
})()`);
const ids = (t) => ev(t, `__app.sound.history().map((h) => h.id)`);
const played = (t, after = 0) => ev(t, `__app.sound.history().slice(${after}).map((h) => h.id)`);
const size = (t) => ev(t, `__app.sound.history().length`);
/** A trip by key has ENDED on `id`. The moment under the sheet changes halfway (scene/camera.js paints it as the sheet fades); the address is written
 *  when the trip ends, in the same call of ui/shell.js arrive() that asks for the arrival motif. Waiting for the moment alone checks the trip before
 *  its sound had its chance, and a key pressed then lands before the arrival. */
const arrived = (t, id) => t.page.waitFor(`document.getElementById("app").dataset.moment === "${id}" && location.hash === "#${id}" && !__app.engine?.S.jump`);

/** A server that holds a snapshot you can change, and answers an approval. */
function server() {
  let snap = rich;
  return {
    set: (s) => { snap = s; },
    answer: async (p, init) => {
      const body = init && init.body ? JSON.parse(init.body) : {};
      if (p.startsWith("/api/snapshot")) return { status: 200, body: snap };
      if (p === "/api/approve") return { status: 200, body: { ok: true, message: `Aprobada: ${body.key}` } };
      return undefined;
    },
  };
}
const problems = (t) => t.page.problems().filter((x) => !x.text.includes("/api/shot?") && !x.text.includes("/api/job?"));

/** The rich snapshot with `n` more emails, thirty seconds later (a poll). */
const withMail = (n, kind = "recruiter", at = 30) => {
  const s = structuredClone(rich);
  s.generated_at = new Date(Date.parse(rich.generated_at) + at * 1000).toISOString();
  for (let i = 0; i < n; i++) s.today.emails.push({ received_at: new Date(Date.parse(rich.generated_at) + (20 + i) * 1000).toISOString(), from: "x@beta.example", company: "Beta Test Co", job_key: "fx:2", kind, subject: `Test mail ${kind} ${i + 1}`, summary: "Test." });
  return s;
};

// ---- the page opens silent -------------------------------------------------------------------------------------------------------------------

test("the page opens with the sound off: no audio context, the indicator says so in words, and `m` turns it on and off", async () => {
  const t = await openPage({ query: "?debug=1&clock=18&lang=es", width: 1440, height: 900 });
  try {
    assert.equal(await ev(t, `__app.sound.enabled`), false);
    assert.equal(await ev(t, `__app.sound.ctx`), null, "no audio context before he asks for the sound");
    const off = await hud(t);
    assert.equal(off.text, "apagado");
    assert.match(off.aria, /^Sonido: apagado\. /);
    assert.equal(off.icon, "#i-volume-x", "the shape of the icon says it too");
    await press(t, "2");
    await arrived(t, "procesos");
    assert.deepEqual(await ids(t), [], "off: not a single sound, and travelling is the proof");
    await press(t, "m");
    await t.page.waitFor(`__app.sound.enabled === true`);
    await t.page.waitFor(`document.querySelector(".hud-sound-text").textContent === "acciones y fondo"`);
    assert.equal((await hud(t)).icon, "#i-volume-2");
    assert.deepEqual(await ids(t), ["switch"], "turning it on is heard once, at the new volume");
    assert.match(await ev(t, `document.querySelector(".toast").textContent`), /activado/i);
    await press(t, "m");
    await t.page.waitFor(`__app.sound.enabled === false`);
    assert.equal((await hud(t)).text, "apagado");
    assert.deepEqual(await ids(t), ["switch"], "turning it off makes no sound");
    assert.deepEqual(problems(t), []);
  } finally { await t.close(); }
});

test("the indicator is a real button between the state of the bot and the key of the palette; its words follow the language", async () => {
  const t = await openPage({ query: "?debug=1&clock=18&lang=en", width: 1920, height: 1080 });
  try {
    const order = await ev(t, `[...document.querySelectorAll("#hud button")].map((b) => b.className.split(" ")[0])`);
    assert.deepEqual(order.slice(-2), ["hud-sound", "hud-search"], "the sound comes right before the palette");
    const v = await hud(t);
    assert.equal(v.text, "off");
    assert.match(v.aria, /^Sound: off\. Open the list of sounds\.$/);
    const tag = await ev(t, `document.querySelector(".hud-sound").tagName + "|" + document.querySelector(".hud-sound").getAttribute("aria-haspopup")`);
    assert.equal(tag, "BUTTON|dialog");
    await press(t, "l");
    await t.page.waitFor(`document.documentElement.lang === "es"`);
    assert.equal((await hud(t)).text, "apagado", "the words changed with the language, in place");
  } finally { await t.close(); }
});

// ---- the palette ------------------------------------------------------------------------------------------------------------------------------

/** Ctrl K, a word, and Enter on the row whose label matches `label`. */
async function choose(t, word, label) {
  await ctrlK(t);
  await t.page.waitFor(`!!document.getElementById("palette-input") && document.activeElement === document.getElementById("palette-input")`);
  await type(t, word);
  await t.page.waitFor(`[...document.querySelectorAll("#palette-list [role=option] .palette-label")].some((l) => ${label}.test(l.textContent))`);
  await ev(t, `(() => { const o = [...document.querySelectorAll("#palette-list [role=option]")].find((x) => ${label}.test(x.querySelector(".palette-label").textContent)); o.dispatchEvent(new MouseEvent("mousemove", { bubbles: true })); })()`);
  await t.page.waitFor(`${label}.test(document.querySelector('#palette-list [aria-selected="true"] .palette-label')?.textContent || "")`);
  await press(t, "Enter");
  await t.page.waitFor(`!document.querySelector(".palette")`);
}

test("the palette turns the sound on, switches the buses apart, silences for an hour and brings it back", async () => {
  const t = await openPage({ query: "?debug=1&clock=18&lang=es", width: 1440, height: 900 });
  try {
    await choose(t, "sonido", /^Sonido: encender/);
    await t.page.waitFor(`__app.sound.enabled === true`);
    assert.equal((await hud(t)).text, "acciones y fondo");
    await choose(t, "fondo", /^Fondo: apagar/);
    await t.page.waitFor(`document.querySelector(".hud-sound-text").textContent === "acciones"`);
    assert.equal(await ev(t, `__app.sound.play("bg.reply")`), false, "the background bus is off: it makes no sound");
    assert.equal(await ev(t, `__app.sound.play("approve")`), true, "and the actions still do");
    await choose(t, "fondo", /^Fondo: encender/);
    await t.page.waitFor(`document.querySelector(".hud-sound-text").textContent === "acciones y fondo"`);
    await choose(t, "silenciar", /^Silenciar 1 hora/);
    await t.page.waitFor(`/^silencio hasta \\d\\d:\\d\\d$/.test(document.querySelector(".hud-sound-text").textContent)`);
    const quiet = await hud(t);
    assert.equal(quiet.mode, "muted");
    assert.match(quiet.aria, /silencio hasta \d\d:\d\d/);
    const before = await size(t);
    assert.equal(await ev(t, `__app.sound.play("approve")`), false, "silenced: nothing sounds");
    await press(t, "3");
    await arrived(t, "cola");
    assert.equal(await size(t), before, "and travelling is silent too");
    // the silence is remembered with its end, so it survives a reload of the page
    const stored = JSON.parse(await ev(t, `localStorage.getItem("panel.soundprefs")`));
    assert.equal(stored.muteUntil, await ev(t, `__app.sound.status().muteUntil`), "the end of the silence is saved, not only held");
    await choose(t, "silencio", /^Quitar el silencio/);
    await t.page.waitFor(`document.querySelector(".hud-sound-text").textContent === "acciones y fondo"`);
    assert.equal(await ev(t, `__app.sound.play("approve")`), true);
    assert.deepEqual(problems(t), []);
  } finally { await t.close(); }
});

// The page's "now" is pinned with ?now= (18:30 in Santiago, summer time), so the end of the silence is measured against the page's own clock and
// never against the real hour: ?clock= moves the page's clock but not Date.now(), and between 04:00 and 06:00 the real hour put that 06:00 under two
// hours away. Which 06:00 each hour of the day picks is test/sound.player.test.mjs; this test is the palette reaching it with the page's clock.
const SILENCE_FROM = Date.parse("2026-11-06T21:30:00Z");
const SILENCE_TO = Date.parse("2026-11-07T09:00:00Z");

test("`Silenciar hasta mañana` ends at the next 06:00, and `Volumen` goes low, medium, high", async () => {
  const t = await openPage({ query: `?debug=1&now=${new Date(SILENCE_FROM).toISOString()}&lang=es`, width: 1440, height: 900 });
  try {
    assert.equal(await ev(t, `__app.shell.ctx.now()`), SILENCE_FROM, "the page's now is the pinned one");
    await press(t, "m");
    await t.page.waitFor(`__app.sound.enabled === true`);
    const levels = [];
    for (let i = 0; i < 3; i++) {
      await choose(t, "volumen", /^Volumen del sonido/);
      levels.push(await ev(t, `__app.sound.status().level`));
    }
    assert.equal(new Set(levels).size, 3, "three levels: " + levels);
    assert.equal(levels[2], await ev(t, `__app.sound.status().level`));
    await choose(t, "silenciar", /^Silenciar hasta mañana/);
    await t.page.waitFor(`__app.sound.status().muted`);
    const until = await ev(t, `__app.sound.status().muteUntil`);
    const clock = new Date(until).toLocaleTimeString("es-CL", { hour: "2-digit", minute: "2-digit", hour12: false, timeZone: "America/Santiago" });
    assert.equal(clock, "06:00", "tomorrow morning");
    assert.equal(until, SILENCE_TO, `the next 06:00, 11.5 hours after 18:30 on the page's clock: ends ${(until - SILENCE_FROM) / 3600000} h later`);
  } finally { await t.close(); }
});

// ---- working hours ----------------------------------------------------------------------------------------------------------------------------

test("working hours are mute by default and say so; nothing is created until he allows it", async () => {
  const t = await openPage({ query: "?debug=1&clock=12&lang=es", width: 1440, height: 900 });
  try {
    await press(t, "m");
    await t.page.waitFor(`__app.sound.enabled === true`);
    await t.page.waitFor(`document.querySelector(".hud-sound").dataset.mode === "work"`);
    const v = await hud(t);
    assert.equal(v.text, "mudo hasta 18:00");
    assert.match(v.aria, /horario laboral hasta las 18:00/, "the accessible name says why");
    assert.equal(v.icon, "#i-clock", "and the icon is not the speaker");
    assert.match(await ev(t, `document.querySelector(".toast").textContent`), /horario laboral|18:00/i, "turning it on says that it is mute now");
    await press(t, "2");
    await arrived(t, "procesos");
    assert.equal(await ev(t, `__app.sound.play("approve")`), false);
    assert.deepEqual(await ids(t), [], "not one sound in working hours");
    assert.deepEqual(await ev(t, `__app.sound.recent()`), [], "and the engine started no voice");
    // allowing it is a choice, and the page shows the hours it applies to
    await choose(t, "laboral", /^Sonido en horario laboral: permitir/);
    await t.page.waitFor(`document.querySelector(".hud-sound-text").textContent === "acciones y fondo"`);
    assert.equal(await ev(t, `__app.sound.play("approve")`), true);
    await choose(t, "laboral", /^Sonido en horario laboral: silenciar/);
    await t.page.waitFor(`document.querySelector(".hud-sound").dataset.mode === "work"`);
    assert.deepEqual(problems(t), []);
  } finally { await t.close(); }
});

test("calm motion drops the cues of navigation and keeps the rest", async () => {
  const t = await openPage({ query: "?debug=1&clock=18&lang=es&motion=calm", width: 1440, height: 900 });
  try {
    await press(t, "m");
    await t.page.waitFor(`__app.sound.enabled === true`);
    assert.equal(await ev(t, `__app.sound.play("step")`), false);
    assert.equal(await ev(t, `__app.sound.play("arrive", { room: "cola" })`), false);
    assert.equal(await ev(t, `__app.sound.play("arm")`), true);
    assert.equal(await ev(t, `__app.sound.play("error")`), true);
  } finally { await t.close(); }
});

test("a saved \"sound on\" before any gesture: no audio context is made, the browser is asked for nothing, the console stays empty, and the first key unlocks it", async () => {
  const s = server();
  const t = await openPage({ kind: "real", query: "?t=tok&debug=1&poll=500&clock=18&lang=es", width: 1440, height: 900, answer: s.answer, wait: false });
  try {
    await t.page.waitFor(`!!document.querySelector("#app h1")`);
    await ev(t, `localStorage.setItem("panel.sound", "1")`);
    await t.page.goto(t.url);                                          // the page opens again with the sound saved as on and no gesture yet
    await t.page.waitFor(`!!document.querySelector("#app h1")`);
    assert.equal(await ev(t, `__app.sound.enabled`), true, "the choice is remembered");
    assert.equal(await ev(t, `__app.sound.ctx`), null, "but no audio context exists until a gesture");
    assert.equal((await hud(t)).text, "acciones y fondo");
    await t.page.wait(1200);
    s.set(withMail(1));
    await t.page.waitFor(`__app.model().snap.today.emails.length > ${rich.today.emails.length}`, { timeout: 6000 });
    assert.deepEqual(await ids(t), [], "a reply that arrives before any gesture is not sounded");
    assert.equal(await ev(t, `__app.sound.ctx`), null, "and the page did not try to start the audio on its own");
    assert.deepEqual(t.page.log.filter((l) => /audio/i.test(l.text)), [], "nothing was said to the console about the audio");
    await press(t, "j");                                                // the first key is the gesture the browser wants
    await t.page.waitFor(`__app.sound.ctx !== null`);
    assert.equal(await ev(t, `__app.sound.play("approve")`), true, "now it sounds");
    assert.deepEqual(t.page.log.filter((l) => /audio/i.test(l.text)), []);
    assert.deepEqual(problems(t), []);
  } finally { await t.close(); }
});

// ---- the actions: the two presses and the result ----------------------------------------------------------------------------------------------

test("approving in COLA is one arm, one confirm and one approve, in that order", async () => {
  const s = server();
  const t = await openPage({ kind: "real", query: "?t=tok&debug=1&clock=18&lang=es#cola", width: 1440, height: 900, answer: s.answer, wait: false });
  try {
    await t.page.send("Emulation.setTimezoneOverride", { timezoneId: "America/Santiago" });
    await t.page.waitFor(`document.querySelectorAll("#app [data-row]").length >= 1`);
    await press(t, "m");
    await t.page.waitFor(`__app.sound.enabled === true`);
    await t.page.wait(1100);                                          // the engine allows two sounds a second: the one of turning it on has gone by
    const from = await size(t);
    await press(t, "a");
    await t.page.waitFor(`__app.sound.history().some((h) => h.id === "arm")`);
    await t.page.wait(450);
    await press(t, "a");
    await t.page.waitFor(`__app.sound.history().some((h) => h.id === "approve")`);
    const got = await played(t, from);
    assert.deepEqual(got, ["arm", "confirm", "approve"]);
    assert.equal(t.api.filter((r) => r.path === "/api/approve").length, 1);
    assert.deepEqual(problems(t), []);
  } finally { await t.close(); }
});

// ---- the background ---------------------------------------------------------------------------------------------------------------------------

test("a reply that arrives while he works is ONE quiet sound; the first snapshot is none, a repeat is none, and the next reply within the minute is dropped", async () => {
  const s = server();
  const t = await openPage({ kind: "real", query: "?t=tok&debug=1&poll=500&clock=18&lang=es", width: 1440, height: 900, answer: s.answer, wait: false });
  try {
    await t.page.waitFor(`!!document.querySelector("#app h1")`);
    await press(t, "m");
    await t.page.waitFor(`__app.sound.enabled === true`);
    await t.page.wait(1600);                                            // three polls of the same data
    assert.deepEqual((await ids(t)).filter((id) => id.startsWith("bg.")), [], "the first snapshot and the ones that repeat it make no sound");
    s.set(withMail(1));
    await t.page.waitFor(`__app.sound.history().some((h) => h.id === "bg.reply")`, { timeout: 6000 });
    await t.page.wait(1600);
    assert.equal((await ids(t)).filter((id) => id === "bg.reply").length, 1, "once, however many polls see it");
    const last = await ev(t, `__app.sound.history().filter((h) => h.id === "bg.reply")[0]`);
    assert.equal(last.bus, "background");
    s.set(withMail(2, "recruiter", 60));
    await t.page.wait(1800);
    assert.equal((await ids(t)).filter((id) => id.startsWith("bg.")).length, 1, "a second background sound inside the minute is dropped, not delayed");
    assert.deepEqual(problems(t), []);
  } finally { await t.close(); }
});

test("an interview invitation is the longest background sound, and it outranks a plain reply in the same snapshot", async () => {
  const s = server();
  const t = await openPage({ kind: "real", query: "?t=tok&debug=1&poll=500&clock=18&lang=es", width: 1440, height: 900, answer: s.answer, wait: false });
  try {
    await t.page.waitFor(`!!document.querySelector("#app h1")`);
    await press(t, "m");
    await t.page.waitFor(`__app.sound.enabled === true`);
    await t.page.wait(1200);
    const both = withMail(1, "recruiter");
    both.today.emails.push({ received_at: "2026-10-07T15:00:20Z", from: "y@gamma.example", company: "Gamma Test Co", job_key: "fx:3", kind: "interview_invite", subject: "Interview invitation", summary: "Test." });
    s.set(both);
    await t.page.waitFor(`__app.sound.history().some((h) => h.id.startsWith("bg."))`, { timeout: 6000 });
    assert.deepEqual((await ids(t)).filter((id) => id.startsWith("bg.")), ["bg.interview"]);
  } finally { await t.close(); }
});

test("the background is silent while sound is off, and while the tab's work hours are on", async () => {
  const s = server();
  const t = await openPage({ kind: "real", query: "?t=tok&debug=1&poll=500&clock=12&lang=es", width: 1440, height: 900, answer: s.answer, wait: false });
  try {
    await t.page.waitFor(`!!document.querySelector("#app h1")`);
    await t.page.wait(1200);
    s.set(withMail(1));
    await t.page.wait(1800);
    assert.deepEqual(await ids(t), [], "off: nothing");
    await press(t, "m");
    await t.page.waitFor(`document.querySelector(".hud-sound").dataset.mode === "work"`);
    s.set(withMail(2, "recruiter", 60));
    await t.page.wait(1800);
    assert.deepEqual(await ids(t), [], "working hours: nothing, and the page said so");
    assert.deepEqual(await ev(t, `__app.sound.recent()`), []);
  } finally { await t.close(); }
});

// ---- "Escuchar los sonidos" -------------------------------------------------------------------------------------------------------------------

const sheet = (t) => ev(t, `(() => {
  const d = document.querySelector(".sound-sheet");
  if (!d) return null;
  const rows = [...d.querySelectorAll(".sound-row")];
  return { role: d.getAttribute("role"), modal: d.getAttribute("aria-modal"), label: document.getElementById(d.getAttribute("aria-labelledby"))?.textContent,
    inert: document.getElementById("doc").inert, local: d.hasAttribute("data-local-keys"), focusIn: d.contains(document.activeElement),
    rows: rows.map((r) => ({ cue: r.dataset.cue, key: r.querySelector("kbd")?.textContent, name: r.querySelector(".sound-name")?.textContent, when: r.querySelector(".sound-when")?.textContent,
      label: r.getAttribute("aria-label"), described: document.getElementById(r.getAttribute("aria-describedby"))?.textContent === r.querySelector(".sound-when")?.textContent })),
    heard: d.querySelector(".sound-heard")?.textContent, note: d.querySelector(".sound-note")?.hidden === false ? d.querySelector(".sound-note").textContent : null,
    switches: [...d.querySelectorAll(".sound-switch")].map((s) => [s.querySelector(".sound-switch-label").textContent, s.getAttribute("aria-checked"), s.querySelector(".sound-state").textContent]) };
})()`);

test("Escuchar los sonidos: a dialog with one button and one key per sound, that plays only when asked, turns the sound on first, and gives the focus back", async () => {
  const t = await openPage({ query: "?debug=1&clock=18&lang=es", width: 1440, height: 900 });
  try {
    await ev(t, `document.querySelector(".hud-sound").focus()`);
    await press(t, "Enter");
    await t.page.waitFor(`!!document.querySelector(".sound-sheet")`);
    const v = await sheet(t);
    assert.deepEqual([v.role, v.modal, v.label], ["dialog", "true", "Escuchar los sonidos"]);
    assert.equal(v.inert, true, "the page behind is inert");
    assert.equal(v.local, true, "the page's own keys are quiet inside it");
    assert.equal(v.focusIn, true);
    assert.deepEqual(v.rows.map((r) => r.cue), CUES.map((c) => c.id), "every sound of the vocabulary, in order");
    assert.equal(new Set(v.rows.map((r) => r.key)).size, CUES.length, "each with a key of its own");
    for (const r of v.rows) {
      assert.ok(r.name && r.name.length > 3 && r.when && r.when.length > 10, `${r.cue} says what it is and when it sounds`);
      assert.equal(r.described, true, `${r.cue}: the button is described by its line`);
      assert.match(r.label, /^Escuchar: /);
    }
    assert.match(v.note, /apagado/, "the sound is off: the sheet says that pressing one turns it on");
    assert.deepEqual(v.switches[0], ["Sonido", "false", "Apagado"]);
    assert.deepEqual(await ids(t), [], "opening plays nothing: the sound is off, and nothing else asks for it");
    // a key plays its sound, once, and turns the sound on with that gesture
    const first = v.rows[0];
    await press(t, first.key);
    await t.page.waitFor(`__app.sound.enabled === true`);
    await t.page.waitFor(`__app.sound.history().some((h) => h.preview === true)`);
    const heard = await ev(t, `__app.sound.history().filter((h) => h.preview).map((h) => h.id)`);
    assert.deepEqual(heard, [first.cue]);
    assert.match((await sheet(t)).heard, /Sonó: /);
    assert.equal(await ev(t, `document.querySelector(".hud-sound-text").textContent`), "acciones y fondo");
    // the keys of the page did nothing: "t" and "m" are the keys of sounds here, not the theme or the sound
    const letter = v.rows.find((r) => r.key === "m");
    assert.ok(letter, "m is the key of a sound inside the sheet");
    await press(t, "m");
    assert.equal(await ev(t, `__app.sound.enabled`), true, "m did not turn the sound off");
    assert.equal(await ev(t, `__app.sound.history().filter((h) => h.preview).at(-1).id`), letter.cue);
    // the buttons play too
    await t.page.click(`.sound-row[data-cue="error"]`);
    assert.equal(await ev(t, `__app.sound.history().at(-1).id`), "error");
    // a sound with two directions answers with the other one the second time
    await t.page.click(`.sound-row[data-cue="switch"]`);
    await t.page.click(`.sound-row[data-cue="switch"]`);
    assert.deepEqual(await ev(t, `__app.sound.history().slice(-2).map((h) => h.id)`), ["switch", "switch"]);
    // Tab stays inside
    const inside = [];
    for (let i = 0; i < 8; i++) { await press(t, "Tab", { code: "Tab" }); inside.push(await ev(t, `!!document.activeElement.closest(".sound-sheet")`)); }
    for (let i = 0; i < 4; i++) { await press(t, "Tab", { code: "Tab", modifiers: 8 }); inside.push(await ev(t, `!!document.activeElement.closest(".sound-sheet")`)); }
    assert.deepEqual(inside, Array(12).fill(true), "the focus is trapped in the dialog");
    // Esc closes it, the page is back, and the focus is on the indicator
    await press(t, "Escape");
    await t.page.waitFor(`!document.querySelector(".sound-sheet")`);
    assert.equal(await ev(t, `document.getElementById("doc").inert`), false);
    assert.equal(await ev(t, `document.activeElement.classList.contains("hud-sound")`), true, "the focus is where it was");
    assert.deepEqual(problems(t), []);
  } finally { await t.close(); }
});

test("the sheet's controls change the same state as the keys: the sound, the buses, the volume, the silence and the working hours", async () => {
  const t = await openPage({ query: "?debug=1&clock=18&lang=es", width: 1440, height: 900 });
  try {
    await t.page.click(".hud-sound");
    await t.page.waitFor(`!!document.querySelector(".sound-sheet")`);
    const click = async (text) => {
      await ev(t, `[...document.querySelectorAll(".sound-sheet button")].find((b) => b.textContent.trim().startsWith(${JSON.stringify(text)})).click()`);
    };
    await click("Sonido");                                              // the master switch
    await t.page.waitFor(`__app.sound.enabled === true`);
    assert.deepEqual((await sheet(t)).switches[0], ["Sonido", "true", "Encendido"]);
    await click("Fondo");
    assert.equal(await ev(t, `__app.sound.status().background`), false);
    assert.equal(await ev(t, `document.querySelector(".hud-sound-text").textContent`), "acciones", "the indicator follows the sheet");
    await click("1 hora");
    assert.equal(await ev(t, `__app.sound.status().muted`), true);
    assert.match((await sheet(t)).switches.length ? await ev(t, `document.querySelector(".sound-sheet .sound-line[role=status]").textContent`) : "", /silencio hasta \d\d:\d\d/);
    await click("Quitar el silencio");
    assert.equal(await ev(t, `__app.sound.status().muted`), false);
    await click("Alto");
    assert.equal(await ev(t, `__app.sound.status().level`), "high");
    assert.equal(await ev(t, `document.querySelector(".sound-sheet [aria-pressed=true]").textContent.replace(/^\\W+/, "")`), "Alto");
    await press(t, "Escape");
    await t.page.waitFor(`!document.querySelector(".sound-sheet")`);
    assert.deepEqual(problems(t), []);
  } finally { await t.close(); }
});

test("Ctrl K from inside the sheet closes it and opens the palette; the palette entry opens it; the language closes it", async () => {
  const t = await openPage({ query: "?debug=1&clock=18&lang=es", width: 1440, height: 900 });
  try {
    await choose(t, "escuchar", /^Escuchar los sonidos/);
    await t.page.waitFor(`!!document.querySelector(".sound-sheet")`);
    assert.equal((await sheet(t)).inert, true);
    await ctrlK(t);
    await t.page.waitFor(`!!document.querySelector(".palette") && !document.querySelector(".sound-sheet")`);
    await press(t, "Escape");
    await t.page.waitFor(`!document.querySelector(".palette")`);
    assert.equal(await ev(t, `document.getElementById("doc").inert`), false);
    await choose(t, "escuchar", /^Escuchar los sonidos/);
    await t.page.waitFor(`!!document.querySelector(".sound-sheet")`);
    await ev(t, `document.querySelector(".sound-close").click()`);
    await t.page.waitFor(`!document.querySelector(".sound-sheet")`);
    assert.deepEqual(problems(t), []);
  } finally { await t.close(); }
});

test("English: the sheet, the indicator and the palette speak English, with the same keys", async () => {
  const t = await openPage({ query: "?debug=1&clock=18&lang=en", width: 1440, height: 900 });
  try {
    await choose(t, "listen", /^Listen to the sounds/);
    await t.page.waitFor(`!!document.querySelector(".sound-sheet")`);
    const v = await sheet(t);
    assert.equal(v.label, "Listen to the sounds");
    assert.equal(v.rows.length, CUES.length);
    assert.match(v.rows[0].label, /^Listen: /);
    assert.doesNotMatch(await ev(t, `document.querySelector(".sound-sheet").textContent`), /⟦|undefined|null|NaN/);
    assert.deepEqual(v.switches[0], ["Sound", "false", "Off"]);
  } finally { await t.close(); }
});

// ---- nothing leaves the page ------------------------------------------------------------------------------------------------------------------

test("the sound asks the network for nothing: every request of the page is its own", async () => {
  const t = await openPage({ query: "?debug=1&clock=18&lang=es", width: 1440, height: 900 });
  try {
    await press(t, "m");
    await t.page.waitFor(`__app.sound.enabled === true`);
    await t.page.click(".hud-sound");
    await t.page.waitFor(`!!document.querySelector(".sound-sheet")`);
    await press(t, "1");
    await press(t, "Escape");
    const origin = new URL(t.url).origin;
    const foreign = t.requests.filter((u) => !u.startsWith(origin) && !u.startsWith("data:") && !u.startsWith("blob:"));
    assert.deepEqual(foreign, [], "no request leaves the page");
    assert.equal(await ev(t, `typeof window.__app.sound.status().work.now`), "boolean");
  } finally { await t.close(); }
});

// ---- small screens ----------------------------------------------------------------------------------------------------------------------------

test("390: the indicator is a 44 px target with the words in its name; the sheet is the whole screen, scrolls inside, and every control is 44 px", async () => {
  const t = await openPage({ query: "?debug=1&clock=18&lang=es", width: 390, height: 844 });
  try {
    const box = await ev(t, `(() => { const r = document.querySelector(".hud-sound").getBoundingClientRect(); return [Math.round(r.width), Math.round(r.height)]; })()`);
    assert.ok(box[0] >= 44 && box[1] >= 44, "a thumb fits: " + box);
    assert.equal(await ev(t, `getComputedStyle(document.querySelector(".hud-sound-text")).display`), "none", "icon only on a phone");
    assert.match((await hud(t)).aria, /Sonido: apagado/);
    assert.equal(await ev(t, `document.documentElement.scrollWidth <= innerWidth`), true, "no sideways scroll");
    await t.page.click(".hud-sound");
    await t.page.waitFor(`!!document.querySelector(".sound-sheet")`);
    const m = await ev(t, `(() => {
      const d = document.querySelector(".sound-sheet").getBoundingClientRect();
      const small = [...document.querySelectorAll(".sound-sheet button")].filter((b) => b.getClientRects().length).map((b) => b.getBoundingClientRect()).filter((r) => r.height < 43.5);
      const font = [...document.querySelectorAll(".sound-sheet *")].filter((n) => n.childNodes.length && [...n.childNodes].some((c) => c.nodeType === 3 && c.textContent.trim())).map((n) => parseFloat(getComputedStyle(n).fontSize));
      return { w: Math.round(d.width), h: Math.round(d.height), small: small.length, minFont: Math.min(...font), side: document.documentElement.scrollWidth <= innerWidth, keys: [...document.querySelectorAll(".sound-row kbd")].filter((k) => getComputedStyle(k).display !== "none").length };
    })()`);
    assert.equal(m.w, 390);
    assert.ok(m.h >= 800, "the sheet fills the screen: " + m.h);
    assert.equal(m.small, 0, "every control is at least 44 px tall");
    assert.ok(m.minFont >= 11, "no text under 11 px: " + m.minFont);
    assert.equal(m.side, true);
    assert.equal(m.keys, 0, "no key caps where there is no keyboard");
    await press(t, "Escape");
    await t.page.waitFor(`!document.querySelector(".sound-sheet")`);
    assert.deepEqual(problems(t), []);
  } finally { await t.close(); }
});

// ---- to look at it (SOUND_SHOTS=dir) ----------------------------------------------------------------------------------------------------------

if (SHOTS) {
  for (const [w, h] of [[1920, 1080], [390, 844]]) {
    for (const theme of ["dark", "light"]) {
      test(`shots ${w} ${theme}`, async () => {
        mkdirSync(SHOTS, { recursive: true });
        const t = await openPage({ query: `?debug=1&clock=18&lang=es&theme=${theme}`, width: w, height: h });
        try {
          await t.page.wait(600);
          await t.page.shot(path.join(SHOTS, `indicator-off-${w}-${theme}.png`));
          await press(t, "m");
          await t.page.waitFor(`__app.sound.enabled === true`);
          await t.page.wait(300);
          await t.page.shot(path.join(SHOTS, `indicator-on-${w}-${theme}.png`));
          await t.page.click(".hud-sound");
          await t.page.waitFor(`!!document.querySelector(".sound-sheet")`);
          await t.page.wait(200);
          await t.page.shot(path.join(SHOTS, `sheet-${w}-${theme}.png`));
          await ev(t, `document.querySelector(".sound-sheet").scrollTop = 700`);
          await t.page.wait(100);
          await t.page.shot(path.join(SHOTS, `sheet-scrolled-${w}-${theme}.png`));
        } finally { await t.close(); }
      });
    }
  }
}
