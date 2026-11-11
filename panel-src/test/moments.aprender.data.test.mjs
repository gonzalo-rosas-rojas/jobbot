import test from "node:test";
import assert from "node:assert/strict";
import { missingLabels, questionKey, missingOf, learnOf, cardOf, parseDossier, eveOf, rejectionsOf, MIN_REJECTED } from "../src/moments/aprender/data.js";
import { clean, safeHref } from "../src/moments/shared/util.js";
import { richAprender, bareAprender, hostileAprender, DOSSIER_MD } from "./helpers/aprender-fixture.mjs";

const HOUR = 3600000;
const nowOf = (snap) => Date.parse(snap.generated_at);

test("missingLabels reads what the filler listed after 'faltan respuestas:' and nothing that follows the dash", () => {
  assert.deepEqual(missingLabels("faltan respuestas: Email *; Why this role? *"), ["Email *", "Why this role? *"]);
  assert.deepEqual(missingLabels("faltan respuestas: Email * — opción no encontrada: Country = Atlantis; no se pudo completar Phone: timeout"), ["Email *"], "the failures after the dash are not questions");
  assert.deepEqual(missingLabels("assist_closed: cerraste la pestaña. Quedaba pendiente: faltan respuestas: Email *"), ["Email *"], "wherever the text sits");
  assert.deepEqual(missingLabels("captcha"), []);
  assert.deepEqual(missingLabels(null), []);
  assert.deepEqual(missingLabels("faltan respuestas: ; ;  "), [], "blank labels are not questions");
});

test("a label that another person wrote is cleaned, cut, and never a control or bidi character", () => {
  const evil = "A\u202eb\u0007c\n" + "x".repeat(300);
  const [label] = missingLabels("faltan respuestas: " + evil);
  assert.ok(label.length <= 120, label.length);
  assert.doesNotMatch(label, /[\u202e\u0007\n]/);
  assert.equal(label.startsWith("A b c "), true, "the bad characters became spaces, not nothing glued together");
});

test("the same question written two ways is one question", () => {
  assert.equal(questionKey("Expected monthly salary (CLP) *"), questionKey("expected monthly SALARY (clp)"));
  assert.equal(questionKey("¿Años de experiencia?"), questionKey("anos de experiencia"));
  assert.notEqual(questionKey("Years of experience with Rust"), questionKey("Years of experience with SQL"));
  assert.ok(questionKey("経験年数") !== "", "a name with no latin letter still has a key");
});

test("missingOf groups the stuck forms by question: the most blocking first, each with the forms it blocks", () => {
  const snap = richAprender();
  const m = missingOf(snap);
  assert.equal(m.forms, 6, "six forms ask for answers the bank does not have");
  const byLabel = Object.fromEntries(m.groups.map((g) => [g.label, g.cards.map((c) => c.key).sort()]));
  assert.deepEqual(byLabel["Expected monthly salary (CLP)"], ["fx:21", "fx:25"], "'(CLP) *' and '(CLP)' are one question");
  assert.deepEqual(byLabel["Years of experience with Kubernetes"], ["fx:21", "fx:22"]);
  assert.deepEqual(byLabel["Link to your GitHub or portfolio"], ["fx:22", "fx:24"]);
  assert.deepEqual(byLabel["Can you work New York hours?"], ["fx:23"], "the failure after the dash is not a question");
  assert.ok(m.groups.every((g, i, all) => i === 0 || all[i - 1].cards.length >= g.cards.length), "most forms first");
  assert.equal(m.groups.length, 6, "six distinct questions: salary, salary (CLP), kubernetes, github, new york and start date");
  assert.equal(m.partial, false);
  // a form whose reason lists no question still counts, apart
  snap.needs_gonza.cards[1].reason = "faltan respuestas:";
  const odd = missingOf(snap);
  assert.equal(odd.unnamed.length, 1);
  assert.equal(odd.forms, 6, "still six forms");
});

test("a queue cut at its cap says so: the count is 'at least'", () => {
  const snap = richAprender();
  snap.needs_gonza.total = 80;
  assert.equal(missingOf(snap).partial, true);
});

test("learnOf is the snapshot's learn with every list a list, topics by what is due, and weak topics with their average", () => {
  const l = learnOf(richAprender());
  assert.deepEqual([l.total, l.seen, l.due], [56, 36, 12]);
  assert.equal(l.topics[0].topic, "rust");
  assert.deepEqual(l.topics.map((t) => t.due), [4, 2, 2, 1, 1, 1, 1], "most due first");
  assert.equal(l.topics.find((t) => t.topic === "sql").average, 3.2);
  assert.equal(l.topics.find((t) => t.topic === "rust").average, null);
  assert.equal(learnOf({}).total, 0, "no learn at all is a zero, never a throw");
  assert.equal(learnOf({ learn: { total: "x", topics: "no", gaps: null } }).topics.length, 0);
  assert.equal(learnOf({}).known, false);
  assert.equal(learnOf(richAprender()).known, true);
});

test("cardOf: a card from the server, null (nothing to ask), or undefined (not a card)", () => {
  assert.deepEqual(cardOf({ id: 3, topic: "rust", question: "Why?", offer: null }), { id: "3", topic: "rust", question: "Why?", offer: null });
  assert.equal(cardOf(null), null);
  assert.equal(cardOf("nope"), undefined);
  assert.equal(cardOf({ topic: "x" }), undefined);
});

test("parseDossier reads the markdown the engine writes: the summary, the likely questions, the stories with their evidence, the gaps", () => {
  const d = parseDossier(DOSSIER_MD);
  assert.match(d.summary, /^Plataforma de datos con 40 ingenieros/);
  assert.equal(d.questions.length, 4);
  assert.equal(d.questions[0].q, "¿Cómo diseñarías un pipeline de ingesta que tolere datos duplicados y fuera de orden?");
  assert.equal(d.questions[0].why, "el aviso habla de colas y reintentos.");
  assert.equal(d.questions[2].why, "");
  assert.deepEqual(d.stories.map((s) => s.title), ["Servicio de colas reescrito en Rust", "Corte de 40 % en la latencia de la API de pagos", "Guardia de dos años en un servicio crítico"]);
  assert.equal(d.stories[0].evidence, "Rewrote the queue service in Rust, halving p99 latency");
  assert.deepEqual(d.gaps.map((g) => g.topic), ["queues-and-backpressure", "sql-windows"]);
  assert.equal(parseDossier("just some text"), null, "no heading of ours: the caller says 'sin dato'");
  assert.equal(parseDossier(""), null);
  assert.equal(parseDossier(null), null);
  const none = parseDossier("## Historias (solo hechos de tu CV maestro)\n\nSin historias verificadas: cuéntame hechos tuyos y las completo.\n\n## Huecos a estudiar\n\nNinguno detectado.\n");
  assert.deepEqual([none.stories.length, none.gaps.length, none.summary], [0, 0, ""]);
});

test("a dossier from outside is text: hostile lines are cleaned and cut", () => {
  const d = parseDossier("## Preguntas probables\n\n1. " + "Q".repeat(500) + "\u202e\n");
  assert.ok(d.questions[0].q.length <= 200);
  assert.doesNotMatch(d.questions[0].q, /\u202e/);
});

test("eveOf picks the interview with a date from the recruiter's mail first, then the one with a dossier, and says where the date came from", () => {
  const snap = richAprender();
  const eve = eveOf(snap, nowOf(snap));
  assert.equal(eve.card.key, "fx:4");
  assert.equal(eve.when.kind, "task");
  assert.equal(eve.when.at, new Date(nowOf(snap) + 28 * HOUR).toISOString().replace(".000Z", "Z"));
  assert.equal(eve.others, 0);
  // no mail, but the card's own note carries a time
  snap.today.tasks = snap.today.tasks.filter((t) => t.kind !== "interview_invite");
  const note = eveOf(snap, nowOf(snap));
  assert.deepEqual([note.when.kind, note.when.text], ["note", "Interview on Thursday 16:00"]);
  // neither: the date is unknown and the block says so
  snap.board.find((c) => c.state === "interview").cards[0].note = "waiting for their reply";
  assert.equal(eveOf(snap, nowOf(snap)).when.kind, "none");
  // an interview already past by two days is not the eve of anything
  const old = richAprender();
  old.today.tasks.find((t) => t.kind === "interview_invite").due_at = new Date(nowOf(old) - 60 * HOUR).toISOString();
  assert.equal(eveOf(old, nowOf(old)).when.kind, "note", "the stale mail is not a date; the card's note is");
  // no interview at all
  assert.equal(eveOf(bareAprender(), nowOf(bareAprender())), null);
  assert.equal(eveOf({ board: [], today: { tasks: [] } }, 0), null);
});

test("with two interviews the nearest date wins and the other is counted, not hidden", () => {
  const snap = richAprender();
  const interview = snap.board.find((c) => c.state === "interview");
  interview.cards.push({ ...interview.cards[0], key: "fx:99", company: "Tau Test Co", has_dossier: true, note: null });
  snap.today.tasks.push({ id: 108, text: "x", job_key: "fx:99", due_at: new Date(nowOf(snap) + 5 * HOUR).toISOString(), overdue: false, link: null, kind: "interview_invite", created_at: snap.generated_at });
  const eve = eveOf(snap, nowOf(snap));
  assert.equal(eve.card.key, "fx:99", "the one in 5 hours comes before the one in 28");
  assert.equal(eve.others, 1);
});

test("rejectionsOf: counts, never rates; a pattern only with enough rejections; and what it cannot know it says", () => {
  assert.equal(MIN_REJECTED, 5);
  const r = rejectionsOf(richAprender());
  assert.equal(r.n, 9);
  assert.equal(r.total, 9);
  const score = r.rows.find((x) => x.id === "score");
  assert.ok(score.rejected < score.alive, `the rejected ones scored lower (${score.rejected} vs ${score.alive})`);
  const source = r.rows.find((x) => x.id === "source");
  assert.deepEqual([source.source, source.k, source.n], ["ashby", 5, 9]);
  assert.equal(r.few, false);
  // few rejections: no pattern, and the block says how many there are
  const few = richAprender();
  const col = few.board.find((c) => c.state === "rejected");
  col.cards = col.cards.slice(0, 3);
  col.total = 3;
  const f = rejectionsOf(few);
  assert.deepEqual([f.n, f.few, f.rows.length], [3, true, 0]);
  // none: nothing to say
  assert.equal(rejectionsOf(bareAprender()).n, 0);
  assert.equal(rejectionsOf({}).n, 0);
  // a column cut at its cap says how many exist
  const cut = richAprender();
  cut.board.find((c) => c.state === "rejected").total = 60;
  assert.deepEqual([rejectionsOf(cut).n, rejectionsOf(cut).total], [9, 60]);
});

test("the hostile snapshot reaches the data layer as text only", () => {
  const snap = hostileAprender();
  const m = missingOf(snap);
  for (const g of m.groups) {
    assert.ok(g.label.length <= 120);
    assert.doesNotMatch(g.label, /[\u202e\n]/);
    for (const c of g.cards) assert.equal(c.url, c.key === "fx:10" ? null : "https://example.com/jobs/" + c.key.slice(3), "javascript: is not an address; the others keep theirs");
  }
  assert.equal(safeHref("javascript:alert(1)"), null);
  assert.equal(clean("a\u202eb"), "a b");
});
