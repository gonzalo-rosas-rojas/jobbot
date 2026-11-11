import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { setLang, setTimeZone } from "../src/i18n/index.js";
import esHoy from "../src/i18n/es/hoy.js";
import enHoy from "../src/i18n/en/hoy.js";
import { model, candidates, turnCount, nightFigures } from "../src/core/model.js";
import { hoyView, turnWord, NEXT_ROWS, SINCE_ROWS, SPELLED_UP_TO, ACTION_LABEL, keyShown } from "../src/moments/hoy/view.js";
import { clean, safeHref } from "../src/moments/shared/util.js";
import { KEYS } from "../src/ui/keys.js";

const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/snapshot.${name}.json`, import.meta.url), "utf8"));
const HOUR = 3600000;
const view = (snap, opts = {}) => {
  const m = model(snap);
  return hoyView(m, { nowMs: Date.parse(snap.generated_at), ...opts });
};
const fresh = (lang = "es") => { setLang(lang); setTimeZone("America/Santiago"); };
const strings = (value, out = []) => {
  if (typeof value === "string") out.push(value);
  else if (Array.isArray(value)) value.forEach((v) => strings(v, out));
  else if (value && typeof value === "object") Object.values(value).forEach((v) => strings(v, out));
  return out;
};

test("A16: every count of HOY is the model's, so changing the snapshot changes the screen and nothing is a literal", () => {
  fresh();
  for (const name of ["empty", "rich", "heavy"]) {
    const snap = fixture(name);
    const m = model(snap);
    const v = view(snap);
    const night = nightFigures(snap);
    assert.equal(v.turn, turnCount(m), name);
    assert.deepEqual(v.figures.map((f) => f.n), [night.read, night.above, night.complete, night.awaiting], name);
    assert.equal(v.strip.slots.length, 48);
    assert.equal(v.more, Math.max(0, turnCount(m) - ((v.next ? 1 : 0) + v.rows.length)), `${name}: "y M más" is what does not fit`);
  }
  const rich = fixture("rich");
  const before = view(rich);
  rich.today.tasks_total += 1;
  rich.today.tasks.push({ id: 999, text: "Write the thank-you note", job_key: null, due_at: null, overdue: false, link: null, kind: "manual", created_at: rich.generated_at });
  rich.pipeline.seen += 40;
  rich.pipeline.above += 3;
  rich.awaiting_approval.total += 2;
  const after = view(rich);
  assert.equal(after.turn, before.turn + 3, "one task and two more to approve");
  assert.equal(after.figures[0].n, before.figures[0].n + 40);
  assert.equal(after.figures[1].n, before.figures[1].n + 3);
  assert.equal(after.figures[3].n, before.figures[3].n + 2);
  assert.ok(after.awaiting.startsWith("3 "), after.awaiting);
});

test("the headline is the real number, the list is cut: LO SIGUIENTE, six rows, and 'y M más' for the rest", () => {
  fresh();
  const rich = view(fixture("rich"));
  assert.equal(NEXT_ROWS, 6);
  assert.equal(rich.rows.length, 6);
  assert.equal(rich.next.id, "task:101");
  assert.equal(rich.headline, "Te tocan trece.", "one run, number spelled inside it");
  assert.equal(rich.after, "después de esta: 12");
  assert.equal(rich.restLabel, "Y después (6)");
  assert.equal(rich.more, 6);
  assert.equal(rich.moreLabel, "y 6 más");
  const heavy = view(fixture("heavy"));
  assert.ok(heavy.turn > 50, `the headline says ${heavy.turn}, not seven`);
  assert.equal(heavy.rows.length, 6);
  assert.equal(heavy.more, heavy.turn - 7);
  assert.equal(heavy.moreLabel, `y ${heavy.turn - 7} más`);
  const one = fixture("rich");
  one.today.tasks = one.today.tasks.slice(0, 1);
  one.today.tasks_total = 1;
  one.needs_gonza = { total: 0, cards: [] };
  one.awaiting_approval = { total: 0, cards: [] };
  one.board = one.board.map((col) => (col.state === "offer" || col.state === "replied" || col.state === "interview" || col.state === "technical_test" || col.state === "ghosted" || col.state === "submitted" ? { ...col, total: 0, cards: [] } : col));
  const single = view(one);
  assert.equal(single.turn, 1);
  assert.equal(single.headline, "Te toca uno.");
  assert.equal(single.after, "es lo último");
  assert.deepEqual([single.rows.length, single.more, single.moreLabel], [0, 0, null]);
});

test("every action is a key of the table that works on HOY; LO SIGUIENTE has three buttons, a row one; the primary verb follows the kind", () => {
  fresh();
  for (const name of ["rich", "heavy"]) {
    const v = view(fixture(name));
    for (const row of [v.next, ...v.rows]) {
      assert.ok(row.actions.length > 0, row.id);
      assert.deepEqual(row.actions.filter((a) => a.primary).length, 1, row.id);
      assert.equal(row.actions[0].primary, true, "the primary leads");
      assert.ok(row.actions.filter((a) => a.visible).length <= (row === v.next ? 3 : 1), `${row.id} draws too many buttons`);
      for (const a of row.actions) {
        const entry = KEYS.find((k) => k.id === a.id);
        assert.ok(entry, `${a.id} is not in the key table`);
        assert.ok(entry.scope === "global" || entry.stops.includes("hoy"), `${a.id} does not work on HOY`);
        assert.equal(a.key, keyShown(a.id));
        assert.ok(ACTION_LABEL[a.id]);
      }
    }
  }
  const v = view(fixture("rich"));
  const byId = Object.fromEntries([v.next, ...v.rows].map((r) => [r.id, r]));
  assert.equal(byId["task:101"].actions[0].id, "done");
  assert.deepEqual(byId["task:101"].actions.filter((a) => a.visible).map((a) => a.id), ["done", "open", "link"], "a task has no snooze (only a card can be put aside); its third button is the posting of the application it belongs to");
  assert.ok(!byId["task:101"].actions.some((a) => a.id === "snooze"));
  assert.equal(byId["card:fx:12"].actions[0].id, "approve");
  assert.equal(byId["card:fx:12"].actions[0].permanent, true);
  assert.equal(byId["card:fx:9"].actions[0].id, "link", "a form the bot could not send is opened with o, not approved");
  assert.equal(byId["card:fx:7"].actions[0].id, "open", "an offer is looked at");
  // x and r are key-only verbs: the server's `marks` of the application decide them, and a task has neither
  const follow = [v.next, ...v.rows].find((r) => r.kindId === "followup");
  assert.ok(byId["card:fx:12"].actions.some((a) => a.id === "withdraw"), "an application that can be withdrawn has x");
  assert.ok(!byId["card:fx:12"].actions.some((a) => a.id === "register"), "a proposal that was never sent has no answer to log");
  assert.ok(!byId["task:101"].actions.some((a) => a.id === "withdraw" || a.id === "register"));
  if (follow) assert.deepEqual(["withdraw", "register"].filter((id) => follow.actions.some((a) => a.id === id)), ["withdraw", "register"], "a sent application takes both");
  assert.ok([...byId["card:fx:12"].actions, ...(byId["task:101"].actions)].filter((a) => a.id === "withdraw" || a.id === "register").every((a) => !a.visible), "they never get a button");
  assert.equal(keyShown("open"), "↵");
  assert.equal(keyShown("snooze"), "s");
});

test("rows carry their own data key: a card is data-card, a task is data-task, and a row's mark is what the diff marks as fresh", () => {
  fresh();
  const v = view(fixture("rich"));
  for (const row of [v.next, ...v.rows]) {
    assert.ok(("card" in row.data) !== ("task" in row.data), row.id);
    assert.equal(row.mark, "card" in row.data ? "card:" + row.data.card : "task:" + row.data.task);
  }
  assert.deepEqual(v.next.data, { task: "101" });
  assert.equal(v.next.open, "fx:2", "a task that belongs to an application opens that application");
});

test("hostile text from outside stays text: one line, no bidi or control characters, cut, and only http(s) is a link", () => {
  fresh();
  const snap = fixture("rich");
  const evil = "A\u202eevil\u202c\u2066x\u2069\nsecond line\u0007" + "y".repeat(400);
  snap.awaiting_approval.cards[0].company = evil;
  snap.awaiting_approval.cards[0].title = "<img src=x onerror=alert(1)>";
  snap.awaiting_approval.cards[0].url = "javascript:alert(1)";
  snap.today.tasks[0].text = evil;
  snap.today.tasks[0].link = "data:text/html,<script>1</script>";
  const v = view(snap);
  const rows = [v.next, ...v.rows];
  const shown = rows.find((r) => r.id === "card:fx:12");
  assert.ok(shown, "the proposal is on the list");
  assert.equal(shown.link, null, "a javascript: URL is no link");
  assert.equal(shown.text, "<img src=x onerror=alert(1)>", "markup is only text: the DOM will make it a text node");
  assert.ok(shown.company.length <= 60 && shown.company.endsWith("…"));
  const task = rows.find((r) => r.id === "task:101");
  assert.equal(task.link, "https://example.com/fx/2", "the task's own data: link is dropped and the posting of its application is used instead");
  for (const s of strings(v)) {
    assert.doesNotMatch(s, /[\u0000-\u001f\u202a-\u202e\u2066-\u2069]/, "no control or bidi character reaches the screen");
    assert.ok(s.length <= 400, `a string of ${s.length} characters`);
  }
  assert.equal(clean("a\u202eb\n c\t d", 20), "a b c d");
  assert.equal(clean("x".repeat(10), 5), "xxxx…");
  assert.equal(clean(null), "");
  assert.equal(safeHref("HTTPS://example.com/a"), "HTTPS://example.com/a");
  assert.deepEqual([safeHref("mailto:a@b.c"), safeHref("//evil"), safeHref(""), safeHref(undefined)], [null, null, null, null]);
});

test("since the last visit: nothing on a first visit; newest first, five rows, counts and the hours since", () => {
  fresh();
  const snap = fixture("rich");
  assert.equal(view(snap).since, null, "a first visit has no last visit");
  const now = Date.parse(snap.generated_at);
  const v = view(snap, { lastVisitMs: now - 5 * HOUR });
  assert.equal(SINCE_ROWS, 5);
  assert.ok(v.since.rows.length > 0 && v.since.rows.length <= 5);
  assert.ok(v.since.head.startsWith("hace 5 h: "), v.since.head);
  assert.deepEqual(v.since.rows.map((r) => r.at), [...v.since.rows.map((r) => r.at)].sort((a, b) => b - a), "newest first");
  for (const r of v.since.rows) {
    assert.ok(r.at > now - 5 * HOUR, "nothing from before the last visit");
    assert.match(r.hour, /^\d\d:\d\d$/);
    assert.ok(["mail:", "card:"].some((p) => r.mark.startsWith(p)));
  }
  assert.ok(v.since.rows.some((r) => r.kind === "mail") && v.since.rows.some((r) => r.kind === "move"));
  const long = view(snap, { lastVisitMs: now - 72 * HOUR });
  assert.equal(long.since.rows.length, 5);
  assert.ok(long.since.total >= long.since.rows.length);
  assert.equal(view(snap, { lastVisitMs: now + HOUR }).since.total, 0, "a last visit in the future (clock skew) shows nothing, not an error");
});

test("the calm state says the truth about the engine; a clock that is wrong never prints a negative time or NaN", () => {
  fresh();
  const empty = view(fixture("empty"));
  assert.equal(empty.calm, true);
  assert.equal(empty.headline, "Nada pendiente.");
  assert.equal(empty.next, null);
  assert.deepEqual([empty.rows.length, empty.more, empty.after, empty.moreLabel], [0, 0, null, null]);
  assert.equal(empty.calmLine, "No hay corridas todavía.");
  const quiet = fixture("rich");
  quiet.today = { tasks_total: 0, tasks: [], emails_total: 0, emails: [] };
  quiet.needs_gonza = { total: 0, cards: [] };
  quiet.awaiting_approval = { total: 0, cards: [] };
  quiet.board = quiet.board.map((col) => ({ ...col, total: 0, cards: [] }));
  const ran = view(quiet);
  assert.equal(ran.calm, true);
  assert.equal(ran.calmLine, "La última corrida fue hace 7 h y no dejó nada para ti.");
  quiet.controls.last_run_at = new Date(Date.parse(quiet.generated_at) + 3 * HOUR).toISOString();
  const skewed = view(quiet);
  assert.equal(skewed.calmLine, "La última corrida fue ahora y no dejó nada para ti.");
  for (const v of [empty, ran, skewed]) for (const s of strings(v)) assert.doesNotMatch(s, /NaN|undefined|null|−|\[object/);
});

test("the strip has the 48 half hours with the busy ones (the run window closed), and says from when to when", () => {
  fresh();
  const v = view(fixture("rich"));
  assert.equal(v.strip.slots.length, 48);
  assert.equal(v.strip.max, Math.max(...v.strip.slots.map((s) => s.seen)));
  assert.ok(v.strip.slots.some((s) => s.busy) && v.strip.slots.some((s) => !s.busy));
  for (const s of v.strip.slots) assert.ok(s.h >= 0 && s.h <= 1);
  assert.equal(v.strip.slots.find((s) => s.seen === v.strip.max).h, 1);
  assert.match(v.strip.title, /^Avisos por media hora, de \d\d:\d\d ayer a \d\d:\d\d hoy; rayado: tus horas$/);
  const unknown = fixture("rich");
  unknown.activity.slots = unknown.activity.slots.map((s) => ({ ...s, open: null }));
  assert.ok(view(unknown).strip.slots.every((s) => !s.busy), "an unknown window is not drawn as busy");
  assert.equal(view({ ...fixture("rich"), activity: { slots: [] } }).strip.title, null);
  assert.equal(view(fixture("empty")).strip.title, null, "a night with nothing seen has no strip to title");
});

test("the foot: the window, the last run (A15), STOP when it is on and the spend, from controls and claude", () => {
  fresh();
  const snap = fixture("rich");
  const v = view(snap);
  assert.deepEqual(v.foot.slice(0, 2), ["Ventana cerrada, abre 18:00", "última corrida hace 7 h"]);
  assert.match(v.foot[2], /^Claude, 24 h: .*1,84.* de .*5,00/, "STOP off is not said");
  snap.controls.stopped = true;
  snap.controls.last_run_at = null;
  snap.controls.engine = "never";
  assert.deepEqual(view(snap).foot.slice(1, 3), ["sin corridas todavía", "STOP activo"]);
  snap.controls.engine = undefined;
  assert.deepEqual(view(snap).foot.slice(0, 2), ["Ventana cerrada, abre 18:00", "STOP activo"], "no engine word and no date: the run is simply not said");
  snap.snoozed = { total: 1, cards: [{ key: "fx:5", company: "Zeta Test Co", title: "Data Engineer", snoozed_until: "2026-10-09T12:00:00Z" }] };
  const z = view(snap);
  assert.deepEqual([z.snoozed.total, z.snoozed.items[0].key, z.snoozed.items[0].company], [1, "fx:5", "Zeta Test Co"]);
});

test("both languages: every key the view asks for exists, and no string shows a missing key", () => {
  const source = readFileSync(new URL("../src/moments/hoy/view.js", import.meta.url), "utf8");
  for (const dict of [esHoy, enHoy]) {
    for (const m of source.matchAll(/\bt(n?)\(\s*"(hoy\.[a-zA-Z.]*[a-zA-Z])"/g)) {
      const key = m[2];
      const ok = dict[key] !== undefined || (m[1] === "n" && (dict[key + ".one"] !== undefined && dict[key + ".other"] !== undefined));
      assert.ok(ok, `missing ${key}`);
    }
    for (const kind of ["task", "test", "interview", "offer", "approve", "handoff", "missing_answers", "blocked", "may_have_sent", "error", "manual_apply", "other", "followup"]) assert.ok(dict["hoy.kind." + kind], kind);
  }
  for (const lang of ["es", "en"]) {
    fresh(lang);
    for (const name of ["empty", "rich", "heavy"]) {
      const now = Date.parse(fixture(name).generated_at);
      for (const s of strings(view(fixture(name), { lastVisitMs: now - 8 * HOUR }))) assert.ok(!s.includes("⟦"), `${lang}/${name}: ${s}`);
    }
  }
  fresh();
  assert.equal(view(fixture("rich")).headline, "Te tocan trece.");
  fresh("en");
  assert.equal(view(fixture("rich")).headline, "You have thirteen.");
  assert.equal(view(fixture("rich")).next.actions[0].label, "Done");
  fresh();
});

test("the headline spells its number up to twenty, in each language, and writes the figure after that", () => {
  for (const [lang, dict] of [["es", esHoy], ["en", enHoy]]) {
    fresh(lang);
    for (let n = 0; n <= SPELLED_UP_TO; n++) assert.ok(dict["hoy.num." + n], `${lang}: hoy.num.${n}`);
    assert.equal(dict["hoy.num." + (SPELLED_UP_TO + 1)], undefined, "nothing is spelled above the limit");
  }
  fresh();
  assert.deepEqual([0, 1, 7, 13, 20, 21, 68].map(turnWord), ["cero", "uno", "siete", "trece", "veinte", "21", "68"]);
  assert.equal(view(fixture("rich")).headline, "Te tocan trece.", "the real number of the rich fixture, 13, spelled inside the one run of the headline");
  assert.equal(view(fixture("heavy")).headline, "Te tocan 68.", "the headline is never cut, and a big number is a figure");
  fresh("en");
  assert.equal(view(fixture("rich")).headline, "You have thirteen.");
  assert.equal(view(fixture("empty")).headline, "Nothing pending.", "nothing waits: the calm title is the whole headline");
  fresh();
  assert.equal(turnWord(-1), "-1");
});

test("a partial snapshot (no activity, no threshold, a card with no score, more counted than listed) never puts NaN, null or undefined on the page", () => {
  for (const lang of ["es", "en"]) {
    fresh(lang);
    const snap = fixture("rich");
    delete snap.activity;
    snap.pipeline.threshold = null;
    snap.pipeline.days = null;
    snap.trust = null;
    snap.claude = null;
    snap.controls.last_run_at = null;
    snap.controls.engine = "never";
    snap.awaiting_approval.cards[0].score = null;
    snap.awaiting_approval.cards[0].source = null;
    snap.needs_gonza.total += 5;
    snap.today.emails[0].subject = null;
    snap.today.emails[0].summary = null;
    const now = Date.parse(snap.generated_at);
    const v = view(snap, { lastVisitMs: now - 30 * HOUR });
    assert.equal(v.turn, turnCount(model(snap)), "the headline counts what the server counts, listed or not");
    assert.equal(v.strip.slots.length, 0);
    assert.equal(v.strip.title, null);
    assert.equal(v.base, null);
    // `mark` is an internal key (the mail id carries the subject, even a missing one); it is never drawn
    const shown = JSON.parse(JSON.stringify(v, (key, value) => (key === "mark" ? undefined : value)));
    for (const s of strings(shown)) assert.doesNotMatch(s, /NaN|undefined|\bnull\b|−|\[object|⟦/, `${lang}: ${s}`);
  }
  fresh();
});
