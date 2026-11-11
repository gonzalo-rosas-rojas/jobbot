import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { setLang, setTimeZone, t } from "../src/i18n/index.js";
import { model } from "../src/core/model.js";
import { sistemaView, ENGINE_STALE_HOURS, STEP_IDS, nightOf } from "../src/moments/sistema/view.js";
import { KEYS } from "../src/ui/keys.js";
import { tokensOf, contrast } from "./helpers/color.mjs";
import { scenario, clone, withRun, STEPS_OK, SKIPS, skipped, HOUR, MIN } from "./helpers/sistema-scenarios.mjs";

const NIGHT = Date.parse("2026-10-08T06:00:00Z");               // 03:00 in Santiago
const view = (snap, opts = {}) => sistemaView(model(snap), { nowMs: Date.parse(snap.generated_at), ...opts });
const fresh = (lang = "es") => { setLang(lang); setTimeZone("America/Santiago"); };
const strings = (value, out = []) => {
  if (typeof value === "string") out.push(value);
  else if (Array.isArray(value)) value.forEach((v) => strings(v, out));
  else if (value && typeof value === "object") Object.values(value).forEach((v) => strings(v, out));
  return out;
};
// the four things a page must never say for a missing or odd value, and a negative time ("hace -3 h")
const SAYS_NOTHING = /\b(?:NaN|null|undefined)\b|⟦|hace -\d|-\d+ ?h\b|Invalid Date/;
const clean = (v, where) => { for (const s of strings(v)) assert.doesNotMatch(s, SAYS_NOTHING, `${where}: "${s}"`); };
const withControls = (snap, patch) => { const s = clone(snap); Object.assign(s.controls, patch); return s; };
const stepOf = (v, id) => v.run.steps.find((s) => s.id === id);

test("the three values of engine tell the three truths: ran 56 min ago, missed a night, never ran (A15)", () => {
  fresh();
  const recent = view(scenario("night"));
  assert.equal(recent.engine, "recent");
  assert.deepEqual([recent.head.lead, recent.head.tail], ["Última corrida", "hace 56 min."]);
  assert.equal(recent.chip.text, "Motor al día");
  assert.equal(recent.eyebrow, "¿Está vivo el motor y con qué cuenta corre? 03:00");
  const stale = view(scenario("stale"));
  assert.equal(stale.engine, "stale");
  assert.deepEqual([stale.head.lead, stale.head.tail], ["Última corrida", "hace 41 h."]);
  assert.equal(stale.chip.text, `Motor atrasado: más de ${ENGINE_STALE_HOURS} h sin correr`);
  const never = view(scenario("never"));
  assert.equal(never.engine, "never");
  assert.deepEqual([never.head.lead, never.head.tail], ["Sin corridas", "todavía."]);
  assert.equal(never.chip.text, "Motor sin corridas todavía");
  assert.equal(never.run.last, null, "no date is invented for a run that never happened");
  assert.equal(ENGINE_STALE_HOURS, 36, "the server's own limit (crates/jobbot-panel/src/snapshot.rs ENGINE_STALE_HOURS)");
});

test("the headline is worded from last_run_at alone: a run that is not a moment, or in the future, never reads as '-3 h'", () => {
  fresh();
  const future = view(withControls(scenario("night"), { last_run_at: "2026-10-08T09:00:00Z" }));
  assert.equal(future.head.tail, "ahora.", "a clock that is ahead says now, not minus three hours");
  const garbage = view(withControls(scenario("night"), { last_run_at: "ayer" }));
  assert.equal(garbage.head.tail, "sin fecha.");
  assert.equal(garbage.run.last, null);
  const missing = view(withControls(scenario("night"), { last_run_at: null }));
  assert.equal(missing.head.tail, "sin fecha.", "the engine says it ran but no date came: the date is not made up");
  const noEngine = view(withControls(scenario("night"), { engine: undefined, last_run_at: null }));
  assert.equal(noEngine.engine, "never", "an engine word that is not there is the careful one");
});

test("last_run_result, every value the Rust writes: ok, stopped, window_closed, error: <text>, and none", () => {
  fresh();
  const result = (r, extra = {}) => view(withRun(scenario("night"), { result: r, ...extra })).run.result;
  assert.deepEqual(result("ok"), { tone: "ok", text: "terminó bien", detail: null });
  assert.deepEqual(result("stopped"), { tone: "warn", text: "se detuvo por STOP", detail: null });
  assert.deepEqual(result("window_closed"), { tone: "warn", text: "se cerró la ventana a mitad de corrida", detail: null });
  assert.deepEqual(result("error: prepare terminó con código 2"), { tone: "bad", text: "terminó con un error", detail: "prepare terminó con código 2" });
  assert.deepEqual(result(undefined), { tone: "none", text: "sin registro del resultado: solo se vio un descubrimiento", detail: null }, "a stamp with no record is discovery's, not a run's");
  assert.equal(result("something new").tone, "none", "a word the page does not know is shown as it came, as plain text");
  assert.equal(result("something new").text, "something new");
  const long = result("error: " + "x".repeat(400));
  assert.ok(long.detail.length <= 160 && long.detail.endsWith("…"), "an error line is cut");
  assert.equal(result("error:   ").detail, null, "an empty error text shows no detail");
  assert.deepEqual(view(scenario("never")).run.result, { tone: "none", text: "sin dato", detail: null });
});

test("last_run_steps: what each of the four steps did, in order, with its reason when it did not run", () => {
  fresh();
  const v = view(scenario("night"));
  assert.deepEqual(v.run.steps.map((s) => s.id), STEP_IDS);
  assert.deepEqual(STEP_IDS, ["discover", "prepare", "submit", "dossiers"]);
  assert.deepEqual(v.run.steps.map((s) => s.label), ["Descubrir", "Preparar", "Enviar", "Dossiers"]);
  assert.deepEqual(v.run.steps.map((s) => s.text), ["3 avisos nuevos", "2 preparadas", "1 enviada, 1 para ti", "2 escritos"]);
  assert.ok(v.run.steps.every((s) => s.tone === "ran"));
  const one = view(withRun(scenario("night"), { steps: { discover: { ran: true, new: 1 }, prepare: { ran: true, prepared: 1 }, submit: { ran: true, submitted: 2, needs_gonza: 0 }, dossiers: { ran: true, written: 1 } } }));
  assert.deepEqual(one.run.steps.map((s) => s.text), ["1 aviso nuevo", "1 preparada", "2 enviadas, 0 para ti", "1 escrito"]);
  const said = { cooldown: "omitido: descubrió hace poco", stop: "omitido: STOP", window_closed: "omitido: se cerró la ventana", after_error: "omitido: un paso anterior falló", error: "con error", not_due: "no tocaba" };
  assert.deepEqual(SKIPS, Object.keys(said), "every reason of the Rust");
  for (const why of SKIPS) {
    const s = stepOf(view(withRun(scenario("night"), { steps: { ...STEPS_OK(), discover: skipped(why) } })), "discover");
    assert.equal(s.text, said[why], why);
    assert.equal(s.tone, why === "error" ? "bad" : "skip", why);
  }
  assert.equal(stepOf(view(withRun(scenario("night"), { steps: { ...STEPS_OK(), prepare: skipped("a new reason") } })), "prepare").text, "omitido (a new reason)", "a reason that came later still reads");
  const code = stepOf(view(withRun(scenario("night"), { steps: { ...STEPS_OK(), prepare: { ran: true, prepared: 1, code: 2 } } })), "prepare");
  assert.deepEqual([code.text, code.tone], ["1 preparada · terminó con código 2", "bad"]);
  // a record with a step missing, a count that is not a number, an entry that is not an object, and no record at all
  const odd = view(withRun(scenario("night"), { steps: { discover: { ran: true }, prepare: { ran: true, prepared: "x" }, submit: 7 } }));
  assert.deepEqual(odd.run.steps.map((s) => s.text), ["corrió", "corrió", "sin dato", "sin dato"]);
  assert.deepEqual(view(withRun(scenario("night"), { steps: undefined })).run.steps.map((s) => s.text), ["sin dato", "sin dato", "sin dato", "sin dato"]);
  assert.deepEqual(view(withRun(scenario("night"), { steps: [1, 2] })).run.steps.map((s) => s.text), ["sin dato", "sin dato", "sin dato", "sin dato"], "a list is not a record");
  clean(odd, "odd steps");
});

test("the scheduled task: null is 'sin dato' and never 'no existe'; exists:false is 'sin tarea programada'; exists:true says when (A15)", () => {
  fresh();
  const task = (value) => view(withControls(scenario("night"), { scheduled_task: value })).task;
  const unknown = task(null);
  assert.equal(unknown.state, "unknown");
  assert.equal(unknown.text, "sin dato: el panel no pudo preguntarle a Windows");
  assert.doesNotMatch(strings(unknown).join(" "), /no existe|sin tarea/i, "not knowing is not saying it is missing");
  assert.equal(task(undefined).state, "unknown", "a snapshot with no key at all knows nothing either");
  const missing = task({ exists: false, next_run_at: null });
  assert.deepEqual([missing.state, missing.text], ["missing", "sin tarea programada"]);
  assert.match(missing.hint, /scripts\/schedule\.ps1/);
  const registered = task({ exists: true, next_run_at: "2026-10-08T06:04:00Z" });
  assert.deepEqual([registered.state, registered.text], ["registered", "programada, próxima 03:04"]);
  assert.match(registered.hint, /cada hora/);
  assert.equal(task({ exists: true, next_run_at: null }).text, "programada");
  assert.equal(task({ exists: true, next_run_at: "mañana" }).text, "programada", "a next run that is not a moment is not shown");
  assert.equal(task("yes").state, "unknown", "a value of the wrong shape is no answer");
  assert.equal(task({ next_run_at: "2026-10-08T06:04:00Z" }).state, "unknown", "no `exists`, no answer");
  assert.equal(view(scenario("never")).task.state, "unknown", "the empty fixture: Windows was never asked");
  assert.equal(view(scenario("stale")).task.state, "missing");
  const tomorrow = task({ exists: true, next_run_at: "2026-10-09T06:04:00Z" });
  assert.match(tomorrow.text, /^programada, próxima 9 oct, 03:04$/, "another day says the day");
});

test("the account of Claude: verified, not verified with the server's reason and the way back, or never checked", () => {
  fresh();
  const ok = view(scenario("night")).account;
  assert.equal(ok.state, "ok");
  assert.equal(ok.text, "verificada");
  assert.equal(ok.who, "d***@example.com · suscripción max");
  assert.equal(ok.folder, "%LOCALAPPDATA%\\demo\\claude");
  assert.equal(ok.access, "claude.ai · firstParty · org org-demo");
  assert.equal(ok.version, "CLI 2.1.0 · comprobada hace 9 h");
  assert.equal(ok.fix, null);
  const bad = view(withControls(scenario("night"), { claude_account: { ok: false, message: "La sesión no es la cuenta propia: corre jobbot claude-login", subscription: null, email: null, checked_at: "2026-10-08T05:00:00Z", auth_method: null, api_provider: null, org_id: null, config_dir: null, cli_version: null } })).account;
  assert.equal(bad.state, "bad");
  assert.equal(bad.text, "no verificada");
  assert.equal(bad.detail, "La sesión no es la cuenta propia: corre jobbot claude-login");
  assert.match(bad.fix, /jobbot claude-login/);
  assert.deepEqual([bad.who, bad.folder, bad.access], [null, null, null], "what the server did not send is not shown");
  const none = view(scenario("never")).account;
  assert.equal(none.state, "none");
  assert.equal(none.text, "sin verificar todavía");
  assert.match(none.fix, /jobbot claude-check/);
  // never a whole address or an org id in full: the server masks, and the page does not trust that alone
  const whole = view(withControls(scenario("night"), { claude_account: { ...scenario("night").controls.claude_account, email: "alex@example.com", org_id: "0a1b2c3d-4e5f-6789-abcd-ef0123456789" } })).account;
  assert.equal(whole.who, "a***@example.com · suscripción max");
  assert.equal(whole.access, "claude.ai · firstParty · org 0a1b2c…6789");
  assert.doesNotMatch(JSON.stringify(whole), /alex@/);
});

test("figures: the next run and the sends of 24 h, every one from the model (A16), with 'sin dato' where there is none (the spend is the account's row)", () => {
  fresh();
  const night = view(scenario("night"));
  assert.deepEqual(night.figures.map((f) => [f.id, f.value, f.label]), [
    ["next", "03:04", "próxima corrida"], ["sends", "2 de 10", "envíos en 24 h"],
  ]);
  assert.ok(night.figures.every((f) => !f.words));
  const snap = scenario("night");
  snap.trust.sent_24h = 7;
  snap.trust.cap = 25;
  const moved = view(snap);
  assert.deepEqual(moved.figures.map((f) => f.value), ["03:04", "7 de 25"]);
  const never = view(scenario("never"));
  assert.deepEqual(never.figures.map((f) => [f.value, f.words]), [["sin dato", true], ["sin dato", true]]);
  const stale = view(scenario("stale"));
  assert.deepEqual([stale.figures[0].value, stale.figures[0].words], ["sin tarea", true], "exists:false says it");
  const tomorrow = view(withControls(scenario("night"), { scheduled_task: { exists: true, next_run_at: "2026-10-09T06:04:00Z" } }));
  assert.equal(tomorrow.figures[0].label, "próxima corrida, 9 oct");
});

test("the window of the night: 18:00 to 09:00 with the last run, the next one, now and the postings read, as marks at their real place", () => {
  fresh();
  const night = nightOf(NIGHT);
  assert.equal(Date.parse(new Date(night.from).toISOString()), Date.parse("2026-10-07T21:00:00Z"), "18:00 of the evening before, in Santiago");
  assert.equal(night.to, Date.parse("2026-10-08T12:00:00Z"), "09:00");
  assert.equal(night.tonight, true);
  assert.equal(nightOf(Date.parse("2026-10-08T15:00:00Z")).tonight, false, "at noon the night that counts is the one that ended");
  assert.equal(nightOf(Date.parse("2026-10-08T15:00:00Z")).from, night.from);
  assert.equal(nightOf(Date.parse("2026-10-08T22:00:00Z")).from, Date.parse("2026-10-08T21:00:00Z"), "at 19:00 it is the night that has just begun");
  const v = view(scenario("night")).strip;
  assert.equal(v.title, "Avisos por media hora: esta noche, de 18:00 a 09:00");
  assert.deepEqual(v.hours.map((h) => h.label), ["18", "21", "00", "03", "06", "09"]);
  assert.deepEqual(v.hours.map((h) => Math.round(h.pct)), [0, 20, 40, 60, 80, 100]);
  const marks = Object.fromEntries(v.marks.map((m) => [m.id, m]));
  assert.deepEqual(Object.keys(marks).sort(), ["last", "next", "now"]);
  assert.equal(marks.last.label, "última 02:04");
  assert.ok(Math.abs(marks.last.pct - (8 + 4 / 60) / 15 * 100) < 0.01, `02:04 sits at ${marks.last.pct}`);
  assert.equal(marks.next.label, "próxima 03:04");
  assert.equal(marks.now.pct, 60);
  assert.ok(v.bars.length > 0 && v.bars.every((b) => b.pct >= 0 && b.pct < 100 && b.h > 0 && b.h <= 1), "every bar is inside the night");
  assert.ok(Math.max(...v.bars.map((b) => b.h)) === 1, "the busiest half hour is the full height");
  assert.equal(v.empty, null);
  // a run before the night began, and a task that is not due until tomorrow: no marks, and the strip says so
  const old = view(scenario("stale")).strip;
  assert.deepEqual(old.marks.map((m) => m.id), ["now"], "the last run was two days ago: it is not on this night");
  assert.equal(old.empty, "Ninguna corrida registrada en esta ventana.");
  assert.equal(view(scenario("never")).strip.empty, "Ninguna corrida registrada en esta ventana.");
  const day = view(scenario("day")).strip;
  assert.equal(day.title, "Avisos por media hora: anoche, de 18:00 a 09:00");
  assert.ok(!day.marks.some((m) => m.id === "now"), "at 12:00 'now' is not inside the window");
});

test("the limits and thresholds are read only, from the model, and say how to change them honestly", () => {
  fresh();
  const v = view(scenario("night")).limits;
  assert.deepEqual(v.rows.map((r) => r.id), ["sends", "score", "auto", "batch"]);
  assert.equal(v.rows[0].value, "2 de 10");
  assert.equal(v.rows[0].sub, "llevas 3 de 5 aprobaciones; después, hasta 25");
  assert.equal(v.rows[1].value, "60");
  assert.equal(v.rows[3].value, "sin dato", "no value for the batch threshold is made up");
  assert.match(v.rows[3].sub, /preferencia de este navegador/, "the batch threshold is a preference of the browser, not a setting of the server");
  assert.equal(v.note, undefined, "the way to change them is in the title of the block, not a paragraph more");
  assert.match(t("sistema.limits.ro"), /Solo lectura.*profile\/\*\.toml/);
  const set = view(scenario("night"), { batchMin: 85 }).limits.rows[3];
  assert.deepEqual([set.value, set.sub], ["85", "preferencia de este navegador: lo que Shift A aprueba en COLA"]);
  const snap = scenario("night");
  snap.trust.review_phase = false;
  snap.trust.reviewed = 5;
  assert.equal(view(snap).limits.rows[0].sub, "ya revisaste las primeras 5");
  snap.trust = null;
  snap.pipeline.threshold = null;
  const none = view(snap).limits.rows;
  assert.deepEqual(none.slice(0, 2).map((r) => r.value), ["sin dato", "sin dato"]);
  clean(none, "no trust");
});

test("the sources of the postings: counts per board, never a rate with fewer than ten sent (A16)", () => {
  fresh();
  const v = view(scenario("night")).sources;
  assert.deepEqual(v.rows.map((r) => [r.name, r.n, r.sent, r.answered]), [["greenhouse", 20, 16, 5], ["ashby", 12, 9, 3], ["lever", 9, 6, 1]]);
  assert.equal(v.week, "7 días: respondieron 3 de 9 enviadas");
  assert.match(v.note, /Conteos, no tasas/);
  const many = scenario("night");
  many.metrics.sent_week = 12;
  many.metrics.responses_week = 3;
  many.metrics.response_rate = 0.25;
  assert.equal(view(many).sources.week, "7 días: respondieron 3 de 12 enviadas · 25%", "with ten or more the rate may be said");
  const never = view(scenario("never")).sources;
  assert.deepEqual([never.rows.length, never.week], [0, null]);
  assert.equal(never.empty, "Todavía no hay envíos por tablón.");
  const wide = scenario("night");
  wide.funnel.by_source = Object.fromEntries(Array.from({ length: 11 }, (_, i) => [`board${String(i).padStart(2, "0")}`, { n: 20 - i, sent: 5, answered: 1, tests: 0, interviews: 0, offers: 0 }]));
  const cut = view(wide).sources;
  assert.equal(cut.rows.length, 8);
  assert.equal(cut.moreLabel, "y 3 más");
  const odd = scenario("night");
  odd.funnel.by_source = { weird: { n: "x", sent: null }, "": {}, ok: { n: 3, sent: 2, answered: 0 } };
  const oddView = view(odd).sources;
  clean(oddView, "odd sources");
  assert.ok(oddView.rows.every((r) => Number.isInteger(r.n) && Number.isInteger(r.sent) && Number.isInteger(r.answered)));
});

test("the auto-approval threshold: from the model only (null is off, a whole 1..100 is 'desde N puntos', anything else is 'sin dato'), read only, and honest that the engine does not use it yet", () => {
  fresh();
  const auto = (value, opts = {}) => {
    const s = scenario("night");
    if (value === undefined) delete s.controls.auto_approve_min; else s.controls.auto_approve_min = value;
    return view(s, opts).limits.rows.find((r) => r.id === "auto");
  };
  assert.equal(auto(null).label, "Auto-aprobación");
  assert.equal(auto(null).value, "apagada");
  for (const n of [73, 100, 2]) assert.equal(auto(n).value, `desde ${n} puntos`, "the number is the model's, never a literal of the page (A16)");
  assert.equal(auto(1).value, "desde 1 punto");
  for (const bad of [0, 101, -5, 7.5, "85", true, {}, [], NaN, Infinity]) assert.equal(auto(bad).value, "sin dato", `${JSON.stringify(bad)} is not a threshold`);
  assert.equal(auto(undefined).value, "sin dato", "a server that does not send it says nothing");
  for (const v of [null, 85, undefined]) assert.match(auto(v).sub, /jobbot auto-approve.*aún no la usa el motor/, "how it is changed, and that the engine does not read it yet");
  const kinds = view(scenario("night")).limits.rows.map((r) => r.id);
  assert.deepEqual(kinds, ["sends", "score", "auto", "batch"], "the batch threshold stays its own row");
  setLang("en");
  assert.equal(auto(null).label, "Auto-approval");
  assert.equal(auto(null).value, "off");
  assert.equal(auto(85).value, "from 85 points");
  assert.equal(auto(1).value, "from 1 point");
  assert.match(auto(85).sub, /jobbot auto-approve.*not used by the engine yet/);
  fresh();
  clean(view(scenario("night")).limits, "limits");
  // A16: no figure is written into the dictionaries for these keys: every number comes in as a variable
  for (const lang of ["es", "en"]) {
    const text = readFileSync(new URL(`../src/i18n/${lang}/sistema.js`, import.meta.url), "utf8");
    const lines = text.split(/\r?\n/).filter((l) => /"sistema\.(limit\.auto|account\.(un)?anchored)/.test(l));
    assert.ok(lines.length >= 5, `${lang}: the keys exist`);
    for (const l of lines) assert.doesNotMatch(l.replace(/\{\w+\}/g, ""), /\d/, `${lang}: a literal figure in ${l.trim()}`);
  }
});

test("the anchoring of the account: unanchored is a real warning with the plain way out, anchored says so quietly, and what is not known is not said (null account, missing field)", () => {
  fresh();
  const acct = (patch) => { const s = scenario("night"); Object.assign(s.controls.claude_account, patch); return view(s).account; };
  const loose = acct({ anchored: false });
  assert.deepEqual(loose.anchor.state, "loose");
  assert.equal(loose.anchor.text, "cuenta sin anclar");
  assert.match(loose.anchor.why, /expected_email/);
  assert.match(loose.anchor.why, /expected_org_id/);
  assert.match(loose.anchor.why, /profile\/claude\.local\.toml/);
  assert.match(loose.anchor.why, /nunca el que se sube|nunca el commiteado|nunca el del repositorio/, "the local file, never the committed one");
  assert.match(loose.anchor.why, /se niega a correr con otra cuenta/);
  assert.equal(loose.state, "ok", "the account is still verified: it is the anchor that is missing");
  const anchored = acct({ anchored: true });
  assert.deepEqual([anchored.anchor.state, anchored.anchor.text], ["ok", "anclada"]);
  assert.equal(anchored.anchor.why, undefined);
  for (const odd of [undefined, null, "false", 0, 1, {}]) assert.equal(acct({ anchored: odd }).anchor, null, `${JSON.stringify(odd)} says nothing`);
  const none = view(scenario("never")).account;
  assert.equal(none.anchor, null, "no account verified, nothing to say about its anchor");
  assert.equal(none.text, "sin verificar todavía");
  const nullAcct = scenario("night");
  nullAcct.controls.claude_account = null;
  const noAccount = view(nullAcct).account;
  assert.deepEqual([noAccount.anchor, noAccount.text], [null, "sin verificar todavía"]);
  const bad = scenario("night");
  Object.assign(bad.controls.claude_account, { ok: false, message: "No es la cuenta propia", anchored: false });
  const badView = view(bad).account;
  assert.equal(badView.state, "bad");
  assert.equal(badView.anchor.state, "loose", "an account that failed the check and is not anchored says both");
  setLang("en");
  const en = acct({ anchored: false });
  assert.equal(en.anchor.text, "account not anchored");
  assert.match(en.anchor.why, /expected_email.*expected_org_id.*profile\/claude\.local\.toml/);
  assert.equal(acct({ anchored: true }).anchor.text, "anchored");
  fresh();
  clean(loose, "unanchored");
});

test("the alert colour of the warning is written in the tokens, is not gold, and keeps 7:1 on the night glass and on the paper", () => {
  const dark = tokensOf(":root {\n  color-scheme: dark;");
  const light = tokensOf(':root[data-theme="light"] {');
  assert.ok(dark["--alert"], "Tinta has an alert colour");
  assert.notEqual(dark["--alert"], dark["--gold"], "outside the gold range: gold is what is his to do");
  for (const ground of ["--night-0", "--night-1", "--night-2"]) assert.ok(contrast(dark["--alert"], dark[ground]) >= 7, `${ground}: ${contrast(dark["--alert"], dark[ground]).toFixed(2)}:1`);
  assert.ok(light["--ink-alert"] && light["--sheet-bg"], "Papel swaps the ink of the alert with the sheet");
  assert.ok(contrast(light["--ink-alert"], light["--sheet-bg"]) >= 7, `Papel: ${contrast(light["--ink-alert"], light["--sheet-bg"]).toFixed(2)}:1`);
});

test("show: when the sheet has no room, fewer boards and errors are listed and the rest is said as 'y N más'", () => {
  fresh();
  const wide = scenario("night");
  wide.funnel.by_source = Object.fromEntries(Array.from({ length: 6 }, (_, i) => [`board${i}`, { n: 20 - i, sent: 5, answered: 1 }]));
  const v = view(wide, { show: { sources: 3, errors: 1 } });
  assert.deepEqual([v.sources.rows.length, v.sources.moreLabel], [3, "y 3 más"]);
  assert.deepEqual([v.errors.rows.length, v.errors.total, v.errors.moreLabel], [1, 2, "y 1 más"], "the count of the title stays the whole one");
  assert.equal(view(wide, { show: { sources: 99, errors: 99 } }).sources.rows.length, 6, "never more than the page's own cut");
  for (const odd of [null, {}, { sources: 0, errors: -1 }, { sources: "3", errors: 1.5 }, { sources: NaN }]) {
    const w = view(wide, { show: odd });
    assert.equal(w.sources.rows.length, 6, JSON.stringify(odd));
    assert.equal(w.errors.rows.length, 2, JSON.stringify(odd));
  }
  clean(v, "show");
});

test("the errors of the night: the run's own error first, then what stopped for him since 24 h ago, cut at four", () => {
  fresh();
  const quiet = scenario("night");
  quiet.needs_gonza = { total: 0, cards: [] };
  const none = view(quiet).errors;
  assert.deepEqual([none.rows.length, none.total], [0, 0]);
  assert.equal(none.empty, "Ninguno en las últimas 24 horas.");
  assert.match(none.sources, /tablones que fallan/i, "the report of a board that did not answer is not saved: said, not guessed");
  const v = view(scenario("night")).errors;
  assert.equal(v.total, 2, "Iota (10:00Z, today) and Lambda (12:00Z, today) are inside the last 24 h; Kappa (a day before) is not");
  assert.ok(v.rows.every((r) => /^\d\d:\d\d$/.test(r.hour)));
  assert.deepEqual(v.rows.map((r) => r.open), ["fx:11", "fx:9"], "newest first, each one opens its posting");
  assert.equal(v.rows[0].why, "Una página me frenó (captcha, ingreso o bloqueo)");
  const failed = view(withRun(scenario("night"), { result: "error: Claude no se usó: sesión vencida", steps: { ...STEPS_OK(), dossiers: skipped("error") } })).errors;
  assert.equal(failed.rows[0].id, "run");
  assert.deepEqual([failed.rows[0].text, failed.rows[0].why], ["La última corrida terminó con un error", "Claude no se usó: sesión vencida"]);
  assert.equal(failed.total, 3);
  const heavy = view(scenario("heavy")).errors;
  assert.equal(heavy.rows.length, 4);
  assert.equal(heavy.moreLabel, `y ${heavy.total - 4} más`);
  assert.ok(heavy.total >= 8);
});

test("STOP: the state, the one verb and the key from the table; an intent that has not come back yet is shown at once", () => {
  fresh();
  const on = view(scenario("night")).stop;
  assert.deepEqual([on.on, on.action, on.label, on.key], [false, "stop", "Parar el motor", KEYS.find((k) => k.id === "engine").display]);
  assert.equal(on.text, "apagado");
  const stopped = view(scenario("stop")).stop;
  assert.deepEqual([stopped.on, stopped.action, stopped.label], [true, "resume", "Quitar STOP"]);
  assert.equal(stopped.text, "puesto: no corre nada automático hasta que lo quites");
  assert.equal(view(scenario("stop")).chip.stop, true);
  const optimistic = view(scenario("night"), { intent: true });
  assert.deepEqual([optimistic.stop.on, optimistic.stop.action], [true, "resume"], "the page answers in under 100 ms, before the snapshot does");
  assert.equal(view(scenario("stop"), { intent: false }).stop.on, false);
  assert.equal(view(scenario("night"), { intent: null }).stop.on, false);
  assert.ok(KEYS.some((k) => k.id === "engine" && k.stops.includes("sistema")), "the table has it for this stop (A19)");
});

test("the right of the eyebrow: the window and STOP from the snapshot, and how old the picture is when it is old", () => {
  fresh();
  assert.equal(view(scenario("night")).status, "Ventana de ejecución abierta");
  assert.equal(view(scenario("stop")).status, "STOP activo · Ventana de ejecución abierta");
  assert.equal(view(scenario("day")).status, "Ventana cerrada, abre 18:00");
  const s = scenario("night");
  assert.equal(sistemaView(model(s), { nowMs: Date.parse(s.generated_at) + 12 * MIN }).old, "Datos de hace 12 min");
  assert.equal(sistemaView(model(s), { nowMs: Date.parse(s.generated_at) + 3 * MIN }).old, null, "a poll is every 30 s: three minutes is still the live picture");
  assert.equal(view(withControls(scenario("night"), { window_open: null, next_window: null })).status, "Ventana de ejecución: sin datos");
});

test("a partial or empty snapshot never puts NaN, null, undefined, a minus-hours or a broken key on the page, in either language", () => {
  const trims = {
    "empty fixture": scenario("never"),
    "no controls keys": (() => { const s = scenario("night"); s.controls = { stopped: false }; return s; })(),
    "no controls at all": (() => { const s = scenario("night"); delete s.controls; return s; })(),
    "no claude, trust, funnel, metrics, pipeline, activity": (() => { const s = scenario("night"); for (const k of ["claude", "trust", "funnel", "metrics", "pipeline", "activity"]) delete s[k]; return s; })(),
    "nulls everywhere": (() => { const s = scenario("night"); s.claude = { spent_24h: null, daily_budget: null, by_stage: null }; s.funnel.by_source = null; s.metrics = { sent_week: null, responses_week: null, response_rate: null }; s.activity.slots = null; s.needs_gonza = null; return s; })(),
    "a stage with no name": (() => { const s = scenario("night"); s.claude.by_stage = [{ stage: null, calls: null, dollars: "x" }, {}]; return s; })(),
    "no needs_gonza cards": (() => { const s = scenario("night"); s.needs_gonza = { total: 3, cards: [{ key: "k" }, { key: "j", company: null, title: undefined, waiting_since: "no", reason_kind: "zzz" }] }; return s; })(),
  };
  for (const lang of ["es", "en"]) {
    fresh(lang);
    for (const [name, snap] of Object.entries(trims)) {
      let v;
      try { v = view(snap); } catch (e) { assert.fail(`${lang}/${name}: ${e.stack}`); }
      clean(v, `${lang}/${name}`);
      assert.ok(v.head.lead && v.chip.text && v.figures.length === 2 && v.run.steps.length === 4, `${lang}/${name}: the frame of the page is there`);
    }
  }
  fresh();
});

test("every text of every scenario is the active language's: Spanish and English say the same things and keep their own words", () => {
  for (const lang of ["es", "en"]) {
    fresh(lang);
    for (const name of ["night", "stale", "never", "stop", "heavy", "day"]) {
      const v = view(scenario(name));
      clean(v, `${lang}/${name}`);
    }
  }
  fresh("en");
  const v = view(scenario("night"));
  assert.deepEqual([v.head.lead, v.head.tail, v.chip.text, v.task.text], ["Last run", "56 min ago.", "Engine up to date", "scheduled, next 03:04"]);
  assert.equal(v.eyebrow, "Is the engine alive and which account does it run on? 03:00");
  fresh();
});

test("the view is pure: the same model and clock give the same view, and the snapshot is left as it was", () => {
  fresh();
  const snap = scenario("night");
  const before = JSON.stringify(snap);
  const a = view(snap);
  const b = view(snap);
  assert.deepEqual(a, b);
  assert.equal(JSON.stringify(snap), before);
  const s = readFileSync(new URL("../src/moments/sistema/view.js", import.meta.url), "utf8");
  assert.doesNotMatch(s, /\bdocument\.|\bwindow\.|Date\.now\(\)|new Date\(\)/, "no DOM and no clock of its own");
});
