import test from "node:test";
import assert from "node:assert/strict";
import { createDrill, GRADES } from "../src/moments/aprender/drill.js";

const flush = () => new Promise((resolve) => setImmediate(resolve));

// A fake server: a queue of cards that moves only when one is graded, as the real one does (SM-2).
function rig({ cards = [{ id: "c1", topic: "rust", question: "What is a lifetime?", offer: null }, { id: "c2", topic: "sql", question: "Window functions?", offer: "Delta Test Co" }], answers = { c1: "A scope.", c2: "OVER()." } } = {}) {
  const log = { api: [], act: [], toasts: [], changes: 0, sounds: [] };
  const state = { queue: [...cards], failNext: false, failAnswer: false, failGrade: false, hold: null };
  const held = async () => { if (state.hold) await state.hold; };
  const drill = createDrill({
    api: async (path) => {
      log.api.push(path);
      await held();
      if (path.startsWith("/api/learn/next")) {
        if (state.failNext) return { ok: false, status: 500, data: { message: "caído" } };
        return { ok: true, status: 200, data: { card: state.queue[0] || null } };
      }
      if (path.startsWith("/api/learn/answer")) {
        if (state.failAnswer) return { ok: false, status: 404, data: { message: "no hay" } };
        const id = decodeURIComponent(path.split("id=")[1]);
        return { ok: true, status: 200, data: { answer: answers[id] ?? null } };
      }
      return { ok: false, status: 404, data: null };
    },
    act: async (path, body, opts = {}) => {
      log.act.push({ path, body });
      await held();
      if (state.failGrade) return { ok: false, status: 409, message: "no se pudo" };
      state.queue.shift();
      if (opts.onOk) opts.onOk({ ok: true });
      return { ok: true, status: 200, message: "ok" };
    },
    said: (r, fallback) => (r.data && r.data.message) || fallback,
    toast: (text, ok) => log.toasts.push([text, ok]),
    t: (key) => key,
    sound: { play: (name) => log.sounds.push(name) },
    onChange: () => { log.changes += 1; },
  });
  return { drill, log, state };
}

test("starting asks the server for the first card, optionally of one topic, and shows it hidden", async () => {
  const r = rig();
  assert.equal(r.drill.state.active, false);
  const started = r.drill.start();
  assert.equal(r.drill.state.active, true);
  assert.equal(r.drill.state.waiting, true, "the card is on its way: the box holds its place");
  await started;
  assert.deepEqual(r.log.api, ["/api/learn/next"]);
  assert.deepEqual([r.drill.state.waiting, r.drill.state.card.id, r.drill.state.shown, r.drill.state.answer], [false, "c1", false, ""]);
  const topic = rig();
  await topic.drill.start("Distributed systems & more");
  assert.deepEqual(topic.log.api, ["/api/learn/next?topic=Distributed%20systems%20%26%20more"], "a topic is sent as the server wrote it, escaped");
  assert.equal(topic.drill.state.topic, "Distributed systems & more");
});

test("revealing fetches the reference answer of that card once, and only with a card on the table", async () => {
  const r = rig();
  await r.drill.reveal();
  assert.deepEqual(r.log.api, [], "nothing is on the table");
  await r.drill.start();
  await Promise.all([r.drill.reveal(), r.drill.reveal()]);
  assert.deepEqual(r.log.api, ["/api/learn/next", "/api/learn/answer?id=c1"], "a double press asks once");
  assert.deepEqual([r.drill.state.shown, r.drill.state.answer], [true, "A scope."]);
  await r.drill.reveal();
  assert.equal(r.log.api.length, 2, "already shown");
  const none = rig({ answers: {} });
  await none.drill.start();
  await none.drill.reveal();
  assert.deepEqual([none.drill.state.shown, none.drill.state.answer], [true, ""], "a card with no reference answer is shown, empty");
});

test("a failed reveal says why and leaves the card as it was", async () => {
  const r = rig();
  await r.drill.start();
  r.state.failAnswer = true;
  await r.drill.reveal();
  assert.deepEqual(r.log.toasts, [["no hay", false]]);
  assert.equal(r.drill.state.shown, false);
  assert.equal(r.drill.state.busy, null, "he can try again");
});

test("grading needs the answer on the table and a grade of 0 to 5; it sends the grade, then brings the next card hidden", async () => {
  assert.deepEqual(GRADES, [0, 1, 2, 3, 4, 5]);
  const r = rig();
  await r.drill.start();
  await r.drill.grade(4);
  assert.deepEqual(r.log.act, [], "the answer was not shown yet");
  await r.drill.reveal();
  for (const bad of [6, -1, 2.5, "x", null, NaN]) await r.drill.grade(bad);
  assert.deepEqual(r.log.act, [], "not a grade");
  await r.drill.grade(4);
  assert.deepEqual(r.log.act, [{ path: "/api/learn/grade", body: { id: "c1", grade: 4 } }]);
  assert.deepEqual([r.drill.state.card.id, r.drill.state.shown, r.drill.state.answer, r.drill.state.busy], ["c2", false, "", null]);
  assert.deepEqual(r.log.sounds, ["confirm"], "one sound for a grade");
});

test("a double press of a grade is one grade; nothing else happens while it is on its way", async () => {
  const r = rig();
  await r.drill.start();
  await r.drill.reveal();
  let release;
  r.state.hold = new Promise((resolve) => { release = resolve; });
  const first = r.drill.grade(5);
  await flush();
  assert.equal(r.drill.state.busy, "grade");
  await r.drill.grade(3);
  await r.drill.reveal();
  assert.equal(r.log.act.length, 1);
  release();
  await first;
  assert.equal(r.log.act.length, 1);
  assert.equal(r.drill.state.card.id, "c2");
});

test("the last card graded: the server has nothing to ask, so the box says the review is done", async () => {
  const r = rig({ cards: [{ id: "only", topic: "rust", question: "Q?", offer: null }], answers: { only: "A." } });
  await r.drill.start();
  await r.drill.reveal();
  await r.drill.grade(5);
  assert.deepEqual([r.drill.state.card, r.drill.state.empty, r.drill.state.active], [null, true, true]);
});

test("a refused grade leaves the card with its answer so that he can grade it again; the toast of the session already said why", async () => {
  const r = rig();
  await r.drill.start();
  await r.drill.reveal();
  r.state.failGrade = true;
  await r.drill.grade(3);
  assert.deepEqual([r.drill.state.card.id, r.drill.state.shown, r.drill.state.busy], ["c1", true, null]);
  assert.deepEqual(r.log.sounds, ["confirm", "error"]);
});

test("graded but the next card cannot be fetched: the old card must not stay up to be graded twice", async () => {
  const r = rig();
  await r.drill.start();
  await r.drill.reveal();
  r.state.failNext = true;
  await r.drill.grade(2);
  assert.equal(r.drill.state.card, null);
  assert.equal(r.drill.state.empty, false, "it is not 'up to date': it is an error");
  assert.equal(r.drill.state.failed, true);
  assert.deepEqual(r.log.toasts.at(-1), ["caído", false]);
  r.state.failNext = false;
  await r.drill.start();
  assert.equal(r.drill.state.card.id, "c2", "starting again brings it");
  assert.equal(r.drill.state.failed, false);
});

test("a first card that cannot be fetched opens nothing and says why", async () => {
  const r = rig();
  r.state.failNext = true;
  await r.drill.start();
  assert.equal(r.drill.state.active, false);
  assert.deepEqual(r.log.toasts, [["caído", false]]);
});

test("leaving closes the review at once, and an answer that arrives afterwards is thrown away", async () => {
  const r = rig();
  let release;
  r.state.hold = new Promise((resolve) => { release = resolve; });
  const started = r.drill.start();
  await flush();
  r.drill.leave();
  assert.equal(r.drill.state.active, false);
  release();
  await started;
  assert.equal(r.drill.state.card, null, "the late card is not shown behind his back");
  r.state.hold = null;
  await r.drill.start();
  await r.drill.reveal();
  r.drill.leave();
  assert.deepEqual([r.drill.state.card, r.drill.state.shown, r.drill.state.answer, r.drill.state.topic], [null, false, "", ""]);
});

test("a card from the server that is not a card is a failure, not a blank card", async () => {
  const r = rig({ cards: [{ topic: "no id" }] });
  await r.drill.start();
  assert.equal(r.drill.state.active, false);
  assert.equal(r.log.toasts.length, 1);
});

test("the question and the topic of a card are cleaned: they came from a model and from a web page", async () => {
  const r = rig({ cards: [{ id: "x", topic: "t\u202eopic", question: "Q".repeat(900) + "\u0007", offer: "o\nffer" }] });
  await r.drill.start();
  assert.ok(r.drill.state.card.question.length <= 600);
  assert.doesNotMatch(r.drill.state.card.topic + r.drill.state.card.question + r.drill.state.card.offer, /[\u202e\u0007\n]/);
});
