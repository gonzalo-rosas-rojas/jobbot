// The page as Rust serves it (the demo's own server cut out), with the test answering /api/* (spec A1, A9): it paints the whole frame, the sky,
// the dial, the HUD, the stops and the sheet with the skeleton of its moment BEFORE the first snapshot arrives; the token of the launch link
// goes out as a header and leaves the address; and an answer that never comes, or comes as a refusal, leaves a page that says so in words.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { openPage } from "./helpers/page.mjs";

const rich = JSON.parse(readFileSync(new URL("./fixtures/snapshot.rich.json", import.meta.url), "utf8"));
const ev = (t, expr) => t.page.eval(expr);

/** A server that holds the snapshot until `release()`, then answers it (or refuses with `status`). */
function held({ status = 200, body = rich } = {}) {
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  return {
    release,
    answer: async (path) => {
      if (!path.startsWith("/api/snapshot")) return undefined;
      await gate;
      return { status, body: status === 200 ? body : { message: "No autorizado" } };
    },
  };
}

const painted = (t) => ev(t, `(() => {
  const sky = document.getElementById("sky"), blank = document.createElement("canvas"); blank.width = sky.width; blank.height = sky.height;
  const q = (s) => document.querySelector(s);
  return { skyPainted: sky.width > 0 && sky.toDataURL() !== blank.toDataURL(), brand: q(".hud-brand")?.textContent.trim().toUpperCase(), tabs: document.querySelectorAll('#stops [role="tab"]').length,
    sheet: !!q("#app"), host: q(".sheet-host")?.children.length || 0, dial: !!q("#dial svg, #dial canvas, #dial *"), title: document.title, search: location.search, h1: q("#app h1")?.textContent || null };
})()`);

test("the stage, the HUD, the stops and the sheet are painted before the snapshot; the token goes out as a header and leaves the address", async () => {
  const server = held();
  const t = await openPage({ kind: "real", query: "?t=tok-123&debug=1&clock=9", width: 1440, height: 900, answer: server.answer, wait: false });
  try {
    await t.page.waitFor(`document.querySelectorAll('#stops [role="tab"]').length === 5`);
    await t.page.wait(400);
    const before = await painted(t);
    assert.equal(before.skyPainted, true, "the sky is drawn with no data");
    assert.equal(before.brand, "PANEL");
    assert.equal(before.tabs, 5);
    assert.ok(before.sheet && before.host > 0, "the sheet holds the skeleton of its moment");
    assert.equal(before.dial, true);
    assert.equal(before.title, "Panel");
    assert.equal(before.search.includes("t="), false, "the token is out of the address");
    assert.equal(t.api.length, 1, "one request so far: the snapshot");
    assert.deepEqual([t.api[0].path, t.api[0].token], ["/api/snapshot", "tok-123"]);
    server.release();
    await t.page.waitFor(`/te tocan|no hay nada|nada urgente|hoy/i.test(document.querySelector("#app h1")?.textContent || "")`);
    const after = await painted(t);
    assert.ok(after.h1, "the data fills the sheet");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("the same on a phone: the header and the sheet exist before the data, and the data fills them", async () => {
  const server = held();
  const t = await openPage({ kind: "real", query: "?t=tok-123&debug=1&clock=9", width: 390, height: 844, answer: server.answer, wait: false });
  try {
    await t.page.waitFor(`document.querySelectorAll('#stops [role="tab"]').length === 6`);
    const before = await ev(t, `({ mode: window.__app.mode(), host: document.querySelector(".sheet-host").children.length, brand: document.querySelector(".hud-brand").textContent.trim().toUpperCase() })`);
    assert.deepEqual([before.mode, before.brand], ["flat", "PANEL"]);
    assert.ok(before.host > 0);
    server.release();
    await t.page.waitFor(`!!document.querySelector("#app h1")?.textContent`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

test("a refused token leaves the page standing and says so in the sheet, in words", async () => {
  const server = held({ status: 401 });
  const t = await openPage({ kind: "real", query: "?t=wrong&debug=1&clock=9", width: 1440, height: 900, answer: server.answer, wait: false });
  try {
    await t.page.waitFor(`document.querySelectorAll('#stops [role="tab"]').length === 5`);
    server.release();
    await t.page.waitFor(`!!document.querySelector(".sheet-banner") && !document.querySelector(".sheet-banner").hidden && document.querySelector(".sheet-banner").textContent.trim().length > 10`);
    const v = await ev(t, `({ banner: document.querySelector(".sheet-banner").textContent, page: document.body.innerText })`);
    assert.doesNotMatch(v.banner + v.page, /undefined|null|NaN|\[object/);
    assert.equal((await painted(t)).skyPainted, true, "the sky stays");
  } finally { await t.close(); }
});

test("a server that answers 500 leaves the page painted and the banner with the words the server gave", async () => {
  const t = await openPage({ kind: "real", query: "?t=tok&debug=1&clock=9", width: 1440, height: 900, answer: async () => ({ status: 500, body: { message: "El servidor falló" } }), wait: false });
  try {
    await t.page.waitFor(`!!document.querySelector(".sheet-banner") && document.querySelector(".sheet-banner").textContent.includes("El servidor falló")`);
    assert.equal((await painted(t)).tabs, 5);
  } finally { await t.close(); }
});

// ---- What the server says is said to him in his words (core/humanize.js, wired in ui/shell.js)

/** A server that answers the snapshot with the fixture and every action the way the real one does: with the internal key and an almost-ISO date. */
function talking() {
  return {
    answer: async (path, init) => {
      const body = init && init.body ? JSON.parse(init.body) : {};
      if (path.startsWith("/api/snapshot")) return { status: 200, body: rich };
      if (path === "/api/approve") return { status: 200, body: { ok: true, message: `Aprobada: ${body.key}` } };
      if (path === "/api/snooze") return { status: 200, body: { ok: true, message: `Pospuesta hasta 2026-10-12 18:03: ${body.key}` } };
      if (path === "/api/unsnooze") return { status: 200, body: { ok: true, message: `De vuelta en tu lista: ${body.key}` } };
      if (path === "/api/stop") return { status: 200, body: { ok: true, message: "Listo: jobbot se detuvo." } };
      return undefined;
    },
  };
}
const key = (t, k) => t.page.key(k, { code: /\d/.test(k) ? "Digit" + k : "Key" + k.toUpperCase(), text: k });
// this server has no capture and no detail to give: the page asks for them and gets a 404, which Chrome logs; that is not what these tests look at
const problems = (t) => t.page.problems().filter((p) => !p.text.includes("/api/shot?") && !p.text.includes("/api/job?"));
const toastText = (t) => ev(t, `document.querySelector(".toast").textContent`);
const liveText = (t) => ev(t, `document.getElementById("live").textContent`);
const COMPANY = Object.fromEntries([...rich.board.flatMap((c) => c.cards), ...rich.needs_gonza.cards, ...rich.awaiting_approval.cards].map((c) => [c.key, c]));

test("approving says the company and the role, not the key the server wrote; the live region says the same", async () => {
  const server = talking();
  const t = await openPage({ kind: "real", query: "?t=tok&debug=1&clock=18#cola", width: 1440, height: 900, answer: server.answer, wait: false });
  try {
    await t.page.send("Emulation.setTimezoneOverride", { timezoneId: "America/Santiago" });
    await t.page.waitFor(`document.querySelectorAll("#app [data-row]").length >= 1`);
    await key(t, "a");                                  // approving asks twice: the second press, not before 350 ms
    await t.page.wait(450);
    await key(t, "a");
    await t.page.waitFor(`document.querySelector(".toast").classList.contains("show")`);
    const sent = JSON.parse(t.api.find((r) => r.path === "/api/approve").body);
    const card = COMPANY[sent.key];
    assert.ok(card, `the page approved a card of the fixture (${sent.key})`);
    const text = await toastText(t);
    assert.ok(text.startsWith("Aprobada: ") && text.includes(card.company), `the toast names the company: ${text}`);
    assert.ok(!text.includes(sent.key), `and not the key: ${text}`);
    await t.page.waitFor(`document.getElementById("live").textContent.includes(${JSON.stringify(card.company)})`);
    assert.ok(!(await liveText(t)).includes(sent.key), "the live region neither");
    assert.deepEqual(problems(t), []);
  } finally { await t.close(); }
});

test("putting a card aside says when it comes back the way he writes it, and Deshacer says the company too", async () => {
  const server = talking();
  const t = await openPage({ kind: "real", query: "?t=tok&debug=1&clock=18#cola", width: 1440, height: 900, answer: server.answer, wait: false });
  try {
    await t.page.send("Emulation.setTimezoneOverride", { timezoneId: "America/Santiago" });
    await t.page.waitFor(`document.querySelectorAll("#app [data-row]").length >= 1`);
    await key(t, "s");
    await t.page.waitFor(`!!document.querySelector("[role=menu], .menu")`);
    await key(t, "1");
    await t.page.waitFor(`document.querySelector(".toast").textContent.startsWith("Pospuesta")`);
    const sent = JSON.parse(t.api.find((r) => r.path === "/api/snooze").body);
    const card = COMPANY[sent.key];
    const text = await toastText(t);
    assert.ok(text.startsWith("Pospuesta hasta lun 12 oct, 18:03: ") && text.includes(card.company), text);
    assert.ok(!text.includes(sent.key) && !text.includes("2026-10-12"), `no key, no raw date: ${text}`);
    await t.page.click(".toast button");
    await t.page.waitFor(`document.querySelector(".toast").textContent.startsWith("De vuelta")`);
    const back = await toastText(t);
    assert.ok(back.includes(card.company) && !back.includes(sent.key), back);
    assert.deepEqual(problems(t), []);
  } finally { await t.close(); }
});

test("a message with nothing to translate is said as it came, and an unknown key is not given a name, and markup stays text", async () => {
  const server = talking();
  const t = await openPage({ kind: "real", query: "?t=tok&debug=1&clock=18#sistema", width: 1440, height: 900, answer: server.answer, wait: false });
  try {
    await t.page.waitFor(`document.getElementById("app").dataset.moment === "sistema" && !!document.querySelector("#app h1")`);
    await ev(t, `__app.shell.ctx.toast("Listo: jobbot se detuvo.", true)`);
    assert.equal(await toastText(t), "Listo: jobbot se detuvo.");
    // a key the model does not have is not invented a name
    await ev(t, `__app.shell.ctx.toast("Aprobada: ghost:99", true)`);
    assert.equal(await toastText(t), "Aprobada: ghost:99");
    // and the same hand wrote the markup: it is text, never nodes
    await ev(t, `__app.shell.ctx.toast("<b>fx:12</b> fx:12 <i>x</i>", true)`);
    assert.equal(await toastText(t), "<b>fx:12</b> Mu Test Co — Full-stack Engineer <i>x</i>");
    assert.equal(await ev(t, `document.querySelector(".toast b, .toast i")`), null, "no element was made");
    assert.deepEqual(problems(t), []);
  } finally { await t.close(); }
});
