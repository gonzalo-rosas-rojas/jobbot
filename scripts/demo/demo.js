// Demo of the panel with made-up data. `panel-src/build.mjs demo` pastes this file into a copy of the page
// (docs/demo/index.html when published). The real panel never loads it: the page only looks for `window.__DEMO__`,
// and nothing on the real server sets it. Nothing here touches the network: every request is answered in memory.
//
// Every company, person, address and number below is invented. The data moves as the page is used: approving, closing
// a task or recording an answer changes what the next snapshot says, as the real server would. Every free text exists in
// Spanish and English as a pair [es, en]; `txt` picks the one for the language the page runs in AT THE MOMENT a snapshot
// (or a capture) is made, so a change of language shows in the next picture, which the page is asked to read at once.
(() => {
  "use strict";
  // The pages the tests serve carry the nonce of this script; the published page has none.
  const nonce = (document.currentScript && document.currentScript.nonce) || "";
  // The published page has no nonces; the pages the tests serve do. Only the published one follows the real clock for the
  // run window (the tests run at any hour and need it closed).
  const published = !nonce;
  const params = new URLSearchParams(location.search);
  // ?scenario=calm | urgent | stop | empty | window | closed | stale | heavy: the mood of the board, for screenshots (stale: the engine missed a night; heavy: 74 live processes).
  const scenario = params.get("scenario") || "";
  // ?lag=<ms>: every read of the snapshot takes that long, like a big base on a slow machine (7 to 12 s on the real one), so the loading state and the
  // refresh loop can be seen and tested. `__DEMO__.stats` counts the reads: how many were asked, how many are in flight and the most at once.
  const lag = Math.max(0, Number(params.get("lag")) || 0);
  const stats = { total: 0, inflight: 0, max: 0 };
  const MIN = 60000, HOUR = 3600000, DAY = 86400000;
  const t0 = Date.now();
  // The page's "now", as src/main.js reads it: ?now= pins an instant; ?clock=<hour> moves the real one to that hour of Santiago by the
  // shorter way round (9, 9.5, 9,5 and 09:30 work; anything else is ignored). Every time of the demo counts from here, never from the real clock.
  const now = (() => {
    const pinned = Date.parse(params.get("now") || "");
    if (Number.isFinite(pinned)) return () => pinned;
    const found = /[?&]clock=([^&#]*)/.exec(location.search || "");
    let raw = "";
    try { raw = found ? decodeURIComponent(found[1]).trim() : ""; } catch (e) { /* a bad escape is no hour */ }
    const hm = /^(\d{1,2}):(\d{2})$/.exec(raw);
    const hour = raw === "" ? NaN : hm ? Number(hm[1]) + Number(hm[2]) / 60 : Number(raw.replace(",", "."));
    if (!(hour >= 0 && hour < 24)) return () => Date.now();
    const parts = Object.fromEntries(new Intl.DateTimeFormat("en-GB", { timeZone: "America/Santiago", hour: "2-digit", minute: "2-digit", hour12: false }).formatToParts(new Date()).map((x) => [x.type, x.value]));
    let delta = ((((hour - (+parts.hour % 24 + +parts.minute / 60)) % 24) + 24) % 24);
    if (delta > 12) delta -= 24;
    const shift = delta * HOUR;
    return () => Date.now() + shift;
  })();
  const ago = (ms) => new Date(now() - ms).toISOString();
  const ahead = (ms) => new Date(now() + ms).toISOString();

  // ---- Language: the page keeps <html lang> in step with what it shows.
  const lang = () => (document.documentElement.lang === "en" ? "en" : "es");
  // a text is a plain string, a pair [es, en], or a function that makes either (for what names a day of the week)
  const txt = (x) => (typeof x === "function" ? txt(x()) : Array.isArray(x) ? x[lang() === "en" ? 1 : 0] : x);
  const locale = () => (lang() === "en" ? "en-GB" : "es-CL");
  const cap = (s) => s.charAt(0).toUpperCase() + s.slice(1);
  const usd = (n) => (lang() === "en" ? "$" + n.toLocaleString("en-US") : "US$" + n.toLocaleString("es-CL"));
  const weekday = (ms) => new Intl.DateTimeFormat(locale(), { weekday: "long", timeZone: "America/Santiago" }).format(new Date(ms));
  // the weekday of the first working day (Monday to Friday, Santiago) at or after `ms` from now: what a company names as a deadline
  const workday = (ms) => {
    let at = now() + ms;
    for (let i = 0; i < 3 && /^S/.test(new Date(at).toLocaleDateString("en-US", { timeZone: "America/Santiago", weekday: "short" })); i++) at += DAY;
    return weekday(at);
  };

  const LABELS = {
    scored: ["Puntuadas", "Scored"], prepared: ["Preparadas", "Prepared"], needs_gonza: ["Necesitan tu ayuda", "Need you"],
    submitted: ["Enviadas", "Sent"], replied: ["Respondieron", "Replied"], technical_test: ["Prueba técnica", "Technical tests"],
    interview: ["Entrevista", "Interviews"], offer: ["Oferta", "Offers"], accepted: ["Aceptadas", "Accepted"],
    rejected: ["Rechazadas", "Rejected"], withdrawn: ["Retiradas", "Withdrawn"], ghosted: ["Sin respuesta", "No reply"],
  };
  const ORDER = Object.keys(LABELS);
  const MARK_WORDS = {
    replied: ["respondió", "replied"], technical_test: ["prueba técnica", "technical test"], interview: ["entrevista", "interview"],
    offer: ["oferta", "offer"], accepted: ["aceptada", "accepted"], rejected: ["rechazada", "rejected"], withdrawn: ["retirada", "withdrawn"],
    ghosted: ["sin respuesta", "no reply"],
  };
  const MARKS = {
    submitted: ["replied", "rejected", "withdrawn", "ghosted"],
    replied: ["technical_test", "interview", "rejected", "withdrawn"],
    technical_test: ["interview", "rejected", "withdrawn"],
    interview: ["offer", "rejected", "withdrawn"],
    offer: ["accepted", "rejected", "withdrawn"],
    ghosted: ["replied", "rejected", "withdrawn"],
  };
  // The sentence a stuck form carries, the way the real server writes it (the page names the kind in its own words).
  const REASONS = {
    handoff: ["La página pide una cuenta nueva: llénala tú y marca «Ya la envié».", "The site wants you to create an account first: fill the form in yourself, then press “I sent it”."],
    // the engine's own line (crates/jobbot-app/src/apply/handoff.rs): the label of the question as the English form wrote it, required asterisk included, in any language
    missing_answers: "faltan respuestas: How many years have you run Kubernetes in production? *",
    blocked: ["Un control de seguridad (captcha) frenó el formulario en la segunda página; ábrela tú y, cuando pase el control, devuélvela a la cola.", "A security check (captcha) stopped the form on its second page; open it yourself and, once the check passes, return it to the queue."],
    may_have_sent: ["La página no mostró ninguna confirmación después de enviar el formulario, así que puede que ya haya salido: mira tu correo o la página de la empresa antes de postular otra vez.", "The page showed no confirmation after the form was sent, so it may already have gone out: check your email or the company's page before applying again."],
  };
  const REASON_SHORT = {
    handoff: ["Envía tú el formulario: la hoja de respuestas está lista", "Send the form yourself: the answer sheet is ready"],
    missing_answers: ["El formulario pide respuestas que no tengo", "The form asks for answers I don't have"],
    blocked: ["Una página me frenó (captcha, inicio de sesión o bloqueo)", "A page stopped me (captcha, login or block)"],
    may_have_sent: ["Puede que ya se haya enviado: revisa antes de volver a postular", "It may already have gone out: check before applying again"],
  };

  const LOCATIONS = [
    ["Remoto, cualquier país", "Remote, anywhere"],
    ["Remoto, América Latina", "Remote, Latin America"],
    ["Remoto, husos horarios de América", "Remote, Americas time zones"],
    ["Remoto, UTC-3 a UTC-6", "Remote, UTC-3 to UTC-6"],
    ["Remoto, Europa o América", "Remote, Europe or the Americas"],
    ["Remoto, EE. UU. y Latinoamérica", "Remote, US and Latin America"],
  ];

  // What each kind of role does and asks for, said once and reused by the postings (the page reads `description` as text).
  const FAMILIES = {
    backend: [["diseñar y operar servicios que atienden tráfico real, y bajar su latencia y su costo", "design and run services that handle real traffic, and cut their latency and cost"],
      ["experiencia en producción, bases de datos relacionales y revisión de código", "production experience, relational databases and code review"]],
    rust: [["escribir servicios en Rust, medir su rendimiento y revisar diseños con el resto del equipo", "write services in Rust, measure their performance and review designs with the rest of the team"],
      ["Rust en producción, concurrencia y gusto por las pruebas", "Rust in production, concurrency and a taste for tests"]],
    fullstack: [["llevar una función desde la base de datos hasta la pantalla, con pruebas y despliegue propios", "take a feature from the database to the screen, with your own tests and deploys"],
      ["TypeScript, una base SQL y criterio de producto", "TypeScript, a SQL database and product judgement"]],
    platform: [["mantener la plataforma interna: despliegues, observabilidad y las herramientas que usa el resto de los equipos", "run the internal platform: deploys, observability and the tooling every other team uses"],
      ["Kubernetes o similar, infraestructura como código y buena comunicación escrita", "Kubernetes or similar, infrastructure as code and good written communication"]],
    frontend: [["construir interfaces rápidas y accesibles, y cuidar el sistema de diseño", "build fast, accessible interfaces and look after the design system"],
      ["TypeScript, rendimiento en el navegador y accesibilidad", "TypeScript, browser performance and accessibility"]],
    ml: [["llevar modelos a producción: evaluarlos, medir regresiones y vigilar su costo", "put models into production: evaluate them, track regressions and watch their cost"],
      ["Python o Rust, evaluación de modelos y sistemas distribuidos", "Python or Rust, model evaluation and distributed systems"]],
    data: [["construir y mantener los pipelines que alimentan a analítica y a producto", "build and maintain the pipelines that feed analytics and product"],
      ["SQL avanzado, orquestación y calidad de datos", "advanced SQL, orchestration and data quality"]],
    sre: [["cuidar la disponibilidad: objetivos de servicio, alertas útiles y guardia rotativa", "own availability: service objectives, useful alerts and a rotating on-call"],
      ["experiencia en incidentes, Linux y automatización", "incident experience, Linux and automation"]],
    game: [["programar el juego en tiempo real y mantener la herramienta que usa el equipo de diseño", "program the real-time game and maintain the tooling the design team uses"],
      ["C++ o Rust, matemática de juego y trabajo con diseñadores", "C++ or Rust, game maths and working with designers"]],
  };
  const describe = (a) => {
    const [does, wants] = FAMILIES[a.family];
    return lang() === "en"
      ? "We are looking for someone to join " + a.company + " as " + a.title + ", in a small remote team.\n\nYou will: " + does[1] + ".\nWe look for: " + wants[1]
        + ".\nWe offer: pay in US dollars, flexible hours and a team that writes its decisions down."
      : "Buscamos una persona para sumarse a " + a.company + " como " + a.title + ", en un equipo pequeño y remoto.\n\nHarás: " + does[0] + ".\nBuscamos: " + wants[0]
        + ".\nOfrecemos: pago en dólares, horario flexible y un equipo que deja sus decisiones por escrito.";
  };

  // [key, company, title, score, state, source, days since the last move, extra]
  const A = (n, company, title, score, state, source, days, extra) => Object.assign({
    key: "demo:" + n, n, company, title, score, state, base: state, source, updated: ago(days * DAY + (n % 5) * HOUR),
    note: null, approved: false, kind: null, salary: null, where: 0, family: "backend", moves: [], touched: false,
  }, extra || {});

  const apps = [
    A(1, "Alba Robotics", "Backend Engineer (Rust)", 91, "submitted", "greenhouse", 12, { salary: [4800, 6200], family: "rust", where: 0 }),
    A(2, "Lumen Pay", "Full-stack Engineer", 88, "replied", "lever", 2, { salary: [4500, 5500], family: "fullstack", where: 2,
      note: ["Pidió disponibilidad esta semana", "Asked for availability this week"] }),
    A(3, "Cobalto Systems", "Senior Platform Engineer", 86, "technical_test", "ashby", 1, { salary: [5000, 6500], family: "platform", where: 3,
      note: ["Prueba de 90 minutos en Rust; entrega en 48 horas", "90-minute Rust exercise; due within 48 hours"] }),
    A(4, "Nimbus Health", "AI Engineer", 90, "interview", "greenhouse", 0, { salary: [5200, 6800], family: "ml", where: 1,
      note: () => ["Entrevista el " + workday(2 * DAY) + " a las 16:00", "Interview on " + workday(2 * DAY) + " at 16:00"] }),
    A(5, "Orbe Studio", "Frontend Engineer", 79, "submitted", "ashby", 0, { family: "frontend", where: 4, night: "sent" }),
    A(6, "Tramontana", "Rust Developer", 84, "submitted", "lever", 0, { salary: [4600, 5800], family: "rust", where: 5, night: "sent" }),
    A(7, "Kestrel Labs", "Staff Backend Engineer", 87, "submitted", "greenhouse", 9, { salary: [6000, 7500], family: "backend", where: 0 }),
    A(8, "Faro Analytics", "Data Engineer", 76, "ghosted", "greenhouse", 21, { family: "data", where: 2,
      note: ["Sin noticias desde la primera respuesta", "No word since their first reply"] }),
    A(9, "Mica Cloud", "Site Reliability Engineer", 93, "offer", "ashby", 0, { salary: [5600, 7000], family: "sre", where: 3,
      note: () => ["Oferta recibida: responder antes del " + workday(3 * DAY), "Offer received: reply by " + workday(3 * DAY)] }),
    A(10, "Vértice AI", "ML Engineer", 82, "submitted", "lever", 1, { family: "ml", where: 4 }),
    A(11, "Quillon", "TypeScript Engineer", 74, "replied", "greenhouse", 4, { family: "fullstack", where: 5,
      note: ["Pidieron mi pretensión de renta; respondí con el piso del rango", "Asked for my salary expectation; I answered with the floor of their range"] }),
    A(12, "Pluma Commerce", "Backend Engineer", 85, "needs_gonza", "greenhouse", 0, { kind: "handoff", where: 2 }),
    A(13, "Sendero", "Software Engineer", 81, "needs_gonza", "lever", 1, { kind: "missing_answers", family: "platform", where: 3 }),
    A(14, "Halcón Tech", "Platform Engineer", 83, "needs_gonza", "ashby", 0, { kind: "blocked", family: "platform", where: 0 }),
    A(27, "Brújula Freight", "Backend Engineer", 84, "needs_gonza", "greenhouse", 1, { kind: "may_have_sent", family: "backend", where: 1 }),
    A(15, "Brisa Mobility", "Full-stack Engineer", 89, "prepared", "greenhouse", 0, { shot: true, salary: [4900, 6000], family: "fullstack", where: 1 }),
    A(16, "Ancla Security", "Rust Engineer", 92, "prepared", "ashby", 0, { shot: true, salary: [5400, 6900], family: "rust", where: 0 }),
    A(17, "Delta Forge", "Backend Engineer", 80, "prepared", "lever", 0, { where: 4 }),
    A(18, "Marea Games", "Gameplay Programmer", 71, "scored", "greenhouse", 0, { family: "game", where: 2 }),
    A(19, "Zenit Bank", "Platform Engineer", 78, "scored", "greenhouse", 0, { family: "platform", where: 5 }),
    A(20, "Cinta Labs", "Rust Engineer", 86, "scored", "ashby", 0, { salary: [5100, 6400], family: "rust", where: 1 }),
    A(21, "Ocaso Media", "Full-stack Engineer", 69, "scored", "lever", 0, { family: "fullstack", where: 3 }),
    A(22, "Peñón Data", "Data Platform Engineer", 77, "scored", "greenhouse", 0, { family: "data", where: 4 }),
    A(23, "Granizo", "Backend Engineer", 72, "rejected", "lever", 5, { where: 2, note: ["Eligieron a otra persona", "They went with another candidate"] }),
    A(24, "Solsticio", "Software Engineer", 70, "rejected", "greenhouse", 14, { where: 5, path: ["submitted", "replied", "interview", "rejected"],
      note: ["Rechazada tras la entrevista con el equipo", "Rejected after the team interview"] }),
    A(25, "Bruma Cloud", "SRE", 75, "rejected", "ashby", 19, { family: "sre", where: 0, path: ["submitted", "replied", "technical_test", "rejected"],
      note: ["Rechazada tras la prueba técnica", "Rejected after the technical test"] }),
    A(26, "Arrecife", "Backend Engineer", 68, "withdrawn", "lever", 11, { salary: [3000, 3800], where: 3,
      note: ["Retirada: el sueldo no llegaba al mínimo", "Withdrew: the pay was below my floor"] }),
  ];

  // ?scenario=heavy: the frame nobody tested. 74 live processes (45 sent, 10 replied, 5 tests, 4 interviews, 1 offer,
  // 9 without news), 8 stuck forms and 3 captures to approve. The names are syllables that mean nothing.
  if (scenario === "heavy") {
    const HEAD = ["Zy", "Qua", "Vri", "Hux", "Tor", "Nym", "Yor", "Ox", "Pri", "Dun", "Mar", "Lio"];
    const MID = ["lum", "nik", "dara", "mole", "tak", "bora", "sil", "zen", "pax", "grim"];
    const TAIL = ["Labs", "Systems", "Works", "Cloud", "Data", "Forge"];
    const ROLES = [["Backend Engineer", "backend"], ["Platform Engineer", "platform"], ["Rust Engineer", "rust"], ["Data Engineer", "data"], ["Full-stack Engineer", "fullstack"], ["Site Reliability Engineer", "sre"]];
    const SOURCES = ["greenhouse", "ashby", "lever"];
    const KINDS = ["handoff", "missing_answers", "blocked"];
    // [state, how many more, days since the last move]
    const MORE = [["submitted", 40, 3], ["replied", 8, 2], ["technical_test", 4, 1], ["interview", 3, 1], ["ghosted", 8, 22], ["needs_gonza", 4, 0], ["prepared", 1, 0]];
    let n = apps.length;
    let i = 0;
    for (const [state, count, days] of MORE) {
      for (let k = 0; k < count; k++, i++) {
        n += 1;
        const [title, family] = ROLES[i % ROLES.length];
        apps.push(A(n, HEAD[i % 12] + MID[Math.floor(i / 12) % 10] + " " + TAIL[(i * 5) % 6], title, 62 + ((i * 7) % 33), state, SOURCES[i % 3], days + (k % 9), {
          family, where: i % LOCATIONS.length, kind: state === "needs_gonza" ? KINDS[k % 3] : null, shot: state === "prepared",
          salary: i % 3 === 0 ? [4500 + (i % 5) * 100, 5800 + (i % 5) * 100] : null,
        }));
      }
    }
  }

  // ---- The history of an application: the states it went through, the last one at its last move, each earlier one 1.4
  // days before the next. What the person does through the page (marks, resolves, confirms) is added after them.
  const PATHS = {
    scored: [], prepared: [], needs_gonza: ["needs_gonza"], submitted: ["submitted"], replied: ["submitted", "replied"],
    technical_test: ["submitted", "replied", "technical_test"], interview: ["submitted", "replied", "interview"],
    offer: ["submitted", "replied", "interview", "offer"], ghosted: ["submitted", "replied", "ghosted"],
    rejected: ["submitted", "rejected"], withdrawn: ["submitted", "withdrawn"],
  };
  const EVENT_NOTES = {
    prepared: ["CV adaptado al aviso", "CV tailored to the posting"], submitted: ["Enviada por jobbot", "Sent by jobbot"],
    replied: ["La empresa respondió", "The company replied"], technical_test: ["Les pidieron una prueba técnica", "They asked for a technical test"],
    interview: ["Pasó a entrevista", "Moved on to an interview"], ghosted: ["Dejaron de responder", "They stopped replying"],
  };
  // key -> ISO time of the overnight move the strip of Hoy shows for it (set by `activity`, read by the cards and the history)
  let night = new Map();
  const baseUpdated = (a) => (!a.touched && night.get(a.key)) || a.updated;
  const updatedOf = (a) => (a.moves.length ? a.moves[a.moves.length - 1].at : baseUpdated(a));
  function history(a) {
    const trail = ["scored"].concat(a.base !== "scored" ? ["prepared"] : [], a.path || PATHS[a.base]);
    const last = Date.parse(baseUpdated(a));
    const made = trail.map((to, i) => ({
      at: new Date(last - (trail.length - 1 - i) * DAY * 1.4).toISOString(), from: i ? trail[i - 1] : null, to,
      note: i === trail.length - 1 ? txt(a.touched ? a.baseNote : a.note) : txt(EVENT_NOTES[to] || null),
    }));
    return made.concat(a.moves.map((m) => ({ at: m.at, from: m.from, to: m.to, note: txt(m.note) })));
  }

  const tasks = [
    { id: 101, text: ["Responder al reclutador de Lumen Pay con dos horarios", "Reply to the Lumen Pay recruiter with two slots"], job_key: "demo:2", due_at: ahead(-2 * HOUR), link: null },
    { id: 102, text: ["Entregar la prueba técnica de Cobalto Systems", "Submit the Cobalto Systems technical test"], job_key: "demo:3", due_at: ahead(30 * HOUR), link: null },
    { id: 103, text: ["Preparar la entrevista de Nimbus Health: repasar sistemas distribuidos", "Prepare for the Nimbus Health interview: review distributed systems"], job_key: "demo:4", due_at: ahead(26 * HOUR), link: null },
    { id: 104, text: ["Decidir la oferta de Mica Cloud", "Decide on the Mica Cloud offer"], job_key: "demo:9", due_at: ahead(70 * HOUR), link: null },
    { id: 105, text: ["Actualizar el portafolio con el nuevo caso de estudio", "Add the new case study to the portfolio"], job_key: null, due_at: null, link: null },
    { id: 106, text: ["Releer el aviso de Kestrel Labs y redactar un seguimiento", "Re-read the Kestrel Labs posting and draft a follow-up"], job_key: null, due_at: null, link: "https://example.com/kestrel" },
  ];
  if (scenario === "heavy") {
    for (let i = 0; i < 4; i++) {
      tasks.push({ id: 107 + i, text: [`Preparar la prueba ${i + 1} de la semana`, `Prepare test ${i + 1} of the week`], job_key: i < 2 ? "demo:" + (27 + i) : null, due_at: ahead((i * 11 + 4) * HOUR), link: null });
    }
  }
  // The mails of the last days, newest first. The subject is what identifies a mail to the page, so it is what changes
  // with the language; a change of language therefore shows the mails as new for a moment.
  const mails = [
    { at: 50 * MIN, from: "talent@lumenpay.example", company: "Lumen Pay", job_key: "demo:2", kind: "recruiter",
      subject: ["¿Podemos conversar el miércoles?", "Can we talk on Wednesday?"],
      summary: ["Quieren una llamada de 30 minutos y vuelven a pedir tu disponibilidad esta semana.", "They want a 30-minute call and ask again for your availability this week."] },
    { at: 4 * HOUR + 20 * MIN, from: "people@nimbushealth.example", company: "Nimbus Health", job_key: "demo:4", kind: "interview_invite",
      subject: ["Invitación a entrevista técnica", "Invitation to a technical interview"],
      summary: () => [cap(workday(2 * DAY)) + " a las 16:00, con el equipo de modelos. Dos entrevistadores, 60 minutos.", cap(workday(2 * DAY)) + " at 16:00, with the models team. Two interviewers, 60 minutes."] },
    { at: 6 * HOUR + 20 * MIN, from: "offers@micacloud.example", company: "Mica Cloud", job_key: "demo:9", kind: "action_required",
      subject: ["Oferta: Site Reliability Engineer", "Offer: Site Reliability Engineer"],
      summary: () => ["Carta de oferta adjunta: " + usd(6200) + " al mes, remoto. Responde antes del " + workday(3 * DAY) + ".",
        "Offer letter attached: " + usd(6200) + " a month, remote. Please reply by " + workday(3 * DAY) + "."] },
    { at: 1 * DAY + 4 * HOUR, from: "hiring@cobalto.example", company: "Cobalto Systems", job_key: "demo:3", kind: "technical_test",
      subject: ["Prueba técnica: servicio de colas", "Technical test: a job-queue service"],
      summary: ["90 minutos, en Rust; entrega en 48 horas.", "90 minutes, in Rust; due within 48 hours."] },
    { at: 5 * DAY + 4 * HOUR, from: "no-reply@granizo.example", company: "Granizo", job_key: "demo:23", kind: "rejection",
      subject: ["Sobre tu postulación", "About your application"],
      summary: ["Avanzaron con otra persona; guardan tu perfil para futuras búsquedas.", "They moved forward with another candidate and will keep your profile on file."] },
  ].map((m) => Object.assign(m, { received_at: ago(m.at) }));

  const DECK = [
    ["Rust", "Rust", ["¿Qué diferencia hay entre `Send` y `Sync`?", "What is the difference between `Send` and `Sync`?"],
      ["`Send`: el valor puede moverse a otro hilo. `Sync`: una referencia compartida (&T) puede usarse desde varios hilos; T es Sync si &T es Send.", "`Send`: the value can be moved to another thread. `Sync`: a shared reference (&T) can be used from several threads; T is Sync if &T is Send."], "Cobalto Systems"],
    ["Rust", "Rust", ["¿Cuándo preferirías `Arc<Mutex<T>>` frente a un canal?", "When would you pick `Arc<Mutex<T>>` over a channel?"],
      ["Cuando varios hilos necesitan leer y mutar el mismo estado; si el flujo es productor-consumidor, un canal evita compartir estado y es más simple de razonar.", "When several threads need to read and change the same state; if the flow is producer-consumer, a channel avoids sharing state and is easier to reason about."], "Alba Robotics"],
    ["Sistemas distribuidos", ["Sistemas distribuidos", "Distributed systems"], ["Explica la diferencia entre consistencia fuerte y eventual con un ejemplo.", "Explain strong versus eventual consistency with an example."],
      ["Fuerte: toda lectura ve la última escritura (un saldo bancario). Eventual: las réplicas convergen con el tiempo (un contador de visitas). Se elige según el costo de leer un dato viejo.", "Strong: every read sees the latest write (a bank balance). Eventual: replicas converge over time (a view counter). You choose by the cost of reading a stale value."], "Kestrel Labs"],
    ["Sistemas distribuidos", ["Sistemas distribuidos", "Distributed systems"], ["¿Qué es la idempotencia y por qué importa en reintentos?", "What is idempotency and why does it matter for retries?"],
      ["Aplicar la operación varias veces deja el mismo resultado que una vez. Permite reintentar tras un timeout sin duplicar efectos; se logra con claves de idempotencia.", "Applying the operation several times leaves the same result as once. It lets you retry after a timeout without duplicating effects; you get it with idempotency keys."], "Lumen Pay"],
    ["SQL", "SQL", ["¿Cuándo un índice compuesto (a, b) NO ayuda a filtrar solo por b?", "When does a composite index (a, b) NOT help when you filter only by b?"],
      ["Cuando b no es el prefijo: el índice se ordena primero por a, así que filtrar solo por b no puede usarlo de forma eficiente.", "When b is not the prefix: the index is sorted by a first, so filtering by b alone cannot use it efficiently."], null],
    ["Diseño de APIs", ["Diseño de APIs", "API design"], ["¿Cómo versionarías una API pública sin romper a los clientes?", "How would you version a public API without breaking its clients?"],
      ["Cambios aditivos compatibles; versión explícita (ruta o cabecera) para los incompatibles; periodo de convivencia con aviso de retiro.", "Additive, compatible changes; an explicit version (path or header) for breaking ones; an overlap period with a deprecation notice."], "Lumen Pay"],
    ["Comportamental", ["Comportamental", "Behavioral"], ["Cuéntame de un error tuyo en producción.", "Tell me about a mistake of yours in production."],
      ["Estructura: contexto, qué falló, cómo lo detectaste, qué hiciste en la hora siguiente, qué cambió para que no se repita. Evita culpar a otros.", "Structure: context, what failed, how you found out, what you did in the next hour, what changed so it does not happen again. Do not blame others."], "Nimbus Health"],
    ["TypeScript", "TypeScript", ["¿Qué hace `satisfies` y cuándo lo usarías en vez de una anotación?", "What does `satisfies` do, and when would you use it instead of an annotation?"],
      ["Verifica que un valor cumple un tipo sin ensanchar el tipo inferido: conservas literales y claves exactas.", "It checks that a value fits a type without widening the inferred type: you keep literals and exact keys."], "Orbe Studio"],
    ["Rust", "Rust", ["¿Por qué `Rc` no es `Send`?", "Why is `Rc` not `Send`?"],
      ["Su contador no es atómico: dos hilos podrían corromperlo. `Arc` usa contadores atómicos.", "Its counter is not atomic: two threads could corrupt it. `Arc` uses atomic counters."], null],
    ["SQL", "SQL", ["¿Qué es un deadlock y cómo lo reduces?", "What is a deadlock and how do you reduce them?"],
      ["Dos transacciones esperan bloqueos que la otra tiene. Orden consistente de acceso, transacciones cortas y reintento ante el error.", "Two transactions wait for locks the other holds. Consistent access order, short transactions and a retry on the error."], "Zenit Bank"],
    ["Diseño de APIs", ["Diseño de APIs", "API design"], ["¿Paginación por offset o por cursor?", "Offset or cursor pagination?"],
      ["Cursor: estable ante inserciones y eficiente en tablas grandes. Offset es simple pero se degrada y salta o repite filas.", "Cursor: stable under inserts and efficient on big tables. Offset is simple but degrades and skips or repeats rows."], null],
    ["Comportamental", ["Comportamental", "Behavioral"], ["¿Por qué quieres cambiar de empresa?", "Why do you want to change companies?"],
      ["Habla de lo que buscas (alcance, producto, aprendizaje), no de lo que dejas. Sé concreto.", "Talk about what you are looking for (scope, product, learning), not what you are leaving. Be specific."], null],
  ].map(([topicKey, topic, question, answer, offer], i) => ({ id: "q" + i, topicKey, topic, question, answer, offer, seen: i < 5, due: i >= 3 && i < 8, grades: [] }));
  const topicOf = (q) => txt(q.topic);

  // ---- What the score is made of: six factors whose points add up to the score of the card. Named by the ids the server uses, in its order
  // and with its weights (30, 15, 10, 20, 15, 10: they add up to 100), so every place that draws them (the drawer, the gate, the bars of COLA)
  // translates them and knows how far each bar can go.
  const WEIGHTS = [["fit", 30], ["compensation", 15], ["interview_style", 10], ["stability", 20], ["learning", 15], ["other", 10]];
  const lcg = (i) => ((i * 9301 + 49297) % 233280) / 233280;
  function factors(score, n) {
    let raw = WEIGHTS.map(([, w], i) => w * (score / 100) * (0.85 + 0.3 * lcg(n * 11 + i)));
    const sum = raw.reduce((x, y) => x + y, 0);
    raw = raw.map((v, i) => Math.min(WEIGHTS[i][1], (v * score) / sum));
    const out = raw.map(Math.floor);
    let rest = score - out.reduce((x, y) => x + y, 0);
    const byRemainder = raw.map((v, i) => [v - Math.floor(v), i]).sort((x, y) => y[0] - x[0]);
    for (let round = 0; rest > 0 && round < 8; round++) {
      for (const [, i] of byRemainder) if (rest > 0 && out[i] < WEIGHTS[i][1]) { out[i] += 1; rest -= 1; }
    }
    return Object.fromEntries(WEIGHTS.map(([id], i) => [id, out[i]]));
  }

  // The last mark the person can still take back (POST /api/mark/undo), as the real server says it: the newest move, made by a
  // mark, under ten minutes old and with nothing after it; null otherwise.
  const UNDO_MS = 10 * MIN;
  const undoOf = (a) => {
    const m = a.moves[a.moves.length - 1];
    if (!m || !m.mark || a.state !== m.to || a.snoozedUntil || now() - Date.parse(m.at) >= UNDO_MS) return null;
    return { from: m.from, to: m.to, at: m.at, until: new Date(Date.parse(m.at) + UNDO_MS).toISOString().replace(/\.\d{3}Z$/, "Z") };
  };

  const cardOf = (a) => {
    const needs = a.state === "needs_gonza";
    const prepared = a.state === "prepared";
    // like the real card: Aprobar only while there is a dry-run capture waiting for the person
    // (a form that may already have gone out has both answers, as the real card does, the "it went out" first: it went out, or it did not)
    const actions = needs ? (a.kind === "handoff" ? ["confirm"] : a.kind === "may_have_sent" ? ["confirm", "resolve"] : ["resolve"]) : prepared && a.shot && !a.approved ? ["approve"] : [];
    return {
      key: a.key, company: a.company, title: a.title, undo: undoOf(a), score: a.score, state: a.state, source: a.source,
      url: "https://example.com/" + a.key.replace(":", "/"),
      reason: needs ? txt(REASONS[a.kind]) : null, reason_kind: needs ? a.kind : null, reason_short: needs ? txt(REASON_SHORT[a.kind]) : null,
      note: txt(a.note), approved: a.approved, has_screenshot: !!a.shot, has_cv: a.state !== "scored", has_dossier: a.state === "interview" || a.state === "offer", has_handoff: needs && a.kind === "handoff",
      updated_at: updatedOf(a), actions, marks: MARKS[a.state] || [],
      posted_at: ago(((a.n % 6) + 2) * DAY), first_seen: ago(((a.n % 6) + 1) * DAY),
      salary_min: a.salary ? a.salary[0] : null, salary_max: a.salary ? a.salary[1] : null,
      days_in_state: Math.max(0, Math.floor((now() - Date.parse(updatedOf(a))) / DAY)),
      snoozed_until: a.snoozedUntil || null, waiting_since: needs ? updatedOf(a) : null,
      age_days: (a.n % 6) + 2, prune_in_days: ["scored", "prepared", "needs_gonza"].includes(a.state) ? 20 - (a.n % 6) : null,
    };
  };

  const santiago = (ms, withDay) => {
    const f = new Intl.DateTimeFormat("sv-SE", { timeZone: "America/Santiago", year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", hour12: false });
    const p = Object.fromEntries(f.formatToParts(new Date(ms)).map((x) => [x.type, x.value]));
    return withDay ? p.year + "-" + p.month + "-" + p.day + " " + p.hour + ":" + p.minute : { h: +p.hour % 24, m: +p.minute };
  };
  const nextWindow = () => {
    const nowS = santiago(now());
    const waitMin = ((18 * 60 - (nowS.h * 60 + nowS.m)) + 1440) % 1440 || 1440;
    return santiago(now() + waitMin * MIN, true);
  };
  // The person's busy hours, as the dial of the board draws them: weekdays 09:00 to 18:00 in Santiago.
  const busyAt = (ms) => {
    const day = new Date(ms).toLocaleDateString("en-US", { timeZone: "America/Santiago", weekday: "short" });
    const p = santiago(ms);
    const mins = p.h * 60 + p.m;
    return day !== "Sat" && day !== "Sun" && mins >= 9 * 60 && mins < 18 * 60;
  };

  let stopped = scenario === "stop";
  // Is the run window open now? The tests (any hour of the day) need it closed, so only the published page follows the
  // clock; ?scenario=window and ?scenario=closed pin it either way.
  const windowOpen = () => (scenario === "window" ? true : scenario === "closed" ? false : published ? !busyAt(now()) : false);
  const listing = (cards, cap) => ({ total: cards.length, cards: cards.slice(0, cap || 50) });

  // The applications this scenario shows. "calm": nothing is waiting for the person; "empty": a fresh install.
  function visible() {
    if (scenario === "empty") return [];
    if (scenario === "calm") return apps.filter((a) => (a.state === "submitted" && now() - Date.parse(baseUpdated(a)) < 10 * DAY) || ["scored", "rejected", "withdrawn"].includes(a.state));
    return apps;
  }

  function snapshot() {
    const list = visible();
    const has = (key) => list.some((a) => a.key === key);
    const act = activity(list);
    let taskList = tasks.filter((x) => !x.job_key || has(x.job_key));
    if (scenario === "empty" || scenario === "calm") taskList = [];
    if (scenario === "urgent") taskList = tasks.slice(0, 3).map((x, i) => (i === 1 ? Object.assign({}, x, { due_at: ahead(5 * HOUR) }) : x));
    const mailList = mails.filter((m) => !m.job_key || has(m.job_key)).map((m) => ({
      received_at: m.received_at, from: m.from, company: m.company, job_key: m.job_key, kind: m.kind, subject: txt(m.subject),
      summary: txt(typeof m.summary === "function" ? m.summary() : m.summary),
    }));
    const nowMs = now();
    const withDue = taskList.map((x) => ({ id: x.id, text: txt(x.text), job_key: x.job_key, due_at: x.due_at, link: x.link,
      overdue: x.due_at ? Date.parse(x.due_at) < nowMs : false, kind: x.job_key ? "recruiter" : "manual", created_at: ago(((x.id % 7) + 1) * 3 * HOUR) }));
    withDue.sort((a, b) => (a.due_at ? Date.parse(a.due_at) : 9e15) - (b.due_at ? Date.parse(b.due_at) : 9e15));
    const board = ORDER.map((state) => {
      const cards = list.filter((a) => a.state === state).map(cardOf).sort((x, y) => y.score - x.score);
      return { state, label: txt(LABELS[state]), total: cards.length, cards };
    });
    const m = metrics(list);
    const open = windowOpen();
    // What the last run did, counted from the night's moves (the same ones the prologue tells), and one dossier per interview on the board.
    // Only the scenarios whose engine ran have a record: a late engine (stale) and a fresh install (empty) have none.
    const night_moves = act.moves_by_state;
    const did = (state) => night_moves[state] || 0;
    const lastRun = scenario === "empty" || scenario === "stale" ? { last_run_result: null, last_run_steps: null } : {
      last_run_result: "ok",
      last_run_steps: {
        discover: { ran: true, new: did("scored") },
        prepare: { ran: true, prepared: did("prepared") },
        submit: { ran: true, submitted: did("submitted"), needs_gonza: did("needs_gonza") },
        dossiers: { ran: true, written: list.filter((a) => a.state === "interview").length },
      },
    };
    return {
      generated_at: new Date(nowMs).toISOString(),
      controls: {
        stopped, window_open: open, next_window: open ? null : nextWindow(),
        // the three truths of the engine: never ran (empty), missed a night (stale), ran 7 hours ago (everything else)
        last_run_at: scenario === "empty" ? null : scenario === "stale" ? ago(41 * HOUR) : ago(7 * HOUR),
        engine: scenario === "empty" ? "never" : scenario === "stale" ? "stale" : "recent",
        ...lastRun,
        claude_account: scenario === "empty" ? null : {
          ok: true, message: null, subscription: "max", email: "d***@example.com", checked_at: ago(8 * HOUR),
          auth_method: "claude.ai", api_provider: "firstParty", org_id: "org-demo", config_dir: "%LOCALAPPDATA%\\demo\\claude", cli_version: "2.1.0", anchored: true,
        },
        auto_approve_min: null,
        scheduled_task: scenario === "empty" ? null : scenario === "stale" ? { exists: false, next_run_at: null } : { exists: true, next_run_at: ahead(11 * HOUR) },
      },
      metrics: m.metrics,
      funnel: m.funnel,
      claude: claude(list),
      pipeline: pipeline(list, m.metrics.sent_week),
      trust: trust(list, act),
      activity: act,
      today: { tasks_total: withDue.length, tasks: withDue, emails_total: mailList.length, emails: mailList },
      needs_gonza: listing(list.filter((a) => a.state === "needs_gonza" && !a.snoozedUntil).map(cardOf)),
      awaiting_approval: listing(list.filter((a) => a.state === "prepared" && a.shot && !a.approved && !a.snoozedUntil).map(cardOf)),
      snoozed: listing(list.filter((a) => a.snoozedUntil).map(cardOf)),   // a card put aside (POST /api/snooze) waits here until its day
      board,
      learn: learn(),
    };
  }

  // ---- What the story's views draw: the last 24 hours, the week's path from posting to send, and trust.
  // Everything here is made up, but it adds up with the lists of the page: the layers count what the lists show.

  /** The week's figures and the all-time funnel, counted from the history of the applications in the list. */
  function metrics(list) {
    const weekAgo = now() - 7 * DAY;
    const reached = (a, states) => history(a).some((e) => states.includes(e.to));
    const inWeek = (a, states) => history(a).some((e) => states.includes(e.to) && Date.parse(e.at) >= weekAgo);
    const answers = ["replied", "technical_test", "interview", "offer", "accepted", "rejected"];
    const count = (fn) => list.filter(fn).length;
    const sentWeek = count((a) => inWeek(a, ["submitted"]));
    const responsesWeek = count((a) => inWeek(a, answers));
    const interviews = count((a) => reached(a, ["interview"]));
    const offers = count((a) => reached(a, ["offer", "accepted"]));
    const bySource = {};
    for (const a of list) {
      const s = (bySource[a.source] = bySource[a.source] || { n: 0, sent: 0, answered: 0, tests: 0, interviews: 0, offers: 0 });
      s.n += 1;
      if (reached(a, ["submitted"])) s.sent += 1;
      if (reached(a, answers)) s.answered += 1;
      if (reached(a, ["technical_test"])) s.tests += 1;
      if (reached(a, ["interview"])) s.interviews += 1;
      if (reached(a, ["offer", "accepted"])) s.offers += 1;
    }
    return {
      metrics: { sent_week: sentWeek, responses_week: responsesWeek, response_rate: sentWeek ? responsesWeek / sentWeek : null,
        tests_week: count((a) => inWeek(a, ["technical_test"])), interviews_week: count((a) => inWeek(a, ["interview"])),
        interviews_total: interviews, offers_total: offers, interview_to_offer: interviews ? offers / interviews : null },
      funnel: { sent: count((a) => reached(a, ["submitted"])), answered: count((a) => reached(a, answers)), tests: count((a) => reached(a, ["technical_test"])),
        interviews, offers, by_source: bySource },
    };
  }

  function claude(list) {
    if (!list.length) return { spent_24h: 0, daily_budget: 5, by_stage: [] };
    const by_stage = [{ stage: "score", calls: 41, dollars: 0.38 }, { stage: "cv", calls: 6, dollars: 0.97 }, { stage: "answers", calls: 9, dollars: 0.34 }, { stage: "dossier", calls: 1, dollars: 0.15 }];
    return { spent_24h: Math.round(by_stage.reduce((x, s) => x + s.dollars, 0) * 100) / 100, daily_budget: 5, by_stage };
  }

  function activity(list) {
    const SLOT = 30 * MIN;
    const nowMs = now();
    const lastStart = Math.floor(nowMs / SLOT) * SLOT;
    const firstStart = lastStart - 47 * SLOT;
    const slots = [];
    for (let i = 0; i < 48; i++) {
      const at = firstStart + i * SLOT;
      let open = !busyAt(at);
      // a window pinned open (?scenario=window) shows jobbot working now, whatever the clock says
      if (scenario === "window" && i >= 45) open = true;
      const hour = santiago(at).h;
      const weight = !open ? 0 : hour >= 18 || hour < 1 ? 9 : hour < 7 ? 6 : 2;
      slots.push({ at: new Date(at).toISOString(), open, seen: list.length ? Math.round(weight * lcg(i + 3)) : 0 });
    }
    // What jobbot moved while the window was open: the sends first (the oldest, at least six hours ago), then what it
    // scored, prepared and could not finish. Each one is placed in an open half hour, spread over all of them.
    const start = (i) => firstStart + i * SLOT;
    const open = slots.map((s, i) => (s.open && i < 47 ? i : -1)).filter((i) => i >= 0);
    const fresh = (a) => !a.touched;
    const sends = list.filter((a) => a.night === "sent" && a.state === "submitted" && fresh(a));
    const queue = [].concat(list.filter((a) => a.state === "scored" && fresh(a)), list.filter((a) => a.state === "prepared" && fresh(a)), list.filter((a) => a.state === "needs_gonza" && fresh(a)));
    const old = open.filter((i) => start(i) <= nowMs - 6 * HOUR);
    const moves = [];
    night = new Map();
    const place = (a, pool, j, total) => {
      if (!pool.length) return;
      const slot = pool[Math.min(pool.length - 1, Math.floor(((j + 0.5) * pool.length) / total))];
      // inside its half hour (the half hour that is running now is never used, so the time does not move between snapshots)
      const at = start(slot) + ((moves.length * 7) % 25) * MIN;
      night.set(a.key, new Date(at).toISOString());
      moves.push({ at: new Date(at).toISOString(), key: a.key, company: a.company, title: a.title, to: a.state });
    };
    sends.forEach((a, j) => place(a, old.length ? old : open, j, sends.length));
    queue.forEach((a, j) => place(a, open, j, queue.length));
    moves.sort((x, y) => Date.parse(y.at) - Date.parse(x.at));
    const moves_by_state = {};
    for (const mv of moves) moves_by_state[mv.to] = (moves_by_state[mv.to] || 0) + 1;
    return { from: new Date(firstStart).toISOString(), to: new Date(lastStart + SLOT).toISOString(), slot_minutes: 30, slots, moves, moves_total: moves.length, moves_by_state };
  }

  function pipeline(list, sentWeek) {
    const counts = new Array(101).fill(0);
    if (!list.length) return { days: 7, threshold: 60, seen: 0, above: 0, with_cv: 0, complete: 0, sent: 0, score_counts: counts };
    const n = (pred) => list.filter(pred).length;
    const sent = sentWeek;
    const complete = sent + n((a) => a.state === "prepared" && (a.shot || a.approved));
    const withCv = complete + n((a) => a.state === "prepared" && !(a.shot || a.approved)) + n((a) => a.state === "needs_gonza");
    const above = withCv + n((a) => a.state === "scored");
    const seen = 287;
    // the scores of those that passed (the lists above, plus the ones sent this week), then the rest, bunched around 46
    const weekAgo = now() - 7 * DAY;
    const sentScores = list.filter((a) => history(a).some((e) => e.to === "submitted" && Date.parse(e.at) >= weekAgo)).map((a) => a.score);
    const passed = list.filter((a) => ["scored", "prepared", "needs_gonza"].includes(a.state)).map((a) => a.score).concat(sentScores).slice(0, above);
    for (const score of passed) counts[score] += 1;
    const rest = seen - passed.length;
    const weights = [];
    for (let s = 22; s <= 59; s++) weights.push([s, Math.exp(-Math.pow((s - 46) / 9, 2) / 2)]);
    const total = weights.reduce((x, [, w]) => x + w, 0);
    // largest remainders, so the bunch is smooth and the total is exact
    const shares = weights.map(([s, w]) => [s, (rest * w) / total]);
    let placed = 0;
    for (const [s, share] of shares) { counts[s] += Math.floor(share); placed += Math.floor(share); }
    shares.sort((x, y) => (y[1] % 1) - (x[1] % 1));
    for (let i = 0; i < rest - placed; i++) counts[shares[i][0]] += 1;
    return { days: 7, threshold: 60, seen, above, with_cv: withCv, complete, sent, score_counts: counts };
  }

  function trust(list, act) {
    if (!list.length) return { reviewed: 0, first_n: 5, review_phase: true, sent_24h: 0, cap: 10, cap_after: 25 };
    const approved = list.filter((a) => a.approved).length;
    const reviewed = 3 + approved;
    return { reviewed, first_n: 5, review_phase: reviewed < 5, sent_24h: act.moves.filter((m) => m.to === "submitted").length, cap: reviewed < 5 ? 10 : 25, cap_after: 25 };
  }

  const WEAK = [["Sistemas distribuidos", 2.1], ["SQL", 2.8], ["Diseño de APIs", 3.4]];
  const GAPS = [
    { id: "g1", topic: ["Colas y backpressure", "Queues and backpressure"], offer_key: "demo:3", offer: "Cobalto Systems", seen: false,
      ask: ["¿Qué haces cuando el consumidor no da abasto?", "What do you do when the consumer cannot keep up?"], cmd: ["colas", "queues"] },
    { id: "g2", topic: ["Evaluación de modelos en producción", "Evaluating models in production"], offer_key: "demo:4", offer: "Nimbus Health", seen: false,
      ask: ["¿Cómo notarías que un modelo empeoró después de desplegarlo?", "How would you notice that a model got worse after you shipped it?"], cmd: ["evaluacion", "evaluation"] },
    { id: "g3", topic: ["Presupuestos de error (SRE)", "Error budgets (SRE)"], offer_key: "demo:9", offer: "Mica Cloud", seen: true,
      ask: ["¿Cómo decides cuándo frenar los despliegues por el presupuesto de error?", "How do you decide when to freeze deploys because of the error budget?"], cmd: ["slo", "error-budgets"] },
  ];

  // ---- The dossier of an interview, written the way the engine writes it (crates/jobbot-app/src/learn/dossier.rs): the title, the day and the
  // posting, then the summary, the likely questions each with its why, the stories each with the literal evidence of the CV, and the gaps on file for
  // that posting (or none). Every interview has its own, about its own application; Nimbus Health's carries its own words.
  const CV_STORIES = [
    { title: ["Ocho años operando servicios", "Eight years running services"],
      evidence: ["Llevo ocho años construyendo y operando servicios de backend en producción.", "For eight years I have built and run backend services in production."],
      use: ["Úsala cuando pregunten por tu experiencia en producción.", "Use it when they ask about your production experience."] },
    { title: ["Rust y TypeScript en producción", "Rust and TypeScript in production"],
      evidence: ["Últimamente trabajo en Rust y TypeScript, y me gusta hacerme cargo de un servicio desde el diseño hasta la guardia.", "Lately I work in Rust and TypeScript, and I like owning a service from design to on-call."],
      use: ["Úsala para mostrar que cuidas un servicio de punta a punta.", "Use it to show that you look after a service from end to end."] },
  ];
  const NIMBUS_PREP = {
    summary: ["Nimbus Health arma un equipo de modelos de lenguaje para soporte. Dos entrevistadores técnicos, 60 minutos. Preguntarán por evaluación offline frente a online y por regresiones tras actualizar un modelo.",
      "Nimbus Health is building a language-model team for support. Two technical interviewers, 60 minutes. They will ask about offline versus online evaluation and about regressions after a model update."],
    questions: [
      [["¿Cómo evaluarías un modelo antes y después de llevarlo a producción?", "How would you evaluate a model before and after it goes to production?"],
        ["El aviso pide evaluación offline y online.", "The posting asks for offline and online evaluation."]],
      [["¿Cómo medirías una regresión después de actualizar un modelo?", "How would you measure a regression after a model update?"],
        ["El equipo actualiza sus modelos con frecuencia.", "The team updates its models often."]],
      [["¿Qué incidente en producción te enseñó más y qué cambió después?", "Which production incident taught you the most, and what changed afterwards?"],
        ["Los entrevistadores técnicos suelen pedir una historia real.", "Technical interviewers usually ask for a real story."]],
    ],
  };
  function dossierOf(a) {
    const en = lang() === "en";
    const [does, wants] = FAMILIES[a.family];
    const L = en
      ? { made: "Generated on", tz: "(Santiago time)", posting: "posting", summary: "## What the company's posting says", questions: "## Likely questions", why: "*Why:*",
        stories: "## Stories (only facts from your master CV)", evidence: "Evidence in the CV: “", close: "”", gaps: "## Gaps to study", none: "None found.", review: "review with" }
      : { made: "Generado el", tz: "(hora de Santiago)", posting: "aviso", summary: "## Qué dice el aviso de la empresa", questions: "## Preguntas probables", why: "*Por qué:*",
        stories: "## Historias (solo hechos de tu CV maestro)", evidence: "Evidencia en el CV: «", close: "»", gaps: "## Huecos a estudiar", none: "Ninguno detectado.", review: "repasa con" };
    const prep = a.key === "demo:4" ? NIMBUS_PREP : {
      summary: ["Para el cargo de " + a.title + " en " + a.company + " tendrás que " + does[0] + ". Piden " + wants[0] + ".",
        "For the " + a.title + " role at " + a.company + " you would " + does[1] + ". They ask for " + wants[1] + "."],
      questions: [
        [["¿Qué te atrae de " + a.company + " y del cargo de " + a.title + "?", "What draws you to " + a.company + " and the " + a.title + " role?"],
          ["El aviso describe un equipo pequeño y remoto; quieren saber por qué ese equipo.", "The posting describes a small remote team; they want to know why that team."]],
        [["¿Qué has hecho antes para " + does[0] + "?", "What have you done before to " + does[1] + "?"],
          ["Es lo que haría tu día a día en el cargo.", "It is what your day to day in the role would be."]],
        [["El aviso pide " + wants[0] + ": ¿cuál es tu ejemplo más sólido?", "The posting asks for " + wants[1] + ": what is your strongest example?"],
          ["Quieren un caso concreto, no una lista de herramientas.", "They want a concrete case, not a list of tools."]],
      ],
    };
    const gaps = GAPS.filter((g) => g.offer_key === a.key);
    return [
      "# Dossier — " + a.company + " · " + a.title, "",
      L.made + " " + santiago(now(), true).slice(0, 10) + " " + L.tz + " · " + L.posting + ": " + cardOf(a).url, "",
      L.summary, "", txt(prep.summary), "",
      L.questions, "",
      ...prep.questions.flatMap(([q, why], i) => [(i + 1) + ". " + txt(q), "   " + L.why + " " + txt(why)]), "",
      L.stories, "",
      ...CV_STORIES.flatMap((s) => ["### " + txt(s.title), "", L.evidence + txt(s.evidence) + L.close, "", txt(s.use), ""]),
      L.gaps, "",
      ...(gaps.length ? gaps.map((g) => "- **" + txt(g.topic) + "** — " + txt(g.ask) + " (" + L.review + " `jobbot prep next --topic " + txt(g.cmd) + "`)") : [L.none]),
      "",
    ].join("\n");
  }
  function learn() {
    if (scenario === "empty") return { total: 0, seen: 0, due: 0, weak_topics: [], topics: [], gaps: [] };
    // calm: nothing is due, and only the gaps of postings still on the board remain
    const calm = scenario === "calm";
    const pending = (q) => q.due && !q.done && !calm;
    const keys = Array.from(new Set(DECK.map((q) => q.topicKey)));
    const names = new Map(DECK.map((q) => [q.topicKey, topicOf(q)]));
    const list = visible();
    return {
      total: DECK.length, seen: DECK.filter((q) => q.seen).length, due: DECK.filter(pending).length,
      weak_topics: WEAK.map(([key, average]) => ({ topic: names.get(key), average })),
      topics: keys.map((key) => {
        const qs = DECK.filter((q) => q.topicKey === key);
        return { topic: names.get(key), seen: qs.filter((q) => q.seen).length, total: qs.length, due: qs.filter(pending).length };
      }),
      gaps: GAPS.filter((g) => list.some((a) => a.key === g.offer_key)).map((g) => ({ id: g.id, topic: txt(g.topic), offer_key: g.offer_key, offer: g.offer, seen: g.seen })),
    };
  }

  function detail(a) {
    const c = cardOf(a);
    const first = Date.parse(history(a)[0].at);
    let dossier = null;
    if (a.state === "interview") {
      dossier = dossierOf(a);
    } else if (a.state === "offer") {
      dossier = lang() === "en"
        ? "Offer: " + usd(6200) + " a month, remote, paid in US dollars as a contractor. The range they published was " + usd(5600) + " to " + usd(7000) + ".\n\nTo weigh: on-call is one week in six, paid; the team is nine people across three time zones. Ask about the error-budget policy before answering."
        : "Oferta: " + usd(6200) + " al mes, remoto, contrato en dólares. El rango que publicaron era de " + usd(5600) + " a " + usd(7000) + ".\n\nPor sopesar: la guardia es una semana de seis, pagada; el equipo son nueve personas en tres husos horarios. Pregunta por su política de presupuesto de error antes de responder.";
    }
    return {
      card: c, location: txt(LOCATIONS[a.where]), remote: true, apply_url: c.url,
      first_seen: new Date(first - 2 * DAY).toISOString(), posted_at: null,
      salary_min: a.salary ? a.salary[0] : null, salary_max: a.salary ? a.salary[1] : null,
      score_detail: factors(a.score, a.n),
      gaps: GAPS.filter((g) => g.offer_key === a.key).map((g) => ({ topic: txt(g.topic), seen: g.seen })),
      events: history(a).reverse(),
      dossier, description: describe(a),
    };
  }

  // ---- The answers of the "server": the same words as the real one, in the language of the page.
  const ok = (message, extra) => ({ ok: true, status: 200, data: Object.assign({ ok: true, message: txt(message) }, extra || {}) });
  const no = (status, message) => ({ ok: false, status, data: { ok: false, message: txt(message) } });
  const byKey = (k) => apps.find((a) => a.key === k);
  const delay = (ms) => new Promise((r) => setTimeout(r, ms));
  // `mark`: the person recorded it by hand (POST /api/mark), so it keeps the note it overwrote and can be taken back for a while
  const move = (a, to, note, mark = false) => {
    if (!a.touched) { a.updated = baseUpdated(a); a.baseNote = a.note; a.touched = true; }
    a.moves.push({ at: new Date(now()).toISOString(), from: a.state, to, note: note || null, mark, prevNote: a.note });
    a.state = to; a.note = note || a.note;
  };
  let queue = DECK.filter((q) => q.due);

  async function handle(path, body) {
    await delay(90);
    const url = new URL(path, location.href);
    const p = url.pathname.replace(/^.*\/api\//, "/api/");
    const missing = (key) => no(404, ["No existe: " + key, "Not found: " + key]);
    if (p === "/api/snapshot") {
      stats.total += 1; stats.inflight += 1; stats.max = Math.max(stats.max, stats.inflight);
      try { if (lag) await delay(lag); return { ok: true, status: 200, data: snapshot() }; } finally { stats.inflight -= 1; }
    }
    if (p === "/api/job") { const a = byKey(url.searchParams.get("key")); return a ? { ok: true, status: 200, data: detail(a) } : no(404, ["No existe esa postulación", "That application does not exist"]); }
    if (p === "/api/stop") { stopped = true; return ok(["Listo: jobbot se detuvo.", "Done: jobbot has stopped."]); }
    if (p === "/api/resume") { stopped = false; return ok(["Listo: jobbot sigue.", "Done: jobbot is running again."]); }
    if (p === "/api/approve" && Array.isArray(body.keys)) {
      // the lote, like the real server: one result per key in order, ok only when every key went through (409 otherwise), and the ones
      // that did stay approved (each approval is its own write)
      if (body.key !== undefined) return no(400, ["Falta key (una) o keys (varias), no ambas", "Send key (one) or keys (several), not both"]);
      if (!body.keys.length) return no(400, ["keys está vacía", "keys is empty"]);
      if (body.keys.length > 200) return no(400, ["Demasiadas claves: hasta 200 por petición", "Too many keys: up to 200 per request"]);
      const results = body.keys.map((raw) => {
        const key = String(raw);
        const a = byKey(key);
        if (!a) return { key, ok: false, message: txt(["No existe: " + key, "Not found: " + key]) };
        if (a.approved) return { key, ok: false, message: txt(["Ya estaba aprobada.", "It was already approved."]) };
        a.approved = true;
        return { key, ok: true, message: txt(["Aprobada: la próxima corrida la envía.", "Approved: the next run sends it."]) };
      });
      const done = results.filter((r) => r.ok).length;
      const every = done === results.length;
      return { ok: every, status: every ? 200 : 409, data: { ok: every, message: txt([done + " de " + results.length + " con éxito", done + " of " + results.length + " done"]), results } };
    }
    if (p === "/api/approve") { const a = byKey(body.key); if (!a) return missing(body.key); a.approved = true; return ok(["Aprobada: la próxima corrida la envía.", "Approved: the next run sends it."]); }
    if (p === "/api/resolve") { const a = byKey(body.key); if (!a) return missing(body.key); move(a, "prepared", ["Resuelta por ti", "Resolved by you"]); a.shot = false; return ok(["Devuelta a la cola.", "Returned to the queue."]); }
    if (p === "/api/confirm") { const a = byKey(body.key); if (!a) return missing(body.key); move(a, "submitted", ["Confirmada por ti", "Confirmed by you"]); return ok(["Marcada como enviada.", "Marked as sent."]); }
    if (p === "/api/task/done") {
      const i = tasks.findIndex((x) => x.id === body.id);
      if (i < 0) return no(409, ["Esa tarea ya estaba cerrada.", "That task was already closed."]);
      tasks.splice(i, 1); return ok(["Tarea cerrada.", "Task closed."]);
    }
    if (p === "/api/task/add") {
      const text = String(body.text || "").trim();
      if (!text) return no(400, ["Falta el texto de la tarea.", "The task needs some text."]);
      tasks.push({ id: Math.max(0, ...tasks.map((x) => x.id)) + 1, text: [text, text], job_key: null, due_at: body.due_at || null, link: null });
      return ok(["Tarea añadida.", "Task added."]);
    }
    if (p === "/api/snooze") {
      const a = byKey(body.key);
      if (!a) return missing(body.key);
      const days = Math.max(1, parseInt(body.until, 10) || 1);
      a.snoozedUntil = new Date(now() + days * DAY).toISOString();
      return ok([days === 1 ? "Pospuesta hasta mañana." : "Pospuesta " + days + " días.", days === 1 ? "Postponed until tomorrow." : "Postponed for " + days + " days."]);
    }
    if (p === "/api/unsnooze") {
      const a = byKey(body.key);
      if (!a) return missing(body.key);
      a.snoozedUntil = null;
      return ok(["Vuelve a la cola.", "Back in the queue."]);
    }
    if (p === "/api/mark") {
      const a = byKey(body.key);
      if (!a) return missing(body.key);
      if (!(MARKS[a.state] || []).includes(body.state)) return no(409, ["No se pudo marcar: ya no se puede pasar a ese estado.", "Couldn't record that: it can no longer move to that state."]);
      move(a, body.state, body.note, true); return ok(["Registrado: " + txt(MARK_WORDS[body.state]) + ".", "Recorded: " + txt(MARK_WORDS[body.state]) + "."]);
    }
    if (p === "/api/mark/undo") {
      const a = byKey(body.key);
      if (!a) return missing(body.key);
      const m = a.moves[a.moves.length - 1];
      if (!m || !m.mark) return no(409, ["No hay una marca reciente que deshacer", "There is no recent mark to undo"]);
      if (a.state !== m.to || a.snoozedUntil) return no(409, ["Hubo otro cambio después: ya no se puede deshacer", "Something else changed since: it can no longer be undone"]);
      if (!undoOf(a)) return no(409, ["Ya pasaron más de 10 minutos: ya no se puede deshacer", "More than 10 minutes have passed: it can no longer be undone"]);
      a.moves.pop(); a.state = m.from; a.note = m.prevNote;
      return ok(["Deshecho: " + a.key + " vuelve a " + m.from, "Undone: " + a.key + " is back to " + m.from]);
    }
    if (p === "/api/learn/next") {
      if (scenario === "empty") return { ok: true, status: 200, data: { card: null } };
      const topic = url.searchParams.get("topic");
      const on = (q) => !topic || topicOf(q) === topic || q.topicKey === topic;
      const pool = queue.filter((q) => !q.done && on(q));
      const q = pool[0] || (scenario === "calm" && !topic ? null : DECK.find((x) => on(x) && !x.done));
      return { ok: true, status: 200, data: { card: q ? { id: q.id, topic: topicOf(q), question: txt(q.question), offer: q.offer } : null } };
    }
    if (p === "/api/learn/answer") { const q = DECK.find((x) => x.id === url.searchParams.get("id")); return q ? { ok: true, status: 200, data: { answer: txt(q.answer) } } : no(404, ["Pregunta desconocida", "Unknown question"]); }
    if (p === "/api/learn/grade") {
      const q = DECK.find((x) => x.id === body.id); if (!q) return no(404, ["Pregunta desconocida", "Unknown question"]);
      q.seen = true; q.done = true;
      return ok(["Nota " + body.grade + " registrada; la repasas " + (body.grade >= 4 ? "en 6 días." : "mañana."), "Grade " + body.grade + " saved; you will review it " + (body.grade >= 4 ? "in 6 days." : "tomorrow.")]);
    }
    return no(404, ["No existe", "Not found"]);
  }

  // ---- The dry-run capture: a form the robot filled in and did not send, drawn here because the demo has no files.
  // An application waiting for approval has one; it is drawn once per language and kept.
  const captures = new Map();
  // the accent of a company's form: one of a few soft colours, chosen by its name
  const ACCENTS = ["#2f6fb5", "#2a8a73", "#6a4fb0", "#b0475f", "#2b7f9e", "#9a6a2c"];
  const accentOf = (text) => ACCENTS[Array.from(text).reduce((h, ch) => (h * 31 + ch.charCodeAt(0)) % 997, 7) % ACCENTS.length];
  const slug = (text) => text.toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "").replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");

  // The picture is drawn in slices of a few texts each, one per frame, and then encoded: a single pass is 70 to 100 ms on an ordinary machine (the first text of
  // each font is the slow one), which is a long frame in the middle of the arrival at COLA. A hidden tab gets no frames, so a timer is the way out.
  const rest = () => new Promise((resolve) => { requestAnimationFrame(() => resolve()); setTimeout(resolve, 120); });

  async function paintForm(a) {
    const en = lang() === "en";
    const W = 900, H = 1120;
    const c = document.createElement("canvas");
    c.width = W; c.height = H;
    const g = c.getContext("2d");
    const SANS = "system-ui, -apple-system, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif";
    const accent = accentOf(a.company);
    const L = en
      ? { careers: "Careers", type: "Full-time", details: "Your details", first: "First name", last: "Last name", email: "Email", phone: "Phone", cv: "Resume / CV",
        uploaded: "Uploaded", letter: "Cover letter", pay: "Expected monthly salary (USD)", visa: "Do you need visa sponsorship?", no: "No", submit: "Submit application",
        fiction: "Fictional form drawn for the public demo",
        body: "Hello " + a.company + " team, I am applying for the " + a.title + " role. For eight years I have built and run backend services in production, lately in Rust and TypeScript, and I like owning a service from design to on-call. I work remotely with distributed teams and can overlap with your hours. My CV, tailored to this role, is attached; I would be glad to walk you through a recent project." }
      : { careers: "Empleos", type: "Jornada completa", details: "Tus datos", first: "Nombre", last: "Apellido", email: "Correo", phone: "Teléfono", cv: "Currículum",
        uploaded: "Subido", letter: "Carta de presentación", pay: "Sueldo mensual esperado (USD)", visa: "¿Necesitas patrocinio de visa?", no: "No", submit: "Enviar postulación",
        fiction: "Formulario ficticio dibujado para la demo pública",
        body: "Hola, equipo de " + a.company + ": postulo al cargo de " + a.title + ". Llevo ocho años construyendo y operando servicios de backend en producción, últimamente en Rust y TypeScript, y me gusta hacerme cargo de un servicio desde el diseño hasta la guardia. Trabajo en remoto con equipos distribuidos y puedo coincidir con tu horario. Adjunto mi currículum, adaptado a este cargo; con gusto te cuento un proyecto reciente." };
    if (!g.roundRect) g.roundRect = function (x, y, w, h) { this.rect(x, y, w, h); };
    const text = (s, x, y, size, weight, color, align) => { g.font = weight + " " + size + "px " + SANS; g.fillStyle = color; g.textAlign = align || "left"; g.fillText(s, x, y); };
    const box = (x, y, w, h, r, fill, stroke) => {
      g.beginPath(); g.roundRect(x, y, w, h, r);
      if (fill) { g.fillStyle = fill; g.fill(); }
      if (stroke) { g.strokeStyle = stroke; g.lineWidth = 2; g.stroke(); }
    };
    const field = (label, value, x, y, w) => { text(label, x, y, 17, 600, "#6b7686"); box(x, y + 10, w, 50, 10, "#ffffff", "#cfd6e0"); text(value, x + 16, y + 43, 22, 400, "#1d2433"); };
    const wrap = (s, width, size, weight) => {
      g.font = weight + " " + size + "px " + SANS;
      const lines = []; let line = "";
      for (const word of s.split(" ")) {
        const next = line ? line + " " + word : word;
        if (g.measureText(next).width > width && line) { lines.push(line); line = word; } else line = next;
      }
      return lines.concat(line ? [line] : []);
    };
    g.fillStyle = "#f3f5f9"; g.fillRect(0, 0, W, H);
    // the company's header: a monogram and a name, nothing that looks like a real brand
    g.fillStyle = "#ffffff"; g.fillRect(0, 0, W, 88);
    g.fillStyle = "#dfe4ec"; g.fillRect(0, 87, W, 2);
    box(40, 20, 48, 48, 12, accent);
    text(a.company.charAt(0).toUpperCase(), 64, 54, 28, 700, "#ffffff", "center");
    text(a.company, 104, 54, 28, 700, "#1d2433");
    text(L.careers, 860, 52, 20, 600, "#6b7686", "right");
    await rest();
    text(a.title, 40, 150, 38, 700, "#1d2433");
    text(txt(LOCATIONS[a.where]) + "  ·  " + L.type, 40, 186, 21, 400, "#6b7686");
    await rest();
    // the form
    box(40, 206, 820, 788, 16, "#ffffff", "#dfe4ec");
    text(L.details, 72, 252, 24, 700, "#1d2433");
    field(L.first, "Alex", 72, 288, 372); field(L.last, "Rivera", 456, 288, 372);
    await rest();
    field(L.email, "alex.rivera@example.com", 72, 384, 372); field(L.phone, "+1 555 0142", 456, 384, 372);
    await rest();
    text(L.cv, 72, 494, 24, 700, "#1d2433");
    box(72, 508, 756, 66, 12, "#f8fafc", "#cfd6e0");
    box(92, 520, 34, 42, 5, "#ffffff", accent);
    g.fillStyle = accent; g.fillRect(99, 534, 20, 3); g.fillRect(99, 542, 20, 3); g.fillRect(99, 550, 12, 3);
    text("alex-rivera-cv-" + slug(a.company) + ".pdf", 144, 548, 21, 600, "#1d2433");
    box(700, 526, 112, 30, 15, "#e4f4ea");
    g.strokeStyle = "#1f7a45"; g.lineWidth = 3; g.beginPath(); g.moveTo(714, 541); g.lineTo(721, 548); g.lineTo(733, 534); g.stroke();
    text(L.uploaded, 742, 548, 16, 600, "#1f7a45");
    await rest();
    text(L.letter, 72, 620, 24, 700, "#1d2433");
    box(72, 634, 756, 186, 10, "#ffffff", "#cfd6e0");
    await rest();
    wrap(L.body, 722, 20, 400).slice(0, 6).forEach((line, i) => text(line, 90, 668 + i * 28, 20, 400, "#1d2433"));
    await rest();
    text(L.pay, 72, 866, 17, 600, "#6b7686"); box(72, 876, 372, 50, 10, "#ffffff", "#cfd6e0");
    text((a.salary ? a.salary[0] : 5000).toLocaleString(en ? "en-US" : "es-CL"), 88, 909, 22, 400, "#1d2433");
    text(L.visa, 456, 866, 17, 600, "#6b7686"); box(456, 876, 372, 50, 10, "#ffffff", "#cfd6e0");
    text(L.no, 472, 909, 22, 400, "#1d2433");
    // the button the robot did not press
    box(40, 1020, 300, 58, 12, accent);
    text(L.submit, 190, 1057, 22, 700, "#ffffff", "center");
    text(L.fiction, 860, 1056, 15, 400, "#9aa3b2", "right");
    await rest();
    return new Promise((resolve) => c.toBlob((blob) => resolve(blob), "image/png"));
  }

  /** What the page asks for with `session.blob`: the capture of an application that has one, else nothing. */
  async function capture(path) {
    await delay(120);
    const url = new URL(path, location.href);
    if (url.pathname.replace(/^.*\/api\//, "/api/") !== "/api/shot") return null;
    const a = byKey(url.searchParams.get("key"));
    if (!a || !a.shot) return null;
    const id = a.key + "|" + lang();
    if (!captures.has(id)) captures.set(id, paintForm(a));
    return captures.get(id);
  }

  // The page hands over its `refresh`: when the language changes, the next picture is asked for at once instead of in
  // half a minute, so the free texts follow the buttons.
  let refreshPage = null;
  let shown = lang();
  function attach(page) { refreshPage = page.refresh; shown = lang(); }
  new MutationObserver(() => {
    if (lang() === shown) return;
    shown = lang();
    if (refreshPage) refreshPage({ force: true });
  }).observe(document.documentElement, { attributes: true, attributeFilter: ["lang"] });

  // What the page says about the demo itself, drawn by HOY in the language of the page when it draws: the line, the note and two links, every text a pair [es, en].
  const about = {
    line: ["jobbot busca avisos, adapta el CV y llena el formulario de noche; tú apruebas cada envío.", "jobbot finds postings, tailors the CV and fills the form overnight; you approve every send."],
    note: ["Demo con datos ficticios: nada se envía.", "Demo with fictional data: nothing is sent."],
    links: [
      { href: "https://github.com/gonzalo-rosas-rojas/jobbot", text: ["Código", "Code"] },
      { href: "https://gonzalo-rosas-rojas.github.io/jobbot-showcase/", text: ["Cómo funciona", "How it works"] },
    ],
  };
  // `brand` is the name the page shows in its mark and its tab instead of "Panel"
  window.__DEMO__ = { api: handle, blob: capture, attach, startedAt: t0, stats, brand: "jobbot", about };
  // A visit from five hours ago, so the page has something to say about what changed since.
  try { if (!localStorage.getItem("panel.lastVisit")) localStorage.setItem("panel.lastVisit", ago(5 * HOUR)); } catch (e) { /* no storage */ }
})();
