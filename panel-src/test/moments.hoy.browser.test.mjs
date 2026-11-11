import test from "node:test";
import assert from "node:assert/strict";
import { openBuilt } from "./helpers/browser.mjs";

const open = (opts = {}) => openBuilt({ args: ["lab", "f0-hoy"], file: "dist/lab-f0-hoy.html", ...opts });
const ev = (t, expr) => t.page.eval(expr);
const NOON = "Date.parse('2026-10-07T12:00:00Z')"; // 09:00 in Santiago
const load = (t, name = "rich", visit = 0) => ev(t, `window.__lab.reduced(true), window.__lab.load(${JSON.stringify(name)}, 9), window.__lab.now(${NOON}), window.__lab.visit(${visit})`);
// tinykeys reads event.code for a letter, so a real key needs its code: "a" is KeyA.
const key = (t, k) => t.page.key(k, k.length === 1 ? { code: "Key" + k.toUpperCase(), text: k } : {});
const text = (t, sel) => ev(t, `(document.querySelector(${JSON.stringify(sel)}) || {}).textContent`);

for (const [width, height, h1Size] of [[1920, 1080, "64px"], [3440, 1440, "80px"]]) {
  test(`HOY at ${width}x${height}: the headline is the real number in words, three buttons for LO SIGUIENTE, six one-button rows, everything in view, no inline style`, async () => {
    const t = await open({ width, height });
    try {
      await load(t, "rich", 8);
      const v = await ev(t, `(() => { const q = (s) => document.querySelector(s); const all = (s) => [...document.querySelectorAll(s)];
        const host = q("#host"); const h1 = q(".hoy .mega");
        return { h1: h1.textContent, size: getComputedStyle(h1).fontSize, family: getComputedStyle(h1).fontFamily, color: getComputedStyle(h1).color, runs: h1.children.length, caps: getComputedStyle(h1).textTransform,
          fits: host.scrollHeight <= host.clientHeight, next: all(".next .act").map((b) => b.dataset.action), rows: all(".rows .row").length,
          oneButton: all(".rows .row").every((r) => r.querySelectorAll(".act").length === 1), inline: all(".hoy [style]").length,
          lenis: q(".rows").hasAttribute("data-lenis-prevent"), overscroll: getComputedStyle(q(".rows")).overscrollBehaviorY,
          figs: all(".fig-n").map((n) => n.textContent), slots: all(".strip rect.busy").length + "/" + all(".strip rect.seen").length, since: all(".srow").length,
          aria: q(".hoy").getAttribute("aria-labelledby"), focusable: q(".mega").getAttribute("tabindex") }; })()`);
      assert.equal(v.h1.replace(/\s+/g, " "), "Te tocan trece.");
      assert.equal(v.size, h1Size, "the headline is --fs-6: LO SIGUIENTE leads");
      assert.match(v.family, /Barlow Condensed/);
      assert.equal(v.runs, 0, "one run of the display face: no accent word and no serif inside the headline (spec §4.2)");
      assert.equal(v.caps, "uppercase");
      assert.equal(v.color, "rgb(233, 239, 247)", "the whole headline is in the ink, gold is for what is his to do");
      assert.equal(v.fits, true, "nothing of HOY is cut or scrolled at this size");
      assert.deepEqual(v.next, ["done", "open", "link"], "a task: do it, look at it, open its posting");
      assert.equal(v.rows, 6);
      assert.equal(v.oneButton, true);
      assert.equal(v.inline, 0, "no style attribute: the CSP forbids them");
      assert.equal(v.lenis, true);
      assert.equal(v.overscroll, "contain");
      assert.deepEqual(v.figs, ["120", "14", "10", "1"]);
      assert.ok(/^\d+\/\d+$/.test(v.slots) && Number(v.slots.split("/")[1]) > 10, `the strip draws the half hours (${v.slots})`);
      assert.ok(v.since >= 1 && v.since <= 5);
      assert.deepEqual([v.aria, v.focusable], ["hoy-title", "-1"], "the shell lands on the headline");
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}

test("heavy: the headline is the real number as a figure, the list is cut to six and says how many more", async () => {
  const t = await open();
  try {
    await load(t, "heavy");
    assert.equal((await text(t, ".hoy .mega")).replace(/\s+/g, " "), "Te tocan 68.");
    assert.equal(await ev(t, `document.querySelectorAll(".rows .row").length`), 6);
    assert.equal(await text(t, ".more"), "y 61 más");
    await t.page.click(".more");
    assert.deepEqual((await ev(t, `window.__lab.state()`)).went, [["procesos", { filter: "turn" }]], "it opens PROCESOS filtered to what is his");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("calm: the truth about the last run, no list, no LO SIGUIENTE", async () => {
  const t = await open();
  try {
    await load(t, "empty");
    const v = await ev(t, `({ calm: document.querySelector(".hoy").dataset.calm, h1: document.querySelector(".mega").textContent.trim(), ask: document.querySelector(".ask").textContent,
      next: !!document.querySelector(".next"), rest: document.querySelector(".rest").hidden, since: document.querySelector(".since").hidden, foot: document.querySelector(".foot-line").textContent })`);
    assert.equal(v.calm, "true");
    assert.equal(v.h1, "Nada pendiente.");
    assert.equal(v.ask, "No hay corridas todavía.", "A15: no last_run_at, so no run is invented");
    assert.deepEqual([v.next, v.rest, v.since], [false, true, true]);
    assert.match(v.foot, /sin corridas todavía/);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("a permanent verb by key: the first press arms and says so, a second inside 350 ms is the same press, then the row is dim until the server answers and gone after", async () => {
  const t = await open();
  try {
    await load(t);
    await ev(t, `window.__lab.cursorTo("c:fx:12")`);
    await key(t, "a");
    let s = await ev(t, `({ armed: document.querySelector('[data-card="fx:12"]').dataset.armed, label: document.querySelector('[data-card="fx:12"] .act-label').textContent, said: window.__lab.state().said, acts: window.__lab.state().acts })`);
    assert.equal(s.armed, "approve");
    assert.equal(s.label, "¿Seguro? Pulsa otra vez");
    assert.deepEqual([s.said, s.acts], [["¿Seguro? Pulsa otra vez"], []], "said aloud for the screen reader, and nothing was sent");
    await key(t, "a");
    assert.deepEqual((await ev(t, `window.__lab.state().acts`)), [], "a second press inside 350 ms is a double click, not a decision");
    await t.page.wait(450);
    await ev(t, `window.__lab.hold()`);
    await key(t, "a");
    await t.page.waitFor(`window.__lab.state().acts.length === 1`);
    assert.deepEqual((await ev(t, `window.__lab.state().acts`)), [{ path: "/api/approve", body: { key: "fx:12" } }]);
    const dim = await ev(t, `(() => { const r = document.querySelector('[data-card="fx:12"]'); return [r.dataset.pending, r.getAttribute("aria-busy"), getComputedStyle(r).opacity, getComputedStyle(r).pointerEvents]; })()`);
    assert.deepEqual(dim, ["true", "true", "0.3", "none"], "the row is at 30 % from the first instant");
    await ev(t, `window.__lab.release(true)`);
    await t.page.waitFor(`!document.querySelector('[data-card="fx:12"]')`);
    assert.equal((await text(t, ".hoy .mega")).replace(/\s+/g, " "), "Te tocan doce.", "the real number went down by one");
    assert.ok((await ev(t, `window.__lab.state().sounds`)).includes("approve"));
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("a no gives the row back: it is not dim, it is still there, the headline did not move and it sounds like an error", async () => {
  const t = await open();
  try {
    await load(t);
    await ev(t, `window.__lab.cursorTo("c:fx:12")`);
    await key(t, "a");
    await t.page.wait(450);
    await ev(t, `window.__lab.hold()`);
    await key(t, "a");
    await t.page.waitFor(`document.querySelector('[data-card="fx:12"]').dataset.pending === "true"`);
    await ev(t, `window.__lab.release(false)`);
    await t.page.waitFor(`!document.querySelector('[data-card="fx:12"]').dataset.pending`);
    assert.equal(await ev(t, `getComputedStyle(document.querySelector('[data-card="fx:12"]')).opacity`), "1");
    assert.equal((await text(t, ".hoy .mega")).replace(/\s+/g, " "), "Te tocan trece.");
    assert.ok((await ev(t, `window.__lab.state().sounds`)).includes("error"));
  } finally { await t.close(); }
});

test("a refresh never takes a half-pressed button away: same node, still armed, still focused; and a refresh that changes nothing writes nothing", async () => {
  const t = await open();
  try {
    await load(t);
    await ev(t, `window.__lab.cursorTo("c:fx:12")`);
    await key(t, "a");
    await ev(t, `(() => { const b = document.querySelector('[data-card="fx:12"] .act'); window.__keep = b; b.focus(); })()`);
    // the same snapshot again: not one node, attribute or text may be written (the cursor mark is the shell's, not HOY's)
    await ev(t, `(() => { window.__mut = 0; window.__obs = new MutationObserver((l) => { window.__mut += l.filter((r) => r.attributeName !== "data-cursor").length; }); window.__obs.observe(document.querySelector(".hoy"), { subtree: true, childList: true, attributes: true, characterData: true }); })()`);
    await ev(t, `window.__lab.refresh()`);
    assert.equal(await ev(t, `window.__mut`), 0, "an identical snapshot changes nothing in the DOM");
    // a snapshot where the armed row's own text, the list around it and the foot all changed
    await ev(t, `window.__lab.refresh((s) => { s.awaiting_approval.cards[0].reason_short = "Cambió el motivo"; s.today.tasks[1].text = "Otra tarea distinta"; s.controls.last_run_at = new Date(Date.parse(s.generated_at) - 3600000).toISOString(); })`);
    const s = await ev(t, `({ same: window.__keep === document.querySelector('[data-card="fx:12"] .act'), connected: window.__keep.isConnected, armed: window.__keep.dataset.armed, label: window.__keep.textContent,
      focused: document.activeElement === window.__keep, why: document.querySelector('[data-card="fx:12"] .why').textContent, task: document.body.textContent.includes("Otra tarea distinta") })`);
    assert.deepEqual([s.same, s.connected, s.armed, s.focused, s.task], [true, true, "true", true, true]);
    assert.match(s.label, /¿Seguro\?/);
    assert.match(s.why, /Cambió el motivo/, "and the row did learn its new text");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("keys: j and k walk LO SIGUIENTE then the rows, Enter and o and s act on the row under the cursor, h on LO SIGUIENTE when there is none", async () => {
  const t = await open();
  try {
    await load(t);
    await key(t, "j");
    assert.equal(await ev(t, `window.__lab.cursor()`), "t:101", "the first j lands on LO SIGUIENTE");
    assert.equal(await ev(t, `document.querySelector(".next").dataset.cursor`), "true");
    await key(t, "j");
    await key(t, "j");
    assert.equal(await ev(t, `document.querySelector('.rows .row[data-cursor="true"]') !== null`), true);
    await key(t, "Enter");
    const opened = (await ev(t, `window.__lab.state().opened`)).at(-1);
    assert.equal(typeof opened[0], "string", "the drawer is asked for an application");
    await ev(t, `window.__lab.cursorTo("c:fx:9")`);
    await key(t, "o");
    const links = await ev(t, `window.__lab.state().links`);
    assert.equal(links.length, 1, "o opens the posting in a new tab");
    assert.ok(links[0].startsWith("https://"), "and only an http(s) address is ever opened");
    await ev(t, `window.__lab.cursorTo("c:fx:10")`);
    await key(t, "s");
    await key(t, "1"); // since T19 `s` asks until when: the first choice is tomorrow
    await t.page.waitFor(`window.__lab.state().acts.length === 1`);
    assert.deepEqual((await ev(t, `window.__lab.state().acts`))[0], { path: "/api/snooze", body: { key: "fx:10", until: "1d" } });
    await t.page.waitFor(`!document.querySelector('[data-card="fx:10"]')`);
    // a key the row does not have does nothing (a task is not approved)
    await ev(t, `window.__lab.cursorTo("t:101")`);
    await key(t, "a");
    assert.equal((await ev(t, `window.__lab.state().acts`)).length, 1);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("clicks: a button does its verb, the body of a row opens its application, only http links are links", async () => {
  const t = await open();
  try {
    await load(t);
    await t.page.click('.rows .row[data-task="103"] .what');
    const opened = (await ev(t, `window.__lab.state().opened`))[0];
    assert.deepEqual(opened, ["fx:4", "task:103"], "the application and the row it came from (the drawer gives focus back to it)");
    await t.page.click('.next .act[data-action="open"]');
    assert.equal((await ev(t, `window.__lab.state().opened`)).length, 2);
    await t.page.click('.next .act[data-action="done"]');
    assert.deepEqual(await ev(t, `window.__lab.state().acts`), [], "done is permanent: the first click only arms it");
    assert.equal(await text(t, '.next .act[data-action="done"] .act-label'), "¿Seguro? Pulsa otra vez");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the language: every word follows, the buttons keep their nodes, the number is spelled in each language", async () => {
  const t = await open();
  try {
    await load(t, "rich", 8);
    await ev(t, `window.__keep = document.querySelector('.next .act')`);
    await ev(t, `window.__lab.lang("en")`);
    assert.equal((await text(t, ".hoy .mega")).replace(/\s+/g, " "), "You have thirteen.");
    assert.equal(await text(t, '.next .act[data-action="done"] .act-label'), "Done");
    assert.match(await text(t, ".eyebrow"), /^What is mine today\? 09:00/);
    assert.match(await text(t, ".since-title"), /^Since your last visit/);
    assert.match(await text(t, ".more"), /^and 6 more$/);
    await ev(t, `window.__lab.lang("es")`);
    assert.equal((await text(t, ".hoy .mega")).replace(/\s+/g, " "), "Te tocan trece.");
    assert.match(await text(t, ".more"), /^y 6 más$/);
  } finally { await t.close(); }
});

test("what is new carries the mark for a moment; and there is no 'since your last visit' on a first visit", async () => {
  const t = await open();
  try {
    await load(t, "rich", 0);
    assert.equal(await ev(t, `document.querySelector(".since").hidden`), true);
    await ev(t, `window.__lab.visit(8)`);
    assert.equal(await ev(t, `document.querySelector(".since").hidden`), false);
    await ev(t, `window.__lab.fresh(["task:101"]); window.__lab.refresh()`);
    assert.equal(await ev(t, `document.querySelector(".next").dataset.fresh`), "true");
    await t.page.waitFor(`!document.querySelector(".next").dataset.fresh`, { timeout: 6000 });
  } finally { await t.close(); }
});

test("hostile text from outside stays text on the page: no foreign element, no link, nothing wider than the sheet, bidi isolated", async () => {
  const t = await open();
  try {
    await load(t, "rich", 8);
    const long = "Z".repeat(300);
    const evil = "<img src=x onerror=alert(1)> \u202eevil\u202c " + long;
    // the hostile strings are built here and handed over as JSON, so what the page receives is exactly their characters
    const hostile = {
      company: evil, title: "<script>alert(2)</script> <iframe src=x></iframe>", url: "javascript:alert(3)", task: evil + "\nsecond line",
      subject: "<b onmouseover=alert(4)>subject</b> " + long, summary: "<a href='javascript:alert(5)'>click</a> " + long,
    };
    await ev(t, `window.__lab.refresh((s) => {
      const h = ${JSON.stringify(hostile)};
      s.awaiting_approval.cards[0].company = h.company;
      s.awaiting_approval.cards[0].title = h.title;
      s.awaiting_approval.cards[0].url = h.url;
      s.today.tasks[0].text = h.task;
      s.today.emails[0].company = h.company;
      s.today.emails[0].subject = h.subject;
      s.today.emails[0].summary = h.summary;
    })`);
    const v = await ev(t, `(() => { const hoy = document.querySelector(".hoy"); const q = (s) => [...hoy.querySelectorAll(s)];
      const row = document.querySelector('[data-card="fx:12"]');
      return { foreign: q("img, script, iframe, b, i, style, svg a, object, embed").map((e) => e.tagName), links: q("a, [href]").length,
        handlers: q("*").filter((e) => [...e.attributes].some((a) => /^on/i.test(a.name))).length,
        text: row.textContent.includes("<script>alert(2)</script>"), overflowX: hoy.scrollWidth > hoy.clientWidth, sheetX: document.documentElement.scrollWidth > innerWidth,
        isolate: getComputedStyle(row.querySelector(".co")).unicodeBidi, wrap: getComputedStyle(document.querySelector(".next-title")).overflowWrap,
        rowH: row.getBoundingClientRect().height, all: hoy.textContent };
    })()`);
    assert.deepEqual(v.foreign, [], "markup in the data is text, never an element");
    assert.equal(v.links, 0, "HOY draws no link of its own: a posting is opened by the shell, and only when it is http(s)");
    assert.equal(v.handlers, 0);
    assert.equal(v.text, true, "the hostile string is on the page as the characters it is made of");
    assert.equal(v.overflowX, false, "nothing is wider than the section");
    assert.equal(v.sheetX, false, "nor than the page");
    assert.equal(v.isolate, "isolate");
    assert.equal(v.wrap, "anywhere");
    assert.ok(v.rowH <= 40, `the row stays one line (${v.rowH} px)`);
    assert.doesNotMatch(v.all, /[\u202a-\u202e\u2066-\u2069]/, "no override character reaches the page");
    assert.deepEqual(t.page.problems(), [], "and the CSP saw no inline handler, script or frame");
  } finally { await t.close(); }
});
