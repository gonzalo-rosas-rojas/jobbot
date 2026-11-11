// The command palette (Ctrl K), its two pure halves: what is in it (ui/palette-items.js: the stops, the applications of the model, the settings) and
// how a query ranks it (ui/palette-rank.js: uFuzzy over the company and the role, highlight as parts, at most eight, and the honest line for a
// query that sounds like an action). Neither touches the DOM, so every case of the table is read here; the page itself is page.palette.browser.test.mjs.
import test from "node:test";
import assert from "node:assert/strict";
import { rankEntries, actionHint, MAX_RESULTS, GROUPS } from "../src/ui/palette-rank.js";
import { buildEntries, cardsOf, actionVerbs } from "../src/ui/palette-items.js";
import { soundEntries, stateWords } from "../src/ui/sound-entries.js";
import { t, setLang } from "../src/i18n/index.js";
import { STOPS_META } from "../src/moments/registry.js";

const card = (key, company, title, state = "submitted") => ({ key, company, title, state, score: 80 });
const snap = (board = [], lists = {}) => ({
  board: [{ state: "submitted", total: board.length, cards: board }],
  needs_gonza: { total: 0, cards: [] }, awaiting_approval: { total: 0, cards: [] }, snoozed: { total: 0, cards: [] }, ...lists,
});

const noRun = () => {};
const RUN = { go: noRun, job: noRun, theme: noRun, lang: noRun, sound: noRun, motion: noRun, help: noRun };
const STATE = { theme: "dark", lang: "es", sound: false, motion: "auto" };
const VERBS = [
  { stop: "cola", words: "aprobar aprueba approve saltar skip enviar send" },
  { stop: "sistema", words: "parar stop detener pausar pause reanudar resume" },
  { stop: "hoy", words: "posponer snooze redactar draft" },
];

function entries(cards, over = {}) {
  return buildEntries({ stops: STOPS_META, cards, current: "hoy", state: STATE, run: RUN, ...over });
}
const labels = (rows) => rows.map((r) => r.entry.label);
const text = (parts) => parts.map((p) => p.text).join("");

// ---- the cards of the model

test("cardsOf: every application of the board and of the three lists once, by key; a card with no name is left out; no snapshot is no cards", () => {
  const s = snap([card("a:1", "Alba Robotics", "Backend Engineer"), card("a:2", "Kestrel Labs", "Staff Engineer", "replied")], {
    needs_gonza: { total: 2, cards: [card("a:1", "Alba Robotics", "Backend Engineer", "needs_gonza"), card("a:3", null, null)] },
    awaiting_approval: { total: 1, cards: [card("a:4", "Mu Test Co", "Full-stack Engineer", "prepared")] },
    snoozed: { total: 1, cards: [card("a:5", "Pausa SA", null, "scored")] },
  });
  const got = cardsOf(s);
  assert.deepEqual(got.map((c) => c.key), ["a:1", "a:2", "a:4", "a:5"], "board first, then needs him, awaits his approval, snoozed; a:1 once; a:3 has no name");
  assert.deepEqual(got[3], { key: "a:5", company: "Pausa SA", title: "", state: "scored" }, "what is missing is an empty text, never null");
  for (const nothing of [null, undefined, {}, { board: null }, { board: [{ cards: null }], needs_gonza: null }, 7]) assert.deepEqual(cardsOf(nothing), [], JSON.stringify(nothing));
});

// ---- what is in it

test("with no query the palette is a menu: the six stops with their number, then the five settings, and no application", () => {
  setLang("es");
  const all = entries([card("a:1", "Alba Robotics", "Backend Engineer")]);
  assert.deepEqual(GROUPS, ["goto", "jobs", "prefs"]);
  const { rows } = rankEntries(all, "");
  assert.deepEqual(rows.map((r) => r.entry.group), ["goto", "goto", "goto", "goto", "goto", "goto", "prefs", "prefs", "prefs", "prefs", "prefs"]);
  assert.deepEqual(rows.slice(0, 6).map((r) => r.entry.id), ["go:prologo", "go:hoy", "go:procesos", "go:cola", "go:aprender", "go:sistema"]);
  assert.deepEqual(rows.slice(0, 6).map((r) => r.entry.key), ["0", "1", "2", "3", "4", "5"]);
  assert.deepEqual(labels(rows.slice(0, 6)), ["Anoche", "Hoy", "Procesos", "Cola", "Aprender", "Sistema"]);
  assert.deepEqual(rows.slice(6).map((r) => r.entry.id), ["pref:theme", "pref:lang", "pref:sound", "pref:motion", "pref:help"]);
  assert.deepEqual(rankEntries(all, "   ").rows.map((r) => r.entry.id), rows.map((r) => r.entry.id), "blanks are no query");
  assert.equal(rankEntries(all, "").hint, null);
  assert.equal(rows[1].entry.current, true, "the stop on screen says so");
  assert.equal(rows.filter((r) => r.entry.current).length, 1);
});

test("the settings say what they will do and what is now, in the language of the page, and the stops are named in it", () => {
  setLang("es");
  const es = Object.fromEntries(entries([]).filter((e) => e.group === "prefs").map((e) => [e.id, e]));
  assert.equal(es["pref:theme"].label, "Tema: claro");
  assert.match(es["pref:theme"].sub, /actual.*oscuro/);
  assert.equal(es["pref:lang"].label, "Idioma: English");
  assert.equal(es["pref:sound"].label, "Sonido: activar");
  assert.equal(es["pref:motion"].label, "Movimiento: calmo");
  assert.equal(es["pref:help"].key, "?");
  const on = Object.fromEntries(entries([], { state: { theme: "light", lang: "es", sound: true, motion: "calm" } }).filter((e) => e.group === "prefs").map((e) => [e.id, e]));
  assert.equal(on["pref:theme"].label, "Tema: oscuro");
  assert.equal(on["pref:sound"].label, "Sonido: silenciar");
  assert.equal(on["pref:motion"].label, "Movimiento: según el sistema", "calm is on, so the one thing to offer is to go back");
  setLang("en");
  try {
    const en = entries([card("a:1", "Alba Robotics", "Backend Engineer")], { state: { ...STATE, lang: "en" } });
    assert.deepEqual(en.filter((e) => e.group === "goto").map((e) => e.label), ["Last night", "Today", "Processes", "Queue", "Learn", "System"]);
    assert.equal(en.find((e) => e.id === "pref:lang").label, "Language: Español");
    assert.equal(en.find((e) => e.id === "pref:sound").label, "Sound: turn on");
  } finally { setLang("es"); }
});

test("an entry runs what the shell gave it and nothing else: stops travel, applications open, settings change; nothing here can approve, snooze, stop or resume", () => {
  const seen = [];
  const run = { go: (id) => seen.push(["go", id]), job: (key) => seen.push(["job", key]), theme: () => seen.push(["theme"]), lang: () => seen.push(["lang"]), sound: () => seen.push(["sound"]), motion: () => seen.push(["motion"]), help: () => seen.push(["help"]) };
  const all = entries([card("a:1", "Alba Robotics", "Backend Engineer")], { run });
  assert.deepEqual(seen, [], "building the list runs nothing");
  for (const e of all) e.run();
  assert.deepEqual(seen, [["go", "prologo"], ["go", "hoy"], ["go", "procesos"], ["go", "cola"], ["go", "aprender"], ["go", "sistema"], ["job", "a:1"], ["theme"], ["lang"], ["sound"], ["motion"], ["help"]]);
  assert.deepEqual([...new Set(all.map((e) => e.group))], ["goto", "jobs", "prefs"]);
  assert.equal(all.filter((e) => e.group === "prefs").length, 5);
  const words = all.map((e) => `${e.label} ${e.sub || ""}`).join(" ").toLowerCase();
  assert.doesNotMatch(words, /aprobar|posponer|\bstop\b|reanudar|detener|approve|snooze|resume/, "no entry is an action of the server");
});

test("the verbs that are done at each stop come from the dictionary, only for the stops that have some", () => {
  setLang("es");
  const verbs = actionVerbs(STOPS_META);
  assert.deepEqual(verbs.map((v) => v.stop), ["hoy", "procesos", "cola", "aprender", "sistema"], "the prologue has nothing to do");
  assert.match(verbs.find((v) => v.stop === "cola").words, /aprobar/);
  assert.match(verbs.find((v) => v.stop === "sistema").words, /stop/);
  assert.equal(actionHint("aprobar", verbs).stop, "cola");
  assert.equal(actionHint("STOP", verbs).stop, "sistema");
  assert.deepEqual(actionVerbs([]), []);
  assert.deepEqual(actionVerbs(null), []);
});

// ---- ranking

test("a company or a role finds the application, accents and capitals do not matter, and the match is parts that the page turns into marks", () => {
  const all = entries([card("a:1", "Ámbar Foods", "Rust Engineer"), card("a:2", "Kestrel Labs", "Staff Backend Engineer"), card("a:3", "Mu Test Co", "Full-stack Engineer")]);
  let { rows } = rankEntries(all, "KESTREL");
  assert.equal(rows[0].entry.id, "job:a:2", "the application comes up first");
  assert.deepEqual(rows[0].label, [{ text: "Kestrel", hit: true }, { text: " Labs", hit: false }]);
  assert.equal(text(rows[0].sub), "Staff Backend Engineer");
  ({ rows } = rankEntries(all, "ambar"));
  assert.deepEqual(rows[0].label, [{ text: "Ámbar", hit: true }, { text: " Foods", hit: false }], "the range is of the original letters, accent kept");
  ({ rows } = rankEntries(all, "rust"));
  const hit = rows.find((r) => r.entry.id === "job:a:1");
  assert.deepEqual(hit.sub, [{ text: "Rust", hit: true }, { text: " Engineer", hit: false }], "the role is highlighted too");
  ({ rows } = rankEntries(all, "kestrl"));
  assert.equal(rows[0].entry.id, "job:a:2", "one slip inside a word is forgiven");
  ({ rows } = rankEntries(all, "backend kestrel"));
  assert.equal(rows[0].entry.id, "job:a:2", "the words may come in any order");
});

test("a stop is found by its name, by its question and by the words people use for it; the label is highlighted only where the label matched", () => {
  const all = entries([]);
  let { rows } = rankEntries(all, "cola");
  assert.equal(rows[0].entry.id, "go:cola");
  assert.deepEqual(rows[0].label, [{ text: "Cola", hit: true }]);
  ({ rows } = rankEntries(all, "approve"));
  assert.equal(rows[0].entry.id, "go:cola", "an English word of the stop finds it in the Spanish page");
  assert.deepEqual(rows[0].label, [{ text: "Cola", hit: false }], "the word was in its aliases, not in its name: nothing of the name is marked");
  ({ rows } = rankEntries(all, "tema"));
  assert.ok(rows.some((r) => r.entry.id === "pref:theme"), "tinta and papel are found by the word 'tema' (and Sistema, which has it inside, comes first in its own group)");
  ({ rows } = rankEntries(all, "atajos"));
  assert.equal(rows.find((r) => r.entry.group === "prefs").entry.id, "pref:help");
  ({ rows } = rankEntries(all, "english"));
  assert.ok(rows.some((r) => r.entry.id === "pref:lang"), "the language is found by the other language's name");
});

test("at most eight rows, taken in turn from the stops, the applications and the settings, and shown in that order", () => {
  const many = Array.from({ length: 40 }, (_, i) => card("m:" + i, "Empresa " + i, "Software Engineer"));
  const all = entries(many);
  const { rows } = rankEntries(all, "e");
  assert.ok(MAX_RESULTS === 8 && rows.length <= 8, `${rows.length} rows`);
  const groups = rows.map((r) => r.entry.group);
  assert.deepEqual(groups, [...groups].sort((a, b) => GROUPS.indexOf(a) - GROUPS.indexOf(b)), "the order of the groups is the order of the table");
  for (const g of GROUPS) assert.ok(groups.includes(g), `${g} has a row in a crowd`);
  assert.equal(rankEntries(all, "empresa").rows.length, 8, "forty applications are eight rows");
});

test("no match is no rows and no hint; a query that breaks a regular expression or is markup is only letters", () => {
  const all = entries([card("a:1", "Alba Robotics", "Backend Engineer")]);
  assert.deepEqual(rankEntries(all, "zzzzqq"), { rows: [], hint: null });
  for (const q of ["(((", "[", "\\", "*+?", "<img src=x onerror=alert(1)>", "\"; DROP TABLE", "null", "undefined", "a".repeat(500)]) {
    assert.doesNotThrow(() => rankEntries(all, q), JSON.stringify(q.slice(0, 20)));
    const { rows } = rankEntries(all, q);
    for (const r of rows) for (const p of [...r.label, ...r.sub]) assert.equal(typeof p.text, "string");
  }
  assert.doesNotThrow(() => rankEntries(all, null));
  assert.doesNotThrow(() => rankEntries(all, undefined));
  assert.deepEqual(rankEntries([], "x"), { rows: [], hint: null });
});

test("what someone else wrote comes back whole: the parts of a hostile company put together are the company, tags and quotes included", () => {
  const hostile = "<img src=x onerror=\"window.__pwned=1\"> & Co";
  const all = entries([card("h:1", hostile, "</mark><script>alert(1)</script> Engineer")]);
  const { rows } = rankEntries(all, "img src");
  assert.equal(rows[0].entry.id, "job:h:1");
  assert.equal(text(rows[0].label), hostile);
  assert.equal(text(rows[0].sub), "</mark><script>alert(1)</script> Engineer");
  assert.ok(rows[0].label.some((p) => p.hit));
});

// ---- the honest line

test("a query that sounds like an action says where it is done, and the rows stay what they are", () => {
  for (const [q, stop] of [["aprobar", "cola"], ["Aprobar kestrel", "cola"], ["aprob", "cola"], ["approve", "cola"], ["stop", "sistema"], ["STOP", "sistema"], ["parar", "sistema"], ["reanudar el motor", "sistema"], ["posponer", "hoy"], ["snooze alba", "hoy"]]) {
    const hint = actionHint(q, VERBS);
    assert.equal(hint && hint.stop, stop, q);
    assert.equal(hint.word, q.trim().split(/\s+/)[0].toLowerCase(), "it names the word it heard");
  }
  for (const q of ["", "   ", "kestrel", "apr", "sta", "cola", "tema", "alba robotics", "empresa aprobar"]) assert.equal(actionHint(q, VERBS), null, JSON.stringify(q));
  assert.equal(actionHint("aprobar", []), null);
  assert.equal(actionHint(null, VERBS), null);
  const all = entries([card("a:1", "Alba Robotics", "Backend Engineer")]);
  const { rows, hint } = rankEntries(all, "aprobar alba", { verbs: VERBS });
  assert.equal(hint.stop, "cola");
  assert.ok(rows.some((r) => r.entry.id === "job:a:1"), "the verb is said, not searched: what follows it finds the application");
  assert.equal(rankEntries(all, "aprobar", { verbs: VERBS }).rows[0].entry.id, "go:cola", "the verb alone finds the stop where it is done");
});

// ---- the sound family (ui/sound-entries.js): one row in the menu, the rest waits for a word

const STATUS = (over = {}) => ({ enabled: false, mode: "off", actions: true, background: true, muted: false, muteUntil: 0, work: { on: true, now: false, from: "09:00", to: "18:00" }, level: "mid", volume: 0.25, ...over });
const at = () => "21:40";
const SOUND_RUN = (seen) => ({ master: () => seen.push("master"), bus: (id) => seen.push("bus:" + id), mute: (kind) => seen.push("mute:" + kind), work: () => seen.push("work"), volume: () => seen.push("volume"), listen: () => seen.push("listen") });
const withSound = (status, seen = []) => entries([], { soundEntries: soundEntries({ status, run: SOUND_RUN(seen), time: at }) });

test("sound: the menu with nothing typed keeps ONE sound row (the m key); the rest of the family waits for a word", () => {
  setLang("es");
  const all = withSound(STATUS());
  const { rows } = rankEntries(all, "");
  assert.deepEqual(rows.slice(6).map((r) => r.entry.id), ["pref:theme", "pref:lang", "pref:sound", "pref:motion", "pref:help"], "the same five settings as before");
  assert.equal(rows.find((r) => r.entry.id === "pref:sound").entry.key, "m");
  assert.deepEqual(all.filter((e) => e.id.startsWith("pref:sound")).map((e) => e.label), [
    "Sonido: encender", "Acciones: apagar", "Fondo: apagar", "Silenciar 1 hora", "Silenciar hasta mañana", "Sonido en horario laboral: permitir", "Volumen del sonido: alto", "Escuchar los sonidos",
  ]);
  for (const word of ["sonido", "silenciar", "fondo", "escuchar", "volumen", "laboral"]) {
    const ids = rankEntries(all, word).rows.map((r) => r.entry.id);
    assert.ok(ids.some((id) => id.startsWith("pref:sound")), `"${word}" finds the family: ${ids}`);
  }
  assert.equal(rankEntries(all, "silenciar 1").rows[0].entry.id, "pref:sound:mute1h");
});

test("sound: the labels say what each will do now (on, off, muted, working hours), in both languages, and run what the shell gave", () => {
  setLang("es");
  const seen = [];
  const on = withSound(STATUS({ enabled: true, mode: "all", level: "high" }), seen);
  assert.deepEqual(on.filter((e) => e.id.startsWith("pref:sound")).map((e) => e.label), [
    "Sonido: apagar", "Acciones: apagar", "Fondo: apagar", "Silenciar 1 hora", "Silenciar hasta mañana", "Sonido en horario laboral: permitir", "Volumen del sonido: bajo", "Escuchar los sonidos",
  ]);
  assert.match(on.find((e) => e.id === "pref:sound").sub, /acciones y fondo/);
  const quiet = withSound(STATUS({ enabled: true, mode: "muted", muted: true, muteUntil: 1 }));
  const labels2 = quiet.filter((e) => e.id.startsWith("pref:sound")).map((e) => e.label);
  assert.ok(labels2.includes("Quitar el silencio") && !labels2.includes("Silenciar 1 hora"), "while it is silent the one thing to offer is to end it: " + labels2);
  assert.match(quiet.find((e) => e.id === "pref:sound").sub, /silencio hasta 21:40/);
  const allowed = withSound(STATUS({ work: { on: false, now: true, from: "09:00", to: "18:00" } }));
  assert.equal(allowed.find((e) => e.id === "pref:sound:work").label, "Sonido en horario laboral: silenciar");
  assert.match(allowed.find((e) => e.id === "pref:sound:work").sub, /09:00 a 18:00/, "the hours it applies to are in the row");
  for (const e of on.filter((x) => x.id.startsWith("pref:sound"))) e.run();
  assert.deepEqual(seen, ["master", "bus:actions", "bus:background", "mute:hour", "mute:tomorrow", "work", "volume", "listen"]);
  setLang("en");
  try {
    const en = withSound(STATUS()).filter((e) => e.id.startsWith("pref:sound")).map((e) => e.label);
    assert.deepEqual(en, ["Sound: turn on", "Actions: turn off", "Background: turn off", "Silence for 1 hour", "Silence until tomorrow", "Sound in working hours: allow", "Sound volume: high", "Listen to the sounds"]);
  } finally { setLang("es"); }
});

test("sound: stateWords is the one phrase the indicator and the rows share", () => {
  setLang("es");
  assert.equal(stateWords(STATUS(), at), "apagado");
  assert.equal(stateWords(STATUS({ enabled: true, mode: "all" }), at), "acciones y fondo");
  assert.equal(stateWords(STATUS({ enabled: true, mode: "actions" }), at), "acciones");
  assert.equal(stateWords(STATUS({ enabled: true, mode: "background" }), at), "solo fondo");
  assert.equal(stateWords(STATUS({ enabled: true, mode: "none" }), at), "sin canales");
  assert.equal(stateWords(STATUS({ enabled: true, mode: "muted", muted: true, muteUntil: 5 }), at), "silencio hasta 21:40");
  assert.equal(stateWords(STATUS({ enabled: true, mode: "work" }), at), "mudo hasta 18:00");
});
