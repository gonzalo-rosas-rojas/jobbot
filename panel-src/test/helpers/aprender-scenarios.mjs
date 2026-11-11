// Scenarios of APRENDER, made from the shared fixtures without editing them (they are the other moments' too). No imports: the lab bundles this
// file for the browser, and test/helpers/aprender-fixture.mjs loads the fixtures from disk for Node. `rich` gets what the night desk needs to be judged
// (several forms stuck on shared questions, an interview with its dossier the next evening, nine rejections, a review with seven topics), `bare` gets
// one question and nothing else, `hostile` has a hostile text where another person's words reach the page. Every name is invented.
// the page guard refuses the literal scheme anywhere in a bundle (build.mjs), so the hostile link is put together here
const SCHEME = ["java", "script"].join("");
const copy = (base) => JSON.parse(JSON.stringify(base));
const HOUR = 3600000;
const DAY = 24 * HOUR;
const at = (snap, ms) => new Date(Date.parse(snap.generated_at) + ms).toISOString().replace(".000Z", "Z");

const stuckCard = (snap, n, company, title, reason, score = 80) => ({
  key: `fx:${n}`, company, title, score, state: "needs_gonza", source: ["ashby", "greenhouse", "lever"][n % 3], url: `https://example.com/jobs/${n}`,
  reason, reason_kind: "missing_answers", reason_short: "El formulario pide respuestas que no tengo", note: null, approved: false, has_screenshot: false, has_cv: true, has_dossier: false,
  updated_at: at(snap, -(n % 5 + 1) * DAY), actions: ["resolve", "confirm"], marks: [], posted_at: at(snap, -4 * DAY), first_seen: at(snap, -3 * DAY), salary_min: null, salary_max: null,
  days_in_state: n % 5 + 1, snoozed_until: null, waiting_since: at(snap, -(n % 5 + 1) * DAY), age_days: 4, prune_in_days: 16,
});

/** What a dossier of the engine looks like (src/dossier.rs `render_markdown`), with invented content. */
export const DOSSIER_MD = [
  "# Dossier — Delta Test Co · Backend Engineer", "", "Generado el 2026-10-07 (hora de Santiago) · aviso: https://example.com/jobs/4", "",
  "## Qué dice el aviso de la empresa", "", "Plataforma de datos con 40 ingenieros; la ronda es técnica, 60 min, con dos ingenieros del equipo de modelos.", "",
  "## Preguntas probables", "",
  "1. ¿Cómo diseñarías un pipeline de ingesta que tolere datos duplicados y fuera de orden?", "   *Por qué:* el aviso habla de colas y reintentos.",
  "2. Explica una decisión de consistencia que tomaste y qué perdiste con ella.", "   *Por qué:* piden diseño de sistemas distribuidos.",
  "3. ¿Cómo evaluarías un modelo que resume notas sin leer los datos reales?", "", "4. ¿Qué harías con una cola que crece sin parar?", "",
  "## Historias (solo hechos de tu CV maestro)", "",
  "### Servicio de colas reescrito en Rust", "", "Evidencia en el CV: «Rewrote the queue service in Rust, halving p99 latency»", "", "Cuenta el antes y el después.", "",
  "### Corte de 40 % en la latencia de la API de pagos", "", "Evidencia en el CV: «Cut payments API latency by 40%»", "", "Una cifra, una causa.", "",
  "### Guardia de dos años en un servicio crítico", "", "Evidencia en el CV: «On call for two years»", "", "Qué aprendiste de la peor noche.", "",
  "## Huecos a estudiar", "", "- **queues-and-backpressure** — ¿Qué es la contrapresión y cómo la aplicas? (repasa con `jobbot prep next --topic queues-and-backpressure`)", "- **sql-windows** — ¿Cuándo una ventana en SQL?", "",
].join("\n");

/** The rich snapshot plus the night desk's own cases. */
export function richOf(base) {
  const s = copy(base);
  const extra = [
    stuckCard(s, 21, "Mu Test Co", "Backend Engineer", "faltan respuestas: Expected monthly salary (CLP) *; Years of experience with Kubernetes *", 92),
    stuckCard(s, 22, "Nu Test Co", "Platform Engineer", "faltan respuestas: Years of experience with Kubernetes *; Link to your GitHub or portfolio", 88),
    stuckCard(s, 23, "Xi Test Co", "Data Engineer", "faltan respuestas: Can you work New York hours? — opción no encontrada: Country = Atlantis", 85),
    stuckCard(s, 24, "Omicron Test Co", "SRE", "faltan respuestas: Earliest start date; Link to your GitHub or portfolio *", 79),
    stuckCard(s, 25, "Pi Test Co", "Full-stack Engineer", "faltan respuestas: Expected monthly salary (CLP)", 74),
  ];
  s.needs_gonza.cards.push(...extra);
  s.needs_gonza.total += extra.length;
  s.board.find((c) => c.state === "needs_gonza").cards.push(...extra);
  s.board.find((c) => c.state === "needs_gonza").total += extra.length;
  // the interview of tomorrow evening: its card has a dossier and the recruiter's mail made a task with the date
  s.board.find((c) => c.state === "interview").cards[0].has_dossier = true;
  s.today.tasks.push({ id: 107, text: "Interview with Delta Test Co", job_key: "fx:4", due_at: at(s, 28 * HOUR), overdue: false, link: null, kind: "interview_invite", created_at: at(s, -3 * HOUR) });
  s.today.tasks_total += 1;
  // nine rejections: most of them a low score, most of them from one board
  const rejected = s.board.find((c) => c.state === "rejected");
  const scores = [66, 71, 68, 72, 64, 74, 69, 81];
  const sources = ["ashby", "ashby", "ashby", "lever", "ashby", "greenhouse", "ashby", "lever"];
  scores.forEach((score, i) => rejected.cards.push({ ...stuckCard(s, 40 + i, `Rho Test Co ${i + 1}`, "Engineer", null, score), state: "rejected", reason: null, reason_kind: null, reason_short: null, actions: [], source: sources[i], note: "They chose someone else", days_in_state: 3 + i }));
  rejected.total = rejected.cards.length;
  // the review: seven topics, three of them weak, two gaps from the dossier
  s.learn = {
    total: 56, seen: 36, due: 12,
    weak_topics: [{ topic: "system-design", average: 2.8 }, { topic: "sql", average: 3.2 }, { topic: "agents-mcp", average: 3.4 }],
    topics: [
      { topic: "rust", total: 12, seen: 9, due: 4 }, { topic: "system-design", total: 10, seen: 7, due: 2 }, { topic: "typescript", total: 8, seen: 6, due: 2 },
      { topic: "sql", total: 7, seen: 5, due: 1 }, { topic: "http-auth", total: 6, seen: 4, due: 1 }, { topic: "devops", total: 6, seen: 3, due: 1 }, { topic: "agents-mcp", total: 7, seen: 2, due: 1 },
    ],
    gaps: [
      { id: "gap-fx-4-1", topic: "queues-and-backpressure", offer_key: "fx:4", offer: "Delta Test Co — Backend Engineer", seen: false },
      { id: "gap-fx-4-2", topic: "sql-windows", offer_key: "fx:4", offer: "Delta Test Co — Backend Engineer", seen: false },
    ],
  };
  return s;
}

/** One form stuck on one question, no interview, nothing due: the smallest night desk that still has work. */
export function bareOf(base) {
  const s = copy(base);
  const one = stuckCard(s, 7, "Sigma Test Co", "Backend Engineer", "faltan respuestas: Expected monthly salary (CLP) *");
  s.needs_gonza = { total: 1, cards: [one] };
  s.board.find((c) => c.state === "needs_gonza").cards = [one];
  s.board.find((c) => c.state === "needs_gonza").total = 1;
  s.controls.last_run_at = at(s, -2 * HOUR);
  s.controls.engine = "recent";
  return s;
}

/** The rich snapshot with a hostile text in every place another person's words reach this moment. */
export function hostileOf(base) {
  const s = richOf(base);
  const evil = "<img src=x onerror=alert(1)> \u202egnirts\u202c\n" + "X".repeat(300);
  s.needs_gonza.cards[1].reason = "faltan respuestas: " + evil + "; " + SCHEME + ":alert(1)";
  s.needs_gonza.cards[1].company = evil;
  s.needs_gonza.cards[1].url = SCHEME + ":alert(1)";
  s.board.find((c) => c.state === "interview").cards[0].company = evil;
  s.board.find((c) => c.state === "interview").cards[0].note = "Interview on " + evil;
  s.learn.gaps[0].topic = evil;
  s.learn.topics[0].topic = evil;
  return s;
}
