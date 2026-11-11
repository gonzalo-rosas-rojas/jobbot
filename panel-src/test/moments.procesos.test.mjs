import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { setLang, setTimeZone, has } from "../src/i18n/index.js";
import esProcesos from "../src/i18n/es/procesos.js";
import enProcesos from "../src/i18n/en/procesos.js";
import { model, stopCounts, LIVE } from "../src/core/model.js";
import { createPlace } from "../src/moments/procesos/place.js";
import { procesosView, entriesOf, rowViewOf, LANES, FILTERS, ORDERS, filterId, gateOf, layersOf, keyDisplay, STALE_DATA_MS } from "../src/moments/procesos/view.js";
import { KEYS } from "../src/ui/keys.js";

const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/snapshot.${name}.json`, import.meta.url), "utf8"));
const fresh = (lang = "es") => { setLang(lang); setTimeZone("America/Santiago"); };
const nowOf = (snap) => Date.parse(snap.generated_at);
const DAY = 86400000;
const view = (snap, opts = {}) => procesosView(model(snap), { nowMs: nowOf(snap), place: opts.place || createPlace(), ...opts });
const strings = (value, out = []) => {
  if (typeof value === "string") out.push(value);
  else if (Array.isArray(value)) value.forEach((v) => strings(v, out));
  else if (value && typeof value === "object") Object.values(value).forEach((v) => strings(v, out));
  return out;
};
const cellsOf = (v) => v.lanes.flatMap((l) => l.cells).filter((c) => c.tomb == null);
const card = (snap, key) => snap.board.flatMap((c) => c.cards).find((c) => c.key === key);
const colOf = (snap, state) => snap.board.find((c) => c.state === state);

test("the words of PROCESOS exist in both languages with the same variables, and every key the code can ask for is there", () => {
  assert.deepEqual(Object.keys(esProcesos).sort(), Object.keys(enProcesos).sort());
  const vars = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(",");
  for (const key of Object.keys(esProcesos)) assert.equal(vars(esProcesos[key]), vars(enProcesos[key]), key);
  for (const key of Object.keys(esProcesos)) assert.match(key, /^procesos\./);
  fresh();
  for (const lane of LANES) for (const state of lane.states) assert.ok(has("procesos.tomb." + state), "a tombstone can name " + state);
  for (const state of ["accepted", "rejected", "withdrawn"]) assert.ok(has("procesos.tomb." + state), state);
});

test("A16: every count of the head is the model's: the number is the bar's, the pieces add up to it, the closed are the board's own totals", () => {
  fresh();
  for (const name of ["empty", "rich", "heavy"]) {
    const snap = fixture(name);
    const m = model(snap);
    const v = view(snap);
    assert.equal(v.open, stopCounts(m).procesos, `${name}: the same number as the stops bar`);
    assert.equal(v.head.you + v.head.snoozed + v.head.wait + v.head.quiet, v.open, `${name}: you + snoozed + waiting + no news is every open application`);
    assert.equal(v.head.closed, snap.board.filter((c) => ["accepted", "rejected", "withdrawn"].includes(c.state)).reduce((n, c) => n + c.total, 0));
    assert.equal(v.closed.total, v.head.closed);
    assert.equal(v.filters[0].n, v.open);
    assert.deepEqual(v.filters.map((f) => f.n), [v.open, v.head.you + v.head.presendYou, v.head.wait, v.head.quiet], "the chip of what is his also counts his 'Por enviar' ones, as the list shows them (J2-5)");
    const live = LIVE.reduce((n, s) => n + (snap.board.find((c) => c.state === s) || { total: 0 }).total, 0);
    assert.equal(v.open, live, name);
  }
  const heavy = view(fixture("heavy"));
  assert.equal(heavy.open, 74, "the heavy frame has 74 live applications");
  const rich = fixture("rich");
  const before = view(rich);
  colOf(rich, "submitted").cards.push({ ...card(rich, "fx:5"), key: "fx:99", company: "Added Test Co" });
  colOf(rich, "submitted").total += 1;
  const after = view(rich);
  assert.equal(after.open, before.open + 1, "one more in the board is one more on the screen");
  assert.equal(after.head.wait, before.head.wait + 1);
});

test("the lanes are the stages of the tracker, what has not gone out first, and each pile is in the lane of its state", () => {
  fresh();
  assert.deepEqual(LANES.map((l) => l.id), ["presend", "submitted", "replied", "technical_test", "interview", "offer", "ghosted"]);
  const snap = fixture("rich");
  const v = view(snap);
  assert.deepEqual(v.lanes.map((l) => l.id), ["presend", "submitted", "replied", "technical_test", "interview", "offer", "ghosted"]);
  for (const lane of v.lanes) {
    for (const cell of lane.cells) {
      const state = card(snap, cell.key).state;
      assert.ok(LANES.find((l) => l.id === lane.id).states.includes(state), `${cell.key} (${state}) is in ${lane.id}`);
      assert.equal(cell.state, state);
    }
    assert.equal(lane.total, lane.cells.length, "nothing is hidden when nothing is filtered");
  }
  assert.equal(v.lanes.find((l) => l.id === "presend").cells.length, 8, "scored, prepared and stuck ones");
  assert.equal(v.lanes.find((l) => l.id === "submitted").cells.length, 4);
  // a lane that has nobody is not drawn
  const lean = fixture("rich");
  colOf(lean, "technical_test").cards = [];
  colOf(lean, "technical_test").total = 0;
  assert.ok(!view(lean).lanes.some((l) => l.id === "technical_test"));
  assert.deepEqual(view(fixture("empty")).lanes, []);
});

test("whose turn it is: his are the ones with a task or an offer and the proposals and stuck forms; a snoozed one is nobody's; the quiet lane is quiet", () => {
  fresh();
  const snap = fixture("rich");
  const turns = Object.fromEntries(entriesOf(model(snap), nowOf(snap)).map((e) => [e.key, e.turn]));
  for (const key of ["fx:2", "fx:4", "fx:3", "fx:7", "fx:8"]) assert.equal(turns[key], "you", key);
  for (const key of ["fx:12", "fx:9", "fx:10", "fx:11"]) assert.equal(turns[key], "you", `${key} waits for his approval or help`);
  assert.equal(turns["fx:6"], "quiet");
  for (const key of ["fx:1", "fx:5", "fx:19", "fx:20"]) assert.equal(turns[key], "wait", key);
  assert.equal(turns["fx:15"], "wait", "a scored one waits for jobbot");
  const away = fixture("rich");
  card(away, "fx:2").snoozed_until = new Date(nowOf(away) + 3 * DAY).toISOString();
  const after = Object.fromEntries(entriesOf(model(away), nowOf(away)).map((e) => [e.key, e.turn]));
  assert.equal(after["fx:2"], "snoozed", "put aside until a date in the future");
  card(away, "fx:2").snoozed_until = new Date(nowOf(away) - DAY).toISOString();
  assert.equal(Object.fromEntries(entriesOf(model(away), nowOf(away)).map((e) => [e.key, e.turn]))["fx:2"], "you", "a date that already passed puts nothing aside");
  const listed = fixture("rich");
  listed.snoozed = { total: 1, cards: [{ ...card(listed, "fx:4") }] };
  assert.equal(Object.fromEntries(entriesOf(model(listed), nowOf(listed)).map((e) => [e.key, e.turn]))["fx:4"], "snoozed", "the server's own list of the snoozed counts too");
});

test("the five layers of a pile: each is done, here or still to come, the second is as long as the score, and the gate says who passed it", () => {
  fresh();
  const snap = fixture("rich");
  const sent = layersOf(card(snap, "fx:1"));
  assert.deepEqual(sent.map((l) => l.id), ["aviso", "puntaje", "cv", "respuestas", "compuerta"]);
  assert.ok(sent.every((l) => l.state === "done"), "a sent application has passed every layer");
  assert.equal(sent[1].w, 91, "the length of the second layer is the score");
  const scored = layersOf(card(snap, "fx:15"));
  assert.deepEqual(scored.map((l) => l.state), ["done", "done", "here", "todo", "todo"]);
  const dry = layersOf(card(snap, "fx:14"));
  assert.equal(dry[3].state, "here", "a dry run waits at the answers layer");
  assert.equal(layersOf({ state: "submitted", score: 250 })[1].w, 100);
  assert.equal(layersOf({ state: "submitted", score: -4 })[1].w, 0);
  assert.equal(layersOf({ state: "submitted", score: null })[1].w, 0);
  // the gate
  assert.equal(gateOf({ state: "submitted", approved: true, source: "ashby" }), "clear", "you approved it");
  assert.equal(gateOf({ state: "submitted", approved: false, source: "ashby" }), "dim", "it went out by itself");
  assert.equal(gateOf({ state: "submitted", approved: false, source: "manual" }), "hollow", "you sent it by hand");
  assert.equal(gateOf({ state: "prepared", approved: true, source: "ashby" }), "clear", "approved, waiting for the run");
  assert.equal(gateOf({ state: "prepared", approved: false, source: "ashby" }), "todo");
  assert.equal(gateOf({ state: "replied", approved: true, source: "manual" }), "hollow");
});

test("the frozen order, session after session: a pile that answers leaves a tombstone where it was and arrives at the end of its new lane", () => {
  fresh();
  const place = createPlace();
  const snap = fixture("rich");
  const first = view(snap, { place });
  const order = first.lanes.find((l) => l.id === "submitted").cells.map((c) => c.key);
  assert.deepEqual(order, ["fx:1", "fx:5", "fx:19", "fx:20"], "the oldest in the stage first");
  const repliedBefore = first.lanes.find((l) => l.id === "replied").cells.map((c) => c.key);
  // fx:5 answers
  const next = fixture("rich");
  const col = colOf(next, "submitted");
  const [moved] = col.cards.splice(col.cards.findIndex((c) => c.key === "fx:5"), 1);
  col.total -= 1;
  moved.state = "replied";
  moved.days_in_state = 0;
  colOf(next, "replied").cards.push(moved);
  colOf(next, "replied").total += 1;
  const after = view(next, { place });
  const submitted = after.lanes.find((l) => l.id === "submitted").cells;
  assert.deepEqual(submitted.map((c) => c.key || "~" + c.tomb), ["fx:1", "~fx:5", "fx:19", "fx:20"], "nobody slid up");
  assert.deepEqual(submitted[1], { tomb: "fx:5", to: "replied", text: "→ respondió" });
  assert.deepEqual(after.lanes.find((l) => l.id === "replied").cells.map((c) => c.key), [...repliedBefore, "fx:5"]);
  assert.deepEqual(after.moves, [{ key: "fx:5", from: "submitted", to: "replied" }]);
  // a tombstone is not a pile: it is not counted, searched or listed
  assert.equal(after.open, first.open);
  assert.equal(after.lanes.find((l) => l.id === "submitted").total, 3);
  // and under a filter the field is compacted (no tombstones, nothing to find them by)
  const filtered = view(next, { place, filter: "wait" });
  assert.ok(filtered.lanes.every((l) => l.cells.every((c) => c.tomb == null)));
  // a new page load forgets
  place.reset();
  const reloaded = view(next, { place });
  assert.ok(!reloaded.lanes.some((l) => l.cells.some((c) => c.tomb != null)));
});

test("a pile that ended leaves a tombstone that says how", () => {
  fresh();
  const place = createPlace();
  const snap = fixture("rich");
  view(snap, { place });
  const next = fixture("rich");
  const col = colOf(next, "submitted");
  const [gone] = col.cards.splice(col.cards.findIndex((c) => c.key === "fx:20"), 1);
  col.total -= 1;
  gone.state = "rejected";
  colOf(next, "rejected").cards.push(gone);
  colOf(next, "rejected").total += 1;
  const after = view(next, { place });
  const tomb = after.lanes.find((l) => l.id === "submitted").cells.find((c) => c.tomb);
  assert.deepEqual([tomb.tomb, tomb.to, tomb.text], ["fx:20", "rejected", "→ rechazó"]);
  assert.equal(after.closed.total, view(snap).closed.total + 1);
});

test("the filters are the four chips, f 1 to f 4: all, yours, waiting, no news; 'Por enviar' follows the ones that are about the live piles", () => {
  fresh();
  assert.deepEqual(FILTERS.map((f) => f.chord), ["f 1", "f 2", "f 3", "f 4"]);
  assert.deepEqual(KEYS.find((k) => k.id === "filter").keys, FILTERS.map((f) => f.chord), "the chips are the table's keys");
  assert.deepEqual(FILTERS.map((f) => f.id), ["all", "turn", "wait", "quiet"]);
  assert.deepEqual(["mine", "turn", "stale", "ghost", "quiet", "wait", "all", "nope", undefined].map(filterId), ["turn", "turn", "quiet", "quiet", "quiet", "wait", "all", "all", "all"]);
  const snap = fixture("rich");
  const keysOf = (v) => cellsOf(v).map((c) => c.key).sort();
  const turn = view(snap, { filter: "turn" });
  assert.deepEqual(keysOf(turn), ["fx:10", "fx:11", "fx:12", "fx:2", "fx:3", "fx:4", "fx:7", "fx:8", "fx:9"]);
  assert.ok(cellsOf(turn).every((c) => c.turn === "you"));
  const wait = view(snap, { filter: "wait" });
  assert.deepEqual(keysOf(wait), ["fx:1", "fx:19", "fx:20", "fx:5"], "the live ones that wait for the company; jobbot's own are not here");
  assert.deepEqual(keysOf(view(snap, { filter: "quiet" })), ["fx:6"]);
  assert.equal(view(snap, { filter: "turn" }).filters.find((f) => f.active).id, "turn");
  assert.equal(view(snap, { filter: "mine" }).filters.find((f) => f.active).id, "turn", "the old id of the shared filter still works");
  assert.equal(view(snap).found, 20 - 2 - 0, "the 18 piles of the field, closed ones are outside");
});

test("another order sorts inside each lane (age, score) and leaves the tombstones out; the stage order is the frozen one", () => {
  fresh();
  const snap = fixture("rich");
  assert.deepEqual(ORDERS, ["stage", "age", "score"]);
  const age = view(snap, { order: "age" }).lanes.find((l) => l.id === "submitted").cells.map((c) => c.days);
  assert.deepEqual(age, [...age].sort((a, b) => b - a));
  const score = view(snap, { order: "score" }).lanes.find((l) => l.id === "submitted").cells.map((c) => c.score);
  assert.deepEqual(score, [...score].sort((a, b) => b - a));
  assert.equal(view(snap, { order: "score" }).orders.find((o) => o.active).id, "score");
  assert.equal(view(snap, { order: "bogus" }).orders.find((o) => o.active).id, "stage");
});

test("the search narrows the field to what looks like the query and marks the letters; it composes with the filter", () => {
  fresh();
  const snap = fixture("rich");
  const v = view(snap, { query: "alpha" });
  assert.deepEqual(cellsOf(v).map((c) => c.key), ["fx:1"]);
  const parts = cellsOf(v)[0].hl;
  assert.equal(parts.company.map((p) => p.text).join(""), "Alpha Test Co");
  assert.ok(parts.company.some((p) => p.hit));
  assert.equal(v.found, 1);
  assert.deepEqual(cellsOf(view(snap, { query: "alpha", filter: "turn" })), [], "alpha is not his");
  assert.equal(view(snap, { query: "alpha", filter: "turn" }).empty.kind, "search");
  assert.equal(view(snap, { query: "zzzzqq" }).empty.text, "Nada se parece a «zzzzqq».");
  assert.equal(view(snap, { query: "" }).empty, null);
  assert.ok(cellsOf(view(snap, { query: "   " })).length > 10, "a blank query is no query");
});

test("the table: his first, then by age; each row says its last move, its next step, its rhythm and whose turn it is", () => {
  fresh();
  const snap = fixture("rich");
  const v = view(snap, { mode: "table" });
  assert.deepEqual(v.lanes, [], "the field is not built when the table is shown");
  const rows = v.table;
  assert.equal(rows.length, 18);
  const tiers = rows.map((r) => ({ you: 0, snoozed: 1, wait: 2, quiet: 3 })[r.turn]);
  assert.deepEqual(tiers, [...tiers].sort((a, b) => a - b), "yours, then the snoozed, then waiting, then no news");
  const yours = rows.filter((r) => r.turn === "you");
  assert.deepEqual(yours.map((r) => r.days), [...yours.map((r) => r.days)].sort((a, b) => b - a), "then by age inside the tier");
  const beta = rows.find((r) => r.key === "fx:2");
  assert.equal(beta.last, "Correo: «Can we talk on Wednesday?»", "the last mail of that application");
  assert.match(beta.next, /^Reply to the Beta/, "the task that is on it");
  assert.equal(beta.turnCell.kind, "you");
  assert.equal(beta.turnCell.key, "↵");
  const alpha = rows.find((r) => r.key === "fx:1");
  assert.equal(alpha.cadence, "seguimiento 2 en 3 d", "12 days: the first follow-up is behind, the second is 3 days away");
  assert.equal(alpha.next, "Esperar respuesta");
  assert.equal(alpha.last, "Enviada, sin respuesta");
  assert.equal(alpha.turnCell.text, "espera");
  assert.equal(rows.find((r) => r.key === "fx:6").cadence, "sin noticias, cierra en 24 d");
  assert.equal(rows.find((r) => r.key === "fx:12").turnCell.text, "Abrir");
  const reorder = view(snap, { mode: "table", order: "score" }).table.map((r) => r.score);
  assert.deepEqual(reorder, [...reorder].sort((a, b) => b - a));
});

test("the rhythm of a follow-up reaches the screen as words, day by day", () => {
  fresh();
  const at = (days, extra = {}) => {
    const snap = fixture("rich");
    Object.assign(card(snap, "fx:1"), { days_in_state: days, ...extra });
    return view(snap, { mode: "table" }).table.find((r) => r.key === "fx:1");
  };
  assert.deepEqual([at(2).cadence, at(2).next], ["seguimiento 1 en 6 d", "Esperar respuesta"]);
  assert.deepEqual([at(9).cadence, at(9).next], ["seguimiento 1, hoy: día 9 de 8–10", "Preguntar cómo va"]);
  assert.deepEqual([at(16).cadence, at(16).next], ["seguimiento 2, hoy: día 16 de 15–17", "Insistir una vez más"]);
  assert.equal(at(30).cadence, "cierra en 15 d");
  assert.deepEqual([at(45).cadence, at(45).next], ["sin noticias, día 45", "Darla por perdida"]);
  fresh("en");
  assert.equal(at(9).cadence, "follow-up 1, today: day 9 of 8–10");
  fresh();
});

test("empty, filtered out and nothing live: the page says which, with the closed ones counted when there are some", () => {
  fresh();
  const empty = view(fixture("empty"));
  assert.deepEqual(empty.empty, { kind: "none", text: "Ninguna postulación viva todavía." });
  assert.equal(empty.open, 0);
  const onlyClosed = fixture("rich");
  for (const state of [...LIVE, "scored", "prepared", "needs_gonza"]) { colOf(onlyClosed, state).cards = []; colOf(onlyClosed, state).total = 0; }
  onlyClosed.awaiting_approval = { total: 0, cards: [] };
  onlyClosed.needs_gonza = { total: 0, cards: [] };
  const v = view(onlyClosed);
  assert.equal(v.empty.kind, "none");
  assert.equal(v.empty.text, "Ninguna postulación viva. Las 2 cerradas están abajo.");
  const rich = fixture("rich");
  colOf(rich, "ghosted").cards = [];
  colOf(rich, "ghosted").total = 0;
  const filtered = view(rich, { filter: "quiet" });
  assert.deepEqual(filtered.empty, { kind: "filter", text: "Ninguna cumple este filtro." });
});

test("A15 and §9.4: the run is told from last_run_at only, and data older than ten minutes says how old it is", () => {
  fresh();
  const rich = fixture("rich");
  assert.match(view(rich).run.join(" · "), /última corrida hace 7 h/);
  assert.equal(view(rich).stale, null);
  const none = fixture("empty");
  assert.equal(none.controls.last_run_at, null);
  assert.ok(view(none).run.includes("sin corridas todavía"), "no run is invented when the engine never ran");
  const old = fixture("rich");
  const v = procesosView(model(old), { nowMs: nowOf(old) + STALE_DATA_MS + 60000, place: createPlace() });
  assert.match(v.stale, /^hace 11 min$/);
  const skew = fixture("rich");
  skew.controls.last_run_at = new Date(nowOf(skew) + 5 * 3600000).toISOString();
  for (const s of strings(view(skew).run)) assert.doesNotMatch(s, /NaN|undefined|null|−/);
});

test("hostile text stays text: control and bidi characters go, the long ones are cut, a link is only http(s), and the page never prints a missing value", () => {
  fresh();
  const snap = fixture("rich");
  const evil = card(snap, "fx:1");
  evil.company = "<img src=x onerror=alert(1)>" + "\u202eevil\n" + "A".repeat(300);
  evil.title = "javascript:alert(1)\u0000" + "B".repeat(500);
  evil.url = "javascript:alert(1)";
  const quiet = card(snap, "fx:5");
  quiet.company = null;
  quiet.title = undefined;
  quiet.score = null;
  quiet.days_in_state = "x";
  quiet.url = "data:text/html,<b>x</b>";
  const v = view(snap, { mode: "table" });
  const row = v.table.find((r) => r.key === "fx:1");
  assert.ok(row.company.length <= 60 && row.title.length <= 90);
  assert.doesNotMatch(row.company + row.title, /[\u0000-\u001f\u202e]/);
  assert.equal(row.link, null, "javascript: is not a link");
  assert.ok(!row.actions.some((a) => a.id === "link"), "so there is no verb for it");
  const odd = v.table.find((r) => r.key === "fx:5");
  assert.deepEqual([odd.company, odd.title, odd.score, odd.days], ["", "", 0, 0]);
  assert.equal(odd.link, null);
  for (const lang of ["es", "en"]) {
    fresh(lang);
    for (const name of ["empty", "rich", "heavy"]) {
      const s = fixture(name);
      for (const mode of ["field", "table"]) for (const text of strings(view(s, { mode, showClosed: true }))) assert.ok(!/⟦|NaN|undefined|\bnull\b/.test(text), `${lang}/${name}/${mode}: ${text.slice(0, 80)}`);
    }
  }
  fresh();
});

test("a pile's verbs are the ones the server allows, in the shape HOY's actions run: open always, the posting only if it is http(s), and withdraw, register and confirm by the card's own marks and actions", () => {
  fresh();
  const snap = fixture("rich");
  const cells = Object.fromEntries(cellsOf(view(snap)).map((c) => [c.key, c]));
  assert.deepEqual(cells["fx:1"].actions.map((a) => a.id), ["open", "link", "snooze", "withdraw", "register"]);
  assert.deepEqual(cells["fx:9"].actions.map((a) => a.id), ["open", "link", "snooze", "confirm"], "a stuck form: he may say he sent it");
  assert.equal(cells["fx:9"].actions.find((a) => a.id === "confirm").permanent, true, "and that takes two presses");
  assert.deepEqual(cells["fx:17"], undefined, "a closed one is not in the field");
  assert.deepEqual(cells["fx:7"].actions.map((a) => a.id), ["open", "link", "snooze", "withdraw", "register"]);
  assert.equal(cells["fx:1"].id, "card:fx:1");
  assert.deepEqual(cells["fx:1"].data, { card: "fx:1" });
  assert.equal(cells["fx:1"].open, "fx:1");
  assert.equal(keyDisplay("open"), "↵");
  assert.equal(keyDisplay("snooze"), "s");
  assert.throws(() => keyDisplay("nope"), /not an action/);
});

test("closed applications stay outside the field until they are asked for; then the server's 25 per column come with the real total", () => {
  fresh();
  const snap = fixture("rich");
  assert.deepEqual(view(snap).closed.items, []);
  const v = view(snap, { showClosed: true });
  assert.deepEqual(v.closed.items.map((c) => c.key).sort(), ["fx:17", "fx:18"]);
  assert.equal(v.closed.more, 0);
  const big = fixture("rich");
  colOf(big, "rejected").total = 40;
  const w = view(big, { showClosed: true });
  assert.equal(w.closed.total, 41);
  assert.equal(w.closed.more, 41 - 2, "the panel brings what the server listed");
});

test("heavy: 74 live piles in six lanes, 3440 and 1920 alike, and the eyebrow has the hour", () => {
  fresh();
  const snap = fixture("heavy");
  const v = view(snap);
  assert.equal(v.lanes.find((l) => l.id === "submitted").total, 45);
  assert.equal(v.lanes.find((l) => l.id === "ghosted").total, 9);
  assert.equal(cellsOf(v).length, 74 + 15, "the 74 live ones and the 15 that have not gone out");
  assert.match(v.eyebrow, /^Procesos: cinco capas por postulación, \d\d:\d\d$/);
  assert.equal(procesosView(model(snap), { nowMs: nowOf(snap), place: createPlace(), mode: "table" }).eyebrow.startsWith("Procesos en tabla, "), true);
  assert.ok(v.pieces.length >= 3);
  assert.ok(v.pieces[0].gold, "what is his is the one piece in gold");
});

test("J2-3: rowViewOf builds from the model the row the list's keys run, for an application the list does not draw; a closed or unknown one has none", () => {
  fresh();
  const snap = fixture("rich");
  const m = model(snap);
  const now = nowOf(snap);
  const drawn = cellsOf(view(snap)).find((c) => c.key === "fx:2");
  assert.deepEqual(rowViewOf(m, "fx:2", now), drawn, "the same row the field draws");
  assert.equal(rowViewOf(m, "fx:2", now).id, "card:fx:2");
  assert.deepEqual(rowViewOf(m, "fx:2", now).data, { card: "fx:2" });
  assert.ok(rowViewOf(m, "fx:2", now).actions.some((a) => a.id === "register"));
  assert.ok(!cellsOf(view(snap, { filter: "quiet" })).some((c) => c.key === "fx:2"), "a filter hides it from the list");
  assert.equal(rowViewOf(m, "fx:17", now), null, "a closed application is not in the field");
  assert.equal(rowViewOf(m, "nope", now), null);
  assert.equal(rowViewOf(null, "fx:2", now), null, "no model, no row");
});

test("J2-4: a closed application shown with 'mostrar cerradas' can only be opened (and followed to its posting), never put off", () => {
  fresh();
  const snap = fixture("rich");
  const v = view(snap, { showClosed: true });
  assert.deepEqual(v.closed.items.map((c) => c.key).sort(), ["fx:17", "fx:18"]);
  for (const item of v.closed.items) assert.deepEqual(item.actions, [{ id: "open", permanent: false }, { id: "link", permanent: false }], item.key);
  card(snap, "fx:18").url = "javascript:alert(1)";
  const unsafe = view(snap, { showClosed: true }).closed.items.find((c) => c.key === "fx:18");
  assert.deepEqual(unsafe.actions.map((a) => a.id), ["open"], "no valid address, no link");
  assert.ok(cellsOf(view(fixture("rich"))).every((c) => c.actions.some((a) => a.id === "snooze")), "the live ones keep theirs");
});

test("J2-5: the number on the 'Te tocan' chip is the number of piles the list shows when it is pressed, 'Por enviar' ones of his included", () => {
  fresh();
  const snap = fixture("rich");
  const v = view(snap, { filter: "turn" });
  const chip = v.filters.find((f) => f.id === "turn");
  assert.equal(chip.n, v.found, "the chip counts what the list will show");
  assert.equal(chip.n, cellsOf(v).length);
  assert.equal(chip.n, 9, "5 live ones plus the 4 of 'Por enviar' that are his (fx:9-12)");
  assert.equal(view(snap).filters.find((f) => f.id === "turn").n, 9, "the same number before it is pressed");
  assert.equal(v.head.you, 5, "the piece of the head and head.you still count the live ones");
  assert.match(v.pieces.find((p) => p.id === "you").text, /^5 /);
  assert.deepEqual(v.filters.filter((f) => f.id !== "turn").map((f) => f.n), [v.open, v.head.wait, v.head.quiet], "the others do not move");
});

test("J2-6: 'Por enviar' says the real total of the column the server cut to 25, never less than what is drawn", () => {
  fresh();
  const snap = fixture("rich");
  const scored = colOf(snap, "scored");
  const template = scored.cards[0];
  scored.cards = Array.from({ length: 30 }, (_, i) => ({ ...template, key: `sc:${i}`, score: 99 - i }));
  scored.total = 80;
  const others = colOf(snap, "prepared").total + colOf(snap, "needs_gonza").total;
  const v = view(snap);
  const lane = v.lanes.find((l) => l.id === "presend");
  assert.equal(lane.total, 80 + others, "the lane header counts all of them");
  assert.equal(lane.cells.length, 30 + others, "what is drawn is what came");
  assert.equal(v.head.presend, 80 + others);
  assert.match(v.pieces.find((p) => p.id === "presend").text, new RegExp(`^${80 + others} `));
  scored.total = 3; // a total that disagrees with the cards never goes below them
  assert.equal(view(snap).lanes.find((l) => l.id === "presend").total, 30 + others);
});

test("J2-6: a scored application pushed out of the server's cut of 25 by a better one is not a tombstone; any other exit still is", () => {
  fresh();
  const cut = (keys, total) => {
    const snap = fixture("rich");
    const scored = colOf(snap, "scored");
    const template = scored.cards[0];
    scored.cards = keys.map((k, i) => ({ ...template, key: k, score: 90 - i }));
    scored.total = total;
    return snap;
  };
  const first = Array.from({ length: 25 }, (_, i) => `sc:${i + 1}`);
  const second = ["sc:0", ...first.slice(0, 24)]; // sc:0 scores higher and pushes sc:25 out of the cut
  const tombs = (v) => v.lanes.flatMap((l) => l.cells).filter((c) => c.tomb != null);
  const place = createPlace();
  view(cut(first, 40), { place });
  const after = view(cut(second, 41), { place });
  assert.deepEqual(tombs(after), [], "it is still alive, only not listed");
  assert.deepEqual(after.moves, [], "and it does not travel anywhere");
  assert.ok(!after.lanes.find((l) => l.id === "presend").cells.some((c) => c.key === "sc:25"));
  // the control: the same disappearance from a column the server did NOT cut is an exit
  const place2 = createPlace();
  view(cut(first, 25), { place: place2 });
  const gone = view(cut(first.slice(0, 24), 24), { place: place2 });
  assert.deepEqual(tombs(gone).map((c) => [c.tomb, c.to, c.text]), [["sc:25", null, "→ fuera"]]);
  // and one that is truly somewhere else (it was sent) keeps the tombstone that says where
  const place3 = createPlace();
  view(cut(first, 40), { place: place3 });
  const sent = cut(second, 41);
  colOf(sent, "submitted").cards.push({ ...colOf(sent, "scored").cards[0], key: "sc:25", state: "submitted" });
  colOf(sent, "submitted").total += 1;
  const moved = view(sent, { place: place3 });
  assert.deepEqual(tombs(moved).map((c) => [c.tomb, c.to]), [["sc:25", "submitted"]]);
});
