// Acceptance test of the encargo docs/encargos/lote-pregunta-toast-check-captura.md: small fixes of the panel in one run. (1) The eyebrow of
// HOY, COLA, APRENDER and SISTEMA asks the question the table of stops names (moments/registry.js, STOPS_META, its `ask`), not a key written
// again in each view; (2) the same action asked again while its request is out is one action for the person too: one toast and one run of
// its callbacks (where the sounds are), not two, also when the server refuses it and also in the demo; and only while the request is out:
// pressed again once it came back, it is a new action, as today; (3) the line of a test step in `npm run check` takes its counts from what the runner wrote to
// stdout, never from stderr appended after it, and a test step that passed with no summary says so. The capture's English word is pinned in
// test/i18n.glosario.test.mjs. No browser: views over the fixtures, a session over a fake server (and over a fake demo), check.mjs over a `node -e` child.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { setLang, setTimeZone, t } from "../src/i18n/index.js";
import { model } from "../src/core/model.js";
import { createSession } from "../src/core/session.js";
import { STOPS_META } from "../src/moments/registry.js";
import { hoyView } from "../src/moments/hoy/view.js";
import { colaView } from "../src/moments/cola/view.js";
import { aprenderView } from "../src/moments/aprender/view.js";
import { sistemaView } from "../src/moments/sistema/view.js";
import { richAprender } from "./helpers/aprender-fixture.mjs";

const check = await import("../tools/check.mjs");

const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/snapshot.${name}.json`, import.meta.url), "utf8"));
const nowOf = (snap) => ({ nowMs: Date.parse(snap.generated_at) });
const fresh = (lang) => { setLang(lang); setTimeZone("America/Santiago"); };

// ---- (1) the question of each stop comes from the table of stops

const EYEBROWS = {
  hoy: () => { const s = fixture("rich"); return hoyView(model(s), nowOf(s)).eyebrow; },
  cola: () => { const s = fixture("rich"); return colaView(model(s), nowOf(s)).eyebrow; },
  aprender: () => { const s = richAprender(); return aprenderView(model(s), nowOf(s)).eyebrow; },
  sistema: () => { const s = fixture("rich"); return sistemaView(model(s), nowOf(s)).eyebrow; },
};

test("stop questions: the eyebrow of HOY, COLA, APRENDER and SISTEMA asks the question that the table of stops names for it", () => {
  for (const lang of ["es", "en"]) {
    fresh(lang);
    for (const [id, eyebrowOf] of Object.entries(EYEBROWS)) {
      const meta = STOPS_META.find((s) => s.id === id);
      assert.ok(meta, `${id} is in STOPS_META`);
      assert.ok(eyebrowOf().startsWith(t(meta.ask)), `${lang}, ${id}: the eyebrow starts with the stop's question`);
      // Another key in the table, for a moment: a view that writes its own stop.<id>.ask again keeps the old question and is caught here.
      const own = meta.ask;
      meta.ask = "stop.prologo.ask";
      try {
        const eyebrow = eyebrowOf();
        assert.ok(eyebrow.startsWith(t("stop.prologo.ask")), `${lang}, ${id}: "${eyebrow}" asks what STOPS_META says the stop asks (${meta.ask})`);
        assert.ok(!eyebrow.includes(t(own)), `${lang}, ${id}: the view does not name ${own} itself`);
      } finally {
        meta.ask = own;
      }
    }
  }
});

// ---- (2) an action asked twice while its request is out: one toast, one run of its callbacks, and only while the request is out

const SNAPSHOT = fixture("rich");
const answer = (status, body) => ({ ok: status >= 200 && status < 300, status, json: async () => structuredClone(body), blob: async () => new Blob(["png"]) });
const settle = () => new Promise((resolve) => setImmediate(resolve));
const DONE = { status: 200, body: { ok: true, message: "Hecho en el servidor." } };

/**
 * A session over a fake server that records every POST (its body's text) and every toast. While `server.hold` is a promise, every POST
 * waits for it; `server.reply` is what a POST answers ({ status, body }).
 */
function rig() {
  setLang("es");
  const server = { posts: [], hold: null, reply: DONE, toasts: [] };
  const fetchImpl = async (path, init = {}) => {
    if (String(init.method || "GET").toUpperCase() !== "POST") return answer(200, path.startsWith("/api/snapshot") ? SNAPSHOT : { ok: true });
    server.posts.push(JSON.parse(init.body).text);
    if (server.hold) await server.hold;
    return answer(server.reply.status, server.reply.body);
  };
  const session = createSession({ fetchImpl, token: "tok", requestTimeoutMs: 1000, freshMs: 1 });
  session.subscribe((event) => { if (event.type === "toast") server.toasts.push(event.toast); });
  return { server, session };
}

/** A promise and the function that settles it. */
const gate = () => { let open; const p = new Promise((resolve) => { open = resolve; }); return { p, open }; };

test("session: the same action asked again while its request is out says its toast once and runs its callbacks once, and asked later it is a new action", async () => {
  const { server, session } = rig();
  const calls = { onOk: [], beforeRefresh: [] };
  const opts = (text) => ({ onOk: () => calls.onOk.push(text), beforeRefresh: () => { calls.beforeRefresh.push(text); } });
  const held = gate();
  server.hold = held.p;
  const first = session.act("/api/task/add", { text: "Llamar" }, opts("Llamar"));
  const twice = session.act("/api/task/add", { text: "Llamar" }, opts("Llamar"));
  const other = session.act("/api/task/add", { text: "Escribir" }, opts("Escribir"));
  await settle();
  server.hold = null;
  held.open();
  const results = await Promise.all([first, twice, other]);
  assert.deepEqual([...server.posts].sort(), ["Escribir", "Llamar"], "the double click is one request (the session already joins it)");
  for (const r of results) assert.deepEqual([r.ok, r.message], [true, "Hecho en el servidor."], "every caller gets the answer");
  assert.equal(server.toasts.length, 2, `one toast per action, not per press: ${JSON.stringify(server.toasts)}`);
  assert.deepEqual(calls.beforeRefresh.sort(), ["Escribir", "Llamar"], "the callbacks that play the sound run once per action");
  assert.deepEqual(calls.onOk.sort(), ["Escribir", "Llamar"], "and so does onOk (the toast with Deshacer)");
  // the first one came back and was read again: asking the same thing now is a new action, never the old answer
  const again = await session.act("/api/task/add", { text: "Llamar" }, opts("Llamar"));
  session.stop();
  assert.deepEqual(server.posts.filter((text) => text === "Llamar"), ["Llamar", "Llamar"], "asked again after it came back: a new POST");
  assert.equal(again.ok, true);
  assert.equal(server.toasts.length, 3, "and a toast of its own");
  assert.equal(calls.beforeRefresh.filter((text) => text === "Llamar").length, 2, "and its own sound");
});

test("session: a double click on an action the server refuses says one error toast and plays one error sound", async () => {
  const { server, session } = rig();
  server.reply = { status: 409, body: { ok: false, message: "Ya estaba cerrada." } };
  const sounds = [];
  const opts = { beforeRefresh: (r) => { sounds.push(r.ok ? "taskdone" : "error"); } };
  const held = gate();
  server.hold = held.p;
  const first = session.act("/api/task/add", { text: "Cerrar" }, opts);
  const twice = session.act("/api/task/add", { text: "Cerrar" }, opts);
  await settle();
  server.hold = null;
  held.open();
  const results = await Promise.all([first, twice]);
  session.stop();
  assert.deepEqual(server.posts, ["Cerrar"], "one request");
  for (const r of results) assert.deepEqual([r.ok, r.status, r.message], [false, 409, "Ya estaba cerrada."], "both presses get the refusal");
  assert.deepEqual(server.toasts, [{ ok: false, message: "Ya estaba cerrada." }], "one error toast");
  assert.deepEqual(sounds, ["error"], "one error sound");
});

test("session: an action is joined only while its request is out; pressed again once it came back, while the first still plays and reads again, it is sent again", async () => {
  const { server, session } = rig();
  // while the POST is out, a second press is the same action
  const held = gate();
  server.hold = held.p;
  const a = session.act("/api/task/add", { text: "Llamar" });
  const b = session.act("/api/task/add", { text: "Llamar" });
  await settle();
  server.hold = null;
  held.open();
  await Promise.all([a, b]);
  assert.deepEqual(server.posts, ["Llamar"], "pressed twice while the request was out: one POST");
  assert.equal(server.toasts.length, 1, "and one toast");
  // the POST came back and the first press is still in its beforeRefresh (the staged word, the sound): a press now is a new action
  const painting = gate();
  const reached = gate();
  const first = session.act("/api/task/add", { text: "Escribir" }, { beforeRefresh: () => { reached.open(); return painting.p; } });
  await reached.p;
  assert.deepEqual(server.posts, ["Llamar", "Escribir"], "the first Escribir went out and came back");
  const second = session.act("/api/task/add", { text: "Escribir" });
  await settle();
  assert.deepEqual(server.posts, ["Llamar", "Escribir", "Escribir"], "pressed after its POST came back: a POST of its own (a new action, as today), not the first one's answer");
  painting.open();
  const results = await Promise.all([first, second]);
  session.stop();
  for (const r of results) assert.equal(r.ok, true);
  assert.equal(server.toasts.length, 3, "Llamar once, and each Escribir its own toast");
});

test("session: in the demo too, a double click is one request to the demo's server, one toast and one run of the callbacks", async () => {
  setLang("es");
  const asked = [];
  const held = gate();
  const demo = {
    api: async (path, body) => {
      if (body === undefined) return { ok: true, status: 200, data: structuredClone(SNAPSHOT) };
      asked.push(path);
      await held.p;
      return { ok: true, status: 200, data: { ok: true, message: "Hecho en el demo." } };
    },
  };
  const session = createSession({ demo, freshMs: 1 });
  const toasts = [];
  session.subscribe((event) => { if (event.type === "toast") toasts.push(event.toast.message); });
  const sounds = [];
  const opts = { beforeRefresh: (r) => { sounds.push(r.ok ? "taskadd" : "error"); } };
  const first = session.act("/api/task/add", { text: "Llamar" }, opts);
  const twice = session.act("/api/task/add", { text: "Llamar" }, opts);
  await settle();
  held.open();
  const results = await Promise.all([first, twice]);
  session.stop();
  for (const r of results) assert.deepEqual([r.ok, r.message], [true, "Hecho en el demo."]);
  assert.deepEqual(asked, ["/api/task/add"], "the demo's server is asked once");
  assert.deepEqual(toasts, ["Hecho en el demo."], "one toast");
  assert.deepEqual(sounds, ["taskadd"], "one sound");
});

// ---- (3) the line of a test step in `npm run check`

/** What `report` prints for one step, through console.log and console.error. */
function printed(step, result) {
  assert.equal(typeof check.report, "function", "tools/check.mjs exports report(step, result, index, total), which prints the line of a step");
  const out = [];
  const { log, error } = console;
  console.log = (...args) => out.push(args.join(" "));
  console.error = (...args) => out.push(args.join(" "));
  try { check.report(step, result, 4, 4); } finally { console.log = log; console.error = error; }
  return out.join("\n");
}

/** One step run for real by check.mjs: a node child that writes `stdout` and then `stderr`, and exits 0. */
function ran(step, { stdout, stderr = "" }) {
  assert.equal(typeof check.runStep, "function", "tools/check.mjs exports runStep(step, { verbose }), which runs one step and returns its result");
  const script = `process.stdout.write(${JSON.stringify(stdout)}); process.stderr.write(${JSON.stringify(stderr)});`;
  return check.runStep({ ...step, args: ["-e", script] }, { verbose: false });
}

const TESTS = { name: "tests", what: "76 test files without a browser", tests: true };
const SUMMARY = ["✔ passes (0.4ms)", "ℹ tests 5", "ℹ suites 0", "ℹ pass 4", "ℹ fail 0", "ℹ cancelled 0", "ℹ skipped 1", "ℹ todo 0", "ℹ duration_ms 12.3", ""].join("\n");

test("check report: a test step's counts come from what the runner wrote to stdout, never from its stderr after it", () => {
  // a child of a test (a server, a build) writes to the step's stderr, after the runner's summary in the joined output
  const result = ran(TESTS, { stdout: SUMMARY, stderr: ["ℹ tests 1", "ℹ pass 1", "ℹ fail 0", ""].join("\n") });
  assert.equal(result.ok, true, "the child exited 0");
  const line = printed(TESTS, result);
  assert.match(line, /^\[4\/4\] tests\s+ok\s/, line);
  assert.ok(line.includes("(tests 5, pass 4, fail 0, cancelled 0, skipped 1, todo 0)"), `the counts of the runner's summary in stdout: ${line}`);
  assert.ok(!line.includes("tests 1"), `nothing from stderr: ${line}`);
});

test("check report: a test step that passed with no summary from the runner says so on its line", () => {
  const result = ran(TESTS, { stdout: "✔ passes (0.4ms)\n", stderr: "ℹ tests 3\nℹ pass 3\n" });
  assert.equal(result.ok, true, "the child exited 0");
  const line = printed(TESTS, result);
  assert.match(line, /^\[4\/4\] tests\s+ok\s/, line);
  assert.ok(line.includes("no test summary"), `a green line with no counts at all hides that nothing was counted: ${line}`);
  assert.ok(!line.includes("tests 3"), `the summary is looked for in stdout only: ${line}`);
  // a step that is not a test runner keeps its line as it is: the first line of what it printed
  const build = { name: "build", what: "the page and its guard" };
  const built = printed(build, ran(build, { stdout: "page dist/check.html 812 KB\nthe table\n" }));
  assert.match(built, /^\[4\/4\] build\s+ok\s+\S+s {2}the page and its guard \(page dist\/check\.html 812 KB\)$/, built);
});
