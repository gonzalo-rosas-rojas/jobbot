import test from "node:test";
import assert from "node:assert/strict";
import { openBuilt } from "./helpers/browser.mjs";
import { decodePng } from "./helpers/png.mjs";
import { contrast } from "./helpers/color.mjs";

// PROCESOS on the real sheet, over the real stage, with the real key table, cursor and Lenis, and a fake server (lab/f1-procesos.entry.js).
const open = (opts = {}) => openBuilt({ args: ["lab", "f1-procesos"], file: "dist/lab-f1-procesos.html", ...opts });
const ev = (t, expr) => t.page.eval(expr);
const load = (t, name = "rich", { reduced = true, hour = 9 } = {}) => ev(t, `window.__lab.reduced(${reduced}), window.__lab.load(${JSON.stringify(name)}, ${hour})`);
// tinykeys reads event.code for a letter or a digit, so a real key needs its code: "a" is KeyA, "3" is Digit3, "/" is Slash.
const code = (k) => (/^[a-z]$/.test(k) ? "Key" + k.toUpperCase() : /^\d$/.test(k) ? "Digit" + k : k === "/" ? "Slash" : k);
const key = (t, k) => t.page.key(k, k.length === 1 ? { code: code(k), text: k } : {});
const text = (t, sel) => ev(t, `(document.querySelector(${JSON.stringify(sel)}) || {}).textContent`);
const MOVE = `(snap) => { const sub = snap.board.find((c) => c.state === "submitted"); const i = sub.cards.findIndex((c) => c.key === "fx:5"); const [c] = sub.cards.splice(i, 1); sub.total -= 1; c.state = "replied"; c.days_in_state = 0; const rep = snap.board.find((c2) => c2.state === "replied"); rep.cards.push(c); rep.total += 1; }`;

for (const [width, height, cols] of [[1920, 1080, 6], [3440, 1440, 7], [1440, 900, 5]]) {
  test(`PROCESOS at ${width}x${height}: ${cols} piles to a line, the number and its breakdown are the model's, gold for what is his, nothing cut and no problems`, async () => {
    const t = await open({ width, height });
    try {
      const open74 = await load(t, "heavy");
      assert.equal(open74, 74);
      const v = await ev(t, `(() => { const q = (s) => document.querySelector(s); const all = (s) => [...document.querySelectorAll(s)];
        const lane = q('[data-lane="submitted"] .cells'); const sheet = q("#app").getBoundingClientRect(); const box = q(".procesos").getBoundingClientRect();
        const you = q('.cell[data-turn="you"]'); const quiet = q('[data-lane="ghosted"] .cell');
        return { h1: q("#procesos-title").textContent, mega: q(".mega").textContent, megaSize: getComputedStyle(q(".mega")).fontSize, megaFamily: getComputedStyle(q(".mega")).fontFamily,
          pieces: q(".pieces").textContent, cols: getComputedStyle(lane).gridTemplateColumns.split(" ").length, lanes: all("[data-lane]").map((l) => l.dataset.lane),
          youBorder: getComputedStyle(you).borderLeftColor, youWidth: getComputedStyle(you).borderLeftWidth, quietName: getComputedStyle(quiet.querySelector(".nm")).color,
          inSheet: box.left >= sheet.left && box.right <= sheet.right + 1 && box.bottom <= sheet.bottom + 1, overflowX: document.documentElement.scrollWidth - innerWidth,
          sheetTop: sheet.top, sheetBottom: sheet.bottom, filters: all(".chip[data-filter]").map((b) => b.textContent.replace(/\\s+/g, " ").trim() + ":" + b.getAttribute("aria-pressed")),
          fullRow: lane.children.length, eyebrow: q(".eyebrow").tagName, lenis: q(".scroller").hasAttribute("data-lenis-prevent"), overscroll: getComputedStyle(q(".scroller")).overscrollBehaviorY,
          hostScroll: q("#host").scrollHeight <= q("#host").clientHeight + 1 }; })()`);
      assert.match(v.h1, /^Procesos: cinco capas por postulación, \d\d:\d\d$/, "the stop says its name where the focus lands");
      assert.equal(v.eyebrow, "H1");
      assert.equal(v.mega, "74", "the number of the stops bar, from the model");
      assert.equal(v.megaSize, height >= 1010 ? (width >= 2400 ? "80px" : "64px") : "34px", "the number is --fs-6, one notch down on a low window");
      assert.match(v.megaFamily, /Barlow Condensed/);
      assert.match(v.pieces, /^abiertas · 3 te tocan · 62 esperan · 9 sin noticias · 2 cerradas · 15 por enviar$/);
      assert.equal(v.cols, cols);
      assert.deepEqual(v.lanes, ["presend", "submitted", "replied", "technical_test", "interview", "offer", "ghosted"]);
      assert.equal(v.youBorder, "rgb(251, 192, 36)", "gold left border: it is his turn");
      assert.equal(v.youWidth, "2px");
      assert.equal(v.quietName, "rgb(158, 172, 190)", "the quiet lane is --ink-dim, not an opacity");
      assert.equal(v.inSheet, true);
      assert.equal(v.overflowX, 0);
      assert.deepEqual([v.sheetTop >= 56, v.sheetBottom <= height - 20], [true, true], "from the HUD to the floor");
      assert.deepEqual(v.filters, ["Todas 74:true", "Te tocan 14:false", "Esperan 62:false", "Sin noticias 9:false"]);
      assert.equal(v.lenis, true);
      assert.equal(v.overscroll, "contain");
      assert.equal(v.hostScroll, true, "the host never scrolls: the list does");
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}

test("a pile is five bars: the second as long as the score, the gate solid when you approved it, hollow when you sent it by hand", async () => {
  const t = await open();
  try {
    await load(t, "rich");
    const v = await ev(t, `(() => { const stk = (key) => [...document.querySelector('[data-card="' + key + '"] .stk').children]; const s = stk("fx:1");
      return { n: s.length, w: s[1].style.getPropertyValue("--w"), width: getComputedStyle(s[1]).width, full: getComputedStyle(s[0]).width, states: s.map((i) => i.dataset.s), gate: s[4].dataset.gate,
        gateBg: getComputedStyle(s[4]).backgroundColor, dim: getComputedStyle(s[0]).backgroundColor }; })()`);
    assert.equal(v.n, 5);
    assert.equal(v.w, "91%", "the length of the second layer is the score");
    assert.ok(Math.abs(parseFloat(v.width) - 0.91 * parseFloat(v.full)) < 1.5, `${v.width} of ${v.full}`);
    assert.deepEqual(v.states, ["done", "done", "done", "done", "done"]);
    assert.equal(v.gate, "dim", "it went out by itself");
    await ev(t, `window.__lab.refresh((snap) => { snap.board.find((c) => c.state === "submitted").cards.find((c) => c.key === "fx:1").approved = true; })`);
    assert.equal(await ev(t, `document.querySelector('[data-card="fx:1"] .stk').children[4].dataset.gate`), "clear");
    assert.equal(await ev(t, `getComputedStyle(document.querySelector('[data-card="fx:1"] .stk').children[4]).backgroundColor`), "rgb(233, 239, 247)", "solid ink");
    await ev(t, `window.__lab.refresh((snap) => { const c = snap.board.find((c) => c.state === "submitted").cards.find((c) => c.key === "fx:1"); c.source = "manual"; })`);
    assert.equal(await ev(t, `document.querySelector('[data-card="fx:1"] .stk').children[4].dataset.gate`), "hollow");
    assert.equal(await ev(t, `getComputedStyle(document.querySelector('[data-card="fx:1"] .stk').children[4]).backgroundColor`), "rgba(0, 0, 0, 0)", "hollow: no fill");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("120 open: the list scrolls INSIDE the sheet, says how many are below, and a wheel step on it never moves the camera (a wheel step elsewhere does)", async () => {
  const t = await open();
  try {
    await load(t, "stress");
    // the count below is measured a frame after the list is laid out: wait for it to be written (at most 4 s) instead of 300 ms, which a busy machine did not always give
    await t.page.waitFor(`(document.querySelector(".scroll-cue") || { textContent: "" }).textContent.length > 0`, { timeout: 4000 });
    assert.equal(await ev(t, `window.__lab.state().acts.length`), 0);
    const v = await ev(t, `(() => { const q = (s) => document.querySelector(s); const sc = q(".scroller"); return { total: sc.scrollHeight, view: sc.clientHeight, open: q(".mega").textContent,
      cue: q(".scroll-cue").textContent, cueShown: getComputedStyle(q(".scroll-cue")).display, track: getComputedStyle(q(".track")).display, scroll: window.__lab.scrolls(), foot: q(".legend").getBoundingClientRect().top, listBottom: sc.getBoundingClientRect().bottom,
      tall: document.documentElement.scrollHeight > innerHeight }; })()`);
    assert.equal(v.open, "120");
    assert.ok(v.total > v.view + 200, `the list is longer than its window (${v.total} in ${v.view})`);
    assert.match(v.cue, /^↓ \d+ más · la rueda mueve la lista, no la cámara$/);
    assert.equal(v.cueShown, "flex");
    assert.equal(v.track, "block");
    assert.ok(v.listBottom <= v.foot + 1, "the list ends where the foot begins");
    assert.equal(v.tall, true, "the page has room to scroll, so the control below is meaningful");
    assert.deepEqual(v.scroll, { page: 0, list: 0 });
    // walking with j: the bottom of the list fades out, and the row under the cursor is lifted out of the fade, so it is never one half gone
    for (let i = 0; i < 112; i++) await key(t, "j");
    await t.page.wait(250);
    const walk = await ev(t, `(() => { const sc = document.querySelector(".scroller"); const b = sc.getBoundingClientRect(); const r = document.querySelector('[data-cursor="true"]').getBoundingClientRect();
      return { list: window.__lab.scrolls().list, gap: b.bottom - r.bottom, top: r.top - b.top, mask: getComputedStyle(sc).maskImage || getComputedStyle(sc).webkitMaskImage }; })()`);
    assert.ok(walk.list > 100, `the list followed the cursor (${walk.list})`);
    assert.ok(walk.gap >= 60, `the cursor row is above the fade (${walk.gap} px from the bottom)`);
    assert.ok(walk.top >= 0, `and inside the list (${walk.top})`);
    assert.match(walk.mask, /linear-gradient/, "the fade is a mask, not a veil that lets the text under it show");
    await ev(t, `document.querySelector(".scroller").scrollTo({ top: 0 })`);
    await t.page.wait(100);
    const wheel =(x, y, deltaY) => t.page.send("Input.dispatchMouseEvent", { type: "mouseWheel", x, y, deltaX: 0, deltaY });
    for (let i = 0; i < 4; i++) { await wheel(600, 600, 120); await t.page.wait(60); }
    await t.page.wait(700);
    const inside = await ev(t, `window.__lab.scrolls()`);
    assert.ok(inside.list > 100, `the list moved (${inside.list})`);
    assert.equal(inside.page, 0, "the camera did not");
    const cue = await ev(t, `document.querySelector(".scroll-cue").textContent`);
    assert.notEqual(cue, v.cue, "the count below went down as the list moved");
    for (let i = 0; i < 6; i++) { await wheel(1500, 500, 120); await t.page.wait(60); }
    await t.page.wait(900);
    const outside = await ev(t, `window.__lab.scrolls()`);
    assert.ok(outside.page > 0, `a step over the stage moves the camera (${outside.page})`);
    assert.equal(outside.list, inside.list, "and not the list");
    // the end of the list: the cue goes away, nothing is left below
    await ev(t, `document.querySelector(".scroller").scrollTo({ top: 99999 })`);
    await t.page.waitFor(`getComputedStyle(document.querySelector(".scroll-cue")).display === "none"`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

// The cue measures the list in the frame after a trip's swap (ACEPTACION A7): a write between two of its reads, or a custom property that the ~1,000 elements
// of the piles inherit, turned that frame into a second long one at CPU x4 (3 style recalculations and 2 layouts, 20-45 ms, for one scroll of the list).
test("120 open: moving the list costs one style recalculation and one layout (the cue reads first and writes after, and the fade lengths do not inherit)", async () => {
  const t = await open();
  try {
    await load(t, "stress");
    await t.page.waitFor(`(document.querySelector(".scroll-cue") || { textContent: "" }).textContent.length > 0`, { timeout: 4000 });
    await t.page.wait(400);
    for (const top of [150, 99999, 0]) {
      const before = await t.page.metrics();
      await ev(t, `document.querySelector(".scroller").scrollTop = ${top}`);
      await t.page.wait(300);                                       // the frame that measures, and the one after
      const after = await t.page.metrics();
      assert.deepEqual([after.RecalcStyleCount - before.RecalcStyleCount, after.LayoutCount - before.LayoutCount], [1, 1], `scrollTop ${top}: the style recalculations and layouts of one scroll`);
    }
    assert.equal(await ev(t, `getComputedStyle(document.querySelector(".procesos .cell")).getPropertyValue("--fb-a")`), "0px", "a pile does not inherit the fade of the list: the registered length keeps its initial value");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the stable place: a pile that answers leaves a dotted gap where it was, nobody slides up, and it TRAVELS 420 ms to the end of its new lane; no hash, and reduced motion is a cut", async () => {
  const t = await open();
  try {
    await load(t, "rich", { reduced: false });
    await t.page.wait(100);
    const before = await ev(t, `(() => { const r = (k) => { const b = document.querySelector('[data-card="' + k + '"]').getBoundingClientRect(); return [Math.round(b.left), Math.round(b.top)]; };
      return { keys: [...document.querySelectorAll('[data-lane="submitted"] [data-card]')].map((e) => e.dataset.card), pos: Object.fromEntries(["fx:1", "fx:19", "fx:20", "fx:5"].map((k) => [k, r(k)])),
        replied: [...document.querySelectorAll('[data-lane="replied"] [data-card]')].map((e) => e.dataset.card), hash: location.hash }; })()`);
    assert.deepEqual(before.keys, ["fx:1", "fx:5", "fx:19", "fx:20"]);
    const flight = await ev(t, `(() => { window.__lab.refresh(${MOVE}); const li = document.querySelector('[data-slot="card:fx:5"]'); return li.getAnimations().map((a) => [a.effect.getTiming().duration, a.effect.getTiming().easing]); })()`);
    assert.deepEqual(flight, [[420, "cubic-bezier(0.2, 0, 0, 1)"]], "the shared element: 420 ms, the jump curve");
    // arrived = the flight of the shared element is over (the state, not 600 ms: a busy machine starts a 420 ms flight late), at most 4 s
    await t.page.waitFor(`document.querySelector('[data-slot="card:fx:5"]').getAnimations().length === 0`, { timeout: 4000 });
    const after = await ev(t, `(() => { const r = (k) => { const b = document.querySelector('[data-card="' + k + '"]').getBoundingClientRect(); return [Math.round(b.left), Math.round(b.top)]; };
      const tomb = document.querySelector('[data-lane="submitted"] .slot.tomb');
      return { slots: [...document.querySelectorAll('[data-lane="submitted"] .slot')].map((e) => (e.querySelector("[data-card]") || { dataset: { card: "~" + e.textContent } }).dataset.card),
        pos: Object.fromEntries(["fx:1", "fx:19", "fx:20"].map((k) => [k, r(k)])), tombText: tomb && tomb.textContent, tombBox: tomb && [Math.round(tomb.getBoundingClientRect().left), Math.round(tomb.getBoundingClientRect().top)],
        tombBorder: tomb && getComputedStyle(tomb).borderBottomStyle, tombHeight: tomb && Math.round(tomb.getBoundingClientRect().height), cellHeight: Math.round(document.querySelector(".slot:not(.tomb)").getBoundingClientRect().height),
        replied: [...document.querySelectorAll('[data-lane="replied"] [data-card]')].map((e) => e.dataset.card), hash: location.hash, anim: document.querySelector('[data-slot="card:fx:5"]').getAnimations().length,
        count: document.querySelector(".mega").textContent, you: document.querySelector('[data-lane="replied"] .lane-n').textContent }; })()`);
    assert.deepEqual(after.slots, ["fx:1", "~→ respondió", "fx:19", "fx:20"], "the gap stays in the order of the lane");
    assert.deepEqual(after.tombBox, before.pos["fx:5"], "and it is exactly where the pile was");
    assert.equal(after.tombText, "→ respondió");
    assert.equal(after.tombBorder, "dashed");
    assert.equal(after.tombHeight, after.cellHeight, "the size of a pile");
    for (const k of ["fx:1", "fx:19", "fx:20"]) assert.deepEqual(after.pos[k], before.pos[k], `${k} did not move`);
    assert.deepEqual(after.replied, [...before.replied, "fx:5"], "it arrives at the end of its new lane");
    assert.equal(after.hash, before.hash, "no hash: the place is not an address");
    assert.equal(after.anim, 0, "the flight is over");
    assert.equal(after.count, "10", "it is still one of the open ones");
    // the gap lives on while the page does (leaving and coming back to the stop), and a new load of the page forgets it
    await ev(t, `window.__lab.load("rich", 9)`);
    assert.equal(await ev(t, `document.querySelectorAll(".slot.tomb").length`), 1, "the same session: the gap is still there");
    await ev(t, `window.__lab.reset(), window.__lab.load("rich", 9)`);
    assert.equal(await ev(t, `document.querySelectorAll(".slot.tomb").length`), 0, "a new page load closes it");
    // reduced motion: the pile simply arrives
    await ev(t, `window.__lab.load("rich", 9), window.__lab.reduced(true)`);
    const cut = await ev(t, `(() => { window.__lab.refresh(${MOVE}); return document.querySelector('[data-slot="card:fx:5"]').getAnimations().length; })()`);
    assert.equal(cut, 0);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("keys: f 1-4 filter, / searches and marks the letters, v flips to the table, j k Enter walk and open; nothing fires inside the field", async () => {
  const t = await open();
  try {
    await load(t, "heavy");
    const shown = () => ev(t, `[...document.querySelectorAll(".procesos [data-card]")].filter((e) => e.getClientRects().length).length`);
    const total = await shown();
    await key(t, "f"); await key(t, "2");
    await t.page.waitFor(`document.querySelector('.chip[data-filter="turn"]').getAttribute("aria-pressed") === "true"`);
    const yours = await ev(t, `[...document.querySelectorAll('.procesos [data-card]')].every((e) => e.dataset.turn === "you")`);
    assert.equal(yours, true, "f 2: only the ones that are his");
    assert.ok((await shown()) < total);
    assert.equal(await ev(t, `document.querySelectorAll(".slot.tomb").length`), 0);
    assert.match(await ev(t, `window.__lab.state().said.at(-1)`), /^Te tocan · 14$/, "and it says so aloud: the number the chip shows (3 live and 11 of Por enviar that are his)");
    await key(t, "f"); await key(t, "1");
    await t.page.waitFor(`document.querySelector('.chip[data-filter="all"]').getAttribute("aria-pressed") === "true"`);
    assert.equal(await shown(), total);
    await key(t, "f"); await key(t, "4");
    await t.page.waitFor(`document.querySelector('.chip[data-filter="quiet"]').getAttribute("aria-pressed") === "true"`);
    assert.ok((await shown()) > 0 && (await ev(t, `[...document.querySelectorAll('.procesos [data-card]')].every((e) => e.dataset.turn === "quiet")`)));
    await key(t, "f"); await key(t, "1");

    // the search: `/` takes the keyboard to the field; typing narrows, marks, and Esc clears then leaves
    await key(t, "/");
    assert.equal(await ev(t, `document.activeElement.className.includes("find-input")`), true);
    await t.page.send("Input.insertText", { text: "xequill" });
    await t.page.waitFor(`document.querySelectorAll(".procesos [data-card]").length <= 3`);
    const found = await ev(t, `[...document.querySelectorAll(".procesos [data-card]")].map((e) => [e.querySelector(".nm").textContent, [...e.querySelectorAll("mark.hit")].map((m) => m.textContent).join("").toLowerCase()])`);
    assert.ok(found.length >= 1 && found.length <= 3, `the search narrows the field (${found.length})`);
    assert.deepEqual(found.find(([name]) => name === "Xequill Freight"), ["Xequill Freight", "xequill"], "the letters that matched are marked, as text parts");
    assert.equal(await ev(t, `document.querySelectorAll(".procesos .nm mark, .procesos .nm *").length > 0 && document.querySelector(".procesos .nm").innerHTML.includes("<img")`), false);
    // keys do not fire inside the field: `v` is a letter there, not the table
    await key(t, "v"); await key(t, "j");
    assert.equal(await ev(t, `document.querySelector(".procesos").dataset.mode`), "field");
    assert.equal(await ev(t, `window.__lab.cursor()`), null);
    await key(t, "Escape");
    assert.equal(await ev(t, `document.querySelector(".find-input").value`), "", "Esc clears");
    assert.equal(await ev(t, `document.activeElement.className.includes("find-input")`), true, "and stays");
    assert.equal(await shown(), total);
    await key(t, "Escape");
    assert.equal(await ev(t, `document.activeElement.className.includes("find-input")`), false, "the second Esc leaves");
    await t.page.send("Input.insertText", { text: "" });
    await key(t, "/");
    await t.page.send("Input.insertText", { text: "zzzzqq" });
    await t.page.waitFor(`document.querySelector(".empty") && !document.querySelector(".empty").hidden`);
    assert.equal(await text(t, ".empty-text"), "Nada se parece a «zzzzqq».");
    await t.page.click(".empty [data-reset]");
    await t.page.waitFor(`document.querySelector(".empty").hidden`);
    assert.equal(await ev(t, `document.querySelector(".find-input").value`), "");

    // the table, the walk and the opening
    await key(t, "v");
    await t.page.waitFor(`document.querySelector(".procesos").dataset.mode === "table"`);
    assert.equal(await ev(t, `document.querySelectorAll('.trow[data-card]').length`), 89, "the table lists every pile: the 74 open and the 15 that have not gone out");
    await key(t, "j");
    const first = await ev(t, `window.__lab.cursor()`);
    assert.match(first, /^c:/);
    assert.equal(await ev(t, `document.querySelector('[data-cursor="true"]').classList.contains("trow")`), true);
    await key(t, "j"); await key(t, "k");
    assert.equal(await ev(t, `window.__lab.cursor()`), first);
    await key(t, "Enter");
    await t.page.waitFor(`document.getElementById("detail").classList.contains("open")`);
    assert.deepEqual((await ev(t, `window.__lab.state()`)).opened[0][0], first.slice(2));
    assert.equal(await ev(t, `document.querySelectorAll("#detail .plate").length`), 5);
    await key(t, "Escape");
    await t.page.waitFor(`!document.getElementById("detail").classList.contains("open")`);
    await key(t, "v");
    await t.page.waitFor(`document.querySelector(".procesos").dataset.mode === "field"`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the table: seven columns, his first and then by age, the turn cell with its key, a header that never scrolls away", async () => {
  const t = await open();
  try {
    await load(t, "rich");
    await key(t, "v");
    await t.page.waitFor(`document.querySelector(".procesos").dataset.mode === "table"`);
    const v = await ev(t, `(() => { const q = (s) => document.querySelector(s); const rows = [...document.querySelectorAll(".trow[data-card]")];
      return { head: [...q(".thead").children].map((c) => c.textContent), cols: getComputedStyle(rows[0]).gridTemplateColumns.split(" ").length, turns: rows.map((r) => r.dataset.turn), n: rows.length,
        first: rows[0].textContent.replace(/\\s+/g, " "), act: q(".trow .c-turn .turn-open") && q(".trow .c-turn .turn-open").textContent, kbd: q(".trow .c-turn kbd").textContent, headPos: getComputedStyle(q(".thead")).position, headOutside: !q(".scroller").contains(q(".thead")), headBottom: q(".thead").getBoundingClientRect().bottom, listTop: q(".scroller").getBoundingClientRect().top, headX: q(".thead .c-turn").getBoundingClientRect().right, rowX: q(".trow[data-card] .c-turn").getBoundingClientRect().right,
        legend: q(".legend").textContent.replace(/\\s+/g, " "), mode: q("[data-mode-toggle]").textContent.replace(/\\s+/g, " ").trim(), days: rows.filter((r) => r.dataset.turn === "you").map((r) => Number(r.querySelector(".c-age").textContent.replace(/\\D/g, ""))) }; })()`);
    assert.deepEqual(v.head, ["Proceso", "Etapa", "Edad", "Último movimiento", "Siguiente paso", "Cadencia", "Turno"]);
    assert.equal(v.cols, 7);
    assert.equal(v.n, 18);
    const order = ["you", "snoozed", "wait", "quiet"];
    assert.deepEqual(v.turns, [...v.turns].sort((a, b) => order.indexOf(a) - order.indexOf(b)), "his first, then the snoozed, then waiting, then no news");
    assert.deepEqual(v.days, [...v.days].sort((a, b) => b - a), "by age inside what is his");
    assert.equal(v.kbd, "↵");
    assert.equal(v.headPos, "absolute");
    assert.equal(v.headOutside, true, "the header is above the list, so what scrolls never shows through it");
    assert.ok(Math.abs(v.headBottom - v.listTop) <= 1, `the list starts where the header ends (${v.headBottom} / ${v.listTop})`);
    assert.ok(Math.abs(v.headX - v.rowX) <= 1, `the header columns line up with the rows (${v.headX} / ${v.rowX})`);
    assert.match(v.legend, /^te tocan primero, luego por edad en la etapa · j k moverte · ↵ abrir · x retirar · s posponer · la rueda mueve la lista, no la cámara$/);
    assert.equal(v.mode, "v Campo", "the button names the view it goes to");
    await key(t, "v");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("a verb that cannot be undone takes two presses: the first arms it (and says so in the foot), Esc takes it back, a held key never confirms, the second does it and the pile travels", async () => {
  const t = await open();
  try {
    await load(t, "rich", { reduced: false });
    await ev(t, `window.__lab.cursorTo("c:fx:9")`);
    await key(t, "c");
    await t.page.waitFor(`document.querySelector('[data-card="fx:9"]').dataset.armed === "confirm"`);
    assert.equal(await ev(t, `window.__lab.state().acts.length`), 0, "the first press only arms");
    assert.equal(await text(t, ".armed"), "Ya la envié: pulsa otra vez para confirmar, Esc cancela");
    assert.equal(await ev(t, `document.querySelector(".legend").hidden`), true);
    await key(t, "Escape");
    await t.page.waitFor(`!document.querySelector('[data-card="fx:9"]').dataset.armed`);
    assert.equal(await ev(t, `document.querySelector(".armed").hidden`), true);
    assert.equal(await ev(t, `window.__lab.state().acts.length`), 0, "and nothing was sent");
    await key(t, "c");
    await t.page.waitFor(`document.querySelector('[data-card="fx:9"]').dataset.armed === "confirm"`);
    await t.page.wait(450);
    // a key that is held down repeats: that is not a second, deliberate press
    await t.page.send("Input.dispatchKeyEvent", { type: "keyDown", key: "c", code: "KeyC", text: "c", autoRepeat: true });
    await t.page.send("Input.dispatchKeyEvent", { type: "keyUp", key: "c", code: "KeyC" });
    assert.equal(await ev(t, `window.__lab.state().acts.length`), 0, "a repeat is not a press");
    await key(t, "c");
    await t.page.waitFor(`window.__lab.state().acts.length === 1`);
    assert.deepEqual((await ev(t, `window.__lab.state()`)).acts, [{ path: "/api/confirm", body: { key: "fx:9" } }]);
    await t.page.waitFor(`document.querySelector('[data-lane="submitted"] [data-card="fx:9"]')`);
    assert.equal(await ev(t, `document.querySelectorAll('[data-lane="presend"] .slot.tomb').length`), 1, "the form that was stuck leaves its place");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the other verbs: o opens the posting only when it is http(s), s asks until when, x asks why, r asks which answer; a key the pile does not have is not caught", async () => {
  const t = await open();
  try {
    await load(t, "rich");
    await ev(t, `window.__lab.cursorTo("c:fx:1")`);
    await key(t, "o");
    assert.deepEqual((await ev(t, `window.__lab.state()`)).links, ["https://example.com/fx/1"]);
    await key(t, "s");
    await t.page.waitFor(`document.querySelector(".menu") && document.querySelector(".menu").getClientRects().length > 0`);
    assert.match(await ev(t, `document.querySelector(".menu").textContent`), /Posponer|Hasta cuándo/i);
    await key(t, "Escape");
    await t.page.waitFor(`!document.querySelector(".menu") || document.querySelector(".menu").getClientRects().length === 0`);
    assert.equal(await ev(t, `window.__lab.state().acts.length`), 0, "closing the menu asked for nothing");
    await key(t, "r");
    await t.page.waitFor(`document.querySelector(".menu") && document.querySelector(".menu").getClientRects().length > 0`);
    await key(t, "1");
    await t.page.waitFor(`window.__lab.state().acts.length === 1`);
    const act = (await ev(t, `window.__lab.state()`)).acts[0];
    assert.equal(act.path, "/api/mark");
    assert.equal(act.body.key, "fx:1");
    assert.equal(act.body.state, "replied", "the first answer the card allows");
    await t.page.waitFor(`document.querySelector('[data-lane="replied"] [data-card="fx:1"]')`);
    // a pile with no such verb: `x` on a card that cannot be withdrawn does nothing and is not swallowed
    await ev(t, `window.__lab.cursorTo("c:fx:15")`);
    await key(t, "c");
    assert.equal(await ev(t, `document.querySelector('[data-card="fx:15"]').dataset.armed || ""`), "", "a scored card cannot be confirmed");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the drawer content: the score with its six weights, five labelled plates, the trail, the ruler over the 45 days, and the buttons run the same verbs as the keys", async () => {
  const t = await open();
  try {
    await load(t, "rich");
    await ev(t, `window.__lab.openDrawer("fx:1")`);
    await t.page.waitFor(`document.getElementById("detail").classList.contains("open")`);
    const v = await ev(t, `(() => { const q = (s) => document.querySelector(s); const all = (s) => [...document.querySelectorAll(s)];
      return { title: q("#detail h2").textContent, eyebrow: q(".pd-eyebrow").textContent, score: q(".pd-score .n").textContent, bars: all(".pd .bars li").map((li) => li.querySelector(".bk").textContent), top: q(".pd .bm[data-top]") ? 1 : 0,
        plates: all(".pd .plate").map((p) => [p.dataset.layer, p.dataset.state, p.querySelector(".pl-name").textContent]), tilted: all(".pd .pl-face").length, trail: all(".pd .trail li").length,
        ruler: !!q(".pd .ruler"), now: q(".rl-now-l").textContent, rule: q(".pd-rule").textContent, acts: all(".pd .act").map((b) => b.dataset.act + ":" + b.querySelector("kbd").textContent), sticky: getComputedStyle(q(".pd-next")).position,
        box: q("#detail").getBoundingClientRect().left >= q("#app").getBoundingClientRect().right, inert: q("#app").inert }; })()`);
    assert.equal(v.title, "Alpha Test Co");
    assert.equal(v.eyebrow, "Enviada, día 12");
    assert.equal(v.score, "91");
    assert.deepEqual(v.bars, ["Encaje", "Sueldo", "Entrevista", "Estabilidad", "Aprendizaje", "Otro"], "the order COLA and both drawers share (hallazgos-pendientes E5)");
    assert.equal(v.top, 1);
    assert.deepEqual(v.plates.map((p) => p[0]), ["aviso", "puntaje", "cv", "respuestas", "compuerta"]);
    assert.deepEqual(v.plates.map((p) => p[2]), ["Aviso", "Puntaje", "CV", "Respuestas", "Revisión final"]);
    assert.ok(v.plates.every((p) => p[1] === "done"));
    assert.equal(v.tilted, 5);
    assert.equal(v.trail, 3);
    assert.equal(v.ruler, true);
    assert.equal(v.now, "hoy, día 12");
    assert.match(v.rule, /^Seguimiento a los 8–10 y 15–17 días; sin noticias a los 45/);
    assert.deepEqual(v.acts, ["register:r", "link:o", "snooze:s", "withdraw:x", "newtask:n"]);
    assert.equal(v.sticky, "sticky");
    assert.equal(v.box, true, "it sits beside the sheet, not over it");
    assert.equal(v.inert, false, "and nothing behind it is inert: the list stays alive");
    // a button is a key: the list's own cursor row gets the verb
    await t.page.click('#detail [data-act="link"]');
    assert.deepEqual((await ev(t, `window.__lab.state()`)).links, ["https://example.com/fx/1"]);
    await t.page.click('#detail [data-act="register"]');
    await t.page.waitFor(`document.querySelector(".menu") && document.querySelector(".menu").getClientRects().length > 0`);
    await key(t, "Escape");
    // the stuck form: its own button is a two-step
    await ev(t, `window.__lab.openDrawer("fx:9")`);
    assert.deepEqual(await ev(t, `(() => { const b = document.querySelector('#detail [data-act="confirm"]'); return [b.querySelector("kbd").textContent, b.querySelector(".act-label").textContent]; })()`), ["c", "Ya la envié"]);
    await t.page.click('#detail [data-act="confirm"]');
    assert.deepEqual(await ev(t, `(() => { const b = document.querySelector('#detail [data-act="confirm"]'); return [b.querySelector("kbd").textContent, b.querySelector(".act-label").textContent]; })()`), ["c", "¿Seguro? Pulsa otra vez"], "the first click only arms it, in the verb: the key stays");
    assert.equal(await ev(t, `window.__lab.state().acts.length`), 0);
    assert.equal(await ev(t, `window.__lab.state().reloads`), 0);
    // the second click does it, and the drawer reads the application again (a refresh of the snapshot does not redraw it)
    await t.page.wait(450);
    await t.page.click('#detail [data-act="confirm"]');
    await t.page.waitFor(`window.__lab.state().acts.length === 1`);
    await t.page.waitFor(`window.__lab.state().reloads === 1`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the states: loading is a skeleton with the sheet's shape, empty says so, a filter that finds nothing offers the way back, and a view that throws says so instead of dying", async () => {
  const t = await open();
  try {
    await load(t, "empty");
    const e = await ev(t, `(() => { const q = (s) => document.querySelector(s); return { text: q(".empty-text").textContent, shown: !q(".empty").hidden, kind: q(".empty").dataset.kind, mega: q(".mega").textContent, lanes: document.querySelectorAll("[data-lane]").length, button: !!q(".empty [data-reset]"), closed: q(".closed").hidden,
      pieces: q(".pieces").textContent, run: q(".runline").textContent }; })()`);
    assert.equal(e.text, "Ninguna postulación viva todavía.");
    assert.equal(await text(t, ".empty"), "Ninguna postulación viva todavía.", "nothing else is in the box (a null child once printed the word null)");
    assert.deepEqual([e.shown, e.kind, e.mega, e.lanes, e.button, e.closed], [true, "none", "0", 0, false, true]);
    assert.match(e.run, /sin corridas todavía/, "A15: no last_run_at, so no run is invented");
    await load(t, "rich");
    await ev(t, `window.__lab.refresh((snap) => { const g = snap.board.find((c) => c.state === "ghosted"); g.cards = []; g.total = 0; })`);
    await key(t, "f"); await key(t, "4");
    await t.page.waitFor(`document.querySelector(".empty") && !document.querySelector(".empty").hidden`);
    assert.equal(await text(t, ".empty-text"), "Ninguna cumple este filtro.");
    await t.page.click(".empty [data-reset]");
    await t.page.waitFor(`document.querySelector(".empty").hidden`);
    assert.equal(await ev(t, `document.querySelector('.chip[data-filter="all"]').getAttribute("aria-pressed")`), "true");
    // a view that throws: the page says so, the rest of the panel is unchanged, and the next good snapshot brings it back
    await ev(t, `window.__lab.fail(true)`);
    assert.equal(await text(t, ".fail-text"), "No pude leer los procesos. El resto del panel sigue igual.");
    assert.equal(await ev(t, `document.querySelectorAll(".fail [data-retry]").length`), 1);
    assert.equal(t.page.problems().length, 1, "the failure is reported once, on the console");
    t.page.log.length = 0;
    await ev(t, `window.__lab.refresh()`);
    await t.page.waitFor(`!document.querySelector(".fail") && document.querySelectorAll("[data-lane]").length > 0`);
    assert.deepEqual(t.page.problems(), []);
    // the skeleton: mounted and waiting for its first snapshot
    await ev(t, `window.__lab.loading()`);
    const s = await ev(t, `({ loading: document.querySelector(".procesos").dataset.loading, busy: document.querySelector(".procesos").getAttribute("aria-busy"), cells: document.querySelectorAll(".skel-cell").length, say: document.querySelector(".procesos > .sr").textContent })`);
    assert.deepEqual([s.loading, s.busy, s.cells, s.say], ["true", "true", 18, "Cargando los procesos…"]);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("hostile text stays text: no element is made from a name, a link that is not http(s) is no link, long names are cut at the edge, nothing widens the sheet; and English has no missing word", async () => {
  const t = await open();
  try {
    await load(t, "hostile");
    const v = await ev(t, `(() => { const host = document.querySelector("#host"); const all = [...host.querySelectorAll("*")].map((e) => e.tagName);
      return { img: host.querySelectorAll("img, script, iframe, svg image").length, tags: [...new Set(all)].sort().join(","), bad: [...document.querySelectorAll("a[href]")].filter((a) => !/^https?:/.test(a.href)).length,
        text: host.textContent, overflow: host.scrollWidth - host.clientWidth, over: [...host.querySelectorAll(".nm")].filter((e) => e.scrollWidth > e.clientWidth + 1).length > 0, long: [...host.querySelectorAll(".nm")].map((e) => e.textContent.length).sort((a, b) => b - a)[0],
        bidi: /[\\u202a-\\u202e\\u2066-\\u2069]/.test(host.textContent), control: /[\\u0000-\\u0008\\u000b-\\u001f]/.test(host.textContent) }; })()`);
    assert.equal(v.img, 0);
    assert.equal(v.bad, 0);
    assert.ok(/<img src=x onerror=alert\(1\)>/.test(v.text), "the markup that was sent shows as the words it is");
    assert.equal(v.overflow, 0, "nothing widens the sheet");
    assert.equal(v.over, true, "a long name is cut at the edge of its box (the whole of it is one hover away)");
    assert.ok(v.long <= 60, `names are cut before they reach the page (${v.long})`);
    assert.deepEqual([v.bidi, v.control], [false, false]);
    await key(t, "v");
    await t.page.waitFor(`document.querySelector(".procesos").dataset.mode === "table"`);
    assert.equal(await ev(t, `document.querySelectorAll("#host img, #host script").length`), 0);
    assert.equal(await ev(t, `[...document.querySelectorAll("#host a[href]")].filter((a) => !/^https?:/.test(a.href)).length`), 0);
    assert.ok((await ev(t, `window.__lab.state()`)).links.length === 0);
    await ev(t, `window.__lab.cursorTo(window.__lab.cursor() || "c:fx:2")`);
    // the one whose address is not http(s) has no `o`: the key is not caught
    const idx = await ev(t, `[...document.querySelectorAll(".trow[data-card]")].findIndex((r) => !r.dataset.actions.includes("link"))`);
    assert.ok(idx >= 0, "one row has no posting link");
    await ev(t, `window.__lab.cursorTo("c:" + [...document.querySelectorAll(".trow[data-card]")][${idx}].dataset.card)`);
    await key(t, "o");
    assert.deepEqual((await ev(t, `window.__lab.state()`)).links, [], "javascript: is never opened");
    // English: every word exists
    await ev(t, `window.__lab.lang("en"), window.__lab.openDrawer(document.querySelector(".trow[data-card]").dataset.card)`);
    const en = await ev(t, `document.querySelector("#host").textContent + " " + document.querySelector("#detail").textContent`);
    assert.ok(!/⟦|NaN|undefined|\\bnull\\b/.test(en), en.slice(0, 200));
    assert.match(await text(t, "#procesos-title"), /^Processes as a table, \d\d:\d\d$/);
    await ev(t, `window.__lab.lang("es")`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("a phone: flat, two piles to a line, buttons of 44 px, the list is the page itself (no inner scroll), no horizontal scroll", async () => {
  const t = await open({ width: 390, height: 844 });
  try {
    await load(t, "heavy");
    const v = await ev(t, `(() => { const q = (s) => document.querySelector(s); const sc = q(".scroller"); const cell = q(".cell").getBoundingClientRect(); const chip = q(".chip[data-filter]").getBoundingClientRect();
      return { cols: getComputedStyle(q(".cells")).gridTemplateColumns.split(" ").length, cellH: Math.round(cell.height), chipH: Math.round(chip.height), pos: getComputedStyle(sc).position, overflow: getComputedStyle(sc).overflowY, over: document.documentElement.scrollWidth - innerWidth,
        cue: getComputedStyle(q(".scroll-cue")).display, kbd: getComputedStyle(q(".chip .k") || q(".mode .k")).display, tall: document.documentElement.scrollHeight > innerHeight }; })()`);
    assert.equal(v.cols, 2);
    assert.ok(v.cellH >= 44, `${v.cellH}`);
    assert.ok(v.chipH >= 44, `${v.chipH}`);
    assert.deepEqual([v.pos, v.overflow], ["static", "visible"]);
    assert.equal(v.over, 0);
    assert.equal(v.cue, "none");
    assert.equal(v.tall, true);
    await key(t, "v");
    await t.page.waitFor(`document.querySelector(".procesos").dataset.mode === "table"`);
    assert.equal(await ev(t, `document.documentElement.scrollWidth - innerWidth`), 0);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

// ---- A18 over the glass: the text measured on the pixels Chrome really paints ----

const lum = ([r, g, b]) => { const l = (c) => ((c /= 255) <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4); return 0.2126 * l(r) + 0.7152 * l(g) + 0.0722 * l(b); };

async function textContrast(t) {
  const boxes = await ev(t, `window.__lab.textBoxes()`);
  await ev(t, `window.__lab.ghost(true)`);
  const png = decodePng(Buffer.from((await t.page.send("Page.captureScreenshot", { format: "png" })).data, "base64"));
  await ev(t, `window.__lab.ghost(false)`);
  const rows = [];
  for (const box of boxes) {
    const color = box.color.match(/\d+(\.\d+)?/g).map(Number).slice(0, 3);
    const grounds = [];
    for (const r of box.rects) {
      const x0 = Math.max(0, Math.floor(r.left) - 2), x1 = Math.min(png.w - 1, Math.ceil(r.right) + 2);
      const y0 = Math.max(0, Math.floor(r.top) - 2), y1 = Math.min(png.h - 1, Math.ceil(r.bottom) + 2);
      for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) grounds.push(png.at(x, y));
    }
    if (grounds.length <= 40) continue; // text scrolled out of the window
    grounds.sort((a, b) => lum(a) - lum(b));
    rows.push({ text: box.text, cls: box.cls, ratio: contrast(color, grounds[Math.floor(grounds.length * 0.98)]) });
  }
  return rows;
}

for (const [theme, hours] of [["dark", [9, 12, 17]], ["light", [12]]]) {
  test(`A18 on PROCESOS in ${theme === "dark" ? "Tinta" : "Papel"}: every text of the field, the table and the drawer keeps 4.5:1 over the sheet at ${hours.join(", ")} h`, async () => {
    const t = await open({ width: 1920, height: 1080 });
    try {
      for (const hour of hours) {
        await ev(t, `document.documentElement.setAttribute("data-theme", ${JSON.stringify(theme)})`);
        await load(t, "stress", { hour });
        await ev(t, `window.__lab.openDrawer(document.querySelector('[data-lane="submitted"] [data-card]').dataset.card), window.__lab.go(${hour})`);
        await t.page.wait(400);
        let rows = await textContrast(t);
        assert.ok(rows.length > 60, `the head, the lanes, the piles and the drawer (${rows.length})`);
        for (const r of rows) assert.ok(r.ratio >= 4.5, `field: "${r.text}" (${r.cls}) is ${r.ratio.toFixed(2)}:1 at ${hour}:00 in ${theme}`);
        await key(t, "v");
        await t.page.waitFor(`document.querySelector(".procesos").dataset.mode === "table"`);
        await t.page.wait(200);
        rows = await textContrast(t);
        for (const r of rows) assert.ok(r.ratio >= 4.5, `table: "${r.text}" (${r.cls}) is ${r.ratio.toFixed(2)}:1 at ${hour}:00 in ${theme}`);
        await key(t, "v");
      }
      assert.deepEqual(t.page.problems(), []);
    } finally { await t.close(); }
  });
}
