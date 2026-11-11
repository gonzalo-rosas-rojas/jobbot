// The review of APRENDER (spec §5.4): one question at a time, the reference answer hidden until he asks, a grade from 0 to 5 (SM-2 is the
// server's), and the next question as soon as the grade lands. This file is the controller and has no DOM: the requests, the clock of
// "one thing at a time" and the toasts are handed in, so the whole of it runs in Node with a fake server. render.js draws `state`.
//
// The routes are the old panel's and already exist: GET /api/learn/next?topic=, GET /api/learn/answer?id=, POST /api/learn/grade {id, grade}.
// What he types in "tu respuesta" is never sent and never saved: it is his rehearsal, kept by the box and gone with the card.

import { cardOf } from "./data.js";

export const GRADES = [0, 1, 2, 3, 4, 5];

/**
 * createDrill({ api, act, said, toast, t, sound, onChange }) -> { state, start(topic), reveal(), grade(n), leave() }
 *   api(path) -> { ok, status, data }      act(path, body, { onOk }) -> { ok, status, message }      said(r, fallback) -> the server's message
 *   state = { active, topic, waiting, card, empty, failed, shown, answer, busy }
 * `busy` is "reveal" or "grade": one thing at a time, so a double press never asks twice. `waiting` means the first card is on its way, `empty`
 * that the server has nothing to ask ("al día"), `failed` that a grade landed but the next card could not be brought.
 */
export function createDrill({ api, act, said, toast, t, sound, onChange = () => {} }) {
  const state = { active: false, topic: "", waiting: false, card: null, empty: false, failed: false, shown: false, answer: "", busy: null };
  let serial = 0; // a newer request for a card (or leaving) makes an older answer stale

  const change = () => onChange();
  const clear = () => Object.assign(state, { waiting: false, card: null, empty: false, failed: false, shown: false, answer: "" });

  /** Asks for the next card. `initial`: it is the first one, so a failure opens nothing. True when the state was brought up to date. */
  async function pull({ fresh = false, initial = false } = {}) {
    const mine = ++serial;
    const r = await api("/api/learn/next" + (state.topic ? "?topic=" + encodeURIComponent(state.topic) : ""));
    if (mine !== serial) return false; // he left, or asked again: this answer is stale
    const card = r.ok && r.data ? cardOf(r.data.card) : undefined;
    if (card === undefined) {
      toast(said(r, t("aprender.err.next")), false);
      if (initial) { state.active = false; state.waiting = false; }
      change();
      return false;
    }
    state.waiting = false;
    if (card && state.card && card.id === state.card.id && !fresh) { change(); return true; } // the queue moves only when a card is graded
    Object.assign(state, { card, empty: !card, failed: false, shown: false, answer: "" });
    change();
    return true;
  }

  async function start(topic = "") {
    if (state.active && state.card && topic === state.topic) return;
    state.active = true;
    state.topic = topic;
    clear();
    state.waiting = true;
    change();
    await pull({ fresh: true, initial: true });
  }

  async function reveal() {
    const card = state.card;
    if (state.busy || !card || state.shown) return;
    state.busy = "reveal";
    change();
    try {
      const r = await api("/api/learn/answer?id=" + encodeURIComponent(card.id));
      if (state.card !== card) return; // he moved on while it came
      if (!r.ok || !r.data) { toast(said(r, t("aprender.err.answer")), false); return; }
      state.answer = typeof r.data.answer === "string" ? r.data.answer : "";
      state.shown = true;
    } finally {
      state.busy = null;
      change();
    }
  }

  async function grade(g) {
    const card = state.card;
    if (state.busy || !card || !state.shown || typeof g !== "number" || !GRADES.includes(g)) return;
    state.busy = "grade";
    change();
    sound.play("confirm");
    let pulling = null;
    try {
      const r = await act("/api/learn/grade", { id: card.id, grade: g }, { onOk: () => { pulling = pull({ fresh: true }); } });
      if (!r || !r.ok) { sound.play("error"); return; }
      if (pulling) {
        const got = await pulling;
        // graded, but the next card could not be brought: the old one must not stay up to be graded twice
        if (!got && state.card === card) Object.assign(state, { card: null, empty: false, failed: true, shown: false, answer: "" });
      }
    } finally {
      state.busy = null;
      change();
    }
  }

  function leave() {
    serial += 1;
    Object.assign(state, { active: false, topic: "", busy: null });
    clear();
    change();
  }

  return { state, start, reveal, grade, leave };
}
