// Brings the two hand-made fixtures to the current snapshot contract and writes the heavy one. Idempotent: it only adds
// what is missing, so running it twice changes nothing.   node test/fixtures/make.mjs
import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { fakePostings } from "./fake.mjs";

const dir = new URL("./", import.meta.url);
const read = (n) => JSON.parse(readFileSync(new URL(n, dir), "utf8"));
const write = (n, v) => writeFileSync(new URL(n, dir), JSON.stringify(v, null, 2) + "\n");
const HOUR = 3600000, DAY = 24 * HOUR;
const at = (s, ms) => new Date(Date.parse(s.generated_at) + ms).toISOString().replace(".000Z", "Z");
const orSet = (o, k, v) => { if (!Object.hasOwn(o, k)) o[k] = v; };
const MARKS = {
  submitted: ["replied", "rejected", "withdrawn", "ghosted"], replied: ["technical_test", "interview", "rejected", "withdrawn"],
  technical_test: ["interview", "rejected", "withdrawn"], interview: ["offer", "rejected", "withdrawn"],
  offer: ["accepted", "rejected", "withdrawn"], ghosted: ["replied", "rejected", "withdrawn"],
};
// The four kinds of help the server tells apart, with the sentence it writes for each (crates/jobbot-panel/src/snapshot/cards.rs, reason_kind).
const STUCK = [
  { kind: "handoff", reason: "handoff: the page asks for a new account", short: "Envía tú el formulario: la hoja de respuestas está lista" },
  { kind: "missing_answers", reason: "faltan respuestas: expected salary", short: "El formulario pide respuestas que no tengo" },
  { kind: "blocked", reason: "captcha", short: "Una página me frenó (captcha, ingreso o bloqueo)" },
  { kind: "manual_apply", reason: "listing_only: no form to fill", short: "Postula tú desde el enlace del aviso" },
];

/** The fields the contract added after the fixtures were drawn. `rank` is stable per card, so a card listed twice (in the board and in a queue) gets the same ones. */
function patchCard(s, c, rank) {
  orSet(c, "posted_at", at(s, -(rank + 2) * DAY));
  orSet(c, "first_seen", at(s, -(rank + 1) * DAY));
  orSet(c, "salary_min", rank % 3 === 0 ? 4500 : null);
  orSet(c, "salary_max", rank % 3 === 0 ? 5500 : null);
  orSet(c, "days_in_state", Math.max(0, Math.floor((Date.parse(s.generated_at) - Date.parse(c.updated_at)) / DAY)));
  orSet(c, "snoozed_until", null);
  orSet(c, "waiting_since", c.state === "needs_gonza" ? c.updated_at : null);
  orSet(c, "has_handoff", c.reason_kind === "handoff");
  orSet(c, "age_days", rank + 2);
  orSet(c, "prune_in_days", ["scored", "prepared", "needs_gonza"].includes(c.state) ? 20 - rank : null);
  orSet(c, "undo", null);                                          // the mark he can still take back (schema v4): none in a drawn card
}

export function patch(s, { empty }) {
  const c = s.controls;
  orSet(c, "last_run_at", empty ? null : at(s, -7 * HOUR));
  orSet(c, "engine", empty ? "never" : "recent");
  orSet(c, "claude_account", empty ? null : {
    ok: true, message: null, subscription: "max", email: "d***@example.com", checked_at: at(s, -8 * HOUR),
    auth_method: "claude.ai", api_provider: "firstParty", org_id: "org-demo", config_dir: "%LOCALAPPDATA%\\demo\\claude", cli_version: "2.1.0", anchored: true,
  });
  if (c.claude_account) orSet(c.claude_account, "anchored", true);
  orSet(c, "auto_approve_min", null);
  orSet(c, "scheduled_task", empty ? null : { exists: true, next_run_at: at(s, 11 * HOUR) });
  orSet(s.funnel, "by_source", empty ? {} : {
    ashby: { n: 12, sent: 9, answered: 3, tests: 1, interviews: 1, offers: 0 },
    greenhouse: { n: 20, sent: 16, answered: 5, tests: 2, interviews: 1, offers: 1 },
    lever: { n: 9, sent: 6, answered: 1, tests: 0, interviews: 1, offers: 0 },
  });
  // `rejected` (of `answered`) and the two other splits of the funnel came after the boards one: by origin and by region, each adding up to the boards.
  const REJECTED = { ashby: 1, greenhouse: 2, lever: 0 };
  for (const [name, group] of Object.entries(s.funnel.by_source)) orSet(group, "rejected", REJECTED[name] ?? 0);
  orSet(s.funnel, "by_origin", empty ? {} : {
    cold: { n: 30, sent: 22, answered: 4, rejected: 2, tests: 2, interviews: 1, offers: 0 },
    referral: { n: 4, sent: 4, answered: 3, rejected: 0, tests: 1, interviews: 1, offers: 1 },
    recruiter: { n: 3, sent: 3, answered: 2, rejected: 1, tests: 0, interviews: 1, offers: 0 },
    unknown: { n: 4, sent: 2, answered: 0, rejected: 0, tests: 0, interviews: 0, offers: 0 },
  });
  orSet(s.funnel, "by_region", empty ? {} : {
    open: { n: 25, sent: 19, answered: 6, rejected: 2, tests: 2, interviews: 2, offers: 1 },
    silent: { n: 12, sent: 9, answered: 2, rejected: 1, tests: 1, interviews: 1, offers: 0 },
    other: { n: 4, sent: 3, answered: 1, rejected: 0, tests: 0, interviews: 0, offers: 0 },
  });
  orSet(s, "snoozed", { total: 0, cards: [] });
  const moves = s.activity.moves || [];
  orSet(s.activity, "moves_by_state", moves.reduce((m, mv) => ((m[mv.to] = (m[mv.to] || 0) + 1), m), {}));
  const ranks = new Map();
  for (const cards of [...s.board.map((col) => col.cards), s.needs_gonza.cards, s.awaiting_approval.cards, s.snoozed.cards]) {
    for (const card of cards) {
      if (!ranks.has(card.key)) ranks.set(card.key, ranks.size % 7);
      patchCard(s, card, ranks.get(card.key));
    }
  }
  s.today.tasks.forEach((task, n) => { orSet(task, "kind", n % 2 ? "recruiter" : "manual"); orSet(task, "created_at", at(s, -(n + 1) * 3 * HOUR)); });
  return s;
}

/** 74 live processes, 8 that need him, 3 captures to approve, 6 tasks, the mails of rich: the 1080-high frame nobody tried. */
export function heavy(rich) {
  const s = JSON.parse(JSON.stringify(rich));
  const seen = new Set();
  // Unique names, and none ending in "-tron": a syllable list can land on a real firm (Photron is one), and nothing real may appear.
  const lines = fakePostings(900, 20261008).map((l) => l.split(" - ")).filter(([, company]) => !/tron /.test(company) && !seen.has(company) && seen.add(company));
  let n = 0;
  const card = (state, extra = {}) => {
    const [title, company] = lines[n];
    const c = {
      key: `fake:${n}`, company, title, score: 60 + ((n * 7) % 38), state, source: ["ashby", "greenhouse", "lever"][n % 3], url: `https://example.com/jobs/${n}`,
      reason: null, reason_kind: null, reason_short: null, note: null, approved: false, has_screenshot: false, has_cv: true, has_dossier: false,
      updated_at: at(s, -((n % 20) + 1) * DAY), actions: [], marks: MARKS[state] || [], ...extra,
    };
    patchCard(s, c, n % 7);
    n += 1;
    return c;
  };
  const want = { submitted: 45, replied: 10, technical_test: 5, interview: 4, offer: 1, ghosted: 9 };
  for (const col of s.board) {
    if (!want[col.state]) continue;
    col.cards = Array.from({ length: want[col.state] }, () => card(col.state));
    col.total = col.cards.length;
  }
  const stuck = Array.from({ length: 8 }, (_, i) => {
    const k = STUCK[i % STUCK.length];
    return card("needs_gonza", { reason: k.reason, reason_kind: k.kind, reason_short: k.short, actions: ["resolve", "confirm"] });
  });
  s.needs_gonza = { total: stuck.length, cards: stuck };
  const waiting = s.board.find((col) => col.state === "needs_gonza");
  waiting.cards = stuck;
  waiting.total = stuck.length;
  const fresh = Array.from({ length: 2 }, () => card("prepared", { has_screenshot: true, actions: ["approve"] }));
  const prepared = s.board.find((col) => col.state === "prepared");
  prepared.cards.push(...fresh);
  prepared.total = prepared.cards.length;
  s.awaiting_approval.cards.push(...fresh);
  s.awaiting_approval.total = s.awaiting_approval.cards.length;
  s.today.tasks = Array.from({ length: 6 }, (_, i) => ({
    id: 900 + i, text: `Prepare test ${i + 1} of the week`, job_key: i < 2 ? `fake:${i + 3}` : null, due_at: at(s, (i - 1) * 9 * HOUR), overdue: i === 0,
    link: null, kind: i % 2 ? "recruiter" : "manual", created_at: at(s, -DAY),
  }));
  s.today.tasks_total = 6;
  s.funnel.by_source = { ashby: { n: 31, sent: 20, answered: 6, rejected: 2, tests: 2, interviews: 1, offers: 0 }, greenhouse: { n: 30, sent: 14, answered: 4, rejected: 1, tests: 2, interviews: 2, offers: 1 }, lever: { n: 29, sent: 11, answered: 2, rejected: 1, tests: 1, interviews: 1, offers: 0 } };
  s.funnel.by_origin = {
    cold: { n: 60, sent: 30, answered: 7, rejected: 2, tests: 3, interviews: 2, offers: 0 }, referral: { n: 10, sent: 6, answered: 3, rejected: 1, tests: 1, interviews: 1, offers: 1 },
    recruiter: { n: 8, sent: 5, answered: 1, rejected: 0, tests: 1, interviews: 1, offers: 0 }, unknown: { n: 12, sent: 4, answered: 1, rejected: 1, tests: 0, interviews: 0, offers: 0 },
  };
  s.funnel.by_region = {
    open: { n: 50, sent: 28, answered: 7, rejected: 2, tests: 3, interviews: 3, offers: 1 }, silent: { n: 30, sent: 14, answered: 4, rejected: 2, tests: 2, interviews: 1, offers: 0 },
    other: { n: 10, sent: 3, answered: 1, rejected: 0, tests: 0, interviews: 0, offers: 0 },
  };
  return s;
}

// Only when it is run by name. A bare `node --test` takes every module under test/ for a test file and runs this one too, which rewrote the
// fixtures under whoever was only running the tests; the runner marks its children with NODE_TEST_CONTEXT.
const runByName = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href && !process.env.NODE_TEST_CONTEXT;
if (runByName) {
  const empty = patch(read("snapshot.empty.json"), { empty: true });
  const rich = patch(read("snapshot.rich.json"), { empty: false });
  write("snapshot.empty.json", empty);
  write("snapshot.rich.json", rich);
  write("snapshot.heavy.json", heavy(rich));
  console.log("fixtures written");
}
