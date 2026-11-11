// APRENDER in a real browser (spec §5.4, §3.4, §9, A18): the page fits the sheet at the four sizes of the spec, says what the model says, the
// keys do their verbs (two presses for u, the review as a mode), the states are honest, hostile text stays text, and every text keeps 4.5:1
// over the glass and the sky. Lab: lab/f2-aprender.entry.js (a fake server that does what the real one would to the snapshot).
import test from "node:test";
import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { openBuilt } from "./helpers/browser.mjs";
import { openPage } from "./helpers/page.mjs";
import { decodePng } from "./helpers/png.mjs";
import { contrast } from "./helpers/color.mjs";

const open = (opts = {}) => openBuilt({ args: ["lab", "f2-aprender"], file: "dist/lab-f2-aprender.html", ...opts });
const ev = (t, expr) => t.page.eval(expr);
const SHOTS = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "dist", "aprender-shots");
// the fonts are binary assets that arrive after the first paint and move the rows by a pixel: measure only once they are in
const load = async (t, name = "rich", hora = 22) => {
  await ev(t, `window.__lab.reduced(true), window.__lab.load(${JSON.stringify(name)}, ${hora})`);
  await t.page.waitFor(`document.fonts.status === "loaded"`);
};
// tinykeys reads event.code for a letter or a digit, so a real key needs its code: "u" is KeyU and "3" is Digit3. Enter carries its "\r" text:
// without it Chrome does not press the focused button nor submit the form, which is what a real Enter does.
const key = (t, k, extra = {}) => t.page.key(k, k.length === 1 ? { code: /[0-9]/.test(k) ? "Digit" + k : "Key" + k.toUpperCase(), text: k, ...extra } : k === "Enter" ? { text: "\r", ...extra } : extra);
const acts = (t) => ev(t, `window.__lab.state().acts`);
const turn = (t) => ev(t, `document.querySelector(".aprender .mega .turn").textContent`);
const rows = (t) => ev(t, `[...document.querySelectorAll(".aprender .ap-row[data-row]")].map((r) => ({ id: r.dataset.row, task: r.dataset.task, q: r.querySelector(".ap-q").textContent, n: r.querySelector(".ap-n").textContent, who: r.querySelector(".ap-who").textContent, actions: r.dataset.actions, pending: r.dataset.pending || null, armed: r.dataset.armed || null }))`);
const shot = async (t, name) => { mkdirSync(SHOTS, { recursive: true }); await t.page.wait(300); await t.page.shot(path.join(SHOTS, name)); };

test("the page fits the sheet at 1920x1080, 3440x1440 and 1440x900 without scrolling, and runs on from the top at 390x844", async () => {
  for (const [width, height] of [[1920, 1080], [3440, 1440], [1440, 900], [390, 844]]) {
    const t = await open({ width, height });
    try {
      await load(t);
      const m = await ev(t, `(() => { const host = document.getElementById("host"); const sheet = document.getElementById("app").getBoundingClientRect(); const q = (s) => document.querySelector(s);
        const bottom = (s) => { const el = q(s); return el && el.getClientRects().length ? el.getBoundingClientRect().bottom : null; };
        return { hostOver: host.scrollHeight - host.clientHeight, docW: document.documentElement.scrollWidth, winW: innerWidth, sheetBottom: sheet.bottom, winH: innerHeight, position: getComputedStyle(q("#app")).position,
          start: bottom(".ap-start"), foot: bottom(".ap-foot"), h1: q(".aprender .mega").textContent.replace(/\\s+/g, " ").trim(), rows: document.querySelectorAll(".aprender .ap-row[data-row]").length,
          more: q(".aprender .ap-more").textContent, sheetTop: sheet.top }; })()`);
      assert.equal(m.h1, "No supo seis.", `${width}x${height}`);
      assert.equal(m.rows, 5, "five questions, the sixth behind “y 1 más”");
      assert.equal(m.more, "y 1 más");
      assert.ok(m.docW <= m.winW, `no horizontal scroll at ${width}x${height} (${m.docW} > ${m.winW})`);
      if (width >= 760) {
        assert.ok(m.hostOver <= 1, `${width}x${height}: the sheet holds the page without scrolling it (${m.hostOver}px over)`);
        assert.ok(m.sheetBottom <= m.winH, `${width}x${height}: the sheet ends inside the window`);
        assert.ok(m.start != null && m.start <= m.sheetBottom, `${width}x${height}: “Empezar el repaso” is on the first screen`);
        if (height >= 1000) assert.ok(m.foot != null && m.foot <= m.sheetBottom, `${width}x${height}: and so is the foot`);
      } else {
        assert.equal(m.position, "static", "under 760 px the sheet is a block of the page");
      }
      await shot(t, `aprender-${width}x${height}.png`);
      assert.deepEqual(t.page.problems(), [], `${width}x${height}`);
    } finally { await t.close(); }
  }
});

test("the page says what the model says: every figure of the headline, the blocks and the review comes from the snapshot, and what it lacks says sin dato", async () => {
  const t = await open();
  try {
    await load(t);
    const v = await ev(t, `(() => { const q = (s) => document.querySelector(s); const all = (s) => [...document.querySelectorAll(s)].map((e) => e.textContent.replace(/\\s+/g, " ").trim());
      return { eyebrow: q(".aprender .eyebrow").textContent, engine: q(".ap-engine").textContent, sub: all(".ap-sub-line"), missH: q("#ap-miss-h").textContent, figNums: all(".ap-figs .fig-n"), noData: all(".ap-figs [data-nodata]"),
        rejH: q("#ap-rej-h").textContent, pats: all(".ap-pat"), eveH: q("#ap-eve-h").textContent, role: q(".ap-eve-role").textContent, qs: all(".ap-qs li"), stories: all(".ap-stories li"), eveAct: all(".ap-eve-act"),
        drillH: q("#ap-drill-h").textContent, topics: all(".ap-topic").length, start: q(".ap-start").textContent, startAria: q(".ap-start").getAttribute("aria-label"), bank: q(".ap-note") && all(".ap-block[data-block=bank] .ap-note"), how: q(".ap-how").textContent,
        learn: window.__lab.snapshot().learn, counts: all(".stops [data-stop], .stop").slice(0, 8) }; })()`);
    assert.match(v.eyebrow, /^¿Qué respuesta me sigue faltando\? 22:00$/i);
    assert.match(v.engine, /Ventana cerrada/i, "the engine line is the controls of the snapshot");
    assert.match(v.missH, /Respuestas que el bot no supo \(6\): bloquean 6 formularios/i);
    assert.equal(v.noData.length, 1, "exactly one figure has no data: the bank itself, which the snapshot does not carry");
    assert.match(v.noData[0], /sin dato/);
    assert.deepEqual(v.figNums, ["10", "6", "36", "sin dato"], "pipeline.complete, the forms held back, the review seen, and the bank that the snapshot does not carry");
    assert.match(v.rejH, /9 cerradas por rechazo|9/);
    assert.match(v.eveH, /^La víspera: Delta Test Co, /);
    assert.equal(v.qs.length, 3);
    assert.equal(v.stories.length, 3);
    assert.match(v.stories[0], /Servicio de colas reescrito en Rust/);
    assert.match(v.drillH, new RegExp(`${v.learn.due} pendientes, vistas ${v.learn.seen} de ${v.learn.total}`, "i"), "the figure of the bar and the one of the page are the same number (A16)");
    assert.ok(v.topics >= 5);
    assert.match(v.start, /Empezar el repaso/);
    assert.equal(v.startAria, "Empezar el repaso de todos los temas", "a screen reader hears the review of all topics, not the ↵ of its key (aprender.drill.start.aria)");
    assert.match(v.how, /answers\.toml/, "how to answer today, in the words of today");
    assert.doesNotMatch(await ev(t, `document.getElementById("host").textContent`), /NaN|undefined|null|⟦/);
    // no route to save an answer exists, so no control that pretends to
    assert.equal(await ev(t, `[...document.querySelectorAll("#host button")].filter((b) => /guardar|save/i.test(b.textContent)).length`), 0);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("u gives a question's forms back to the queue in two presses, the row is dim while the server answers, and a refusal gives it back", async () => {
  const t = await open();
  try {
    await load(t);
    const before = await rows(t);
    assert.deepEqual(before[0].actions.split(","), ["resolve", "open", "link"]);
    assert.equal(await turn(t), "seis.");
    await key(t, "j");
    assert.equal(await ev(t, `window.__lab.cursor()`), "t:" + before[0].id, "j puts the cursor on the first question");
    await key(t, "u");
    assert.deepEqual(await acts(t), [], "the first press only asks");
    let first = (await rows(t))[0];
    assert.equal(first.armed, "resolve");
    assert.equal(await ev(t, `document.querySelector(".ap-row[data-armed] .act-label").textContent`), "¿Seguro? Pulsa otra vez");
    await t.page.wait(450); // a second press inside 350 ms is a double click, not a decision
    await ev(t, `window.__lab.hold()`);
    await key(t, "u");
    await t.page.waitFor(`document.querySelector(".ap-row[data-pending]")`);
    const sent = await acts(t);
    assert.equal(sent.length, 1);
    assert.equal(sent[0].path, "/api/resolve");
    assert.deepEqual([...sent[0].body.keys].sort(), ["fx:21", "fx:25"], "every form that waits for that question, in one request");
    await key(t, "u"); // the dim row ignores a third press
    assert.equal((await acts(t)).length, 1);
    await ev(t, `window.__lab.release(true)`);
    await t.page.waitFor(`document.querySelector(".aprender .mega .turn").textContent === "cinco."`);
    assert.ok(!(await rows(t)).some((r) => r.id === before[0].id), "the question is gone with the forms it held back");
    assert.ok((await ev(t, `window.__lab.state().sounds`)).includes("undo"));

    // a refusal: the server says no, the row is his again and the toast already said why
    const next = (await rows(t))[0];
    await ev(t, `window.__lab.cursorTo(${JSON.stringify("t:" + next.id)})`);
    await key(t, "u");
    await t.page.wait(450);
    await ev(t, `window.__lab.hold()`);
    await key(t, "u");
    await t.page.waitFor(`document.querySelector(".ap-row[data-pending]")`);
    await ev(t, `window.__lab.release(false, 409)`);
    await t.page.waitFor(`!document.querySelector(".ap-row[data-pending]")`);
    assert.ok((await rows(t)).some((r) => r.id === next.id), "the row is still there");
    assert.ok((await ev(t, `window.__lab.state().sounds`)).includes("error"));
    assert.equal(await turn(t), "cinco.");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("a click on the button does what u does; u with the cursor on none asks about the first question, and on the eve it is not a verb", async () => {
  const t = await open();
  try {
    await load(t);
    const [first, second] = await rows(t);
    await key(t, "u");
    assert.equal((await rows(t))[0].armed, "resolve", "with no cursor u asks about the question at the top, visibly");
    await ev(t, `window.__lab.cursorTo("c:fx:4")`);
    await key(t, "u");
    assert.deepEqual(await acts(t), [], "the eve has no resolve verb");
    // The click goes to the SECOND question: arming a target other than the armed one does not depend on how long ago the first press was
    // (on the armed target itself a click inside ARM_MIN_MS is ignored and one after it is the confirmation, which made this test fail under load).
    await t.page.click(`.ap-row[data-row="${second.id}"] .act`);
    assert.deepEqual((await rows(t))[1].armed, "resolve");
    assert.equal((await rows(t))[0].armed, null, "arming another question forgets the first");
    await t.page.wait(450);
    await t.page.click(`.ap-row[data-row="${second.id}"] .act`);
    await t.page.waitFor(`window.__lab.state().acts.length === 1`);
    assert.equal((await acts(t))[0].path, "/api/resolve");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("j and k walk the questions and the eve, Enter on a question opens its application, and the posting link opens only when it is http", async () => {
  const t = await open();
  try {
    await load(t);
    const r = await rows(t);
    for (let i = 0; i < 3; i++) await key(t, "j");
    assert.equal(await ev(t, `window.__lab.cursor()`), "t:" + r[2].id);
    assert.equal(await ev(t, `document.querySelectorAll("[data-cursor]").length`), 1);
    await key(t, "k");
    assert.equal(await ev(t, `window.__lab.cursor()`), "t:" + r[1].id);
    await key(t, "Enter");
    const opened = await ev(t, `window.__lab.state().opened`);
    assert.equal(opened.length, 1);
    assert.match(opened[0][0], /^fx:/, "the first application that waits for that question");
    assert.equal(opened[0][1], r[1].id);
    await key(t, "o");
    assert.deepEqual(await ev(t, `window.__lab.state().links`), [/^https:\/\/example\.com\/jobs\/\d+$/.exec((await ev(t, `window.__lab.state().links`))[0])[0]]);
    // the eve is in the same list of the keyboard
    await ev(t, `window.__lab.cursorTo("c:fx:4")`);
    await key(t, "Enter");
    const again = await ev(t, `window.__lab.state().opened`);
    assert.equal(again.at(-1)[0], "fx:4");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the review is a mode: Enter opens it on the first question, Enter shows the answer, 0-5 grades and brings the next, Esc leaves; the keys of the page are quiet inside", async () => {
  const t = await open();
  try {
    await load(t);
    await key(t, "Enter"); // no cursor: the review
    await t.page.waitFor(`document.querySelector("#ap-question")`);
    assert.equal(await ev(t, `document.querySelector("#ap-question").textContent`), "What is a lifetime in Rust, and why does the borrow checker need them?");
    assert.equal(await ev(t, `document.activeElement.id`), "ap-question", "the keyboard is on the question");
    assert.equal(await ev(t, `document.querySelector(".ap-block[data-block=eve]").hidden`), true, "the eve steps aside while he studies");
    assert.equal(await ev(t, `document.querySelector(".ap-box").hasAttribute("data-local-keys")`), false, "before the answer, 1-5 still travel");
    await shot(t, "aprender-review-question.png");
    await key(t, "Enter");
    await t.page.waitFor(`document.querySelector("#ap-answer")`);
    assert.match(await ev(t, `document.querySelector("#ap-answer").textContent`), /^A lifetime names how long/);
    assert.equal(await ev(t, `document.querySelector(".ap-box").hasAttribute("data-local-keys")`), true, "a box with keys of its own once the answer is out");
    assert.equal(await ev(t, `document.activeElement.id`), "ap-answer");
    await shot(t, "aprender-review-answer.png");
    // the shell's keys are quiet inside: j would move the cursor, u would ask about a question
    await key(t, "j");
    await key(t, "u");
    assert.equal(await ev(t, `window.__lab.cursor()`), null);
    assert.deepEqual(await acts(t), []);
    await key(t, "4");
    await t.page.waitFor(`window.__lab.state().acts.length === 1`);
    assert.deepEqual((await acts(t))[0], { path: "/api/learn/grade", body: { id: "c1", grade: 4 } });
    await t.page.waitFor(`document.querySelector("#ap-question") && document.querySelector("#ap-question").textContent.startsWith("How would you design")`);
    assert.equal(await ev(t, `document.querySelector("#ap-answer")`), null, "the next card comes with its answer hidden");
    assert.equal(await ev(t, `document.activeElement.id`), "ap-question");
    assert.equal(await ev(t, `window.__lab.snapshot().learn.due`), 11, "the grade moved the server's queue and the page followed");
    assert.match(await ev(t, `document.querySelector("#ap-drill-h").textContent`), /11 pendientes/);
    await key(t, "Escape");
    assert.equal(await ev(t, `document.querySelector(".ap-box").hidden`), true);
    assert.equal(await ev(t, `document.querySelector(".ap-block[data-block=eve]").hidden`), false);
    assert.ok(await ev(t, `document.activeElement.classList.contains("ap-start") || document.activeElement.classList.contains("ap-topic")`), "the keyboard goes back to where the review was opened");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("what he types as his own answer stays in the field: no key of the page acts on it, and Ctrl+Enter shows the answer", async () => {
  const t = await open();
  try {
    await load(t);
    await t.page.click(".ap-start");
    await t.page.waitFor(`document.querySelector("#ap-mine")`);
    await t.page.click("#ap-mine");
    for (const k of ["3", "j", "u", "a", "l", "m", "1"]) await key(t, k);
    assert.equal(await ev(t, `document.querySelector("#ap-mine").value`), "3jualm1", "every key is a letter of his answer");
    // the shell asks again every 30 s: the field, its text and the caret are where he left them
    await ev(t, `window.__lab.refresh((s) => { s.learn.due += 1; })`);
    assert.deepEqual(await ev(t, `[document.querySelector("#ap-mine").value, document.activeElement.id]`), ["3jualm1", "ap-mine"], "a refresh in the middle of an attempt keeps the text and the focus");
    assert.equal(await ev(t, `window.__lab.cursor()`), null);
    assert.deepEqual(await acts(t), [], "nothing was graded, resolved or sent");
    assert.equal(await ev(t, `window.__lab.state().apis.filter((p) => p.includes("answer")).length`), 0, "and what he typed never leaves the page");
    const CR = String.fromCharCode(13);
    await t.page.send("Input.dispatchKeyEvent", { type: "keyDown", key: "Enter", code: "Enter", modifiers: 2, text: CR }); // Ctrl+Enter inside the field
    await t.page.send("Input.dispatchKeyEvent", { type: "keyUp", key: "Enter", code: "Enter", modifiers: 2 });
    await t.page.waitFor(`document.querySelector("#ap-answer")`);
    assert.equal(await ev(t, `document.querySelector("#ap-mine").value`).then((v) => v.length > 0), true, "his attempt is still there beside the reference answer");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("states: al día, no review data, an engine that does not run, the dossier loading, failing and missing, and a review that cannot start", async () => {
  const t = await open();
  try {
    await load(t, "calm");
    assert.match(await ev(t, `document.querySelector(".aprender .mega").textContent`), /Nada que repasar/i);
    assert.equal(await ev(t, `document.querySelectorAll(".ap-row[data-row]").length`), 0);
    assert.match(await ev(t, `document.querySelector(".ap-block[data-block=missing] .ap-none").textContent`), /ninguna respuesta/i);
    assert.equal(await ev(t, `document.querySelector(".ap-start").hidden`), true, "nothing is due: no button that promises a review");
    await shot(t, "aprender-calm.png");

    await load(t, "nolearn");
    assert.match(await ev(t, `document.querySelector("#ap-drill-h").textContent`), /sin dato/i, "no review data in the snapshot says so, it does not draw zero");
    assert.equal(await ev(t, `document.querySelector(".ap-start").hidden`), true);

    await load(t, "empty");
    assert.deepEqual(await ev(t, `[document.querySelectorAll(".ap-row[data-row]").length, document.querySelector(".ap-start").hidden]`), [0, true]);

    await load(t, "stale");
    assert.match(await ev(t, `document.querySelector(".ap-sub").textContent`), /motor corrió por última vez|hasta que corra/i, "A15: an engine that does not run prepares nothing, and the page says it");

    // the dossier: loading, then read; an error; no dossier
    await ev(t, `window.__lab.load("rich", 22)`);
    await ev(t, `window.__lab.dossier("hold"); window.__lab.mountOnly(); window.__lab.refresh()`);
    assert.match(await ev(t, `document.querySelector(".ap-block[data-block=eve]").textContent`), /Leyendo el dossier/);
    await ev(t, `window.__lab.dossierRelease()`);
    await t.page.waitFor(`document.querySelectorAll(".ap-qs li").length === 3`);
    await ev(t, `window.__lab.dossier("error"); window.__lab.mountOnly(); window.__lab.refresh()`);
    await t.page.waitFor(`/No pude leer el dossier: No existe esa postulación/.test(document.querySelector(".ap-block[data-block=eve]").textContent)`);
    await ev(t, `window.__lab.dossier("none"); window.__lab.mountOnly(); window.__lab.refresh()`);
    await t.page.waitFor(`/no tiene dossier/i.test(document.querySelector(".ap-block[data-block=eve]").textContent)`);

    // a review that cannot start opens nothing and says why
    await load(t);
    await ev(t, `window.__lab.failNext(true)`);
    await key(t, "Enter");
    await t.page.waitFor(`window.__lab.state().toasts.length === 1`);
    assert.equal(await ev(t, `window.__lab.state().toasts[0].text`), "El servidor falló.");
    assert.equal(await ev(t, `document.querySelector(".ap-box").hidden`), true);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the first paint is a skeleton, not a spinner and not a flash of nothing: grey lines where the questions will be", async () => {
  const t = await open();
  try {
    await ev(t, `window.__lab.load("rich", 22)`);
    await ev(t, `window.__lab.mountOnly()`);
    assert.equal(await ev(t, `document.querySelector(".aprender").dataset.loading`), "true");
    assert.equal(await ev(t, `document.querySelectorAll(".ap-row.skeleton").length`), 3);
    assert.doesNotMatch(await ev(t, `document.getElementById("host").textContent`), /undefined|NaN/);
    await ev(t, `window.__lab.refresh()`);
    assert.equal(await ev(t, `document.querySelector(".aprender").dataset.loading || null`), null);
    assert.equal(await ev(t, `document.querySelectorAll(".ap-row.skeleton").length`), 0);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("a refresh that changes nothing writes nothing, and one that changes a number keeps the armed press, the cursor and what he typed", async () => {
  const t = await open();
  try {
    await load(t);
    await key(t, "j");
    await key(t, "u"); // armed
    await ev(t, `window.__mutations = 0; window.__obs = new MutationObserver((m) => { window.__mutations += m.filter((r) => r.attributeName !== "data-cursor").length; }); window.__obs.observe(document.getElementById("host"), { subtree: true, childList: true, characterData: true, attributes: true })`);
    await ev(t, `window.__lab.refresh()`);
    await t.page.wait(100);
    assert.equal(await ev(t, `window.__mutations`), 0, "the same snapshot again: not one node written");
    await ev(t, `window.__lab.refresh((s) => { s.learn.due += 1; })`);
    assert.equal((await rows(t))[0].armed, "resolve", "the press that was armed is still armed after a refresh that changed something else");
    assert.equal(await ev(t, `document.querySelectorAll("[data-cursor]").length`), 1);
    assert.match(await ev(t, `document.querySelector("#ap-drill-h").textContent`), /13 pendientes/);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("hostile text from the server stays text: no element, no script link, no bidi override, nothing wider than the sheet", async () => {
  const t = await open();
  try {
    await load(t, "hostile");
    const v = await ev(t, `(() => { const host = document.getElementById("host"); const text = host.textContent;
      return { imgs: host.querySelectorAll("img, script, iframe").length, badLinks: [...host.querySelectorAll("[href]")].filter((a) => !/^https?:/.test(a.getAttribute("href"))).length,
        bidi: /[\\u202a-\\u202e\\u2066-\\u2069]/.test(text), showsTag: text.includes("<img src=x"), wide: host.scrollWidth - host.clientWidth, onerror: [...host.querySelectorAll("*")].some((e) => e.hasAttribute("onerror")) }; })()`);
    assert.deepEqual([v.imgs, v.badLinks, v.bidi, v.onerror], [0, 0, false, false]);
    assert.ok(v.wide <= 1, "a 300-letter word does not push the sheet wider");
    assert.equal(v.showsTag, true, "the markup is shown as the letters it is");
    // a link that is not http is never offered
    const dangerous = await ev(t, `[...document.querySelectorAll(".ap-row")].filter((r) => r.dataset.actions && r.dataset.actions.includes("link")).length`);
    assert.ok(dangerous >= 1, "the other rows keep their posting link");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the page follows the language: the whole page in English with the same figures", async () => {
  const t = await open();
  try {
    await load(t);
    await ev(t, `window.__lab.lang("en")`);
    await t.page.waitFor(`document.fonts.status === "loaded"`);
    const v = await ev(t, `({ h1: document.querySelector(".aprender .mega").textContent.replace(/\\s+/g, " ").trim(), miss: document.querySelector("#ap-miss-h").textContent, start: document.querySelector(".ap-start").textContent, startAria: document.querySelector(".ap-start").getAttribute("aria-label"), over: document.getElementById("host").scrollHeight - document.getElementById("host").clientHeight })`);
    assert.equal(v.h1, "Stuck on six.");
    assert.match(v.miss, /\(6\): they block 6 forms/i);
    assert.match(v.start, /Start the review/i);
    assert.equal(v.startAria, "Start the review of all topics", "the button's name follows the language too");
    assert.ok(v.over <= 1, "the English page fits too");
    assert.doesNotMatch(await ev(t, `document.getElementById("host").textContent`), /⟦|undefined|NaN/);
    await ev(t, `window.__lab.lang("es")`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

// ---- A18: every text keeps 4.5:1 over the glass and the sky, measured on the pixels Chrome really paints ----

const lum = ([r, g, b]) => { const l = (c) => ((c /= 255) <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4); return 0.2126 * l(r) + 0.7152 * l(g) + 0.0722 * l(b); };

/** For each text that is on screen: the contrast of its colour against the ground behind its glyphs (the frame with the glyphs made invisible). */
async function textContrast(t, scenario, hora) {
  await ev(t, `window.__lab.load(${JSON.stringify(scenario)}, ${hora})`);
  await t.page.waitFor(`document.fonts.status === "loaded"`);
  await t.page.wait(250);
  const boxes = (await ev(t, `window.__lab.textBoxes()`)).filter((b) => b.rects.length > 0);
  await ev(t, `window.__lab.ghost(true)`);
  const png = decodePng(Buffer.from((await t.page.send("Page.captureScreenshot", { format: "png" })).data, "base64"));
  await ev(t, `window.__lab.ghost(false)`);
  const out = [];
  for (const box of boxes) {
    const color = box.color.match(/\d+(\.\d+)?/g).map(Number).slice(0, 3);
    const grounds = [];
    for (const r of box.rects) {
      const x0 = Math.max(0, Math.floor(r.left) - 2), x1 = Math.min(png.w - 1, Math.ceil(r.right) + 2);
      const y0 = Math.max(0, Math.floor(r.top) - 2), y1 = Math.min(png.h - 1, Math.ceil(r.bottom) + 2);
      for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) grounds.push(png.at(x, y));
    }
    if (grounds.length <= 40) continue; // a text cut off by its box (scrolled out of the list) has no pixels to measure
    grounds.sort((a, b) => lum(a) - lum(b));
    out.push({ text: box.text, cls: box.cls, ratio: contrast(color, grounds[Math.floor(grounds.length * 0.98)]) });
  }
  return out;
}

const HOURS = Array.from({ length: 24 }, (_, i) => i);

/**
 * Every text, every hour, both themes: the ones that fail, as sentences. `hours` is each hour the stage can be at (the sky is a different
 * picture at each one: night, dawn, the brightest afternoon, dusk), the glass is translucent in Tinta and opaque in Papel.
 */
async function sweep(t, scenarios, hours, themes, size) {
  const fails = [];
  for (const theme of themes) {
    await ev(t, `window.__lab.theme(${JSON.stringify(theme)})`);
    for (const scenario of scenarios) {
      for (const hora of hours) {
        const measured = await textContrast(t, scenario, hora);
        assert.ok(measured.length >= 20, `${scenario}: the headline, the rows, the figures and the topics are measured (${measured.length} texts at ${hora}:00)`);
        for (const r of measured) if (r.ratio < 4.5) fails.push(`${scenario}/${theme}/${hora}:00 ${size}: "${r.text}" (${r.cls}) is ${r.ratio.toFixed(2)}:1`);
      }
    }
  }
  return fails;
}

test("A18 all day: every text of APRENDER keeps 4.5:1 over the glass and the sky at EVERY hour, 0 to 23, in Tinta and in Papel, at 1920x1080 and 1440x900", async () => {
  for (const [width, height] of [[1920, 1080], [1440, 900]]) {
    const t = await open({ width, height });
    try {
      const fails = await sweep(t, ["rich"], HOURS, ["dark", "light"], `${width}x${height}`);
      assert.deepEqual(fails, []);
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  }
});

test("A18 on the data the other fixtures bring: the snapshot as the server sends it (topic names with spaces, wider than their column) and a hostile one, at every hour in Tinta, and at the turning hours in Papel", async () => {
  for (const [width, height] of [[1920, 1080], [1440, 900]]) {
    const t = await open({ width, height });
    try {
      const fails = [
        ...await sweep(t, ["plain", "hostile"], HOURS, ["dark"], `${width}x${height}`),
        ...await sweep(t, ["plain", "hostile", "calm"], [0, 6, 12, 15, 18, 22], ["light"], `${width}x${height}`),
      ];
      assert.deepEqual(fails, []);
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  }
});

test("a long name never reaches beyond its own cell: no text of a topic of the review is laid out under the bar beside it", async () => {
  const t = await open();
  try {
    await load(t, "plain");
    const over = await ev(t, `(() => { const out = []; for (const el of document.querySelectorAll(".ap-topic > span")) { const cell = el.getBoundingClientRect(); const range = document.createRange(); range.selectNodeContents(el);
      for (const r of range.getClientRects()) if (r.right > cell.right + 1 || r.left < cell.left - 1) out.push(el.className + ": " + el.textContent.slice(0, 30)); } return out; })()`);
    assert.deepEqual(over, [], "text clipped by its cell (an ellipsis) is still laid out wide, and the contrast measure reads what is painted under that width: the names wrap instead");
    await load(t, "hostile");
    const wide = await ev(t, `document.getElementById("host").scrollWidth - document.getElementById("host").clientWidth`);
    assert.ok(wide <= 1);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

// ---- the same measure on the MOUNTED page (the real shell, the real camera, the sky of the stage at the hour of ?clock=) ----

// Page side: each text of the sheet with the parts of it that are painted (clipped by every ancestor that clips), its colour, and the switch
// that makes the glyphs invisible so that a screenshot shows only what they are painted on. The same code as the lab's, for the same reason.
const PAGE_MEASURE = `(() => {
  const root = document.querySelector("#app .aprender");
  const clips = (el) => { const out = []; for (let e = el; e && e !== document.body; e = e.parentElement) { const cs = getComputedStyle(e); if (cs.overflowX !== "visible" || cs.overflowY !== "visible") out.push(e.getBoundingClientRect()); } return out; };
  window.__ghost = (on) => { for (const el of root.querySelectorAll("*")) { if (on) { el.style.setProperty("color", "transparent", "important"); el.style.setProperty("border-color", "transparent", "important"); el.style.setProperty("background-image", "none", "important"); } else { el.style.removeProperty("color"); el.style.removeProperty("border-color"); el.style.removeProperty("background-image"); } } };
  return [...root.querySelectorAll("*")].flatMap((el) => [...el.childNodes].filter((n) => n.nodeType === 3 && n.textContent.trim()).map((n) => {
    const range = document.createRange(); range.selectNodeContents(n); const boxes = clips(el);
    const rects = [...range.getClientRects()].map((r) => boxes.reduce((a, b) => ({ left: Math.max(a.left, b.left), top: Math.max(a.top, b.top), right: Math.min(a.right, b.right), bottom: Math.min(a.bottom, b.bottom) }), { left: r.left, top: r.top, right: r.right, bottom: r.bottom })).filter((r) => r.right - r.left > 1 && r.bottom - r.top > 1);
    return { cls: String(el.className), text: n.textContent.trim().slice(0, 40), color: getComputedStyle(el).color, rects };
  }));
})()`;

/** The page at `hour` (the stage's sky and the page's clock), on the aprender stop with its data in, measured on the pixels it paints. */
async function pageContrast(t, base, hour, theme, scenario) {
  const query = `?debug=1&clock=${hour}&lang=es&motion=calm&theme=${theme}${scenario ? "&scenario=" + scenario : ""}`;
  await t.page.goto(base + query);
  await t.page.waitFor(`!!document.querySelector("#app h1") && document.fonts.status === "loaded"`); // the page opens on the stop it was left on
  await ev(t, `window.__app.go("aprender")`);
  await t.page.waitFor(`document.getElementById("app").dataset.moment === "aprender" && document.querySelector("#app .aprender") && !document.querySelector("#app .aprender[data-loading]") && getComputedStyle(document.getElementById("app")).opacity === "1"`);
  await t.page.wait(450);
  const boxes = (await ev(t, PAGE_MEASURE)).filter((b) => b.rects.length > 0);
  await ev(t, `window.__ghost(true)`);
  const png = decodePng(Buffer.from((await t.page.send("Page.captureScreenshot", { format: "png" })).data, "base64"));
  await ev(t, `window.__ghost(false)`);
  const out = [];
  for (const box of boxes) {
    const color = box.color.match(/\d+(\.\d+)?/g).map(Number).slice(0, 3);
    const grounds = [];
    for (const r of box.rects) {
      const x0 = Math.max(0, Math.floor(r.left) - 2), x1 = Math.min(png.w - 1, Math.ceil(r.right) + 2);
      const y0 = Math.max(0, Math.floor(r.top) - 2), y1 = Math.min(png.h - 1, Math.ceil(r.bottom) + 2);
      for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) grounds.push(png.at(x, y));
    }
    if (grounds.length <= 40) continue;
    grounds.sort((a, b) => lum(a) - lum(b));
    out.push({ text: box.text, cls: box.cls, ratio: contrast(color, grounds[Math.floor(grounds.length * 0.98)]) });
  }
  return out;
}

test("A18 on the mounted page: the real shell over the real sky, every hour 0 to 23 in Tinta and the turning hours in Papel, at 1920x1080 and 1440x900", async () => {
  for (const [width, height] of [[1920, 1080], [1440, 900]]) {
    const t = await openPage({ query: "?debug=1&clock=9&lang=es&motion=calm", width, height });
    try {
      const base = t.url.split("?")[0];
      const fails = [];
      const run = async (hours, theme, scenario) => {
        for (const hour of hours) {
          const measured = await pageContrast(t, base, hour, theme, scenario);
          assert.ok(measured.length >= 20, `the headline, the rows, the figures and the topics are measured (${measured.length} texts at ${hour}:00 in ${theme})`);
          for (const r of measured) if (r.ratio < 4.5) fails.push(`page/${theme}/${scenario || "default"}/${hour}:00 ${width}x${height}: "${r.text}" (${r.cls}) is ${r.ratio.toFixed(2)}:1`);
        }
      };
      await run(HOURS, "dark");
      await run([0, 6, 12, 15, 18, 22], "light");
      if (width === 1920) await run([0, 9, 15, 18], "dark", "heavy");
      assert.deepEqual(fails, []);
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  }
});
