import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { setLang, setTimeZone, has } from "../src/i18n/index.js";
import esCola from "../src/i18n/es/cola.js";
import enCola from "../src/i18n/en/cola.js";
import { model, nightFigures } from "../src/core/model.js";
import {
  colaView, engineOf, factorsOf, funnelOf, trustOf, pathOf, orderedProposals, keyShown,
  FACTOR_WEIGHTS, DEFAULT_BATCH_MIN, PENDING_SHOWN, MIN_BASE, ANSWER_KEY,
} from "../src/moments/cola/view.js";
import { KEYS } from "../src/ui/keys.js";
import { STOPS_META } from "../src/moments/registry.js";

const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/snapshot.${name}.json`, import.meta.url), "utf8"));
const HOUR = 3600000;
const view = (snap, opts = {}) => colaView(model(snap), { nowMs: Date.parse(snap.generated_at), ...opts });
const fresh = (lang = "es") => { setLang(lang); setTimeZone("America/Santiago"); };
const strings = (value, out = []) => {
  if (typeof value === "string") out.push(value);
  else if (Array.isArray(value)) value.forEach((v) => strings(v, out));
  else if (value && typeof value === "object") Object.values(value).forEach((v) => strings(v, out));
  return out;
};
// A proposal to approve, as the server sends it, with whatever the test changes.
const proposal = (n, over = {}) => ({
  key: "p:" + n, company: "Company " + n, title: "Role " + n, score: 70 + n, state: "prepared", source: "ashby", url: "https://example.com/p/" + n,
  reason: null, reason_kind: null, reason_short: null, note: null, approved: false, has_screenshot: true, has_cv: true, has_dossier: false,
  updated_at: "2026-10-07T06:00:00Z", actions: ["approve"], marks: [], posted_at: "2026-10-02T15:00:00Z", first_seen: "2026-10-03T15:00:00Z",
  salary_min: 5000, salary_max: 6500, days_in_state: 1, snoozed_until: null, waiting_since: null, age_days: 4, prune_in_days: 10, ...over,
});
const withProposals = (snap, cards) => { snap.awaiting_approval = { total: cards.length, cards }; return snap; };
const noNull = (v, label) => { for (const s of strings(v)) assert.doesNotMatch(s, /\b(?:null|undefined|NaN)\b/, label + ": " + s); };

test("A16: every figure of COLA is the snapshot's, so changing the snapshot changes the screen and nothing is a literal", () => {
  fresh();
  for (const name of ["empty", "rich", "heavy"]) {
    const snap = fixture(name);
    const v = view(snap);
    const night = nightFigures(snap);
    assert.equal(v.head.total || night.awaiting, night.awaiting, name);
    assert.deepEqual(v.funnel.figs.map((f) => f.n), [snap.pipeline.seen, snap.pipeline.above, snap.pipeline.with_cv, snap.pipeline.complete, snap.pipeline.sent], name);
    if (snap.trust) assert.deepEqual([v.trust.reviewed, v.trust.total, v.trust.sent, v.trust.cap], [snap.trust.reviewed, snap.trust.first_n, snap.trust.sent_24h, snap.trust.cap], name);
    else assert.equal(v.trust, null, "without `trust` the band is not drawn");
  }
  const snap = withProposals(fixture("rich"), [proposal(1), proposal(2), proposal(3)]);
  const before = view(snap);
  assert.equal(before.head.total, 3);
  snap.awaiting_approval.total += 4;
  snap.pipeline.seen += 40;
  snap.trust.reviewed += 1;
  snap.trust.sent_24h += 3;
  const after = view(snap);
  assert.equal(after.head.total, 7, "a listing cut short still says how many wait");
  assert.equal(after.head.of, "de 7");
  assert.equal(after.funnel.figs[0].n, before.funnel.figs[0].n + 40);
  assert.equal(after.trust.reviewed, before.trust.reviewed + 1);
  assert.equal(after.trust.sent, before.trust.sent + 3);
  assert.equal(after.trust.sentLabel, "Envíos en 24 h: 5 de 10");
  assert.equal(after.trust.reviewLabel, "Aprobadas por ti: 4 de 5");
});

test("the headline is the position of the proposal in focus among those that wait, best score first", () => {
  fresh();
  const snap = withProposals(fixture("rich"), [proposal(1), proposal(5), proposal(3)]);
  assert.deepEqual(orderedProposals(snap).map((c) => c.key), ["p:5", "p:3", "p:1"], "best score first");
  const first = view(snap);
  assert.deepEqual([first.head.lead, first.head.n, first.head.of, first.head.ask], ["Por aprobar", 1, "de 3", "¿La dejas pasar?"]);
  assert.equal(first.focus.key, "p:5", "no focus asked: the best one");
  const third = view(snap, { focusKey: "p:1" });
  assert.deepEqual([third.head.n, third.focus.key, third.focus.step.label], [3, "p:1", "3 de 3"]);
  assert.equal(view(snap, { focusKey: "p:gone" }).focus.key, "p:5", "a proposal that went away falls back to the best");
  assert.deepEqual(view(snap, { focusKey: "p:3" }).queue.groups[0].rows.map((r) => r.data.card), ["p:5", "p:1"], "the others, best first, are the rows under it");
  assert.equal(PENDING_SHOWN, 4);
});

test("A15: the engine is told as it is; approving says what really happens, never that it was sent", () => {
  fresh();
  const now = Date.parse("2026-10-07T15:00:00Z");
  const running = engineOf({ engine: "recent", last_run_at: new Date(now - 7 * HOUR).toISOString(), window_open: true, scheduled_task: null }, now);
  assert.equal(running.approve, "recent");
  assert.deepEqual(running.warn, [], "a running engine needs no warning, and a null task is not mentioned");
  assert.match(running.run, /^última corrida .* \(hace 7 h\)$/);
  assert.equal(running.window, "Ventana abierta");

  const stale = engineOf({ engine: "stale", last_run_at: new Date(now - 80 * HOUR).toISOString(), window_open: false, next_window: "2026-10-07 18:00", scheduled_task: null }, now);
  assert.equal(stale.approve, "idle");
  assert.deepEqual(stale.warn, ["El motor corrió por última vez hace 3 días: aprobar no envía nada hasta que corra.", "Tarea programada: sin dato."], "null is 'sin dato', never 'no existe'");
  assert.equal(stale.window, "Ventana cerrada, abre 18:00");

  const noTask = engineOf({ engine: "stale", last_run_at: new Date(now - 80 * HOUR).toISOString(), scheduled_task: { exists: false } }, now);
  assert.equal(noTask.warn[1], "Sin tarea programada.", "'no task' needs exists === false");
  const task = engineOf({ engine: "never", last_run_at: null, scheduled_task: { exists: true, next_run_at: "2026-10-08T02:00:00Z" } }, now);
  assert.deepEqual(task.warn, ["El motor todavía no ha corrido: aprobar no envía nada hasta que corra.", "Próxima ejecución 7 oct, 23:00."]);
  assert.equal(task.run, "sin corridas todavía");

  const stopped = engineOf({ engine: "recent", stopped: true, last_run_at: new Date(now - HOUR).toISOString() }, now);
  assert.equal(stopped.approve, "stopped");
  assert.deepEqual(stopped.warn, ["STOP está activo: aprobar no envía nada hasta que lo quites."]);
  const unknown = engineOf({}, now);
  assert.deepEqual([unknown.kind, unknown.run, unknown.window, unknown.warn], ["unknown", "sin datos de corridas", "Ventana sin datos", []]);

  const snap = withProposals(fixture("rich"), [proposal(1)]);
  const label = (controls) => view({ ...snap, controls: { ...snap.controls, ...controls } }).focus.actions.find((a) => a.id === "approve").label;
  assert.equal(label({}), "Aprobar: la próxima corrida la envía");
  assert.equal(label({ stopped: true }), "Aprobar: saldrá cuando reanudes");
  assert.equal(label({ engine: "stale" }), "Aprobar: no sale hasta que el motor corra");
  assert.equal(label({ engine: "never", last_run_at: null }), "Aprobar: no sale hasta que el motor corra");
  for (const c of [{}, { stopped: true }, { engine: "never", last_run_at: null }]) assert.doesNotMatch(label(c), /envió|enviada|enviado/, "an approval is not a send");
  setLang("en");
  assert.equal(label({}), "Approve: the next run sends it");
  fresh();
});

test("the verbs are the keys of the one table: a once per row, the label of the proposal says the whole thing", () => {
  fresh();
  const snap = withProposals(fixture("rich"), [proposal(1), proposal(2)]);
  const v = view(snap);
  const visible = v.focus.actions.filter((a) => a.visible);
  assert.deepEqual(visible.map((a) => [a.id, a.key]), [["approve", "a"], ["snooze", "s"], ["open", "↵"]]);
  assert.equal(v.focus.actions.find((a) => a.id === "approve").permanent, true, "approve takes two presses");
  assert.deepEqual(v.focus.actions.filter((a) => !a.visible).map((a) => [a.id, a.key]), [["link", "o"]], "the link works by key only");
  assert.deepEqual([v.batch.key, v.foot.stop.key], ["⇧A", "p"]);
  assert.equal(keyShown("open"), "↵");
  assert.throws(() => keyShown("nope"), /not an action/);
  for (const a of v.focus.actions) assert.ok(KEYS.some((k) => k.id === a.id), `${a.id} is in the key table`);
  // one key per row: no key is shown twice inside a row
  for (const g of v.queue.groups) for (const row of g.rows) {
    const shown = row.actions.filter((a) => a.visible).map((a) => a.key);
    assert.equal(new Set(shown).size, shown.length, row.id);
    assert.ok(shown.length <= 1, `${row.id}: a row shows one verb`);
  }
  assert.equal(ANSWER_KEY, STOPS_META.find((s) => s.id === "aprender").key, "Contestar is the key of the stop where the answers are written");
});

test("the lote: the picks are what reaches the threshold and can be approved, with the names the confirmation will draw", () => {
  fresh();
  const snap = withProposals(fixture("rich"), [proposal(20, { score: 92 }), proposal(19, { score: 89 }), proposal(18, { score: 84 }), proposal(17, { score: 95, actions: [] })]);
  const v = view(snap);
  assert.equal(DEFAULT_BATCH_MIN, 85);
  assert.deepEqual(v.batch.picks, [{ key: "p:20", company: "Company 20", score: 92 }, { key: "p:19", company: "Company 19", score: 89 }], "a proposal with no approve action is not in the lote");
  assert.equal(v.batch.label, "Aprobar las de 85 o más (2)");
  assert.equal(v.batch.names, "Company 20 92 · Company 19 89");
  assert.equal(v.batch.ask, "¿aprobar 2?");
  assert.equal(v.batch.note, "al llegar a 5 aprobadas, jobbot envía solo", "he has 3 reviewed and the lote adds 2: it ends the review phase, and says so");
  const high = view(snap, { batchMin: 90 });
  assert.deepEqual(high.batch.picks.map((p) => p.key), ["p:20"]);
  assert.equal(high.batch.label, "Aprobar las de 90 o más (1)");
  assert.equal(high.batch.note, "", "one more makes 4 of 5: the review phase goes on");
  const none = view(snap, { batchMin: 99 });
  assert.deepEqual([none.batch.can, none.batch.count, none.batch.label], [false, 0, "Ninguna llega a 99"]);
  const more = withProposals(fixture("rich"), Array.from({ length: 7 }, (_, i) => proposal(i + 1, { score: 90 + (i % 5) })));
  const big = view(more);
  assert.equal(big.batch.count, 7);
  assert.match(big.batch.names, / y 3 más$/, "the confirmation names four and counts the rest");
  assert.equal(big.batch.note, "al llegar a 5 aprobadas, jobbot envía solo", "the lote crosses the end of the review phase: it says so");
});

test("why it is his turn: the count of the review phase comes from `trust`, and without `trust` nothing is invented", () => {
  fresh();
  const snap = withProposals(fixture("rich"), [proposal(1)]);
  assert.equal(snap.trust.reviewed, 3);
  assert.match(view(snap).focus.why.text, /^Llené el formulario [^.]*\. Si la apruebas, serán 4 de tus 5 primeras aprobaciones\. Si está bien, la próxima corrida la envía;/);
  snap.trust.reviewed = 4;
  assert.match(view(snap).focus.why.text, /Con esta completas tus 5 primeras aprobaciones: después jobbot envía solo, hasta 25 al día\./);
  snap.trust.reviewed = 9;
  snap.trust.review_phase = false;
  assert.doesNotMatch(view(snap).focus.why.text, /primeras aprobaciones/, "past the review phase there is no count to give");
  delete snap.trust;
  const none = view(snap);
  assert.equal(none.trust, null);
  assert.equal(none.foot.left, "");
  assert.doesNotMatch(none.focus.why.text, /primeras|serán/);
});

test("the five layers say what the card proves, and a layer that is not there is missing, never drawn as done", () => {
  fresh();
  const full = pathOf(proposal(1), { threshold: 60 });
  assert.deepEqual(full.map((l) => l.id), ["aviso", "puntaje", "cv", "respuestas", "compuerta"]);
  assert.deepEqual(full.map((l) => l.state), ["done", "done", "done", "done", "here"]);
  assert.equal(full[0].note, "ashby, 3 oct, 12:00");
  assert.equal(full[1].note, "71 de 100, umbral 60");
  assert.equal(full[4].here, "aquí");
  const bare = pathOf(proposal(1, { has_cv: false, has_screenshot: false, score: null, first_seen: null }), { threshold: null });
  assert.deepEqual(bare.map((l) => l.state), ["done", "missing", "missing", "missing", "here"]);
  assert.equal(bare[1].note, "");
  assert.equal(bare[0].note, "ashby");
  assert.equal(pathOf(proposal(1), { threshold: null })[1].note, "71 de 100");
});

test("the six bars: the weights of the score, two columns of three, a factor the page does not know is still shown", () => {
  fresh();
  assert.deepEqual(FACTOR_WEIGHTS.map(([id]) => id), ["fit", "compensation", "interview_style", "stability", "learning", "other"]);
  assert.equal(FACTOR_WEIGHTS.reduce((n, [, w]) => n + w, 0), 100);
  const ok = { status: "ok", data: { score_detail: { fit: 27, compensation: 15, stability: 18, learning: 13, interview_style: 10, other: 9, novelty: 4 } } };
  const f = factorsOf(ok);
  assert.equal(f.state, "ok");
  assert.deepEqual(f.bars.map((b) => b.id), ["fit", "compensation", "interview_style", "stability", "learning", "other", "novelty"]);
  assert.deepEqual(f.bars.map((b) => b.points), [27, 15, 10, 18, 13, 9, 4]);
  assert.deepEqual(f.bars.slice(0, 3).map((b) => b.max), [30, 15, 10]);
  assert.equal(f.bars[6].max, null, "an unknown factor has no maximum to show");
  assert.equal(f.bars[0].k, 0.9);
  assert.equal(f.bars[0].label, "Encaje");
  assert.ok(f.bars.every((b) => b.k >= 0 && b.k <= 1));
  assert.equal(factorsOf({ status: "ok", data: { score_detail: { fit: 99 } } }).bars[0].k, 1, "a bar never leaves its track");
  assert.deepEqual([factorsOf(null).state, factorsOf({ status: "loading" }).state, factorsOf({ status: "error" }).state, factorsOf({ status: "ok", data: {} }).state], ["loading", "loading", "error", "none"]);
  assert.deepEqual(factorsOf({ status: "ok", data: { score_detail: { fit: "nope", compensation: 10 } } }).bars.map((b) => b.id), ["compensation"], "a point that is not a number is not a bar (never NaN, never a made-up zero), as in the two drawers");
});

test("the funnel: five figures from the pipeline, a share only when the base is big enough to mean something, the last one marked", () => {
  fresh();
  const f = funnelOf({ days: 7, threshold: 60, seen: 287, above: 20, with_cv: 15, complete: 11, sent: 9 });
  assert.deepEqual(f.figs.map((x) => x.n), [287, 20, 15, 11, 9]);
  assert.deepEqual(f.figs.map((x) => x.id), ["read", "above", "cv", "form", "sent"]);
  assert.deepEqual(f.figs.map((x) => x.last), [false, false, false, false, true]);
  assert.deepEqual(f.figs.map((x) => x.sub), ["en 7 días", "7% de los leídos", "75% de los que pasaron", "73% de los que tienen CV", "82% de las completas"]);
  assert.equal(f.title, "Del aviso al envío, últimos 7 días (escala logarítmica)");
  assert.equal(f.log[0], 1);
  assert.ok(f.log.every((x, i) => i === 0 || x <= f.log[i - 1]), "the band only narrows");
  const small = funnelOf({ days: 7, seen: 40, above: 4, with_cv: 3, complete: 2, sent: 1 });
  assert.equal(MIN_BASE, 10);
  assert.deepEqual(small.figs.map((x) => x.sub), ["en 7 días", "10% de los leídos", "3 de 4 que pasaron", "2 de 3 con CV", "1 de 2 completas"], "under ten, a share is noise: it is said as n of base");
  const empty = funnelOf({ days: 7, seen: 0, above: 0, with_cv: 0, complete: 0, sent: 0 });
  assert.equal(empty.empty, true);
  assert.deepEqual(empty.log, [0, 0, 0, 0, 0]);
  assert.deepEqual(funnelOf(undefined).figs.map((x) => x.n), [0, 0, 0, 0, 0]);
  assert.equal(funnelOf({ days: 7, seen: 5, above: 5, with_cv: 5, complete: 5, sent: 5 }).figs[1].sub, "5 de 5 leídos");
});

test("trust: squares filled with what he reviewed, a bar of what the cap holds, no band without `trust`", () => {
  fresh();
  const t5 = trustOf({ reviewed: 3, first_n: 5, review_phase: true, sent_24h: 2, cap: 10, cap_after: 25 });
  assert.deepEqual(t5.squares, [true, true, true, false, false]);
  assert.equal(t5.k, 0.2);
  assert.equal(t5.reviewSub, "Al llegar a 5 jobbot envía solo");
  assert.equal(t5.sentSub, "El tope cuenta clics, con o sin confirmación");
  const done = trustOf({ reviewed: 7, first_n: 5, review_phase: false, sent_24h: 30, cap: 25, cap_after: 25 });
  assert.deepEqual(done.squares, [true, true, true, true, true]);
  assert.equal(done.k, 1, "past the cap the bar is full, not longer");
  assert.equal(done.reviewSub, "Ya pasaste la revisión: jobbot envía solo");
  assert.equal(trustOf({ reviewed: 1, first_n: 40, review_phase: true, sent_24h: 0, cap: 0, cap_after: 0 }).squares.length, 12, "never more squares than fit");
  assert.equal(trustOf({ reviewed: 1, first_n: 5, review_phase: true, sent_24h: 1, cap: 0, cap_after: 25 }).k, 0, "a cap of zero is not a division by zero");
  assert.equal(trustOf(null), null);
  assert.equal(trustOf(undefined), null);
});

test("trust: past 12 the squares keep the proportion reviewed/total, and are all filled only when everything was reviewed", () => {
  fresh();
  const on = (reviewed, total) => trustOf({ reviewed, first_n: total, review_phase: reviewed < total, sent_24h: 0, cap: 10, cap_after: 25 }).squares.filter(Boolean).length;
  assert.equal(on(13, 20), 7, "13 of 20 is 65%: 7 of 12");
  assert.equal(on(10, 20), 6);
  assert.equal(on(0, 20), 0);
  assert.equal(on(19, 20), 11, "one short of done never shows full");
  assert.equal(on(39, 40), 11);
  assert.equal(on(20, 20), 12);
  assert.equal(on(25, 20), 12, "more than asked is still just full");
  assert.equal(on(12, 12), 12);
  assert.equal(on(11, 12), 11, "up to 12 it is still one square per approval");
  assert.equal(on(0, 0), 0);
});

test("the queue under the proposal: the others by score, what is approved and waits, what is blocked by answers, what went out", () => {
  fresh();
  const snap = withProposals(fixture("rich"), [proposal(9, { score: 91 }), proposal(8, { score: 80 })]);
  const v = view(snap);
  const [pending, approved, blocked, sent] = v.queue.groups;
  assert.deepEqual(pending.rows.map((r) => r.data.card), ["p:8"]);
  assert.equal(pending.rows[0].kind, "Preparada");
  assert.deepEqual(pending.rows[0].actions.filter((a) => a.visible).map((a) => [a.id, a.key, a.label]), [["approve", "a", "Aprobar"]]);
  assert.deepEqual(approved.rows.map((r) => r.data.card), ["fx:13"], "approved by him and waiting for the next run");
  assert.match(approved.rows[0].meta, /sale en la próxima corrida$/);
  assert.deepEqual(blocked.rows.map((r) => r.data.card), ["fx:10"], "only what waits for answers: the other stuck kinds are HOY's");
  assert.deepEqual(blocked.rows[0].actions.filter((a) => a.visible).map((a) => [a.id, a.key, a.label]), [["answer", "4", "Contestar"]]);
  assert.ok(blocked.rows[0].actions.some((a) => a.id === "resolve" && !a.visible), "u still works by key");
  assert.equal(sent.rows.length, 1);
  assert.equal(sent.rows[0].kind, "Enviadas 2");
  assert.match(sent.rows[0].text, /^Upsilon Test Co 13:00|Tau Test Co 16:00/);
  assert.equal(sent.rows[0].meta, "2 de 10 del tope");
  assert.equal(sent.rows[0].open, "fx:19", "it opens the newest");
  assert.equal(v.queue.title, "La cola: 1 preparada espera tu tecla, 1 aprobada espera al motor, 1 bloqueada por respuestas, 2 enviadas en 24 h");
  snap.controls = { ...snap.controls, stopped: true };
  assert.match(view(snap).queue.groups[1].rows[0].meta, /saldrá cuando reanudes$/);
  // moves older than a day are not "tonight"
  const old = withProposals(fixture("rich"), [proposal(1)]);
  old.activity.moves = old.activity.moves.map((x) => ({ ...x, at: "2026-10-01T10:00:00Z" }));
  assert.equal(view(old).queue.groups[3].rows.length, 0);
  assert.equal(view(old).queue.title.includes("enviada"), false);
});

test("a long list is cut and says how many are left", () => {
  fresh();
  const snap = withProposals(fixture("rich"), Array.from({ length: 9 }, (_, i) => proposal(i + 1)));
  const v = view(snap);
  const pending = v.queue.groups[0];
  assert.equal(pending.rows.length, PENDING_SHOWN);
  assert.equal(pending.count, 8);
  assert.equal(pending.more, 4);
  assert.equal(pending.title, "8 preparadas esperan tu tecla");
});

test("calm: nothing to approve says the truth about the last run and draws no proposal", () => {
  fresh();
  const empty = view(fixture("empty"));
  assert.equal(empty.empty, true);
  assert.equal(empty.focus, null);
  assert.equal(empty.head.lead, "Nada por aprobar.");
  assert.equal(empty.head.sub, "Lo que el bot descubra esta noche aparece aquí.");
  assert.equal(empty.engine.line, "Ventana sin datos · sin corridas todavía");
  assert.equal(empty.batch.can, false);
  assert.equal(empty.funnel.empty, true);
  assert.equal(empty.trust, null);
  const ran = withProposals(fixture("rich"), []);
  assert.match(view(ran).head.sub, /^La última corrida fue hace 7 h y no dejó nada para aprobar\.$/);
  noNull(empty, "empty");
});

test("a partial snapshot never puts null, undefined or NaN on the page", () => {
  fresh();
  const snap = withProposals(fixture("rich"), [proposal(1, { score: null, source: null, salary_min: null, salary_max: null, updated_at: null, first_seen: null, days_in_state: null, title: null, company: null, url: null })]);
  delete snap.trust;
  snap.pipeline = {};
  snap.activity = undefined;
  snap.controls = { stopped: false };
  const v = view(snap, { detail: () => ({ status: "ok", data: { location: null, remote: undefined, score_detail: { fit: undefined } } }) });
  noNull(v, "partial");
  assert.equal(v.focus.score, "n/a");
  assert.equal(v.focus.facts.every((f) => f.length > 0), true);
  const bare = colaView(model({ ...fixture("empty"), awaiting_approval: undefined, needs_gonza: undefined, board: [], trust: undefined, activity: undefined, controls: undefined, pipeline: undefined, funnel: undefined }), { nowMs: Date.parse("2026-10-07T15:00:00Z") });
  noNull(bare, "bare");
  assert.equal(bare.empty, true);
});

test("hostile text stays text: cut, one line, no bidi controls, a link only when it is http(s)", () => {
  fresh();
  const evil = "<img src=x onerror=alert(1)>\u202e drowssap\u2066\n" + "x".repeat(300);
  const snap = withProposals(fixture("rich"), [proposal(1, { company: evil, title: evil, source: "<b>x</b>", url: "javascript:alert(1)" }), proposal(2, { company: evil, title: evil, url: "https://example.com/ok" })]);
  const v = view(snap, { focusKey: "p:1", detail: () => ({ status: "ok", data: { location: evil, apply_url: "javascript:alert(2)", score_detail: {} } }) });
  for (const s of strings(v)) {
    assert.doesNotMatch(s, /[\u202a-\u202e\u2066-\u2069\u061c\u200e\u200f\n\r]/, "no bidi control or line break survives");
    assert.ok(s.length <= 400, `a text from outside is cut (${s.length}): ${s.slice(0, 40)}`);
  }
  assert.equal(v.focus.link, null, "a javascript: address is never a link");
  assert.ok(v.focus.company.endsWith("…") && v.focus.company.length <= 60);
  const second = view(snap, { focusKey: "p:2" });
  assert.equal(second.focus.link, "https://example.com/ok");
  assert.match(second.focus.company, /^<img src=x onerror=alert\(1\)>/, "kept as the text it is: the DOM never parses it");
});

test("every `cola.` key the code asks for exists in both languages (a plural by its .other)", () => {
  const dir = new URL("../src/moments/", import.meta.url);
  const files = [...(existsSync(new URL("cola.js", dir)) ? [new URL("cola.js", dir)] : []), ...readdirSync(new URL("cola/", dir)).filter((f) => f.endsWith(".js")).map((f) => new URL("cola/" + f, dir))];
  const wanted = new Set();
  for (const f of files) for (const m of readFileSync(f, "utf8").matchAll(/["'`](cola\.[a-zA-Z0-9_.]+)["'`]/g)) wanted.add(m[1]);
  assert.ok(wanted.size > 20, "the code was read (" + wanted.size + " keys)");
  // a key ending in a dot is a prefix the code finishes by hand ("cola.act.approve." + engine.approve): something must start with it
  for (const [lang, dict] of [["es", esCola], ["en", enCola]]) {
    setLang(lang);
    for (const key of wanted) {
      if (key.endsWith(".")) assert.ok(Object.keys(dict).some((k) => k.startsWith(key)), `${lang}: nothing starts with ${key}`);
      else assert.ok(has(key) || has(key + ".other"), `${lang}: missing ${key}`);
    }
  }
  // and the other way: a key nobody asks for is dead text
  const sources = files.map((f) => readFileSync(f, "utf8")).join("\n");
  const dead = Object.keys(esCola).filter((k) => {
    const base = k.replace(/\.(zero|one|other)$/, "");
    const plural = base !== k && ["zero", "one", "other"].some((s) => base + "." + s !== k && base + "." + s in esCola);
    if (!plural) return !sources.includes(JSON.stringify(k)) && ![...wanted].some((w) => w.endsWith(".") && k.startsWith(w));
    return !sources.includes(JSON.stringify(k)) && !sources.includes(JSON.stringify(base)) && ![...wanted].some((w) => w.endsWith(".") && base.startsWith(w));
  });
  assert.deepEqual(dead, [], "keys no code asks for");
  fresh();
});

test("English reads like English: the same view, the same numbers", () => {
  const snap = withProposals(fixture("rich"), [proposal(1), proposal(2)]);
  fresh("es");
  const es = view(snap);
  fresh("en");
  const en = view(snap);
  assert.deepEqual([en.head.lead, en.head.of, en.head.ask], ["To approve", "of 2", "Let it pass?"]);
  assert.deepEqual(en.funnel.figs.map((f) => f.n), es.funnel.figs.map((f) => f.n));
  assert.equal(en.batch.count, es.batch.count);
  assert.equal(en.foot.stop.label, "Stop the engine");
  noNull(en, "en");
  fresh();
});
