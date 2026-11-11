// The logic of the old page, ported: the model, the ticket queue, the signals, the diffs, the session (against a
// fake server and fake timers), the store and the small formatters. Fictional fixtures only (test/fixtures).
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { useStorage, keep, local, createView, restoreView, saveView, routeOfHash, hashOf, nextTheme, loadTheme, saveTheme, THEMES } from "../src/core/store.js";
import { setLang, setTimeZone, t } from "../src/i18n/index.js";
import { model, candidates, turnCount, layerOf, pathOf, ownerOf, countOf, LAYERS } from "../src/core/model.js";
import { diffSnapshots, sinceLastVisit, rememberVisit, lastVisit, mailId } from "../src/core/diff.js";
import { createSession, parseLaunch } from "../src/core/session.js";
import * as format from "../src/core/format.js";
import { hourOf, dayOf, hourNumber, clockHours, serverDay, placeMoves, axisHours, sumFrom } from "../src/core/time.js";

const load = (name) => JSON.parse(readFileSync(new URL(`./fixtures/${name}`, import.meta.url), "utf8"));
const rich = () => load("snapshot.rich.json");
const empty = () => load("snapshot.empty.json");
const NOW = Date.parse("2026-10-07T15:00:00Z");
const HOUR = 3600000;

function fakeStorage() {
  const m = new Map();
  return { getItem: (k) => (m.has(k) ? m.get(k) : null), setItem: (k, v) => m.set(k, String(v)), removeItem: (k) => m.delete(k) };
}
function reset(lang = "es") {
  useStorage({ local: fakeStorage(), session: fakeStorage() });
  setTimeZone("UTC");
  setLang(lang);
}
const keysOf = (list) => list.map((p) => p.card.key);

// ---------------------------------------------------------------- model

test("model: every live application is a process, sorted by whose turn it is, then by deadline, then by wait", () => {
  reset();
  const m = model(rich());
  assert.equal(m.nowMs, NOW);
  assert.deepEqual(keysOf(m.procs), ["fx:2", "fx:4", "fx:3", "fx:8", "fx:7", "fx:6", "fx:1", "fx:5", "fx:19", "fx:20"]);
  const turn = Object.fromEntries(m.procs.map((p) => [p.card.key, p.turn]));
  assert.deepEqual(turn, { "fx:2": "mine", "fx:4": "mine", "fx:3": "mine", "fx:8": "mine", "fx:7": "mine", "fx:6": "check", "fx:1": "wait", "fx:5": "wait", "fx:19": "wait", "fx:20": "wait" });
  assert.deepEqual(m.procs.filter((p) => p.stale).map((p) => p.card.key), ["fx:1"], "only a sent application, ten days or more without news");
  assert.equal(m.procs.find((p) => p.card.key === "fx:1").days, 12);
  assert.equal(m.procs.find((p) => p.card.key === "fx:20").days, 0);
});

test("ownerOf: a key wins, a company named once pins the task, two companies or none pin nothing", () => {
  reset();
  const m = model(rich());
  const owner = (id) => (m.tasks.find((x) => x.task.id === id).owner || {}).key || null;
  assert.equal(owner(101), "fx:2", "by job key");
  assert.equal(owner(105), "fx:8", "by the company the text names");
  assert.equal(owner(106), null, "a text that names two companies belongs to none");
  assert.equal(owner(104), null);
  const cards = [{ key: "a", company: "Acme" }, { key: "b", company: "Acme Labs" }, { key: "c", company: "ab" }];
  assert.equal(ownerOf({ job_key: "zzz", text: "Acme" }, cards), null, "a key that matches nothing is not matched by name");
  assert.equal(ownerOf({ job_key: null, text: "Call Acme Labs today" }, cards), null, "naming Acme Labs also names Acme: two applications, so none");
  assert.equal(ownerOf({ job_key: null, text: "Call Acme today" }, cards).key, "a");
  assert.equal(ownerOf({ job_key: null, text: "Call Acmeish today" }, cards), null, "whole words only");
  assert.equal(ownerOf({ job_key: null, text: "Call Labs today" }, [{ key: "z", company: "Labs" }]).key, "z");
  assert.equal(ownerOf({ job_key: null, text: "ab ab" }, cards), null, "names under three letters never match");
  assert.equal(ownerOf({ job_key: null, text: "Acme and Acme Labs" }, cards), null);
});

test("candidates: late and soon tasks, offers, approvals, stuck forms by group, other tasks, then follow-ups", () => {
  reset();
  const m = model(rich());
  const list = candidates(m);
  assert.deepEqual(list.map((c) => c.id), [
    "task:101", "task:103", "task:102",
    "card:fx:7", "card:fx:12",
    "card:fx:9", "card:fx:10", "card:fx:11",
    "task:104", "task:105", "task:106",
    "card:fx:6", "card:fx:1",
  ]);
  const first = list[0];
  assert.equal(first.tone, "late");
  assert.equal(first.kind, "Tarea");
  assert.equal(first.big, "VENCIDA");
  assert.equal(first.small, true);
  assert.equal(first.sub, "Venció hace 2 h");
  assert.equal(first.why, "Beta Test Co: Respondió");
  assert.equal(first.open, "fx:2");
  const soon = list[1];
  assert.deepEqual([soon.tone, soon.big, soon.sub], ["turn", "26 H", "Vence en 26 h"]);
  const offer = list[3];
  assert.deepEqual([offer.kind, offer.big, offer.sub, offer.why, offer.extra], ["Oferta", "OFERTA", "decide", "Una oferta espera tu respuesta.", true]);
  const approval = list[4];
  assert.deepEqual([approval.kind, approval.tone, approval.big, approval.sub, approval.actions], ["Por aprobar", "approve", null, null, ["approve"]], "an approval carries no big number and no word under it: nobody reads them");
  assert.equal(approval.why, "Revisa la captura: si está bien, la próxima corrida la envía.");
  const stuck = list[5];
  assert.deepEqual([stuck.kind, stuck.why, stuck.actions], ["Formularios para enviar tú", "Envía tú el formulario: la hoja de respuestas está lista", ["resolve", "confirm"]]);
  assert.equal(list[8].big, "SIN FECHA");
  assert.equal(list[8].sub, "cuando puedas");
  const ghost = list[11];
  assert.deepEqual([ghost.kind, ghost.big, ghost.sub, ghost.why, ghost.extra], ["Seguimiento", "21 D", "sin noticias", "Sin noticias: decide si insistes.", true]);
  assert.equal(list[12].why, "Sin noticias hace 12 días: decide si insistes.");
  assert.equal(list.filter((c) => c.extra).length, 3, "the offer and the two follow-ups are not counted by the server");
});

test("candidates: the same queue reads in English when the language is English", () => {
  reset("en");
  const list = candidates(model(rich()));
  assert.deepEqual([list[0].kind, list[0].big, list[0].sub, list[0].why], ["Task", "OVERDUE", "Overdue by 2 h", "Beta Test Co: Replied"]);
  assert.deepEqual([list[3].kind, list[3].why], ["Offer", "An offer is waiting for your answer."]);
  assert.equal(list[4].kind, "To approve");
  assert.equal(list[12].why, "No news for 12 days: decide whether to nudge them.");
  reset();
});

test("candidates: a task due in more than 48 h comes after the stuck forms, and a reason kind nobody knows is dropped", () => {
  reset();
  const s = rich();
  s.today.tasks.push({ id: 108, text: "Send the invoice", job_key: null, due_at: new Date(NOW + 5 * 24 * HOUR).toISOString(), overdue: false, link: null });
  s.today.tasks_total += 1;
  s.needs_gonza.cards[0].reason_kind = "something_new";
  const ids = candidates(model(s)).map((c) => c.id);
  assert.ok(ids.indexOf("task:108") > ids.indexOf("card:fx:11") && ids.indexOf("task:108") < ids.indexOf("task:104"), ids.join());
  assert.ok(!ids.includes("card:fx:9"), "legacy quirk: an unknown reason kind belongs to no group");
  const far = candidates(model(s)).find((c) => c.id === "task:108");
  assert.deepEqual([far.big, far.sub], ["5 D", "Quedan 5 días"]);
});

test("turnCount: the server's counts plus the offers and follow-ups that only the queue knows", () => {
  reset();
  const s = rich();
  assert.equal(turnCount(model(s)), 6 + 1 + 3 + 3);
  assert.equal(turnCount(model(empty())), 0);
});

// ---------------------------------------------------------------- layers

test("layerOf says where an application stands and whose turn it is", () => {
  reset();
  const card = (state, extra) => Object.assign({ state, approved: false, actions: [], reason_kind: null, reason_short: null }, extra);
  const at = (c) => { const l = layerOf(c); return [l.at, l.who]; };
  assert.deepEqual(at(card("scored")), [2, "robot"]);
  assert.deepEqual(at(card("prepared")), [3, "robot"]);
  assert.deepEqual(at(card("prepared", { actions: ["approve"] })), [4, "you"]);
  assert.deepEqual(at(card("prepared", { approved: true })), [4, "robot"]);
  assert.deepEqual(at(card("needs_gonza", { reason_kind: "missing_answers" })), [3, "you"]);
  assert.deepEqual(at(card("needs_gonza", { reason_kind: "blocked" })), [3, "you"]);
  assert.deepEqual(at(card("needs_gonza", { reason_kind: "error" })), [3, "you"]);
  assert.deepEqual(at(card("needs_gonza", { reason_kind: "handoff" })), [4, "you"]);
  assert.deepEqual(at(card("needs_gonza", { reason_kind: "manual_apply" })), [4, "you"]);
  assert.deepEqual(at(card("needs_gonza", { reason_kind: "may_have_sent" })), [4, "you"]);
  assert.deepEqual(at(card("needs_gonza")), [4, "you"]);
  for (const done of ["submitted", "replied", "technical_test", "interview", "offer", "accepted", "rejected", "ghosted"]) assert.deepEqual(at(card(done)), [5, "done"], done);
  assert.deepEqual(at(card("withdrawn")), [-1, "closed"]);
  assert.equal(layerOf(card("scored")).text, "jobbot prepara el CV en la próxima ventana");
  assert.equal(layerOf(card("needs_gonza", { reason_kind: "blocked" })).text, "Una página me frenó (captcha, ingreso o bloqueo)");
  assert.equal(layerOf(card("needs_gonza", { reason_kind: "novel", reason_short: "Algo nuevo" })).text, "Algo nuevo", "an unknown kind shows what the server said");
  assert.equal(layerOf(card("needs_gonza", { reason_kind: "novel" })).text, "necesita una mirada tuya");
  assert.deepEqual(LAYERS, ["aviso", "puntaje", "cv", "respuestas", "compuerta"]);
});

test("pathOf: five dots under a queue row, or nothing once it went out", () => {
  reset();
  const cards = Object.fromEntries(rich().board.flatMap((col) => col.cards).map((c) => [c.key, c]));
  const awaiting = pathOf(cards["fx:12"]);
  assert.deepEqual([awaiting.id, awaiting.name, awaiting.who, awaiting.dots], ["compuerta", "Revisión final", "you", ["done", "done", "done", "done", "here you"]]);
  assert.equal(awaiting.text, "espera tu aprobación");
  assert.deepEqual(pathOf(cards["fx:15"]).dots, ["done", "done", "here", "", ""]);
  assert.equal(pathOf(cards["fx:1"]), null, "sent");
  assert.equal(pathOf(cards["fx:18"]), null, "withdrawn");
  setLang("en");
  assert.equal(pathOf(cards["fx:12"]).name, "Final check");
  setLang("es");
});

// ---------------------------------------------------------------- signals

test("countOf: the cards of one state of the board, and nothing for a state that is not there", () => {
  reset();
  const s = rich();
  assert.equal(countOf(s, "scored"), 2);
  assert.equal(countOf(s, "nothing"), 0);
});

// ---------------------------------------------------------------- diffs

test("diffSnapshots: new mail, new task and a card that moved", () => {
  reset();
  const prev = rich();
  const next = rich();
  const mail = { received_at: "2026-10-07T15:00:00Z", from: "x@epsilon.example", company: "Epsilon Test Co", job_key: "fx:5", kind: "recruiter", subject: "Call?", summary: "" };
  next.today.emails.unshift(mail);
  next.today.emails.push({ received_at: "2026-10-07T15:01:00Z", from: "y@example.com", company: null, job_key: null, kind: "other", subject: "Hello", summary: "" });
  next.today.tasks.push({ id: 107, text: "A new task", job_key: null, due_at: null, overdue: false, link: null });
  const sub = next.board.find((c) => c.state === "submitted");
  const [c5] = sub.cards.splice(sub.cards.findIndex((c) => c.key === "fx:5"), 1);
  c5.state = "interview";
  next.board.find((c) => c.state === "interview").cards.push(c5);
  const news = diffSnapshots(prev, next);
  assert.deepEqual(Array.from(news.fresh).sort(), ["card:fx:5", "mail:" + mailId(mail), "mail:2026-10-07T15:01:00Z|Hello", "task:107"].sort());
  assert.deepEqual(news.lines, ["Epsilon Test Co: Reclutador", "Correo nuevo: Otro", "Tarea nueva", "Epsilon Test Co: Entrevista"]);
  assert.equal(news.big, true, "an interview is a ceremony");
  setLang("en");
  assert.deepEqual(diffSnapshots(prev, next).lines, ["Epsilon Test Co: Recruiter", "New email: Other", "New task", "Epsilon Test Co: Interview"]);
  setLang("es");
});

test("diffSnapshots: nothing new, no previous picture, and a card that only appeared are all quiet", () => {
  reset();
  const prev = rich();
  assert.deepEqual(diffSnapshots(prev, rich()), { fresh: new Set(), lines: [], big: false });
  assert.deepEqual(diffSnapshots(null, rich()), { fresh: new Set(), lines: [], big: false });
  const next = rich();
  next.board.find((c) => c.state === "scored").cards.push(Object.assign({}, next.board[0].cards[0], { key: "fx:99" }));
  assert.equal(diffSnapshots(prev, next).lines.length, 0, "a new card is not a move");
  const moved = rich();
  const sent = moved.board.find((c) => c.state === "submitted");
  const [c19] = sent.cards.splice(sent.cards.findIndex((c) => c.key === "fx:19"), 1);
  c19.state = "replied";
  moved.board.find((c) => c.state === "replied").cards.push(c19);
  const n = diffSnapshots(prev, moved);
  assert.deepEqual([n.lines, n.big], [["Tau Test Co: Respondió"], false], "a reply is news but not a ceremony");
});

test("sinceLastVisit: what was updated after the stored moment, in the sent and closed columns only", () => {
  reset();
  const s = rich();
  assert.deepEqual(sinceLastVisit(s), { fresh: new Set(), lines: [], big: false }, "no stored visit, no news");
  rememberVisit(NOW - 6 * HOUR);
  assert.equal(lastVisit(), NOW - 6 * HOUR);
  const news = sinceLastVisit(s);
  // the two mails of the last hours and Eta (offer, 2 h ago); Lambda moved 3 h ago but it is a stuck form, not news
  assert.deepEqual(Array.from(news.fresh).sort(), ["card:fx:7", "mail:" + mailId(s.today.emails[0]), "mail:" + mailId(s.today.emails[1])].sort());
  assert.deepEqual(news.lines, ["Desde tu última visita: 3 novedades"]);
  rememberVisit(NOW - 3 * HOUR + 5 * 60000);
  assert.deepEqual(sinceLastVisit(s).lines, ["Desde tu última visita: 2 novedades"], "the mail of three hours ago is older than the visit");
  setLang("en");
  rememberVisit(NOW - 60 * 60000);
  assert.deepEqual(sinceLastVisit(s).lines, ["Since your last visit: 1 update"], "only the mail of 50 minutes ago");
  assert.deepEqual(sinceLastVisit(s, 0).lines, []);
  setLang("es");
  rememberVisit(NOW + 1000);
  assert.equal(sinceLastVisit(s).lines.length, 0, "nothing is newer than a visit that has not happened yet");
});

// ---------------------------------------------------------------- the session

const settle = async () => {
  for (let i = 0; i < 30; i++) await Promise.resolve();
  await new Promise((resolve) => setImmediate(resolve));
};

function fakeTimers() {
  let now = 0;
  let seq = 0;
  const items = [];
  return {
    setTimeout(fn, ms) { const id = ++seq; items.push({ id, at: now + ms, fn }); return id; },
    clearTimeout(id) { const at = items.findIndex((x) => x.id === id); if (at >= 0) items.splice(at, 1); },
    now: () => now,
    waits: () => items.map((x) => x.at - now).sort((a, b) => a - b),
    async advance(ms) {
      const end = now + ms;
      for (;;) {
        items.sort((a, b) => a.at - b.at || a.id - b.id);
        const next = items[0];
        if (!next || next.at > end) break;
        items.shift();
        now = next.at;
        next.fn();
        await settle();
      }
      now = end;
      await settle();
    },
  };
}

function fakeServer(initial) {
  const s = {
    calls: [], snapshot: initial, status: 200, hold: null, throws: false,
    post: { status: 200, body: { ok: true, message: "Aprobada: la próxima corrida la envía." } },
  };
  s.fetchImpl = async (path, init = {}) => {
    s.calls.push({ path, method: init.method || "GET", headers: init.headers, body: init.body });
    if (s.throws) throw new TypeError("network down");
    if (s.hold) await s.hold();
    const post = init.method === "POST";
    const status = post ? s.post.status : s.status;
    const body = post ? s.post.body : s.snapshot;
    return { ok: status >= 200 && status < 300, status, json: async () => structuredClone(body) };
  };
  return s;
}

function rig(options = {}) {
  reset();
  const timers = fakeTimers();
  const server = fakeServer(rich());
  const events = [];
  const flags = { typing: false, visible: true };
  const session = createSession({
    fetchImpl: server.fetchImpl, token: "tok-123", timers, now: timers.now,
    isTyping: () => flags.typing, isVisible: () => flags.visible, ...options,
  });
  session.subscribe((e) => events.push(e));
  return { session, server, timers, events, flags };
}
const types = (events) => events.map((e) => e.type);
const withTime = (snapshot, ms) => Object.assign(snapshot, { generated_at: new Date(ms).toISOString() });

test("session: the first read is forced, sends the token and builds the model", async () => {
  const { session, server, events } = rig();
  assert.equal(session.snapshot, null);
  assert.equal(await session.start(), true);
  assert.deepEqual(server.calls.map((c) => [c.method, c.path, c.headers["X-Panel-Token"]]), [["GET", "/api/snapshot", "tok-123"]]);
  assert.equal(session.snapshot.generated_at, "2026-10-07T15:00:00Z");
  assert.equal(session.model.procs.length, 10);
  assert.equal(turnCount(session.model), 13);
  assert.deepEqual(types(events), ["snapshot"]);
  assert.equal(events[0].model, session.model);
  assert.equal(session.error, null);
  session.stop();
});

test("session: polls every 30 seconds, draws only what changed, and draws again once the clock moved ten minutes", async () => {
  const { session, server, timers, events } = rig();
  await session.start();
  await timers.advance(29999);
  assert.equal(server.calls.length, 1);
  await timers.advance(1);
  assert.equal(server.calls.length, 2, "one poll after 30 s");
  assert.deepEqual(types(events), ["snapshot"], "same content, only the clock changed a little: nothing to draw");
  withTime(server.snapshot, NOW + 60000);
  server.snapshot.today.tasks_total += 0;
  server.snapshot.controls.stopped = true;
  await timers.advance(30000);
  assert.equal(server.calls.length, 3);
  assert.deepEqual(types(events), ["snapshot", "snapshot"], "a change is drawn");
  assert.equal(session.snapshot.controls.stopped, true);
  // the same content ten minutes later: the countdowns need redrawing
  withTime(server.snapshot, NOW + 60000 + 10 * 60000);
  await timers.advance(30000);
  assert.deepEqual(types(events), ["snapshot", "snapshot", "snapshot"]);
  session.stop();
  await timers.advance(120000);
  assert.equal(server.calls.length, 4, "stop() ends the polling");
});

test("session: never two requests at once; a forced refresh during one runs once more afterwards", async () => {
  const { session, server } = rig();
  await session.start();
  let release;
  server.hold = () => { server.hold = null; return new Promise((resolve) => { release = resolve; }); };
  const a = session.refresh({ force: false });
  const b = session.refresh({ force: false });
  assert.equal(a, b, "a second ask joins the one in flight");
  const c = session.refresh({ force: true });
  await settle();
  assert.equal(server.calls.length, 2, "still only the first request left");
  release();
  await Promise.all([a, b, c]);
  assert.equal(server.calls.length, 3, "the forced one ran after it, not in parallel");
  session.stop();
});

test("session: an answer older than the picture on screen is ignored", async () => {
  const { session, server, events } = rig();
  await session.start();
  const stale = rich();
  withTime(stale, NOW - 60000);
  stale.controls.stopped = true;
  server.snapshot = stale;
  assert.equal(await session.refresh({ force: true }), true);
  assert.equal(session.snapshot.controls.stopped, false);
  assert.deepEqual(types(events), ["snapshot"]);
  session.stop();
});

test("session: after failures the wait doubles up to two minutes, and a good answer brings it back to 30 s", async () => {
  const { session, server, timers, events } = rig();
  await session.start();
  // (the 4-second timer that clears the marks of what is new is not the poll)
  const poll = () => timers.waits().filter((ms) => ms > 4000);
  assert.deepEqual(poll(), [30000]);
  server.status = 500;
  server.snapshot = { ok: false, message: "Error interno: boom" };
  await timers.advance(30000);
  assert.deepEqual(poll(), [60000], "after one failure");
  await timers.advance(60000);
  assert.deepEqual(poll(), [120000], "after two");
  await timers.advance(120000);
  assert.deepEqual(poll(), [120000], "never longer than two minutes");
  assert.equal(session.error.kind, "server");
  assert.equal(session.error.status, 500);
  assert.equal(session.error.message, "Error interno: boom", "the server's own words");
  assert.equal(session.error.fatal, false);
  assert.ok(types(events).includes("toast") && events.find((e) => e.type === "toast").toast.ok === false);
  server.status = 200;
  server.snapshot = rich();
  await timers.advance(120000);
  assert.equal(session.error, null);
  assert.equal(events.at(-1).type, "recovered", "same content as before the outage, so nothing to draw but the error is gone");
  assert.deepEqual(poll(), [30000]);
  session.stop();
});

test("session: a wrong token is a fatal error in plain words, with no toast, and the page keeps its last picture", async () => {
  const { session, server, events } = rig();
  server.status = 401;
  server.snapshot = { ok: false, message: "Falta el token del panel o no es válido" };
  assert.equal(await session.start(), false);
  assert.deepEqual([session.error.kind, session.error.fatal, session.error.status], ["token", true, 401]);
  assert.match(session.error.message, /clave/);
  assert.deepEqual(types(events), ["error"], "no toast: the page itself says it");
  assert.equal(session.snapshot, null);
  server.status = 403;
  await session.refresh({ force: true });
  assert.equal(session.error.kind, "token");
  setLang("en");
  server.status = 401;
  await session.refresh({ force: true });
  assert.match(session.error.message, /key changes/);
  setLang("es");
  server.status = 200;
  server.snapshot = rich();
  await session.refresh({ force: true });
  assert.equal(session.error, null);
  assert.equal(session.snapshot.generated_at, "2026-10-07T15:00:00Z");
  session.stop();
});

test("session: a server that cannot be reached is a network error with the old message and a toast", async () => {
  const { session, server, events } = rig();
  server.throws = true;
  assert.equal(await session.start(), false);
  assert.deepEqual([session.error.kind, session.error.status], ["network", 0]);
  assert.equal(session.error.message, "No se pudo hablar con el panel: ¿se cerró jobbot panel?");
  assert.deepEqual(types(events), ["error", "toast"]);
  server.throws = false;
  server.status = 200;
  assert.equal(await session.refresh({ force: true }), true);
  session.stop();
});

test("session: a snapshot the page cannot read is an error, not a crash", async () => {
  const { session, server } = rig();
  server.snapshot = { generated_at: "2026-10-07T15:00:00Z" };
  const quiet = console.error;
  console.error = () => {};
  try {
    assert.equal(await session.start(), false);
  } finally {
    console.error = quiet;
  }
  assert.equal(session.error.kind, "data");
  assert.equal(session.snapshot, null);
  session.stop();
});

test("session: not forced, a refresh waits while the person is typing; forced, it does not", async () => {
  const { session, server, timers, flags, events } = rig();
  await session.start();
  flags.typing = true;
  server.snapshot = rich();
  server.snapshot.controls.stopped = true;
  await timers.advance(30000);
  assert.equal(server.calls.length, 2, "it did look");
  assert.deepEqual(types(events), ["snapshot"], "but did not redraw under a half-typed answer");
  assert.equal(session.snapshot.controls.stopped, false);
  flags.typing = false;
  await timers.advance(30000);
  assert.equal(session.snapshot.controls.stopped, true, "the next poll draws it");
  server.snapshot.controls.stopped = false;
  flags.typing = true;
  await session.refresh({ force: true });
  assert.equal(session.snapshot.controls.stopped, false, "forced: after an action the page is always read again");
  session.stop();
});

test("session: polls only while the page is visible, and wake() reads now when it comes back after a while", async () => {
  const { session, server, timers, flags } = rig();
  await session.start();
  flags.visible = false;
  await timers.advance(30000);
  await timers.advance(30000);
  assert.equal(server.calls.length, 1, "a hidden page pays nothing");
  assert.deepEqual(timers.waits().filter((ms) => ms > 4000), [30000], "but the loop keeps its place");
  flags.visible = true;
  assert.equal(await session.wake(), true, "it has been longer than a poll");
  assert.equal(server.calls.length, 2);
  assert.equal(await session.wake(), false, "and not again straight away");
  session.stop();
});

test("session: an action posts JSON, tells the server's words, can hold the refresh, then reads everything again", async () => {
  const { session, server, events } = rig();
  await session.start();
  const order = [];
  let okSeen = null;
  const result = await session.act("/api/approve", { key: "fx:12" }, {
    onOk: (r) => { okSeen = r.status; },
    beforeRefresh: async () => { order.push("before:" + server.calls.length); },
  });
  const post = server.calls[1];
  assert.deepEqual([post.method, post.path, post.headers["Content-Type"], post.body], ["POST", "/api/approve", "application/json", '{"key":"fx:12"}']);
  assert.equal(server.calls[2].path, "/api/snapshot", "everything is read again, forced");
  assert.deepEqual(order, ["before:2"], "the refresh waited for beforeRefresh");
  assert.equal(okSeen, 200);
  assert.deepEqual([result.ok, result.message], [true, "Aprobada: la próxima corrida la envía."]);
  assert.deepEqual(events.filter((e) => e.type === "toast").map((e) => e.toast), [{ ok: true, message: "Aprobada: la próxima corrida la envía." }]);
  // refused: the server's message, ok false, and onOk is not called
  server.post = { status: 409, body: { ok: false, message: "No se pudo marcar: ya no se puede pasar a ese estado." } };
  okSeen = null;
  const refused = await session.act("/api/mark", { key: "fx:3", state: "replied", note: null }, { onOk: () => { okSeen = true; } });
  assert.deepEqual([refused.ok, refused.status, okSeen], [false, 409, null]);
  assert.equal(events.filter((e) => e.type === "toast").at(-1).toast.message, "No se pudo marcar: ya no se puede pasar a ese estado.");
  // no message from the server: the page's own
  server.post = { status: 200, body: {} };
  assert.equal((await session.act("/api/task/done", { id: 101 })).message, "Hecho.");
  server.post = { status: 500, body: null };
  assert.equal((await session.act("/api/stop", {})).message, "No se hizo, y el panel no dijo por qué. Reintenta; si sigue igual, mira la consola de jobbot panel.");
  session.stop();
});

test("session: the marks of what is new last four seconds", async () => {
  const { session, server, timers } = rig();
  await session.start();
  const next = rich();
  next.today.tasks.push({ id: 107, text: "A new task", job_key: null, due_at: null, overdue: false, link: null });
  next.today.tasks_total += 1;
  server.snapshot = next;
  await session.refresh({ force: false });
  assert.deepEqual(Array.from(session.fresh), ["task:107"]);
  assert.deepEqual(session.news.lines, ["Tarea nueva"]);
  await timers.advance(3999);
  assert.equal(session.fresh.size, 1);
  await timers.advance(1);
  assert.equal(session.fresh.size, 0);
  session.stop();
});

test("session: the first picture of a visit says what happened since the last one", async () => {
  const { session } = rig();
  rememberVisit(NOW - 6 * HOUR);
  await session.start();
  assert.deepEqual(session.news.lines, ["Desde tu última visita: 3 novedades"]);
  assert.equal(session.fresh.size, 3);
  session.stop();
});

test("session: the demo hands in its own api and the page needs no fetch", async () => {
  reset();
  const calls = [];
  const demo = { api: async (path, body) => { calls.push([path, body]); return { ok: true, status: 200, data: path === "/api/snapshot" ? rich() : { ok: true, message: "Listo" } }; } };
  const session = createSession({ demo, timers: fakeTimers(), fetchImpl: () => { throw new Error("no fetch in the demo"); } });
  assert.equal(await session.start(), true);
  await session.act("/api/stop", {});
  assert.deepEqual(calls.map((c) => c[0]), ["/api/snapshot", "/api/stop", "/api/snapshot"]);
  session.stop();
});

test("session: the demo draws its own captures and is handed `refresh`; fetch is never used", async () => {
  reset();
  const calls = [];
  const demo = {
    api: async (path) => { calls.push(path); return { ok: true, status: 200, data: path === "/api/snapshot" ? rich() : { ok: true, message: "Listo" } }; },
    blob: async (path) => (path.includes("demo:1") ? { fake: "picture" } : null),
    attach(page) { demo.page = page; },
  };
  const session = createSession({ demo, timers: fakeTimers(), fetchImpl: () => { throw new Error("no fetch in the demo"); } });
  assert.equal(typeof demo.page.refresh, "function", "the demo is handed the page's refresh");
  assert.deepEqual(await session.blob("/api/shot?key=demo:1"), { fake: "picture" });
  assert.equal(await session.blob("/api/shot?key=demo:2"), null, "no capture for that one");
  demo.blob = async () => { throw new Error("boom"); };
  assert.equal(await session.blob("/api/shot?key=demo:1"), null, "a failing hook is no picture, not a crash");
  assert.equal(await session.start(), true);
  assert.deepEqual(calls, ["/api/snapshot"]);
  await demo.page.refresh({ force: true });
  assert.deepEqual(calls, ["/api/snapshot", "/api/snapshot"], "the demo can ask for a new picture when its language changes");
  session.stop();
});

test("parseLaunch reads the token and whether the panel is embedded", () => {
  assert.deepEqual(parseLaunch("?t=abc123", false), { token: "abc123", embedded: false });
  assert.deepEqual(parseLaunch("?t=abc&embed=1", false), { token: "abc", embedded: true });
  assert.deepEqual(parseLaunch("", true), { token: "", embedded: true });
  assert.deepEqual(parseLaunch(undefined, false), { token: "", embedded: false });
});

// ---------------------------------------------------------------- store

test("store: keep and local survive missing or throwing storage by using memory", () => {
  reset();
  keep.set("view", { room: "cola", n: [1, 2] });
  assert.deepEqual(keep.get("view"), { room: "cola", n: [1, 2] });
  assert.equal(keep.get("nothing"), null);
  local.set("theme", "dark");
  assert.equal(local.get("theme"), "dark");
  local.remove("theme");
  assert.equal(local.get("theme"), null);
  const broken = { getItem() { throw new Error("blocked"); }, setItem() { throw new Error("blocked"); }, removeItem() { throw new Error("blocked"); } };
  useStorage({ local: broken, session: broken });
  local.set("lang", "en");
  assert.equal(local.get("lang"), "en", "kept in memory for this page");
  keep.set("view", { room: "hoy" });
  assert.deepEqual(keep.get("view"), { room: "hoy" });
  local.remove("lang");
  assert.equal(local.get("lang"), null);
  useStorage({ local: null, session: null });
  keep.set("x", 1);
  assert.equal(keep.get("x"), 1);
  const garbage = fakeStorage();
  garbage.setItem("panel.view", "{not json");
  useStorage({ local: fakeStorage(), session: garbage });
  assert.equal(keep.get("view"), null, "garbage reads as nothing");
  reset();
});

test("store: the address and the saved view say which room opens, and an embedded panel ignores what was saved", () => {
  reset();
  assert.deepEqual(routeOfHash("#procesos/a%2Fb"), { room: "procesos", job: "a/b" });
  assert.deepEqual(routeOfHash("#learn"), { room: "aprender", job: null }, "the old name of the fourth room");
  assert.deepEqual(routeOfHash("#sistema"), { room: "sistema", job: null });
  assert.deepEqual(routeOfHash("#cola/%E0%A4%A"), { room: "cola", job: null }, "a broken address is just no job");
  assert.equal(routeOfHash("#nowhere"), null);
  assert.equal(routeOfHash(""), null);
  assert.equal(hashOf("procesos", "a/b"), "#procesos/a%2Fb");
  assert.equal(hashOf("hoy"), "#hoy");

  const view = createView();
  assert.deepEqual(view, { room: "hoy", filter: "all", folds: {}, cursor: null, openKey: null, note: "" });
  view.room = "cola"; view.filter = "stale"; view.folds = { "dec-approve": true };
  saveView(view);
  const again = createView();
  assert.equal(restoreView(again, { embedded: false, hash: "" }), null);
  assert.deepEqual([again.room, again.filter, again.folds], ["cola", "stale", { "dec-approve": true }]);
  const routed = createView();
  assert.deepEqual(restoreView(routed, { hash: "#sistema/x" }), { room: "sistema", job: "x" });
  assert.equal(routed.room, "sistema", "the address wins over the saved room");
  assert.equal(routed.filter, "stale");
  const embedded = createView();
  restoreView(embedded, { embedded: true, hash: "" });
  assert.deepEqual([embedded.room, embedded.filter], ["hoy", "all"]);
  keep.set("view", { tab: "learn", filter: "bogus", folds: 7 });
  const legacy = createView();
  restoreView(legacy, {});
  assert.deepEqual([legacy.room, legacy.filter, legacy.folds], ["aprender", "all", {}], "an old saved view still opens, and bad parts are ignored");
});

test("store: the theme cycles auto, light, dark and is kept on this machine", () => {
  reset();
  assert.deepEqual(THEMES, ["auto", "light", "dark"]);
  assert.deepEqual(["auto", "light", "dark", "dark"].map(nextTheme), ["light", "dark", "auto", "auto"]);
  assert.equal(nextTheme("something"), "auto");
  assert.equal(loadTheme(), "auto");
  saveTheme("dark");
  assert.equal(loadTheme(), "dark");
  assert.equal(local.get("theme"), "dark");
  saveTheme("auto");
  assert.equal(local.get("theme"), null);
  local.set("theme", "purple");
  assert.equal(loadTheme(), "auto");
});

// ---------------------------------------------------------------- words, time and numbers

test("format: countdown in every band, in both languages", () => {
  reset();
  const at = (ms, extra) => format.countdown(Object.assign({ due_at: new Date(NOW + ms).toISOString(), overdue: false }, extra), NOW);
  assert.equal(format.countdown({ due_at: null }, NOW), null);
  assert.deepEqual(at(-30 * 60000), { text: "Venció hace 1 h", tone: "late" });
  assert.deepEqual(at(-5 * HOUR), { text: "Venció hace 5 h", tone: "late" });
  assert.deepEqual(at(-50 * HOUR), { text: "Venció hace 2 días", tone: "late" });
  assert.deepEqual(at(30 * 60000, { overdue: true }), { text: "Venció hace 1 h", tone: "late" }, "the server says it is overdue");
  assert.deepEqual(at(30 * 60000), { text: "Vence en menos de 1 h", tone: "soon" });
  assert.deepEqual(at(30 * HOUR), { text: "Vence en 30 h", tone: "soon" });
  assert.deepEqual(at(47 * HOUR + 59 * 60000), { text: "Vence en 47 h", tone: "soon" });
  assert.deepEqual(at(48 * HOUR), { text: "Quedan 2 días", tone: "" });
  setLang("en");
  assert.deepEqual(at(-5 * HOUR), { text: "Overdue by 5 h", tone: "late" });
  assert.deepEqual(at(-50 * HOUR), { text: "Overdue by 2 days", tone: "late" });
  assert.deepEqual(at(30 * HOUR), { text: "Due in 30 h", tone: "soon" });
  assert.deepEqual(at(72 * HOUR), { text: "3 days left", tone: "" });
  assert.equal(format.lowerFirst(at(30 * HOUR).text), "due in 30 h");
  setLang("es");
});

test("format: small helpers", () => {
  reset();
  assert.equal(format.pct(0.5), "50%");
  assert.equal(format.pct(null), "n/a");
  assert.equal(format.usd(1.84), "US$1,84");
  assert.equal(format.plural(1, "time.ago.days"), "hace 1 día");
  assert.equal(format.plural(3, "time.ago.days"), "hace 3 días");
  assert.equal(format.ago("2026-10-07T12:00:00Z", NOW), "hace 3 h");
  assert.equal(format.when("2026-10-07T14:05:00Z"), "7 oct, 14:05");
  assert.equal(format.daysSince("2026-09-25T15:00:00Z", NOW), 12);
  assert.equal(format.daysSince("2026-10-08T15:00:00Z", NOW), 0, "never negative");
  assert.equal(format.norm(null), "");
  assert.equal(format.norm("ÁbC"), "ábc");
  assert.equal(format.escapeRegExp("a.b*c"), "a\\.b\\*c");
  assert.equal(format.initials("Alpha Test Co"), "ALP");
  assert.equal(format.initials("¡Ñandú!"), "ÑAN");
  assert.equal(format.initials(""), "", "legacy quirk: the '?' fallback is stripped like any other symbol");
  assert.equal(format.asSentence("espera tu aprobación"), "Espera tu aprobación.");
  assert.equal(format.asSentence("Hecho!"), "Hecho!");
});

test("format: names of ids come from the dictionary, an unknown id shows as it is, the reason is by kind", () => {
  reset();
  assert.equal(format.stateLabel("needs_gonza"), "Necesita tu ayuda");
  assert.equal(format.stateLabel("brand_new_state"), "brand_new_state");
  assert.equal(format.stateName("submitted"), "Enviadas");
  assert.equal(format.stageLabel("technical_test"), "Prueba técnica");
  assert.equal(format.markPhrase("ghosted", "submitted"), "Dejaron de responder");
  assert.equal(format.markPhrase("interview", "replied"), "Me citaron a entrevista");
  assert.equal(format.markPhrase("interview", "interview"), "Me citaron a otra entrevista", "a second round has its own words");
  assert.equal(format.markPhrase("brand_new_state", "replied"), "brand_new_state", "an answer the dictionary does not know shows as its state");
  assert.equal(format.markQuestion("offer", "interview"), "¿La empresa te hizo una oferta?");
  assert.equal(format.markQuestion("interview", "interview"), "¿La empresa te citó a otra ronda de entrevistas?");
  assert.equal(format.markQuestion("brand_new_state", "replied"), "¿Pasarla a «brand_new_state»?");
  assert.equal(format.kindLabel("interview_invite"), "Invitación a entrevista");
  assert.equal(format.turnLabel("check"), "Define el paso");
  assert.equal(format.statusWord("robot"), "jobbot trabaja aquí");
  assert.equal(format.actionLabel("confirm"), "Ya la envié");
  assert.equal(format.actionHint("resolve"), "Arreglé lo que la detuvo; que vuelva a la cola.");
  assert.equal(format.actionHint("mystery"), "");
  assert.equal(format.reasonShort({ reason_kind: "handoff", reason_short: "texto del servidor" }), "Envía tú el formulario: la hoja de respuestas está lista");
  assert.equal(format.reasonShort({ reason_kind: "novel", reason_short: "texto del servidor" }), "texto del servidor");
  assert.equal(format.reasonShort({ reason_kind: null, reason_short: null }), null);
  assert.equal(format.reasonGroupTitle("blocked"), "Una página me frenó");
  assert.equal(format.reasonShort({ reason_kind: "may_have_sent", reason_short: "texto del servidor" }), "Puede que ya se haya enviado: revisa antes de volver a postular");
  setLang("en");
  assert.equal(format.stateLabel("needs_gonza"), "Needs you");
  assert.equal(format.reasonShort({ reason_kind: "handoff" }), "Send the form yourself: the answer sheet is ready");
  assert.equal(format.reasonShort({ reason_kind: "may_have_sent" }), "It may already have gone out: check before applying again");
  assert.equal(format.actionLabel("approve"), "Approve");
  setLang("es");
});

test("format: the run window and the spend, as the header says them", () => {
  reset();
  const today = "2026-10-07";
  assert.equal(format.windowLabel({ window_open: true }, today), "Ventana de ejecución abierta");
  assert.equal(format.windowLabel({ window_open: null }, today), "Ventana de ejecución: sin datos");
  assert.equal(format.windowLabel({ window_open: false, next_window: null }, today), "Ventana cerrada");
  assert.equal(format.windowLabel({ window_open: false, next_window: "2026-10-07 18:00" }, today), "Ventana cerrada, abre 18:00");
  assert.equal(format.windowLabel({ window_open: false, next_window: "2026-10-08 18:00" }, today), "Ventana cerrada, abre 8 oct 18:00");
  assert.equal(format.nextWindowLabel("weird", today), "weird");
  assert.equal(format.spendLabel({ spent_24h: 1.84, daily_budget: 5 }), "Claude, 24 h: US$1,84 de US$5,00");
  assert.equal(format.spendLabel({ spent_24h: 0, daily_budget: null }), "Claude, 24 h: US$0,00");
  setLang("en");
  assert.equal(format.windowLabel({ window_open: false, next_window: "2026-10-08 18:00" }, today), "Run window closed, opens 8 Oct 18:00");
  assert.equal(format.spendLabel({ spent_24h: 1.84, daily_budget: 5 }), "Claude, last 24 h: $1.84 of $5.00");
  setLang("es");
});

test("time: hours and days follow the pinned zone; the server's clock is always Santiago's", () => {
  reset();
  const at = "2026-10-07T02:30:00Z";
  assert.deepEqual([hourOf(at), dayOf(at), hourNumber(at), clockHours(Date.parse(at))], ["02:30", "2026-10-07", 2, 2.5]);
  setTimeZone("America/Santiago");
  assert.deepEqual([hourOf(at), dayOf(at), hourNumber(at), clockHours(Date.parse(at))], ["23:30", "2026-10-06", 23, 23.5]);
  setTimeZone("UTC");
  assert.equal(serverDay(Date.parse("2026-10-07T02:30:00Z")), "2026-10-06", "Santiago is still on the 6th");
  assert.equal(serverDay(NOW), "2026-10-07");
  assert.equal(hourOf("nonsense"), "nonsense");
  assert.equal(dayOf("nonsense"), "nonsense");
  assert.equal(hourNumber("nonsense"), 0);
  assert.equal(hourNumber("2026-10-07T00:10:00Z"), 0, "midnight is 0, not 24");
});

test("time: the strip of the last day, moves in rows and hour marks", () => {
  reset();
  const s = rich();
  const a = s.activity;
  const from = Date.parse(a.from);
  const span = Date.parse(a.to) - from;
  const placed = placeMoves(a.moves, from, span, 960);
  assert.equal(placed.length, a.moves.length);
  assert.deepEqual(placed.map((p) => p.move.key), ["fx:20", "fx:19", "fx:15", "fx:13", "fx:12", "fx:9", "fx:11", "fx:7"], "oldest first");
  assert.ok(placed.every((p) => p.x >= 0 && p.x <= 960 && p.row >= 0 && p.row <= 4));
  assert.ok(placed.every((p, i) => i === 0 || p.x >= placed[i - 1].x), "left to right");
  const close = placeMoves([
    { at: "2026-10-07T10:00:00Z" }, { at: "2026-10-07T10:01:00Z" }, { at: "2026-10-07T10:02:00Z" }, { at: "2026-10-07T20:00:00Z" },
  ], Date.parse("2026-10-06T15:30:00Z"), span, 960);
  assert.deepEqual(close.map((p) => p.row), [0, 1, 2, 0], "a move that would touch the last one goes to the next row");
  const clamped = placeMoves([{ at: "2026-10-01T00:00:00Z" }, { at: "2026-12-01T00:00:00Z" }], from, span, 960);
  assert.deepEqual(clamped.map((p) => p.x), [0, 960]);
  const nowPercent = ((NOW - from) / span) * 100;
  const marks = axisHours(a.slots, nowPercent);
  assert.ok(marks.every((m) => m.hour % 3 === 0 && Math.abs(m.percent - nowPercent) >= 11 && m.percent >= 2));
  assert.deepEqual(marks.map((m) => m.hour), [18, 21, 0, 3, 6, 9, 12], "every third hour; 15:00 sits on top of now and is left out");
  assert.equal(sumFrom([1, 2, 3, 4], 2), 7);
  assert.equal(sumFrom([1, 2, 3, 4], -5), 10);
});
