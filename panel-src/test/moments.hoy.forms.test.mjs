import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { setLang, setTimeZone, has } from "../src/i18n/index.js";
import { snoozeChoices, withdrawChoices, registerChoices, DUE, dueAt, taskBody } from "../src/moments/hoy/forms.js";
import { ASKS_AFTER } from "../src/moments/hoy/prompt.js";
import { requestOf } from "../src/moments/hoy/actions.js";
import { KEYS } from "../src/ui/keys.js";

const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/snapshot.${name}.json`, import.meta.url), "utf8"));
const fresh = (lang = "es") => { setLang(lang); setTimeZone("America/Santiago"); };
const NOW = Date.parse("2026-10-07T12:00:00Z");

test("postponing offers four horizons the server reads, each with its own key and words in both languages", () => {
  for (const lang of ["es", "en"]) {
    fresh(lang);
    const choices = snoozeChoices();
    assert.deepEqual(choices.map((c) => [c.key, c.until]), [["1", "1d"], ["2", "3d"], ["3", "7d"], ["4", "14d"]]);
    for (const c of choices) assert.ok(c.label && !c.label.includes("⟦"), `${lang}: ${c.until}`);
  }
  fresh();
  assert.equal(snoozeChoices()[0].label, "Mañana");
});

test("withdrawing offers a reason that is written on the application as its note; no reason is no note", () => {
  fresh();
  const choices = withdrawChoices();
  assert.deepEqual(choices.map((c) => c.key), ["1", "2", "3"]);
  assert.equal(choices[0].note, null);
  assert.deepEqual(choices.slice(1).map((c) => c.note), ["Ya no me interesa", "Acepté otra oferta"]);
  fresh("en");
  assert.deepEqual(withdrawChoices().slice(1).map((c) => c.note), ["No longer interested", "Took another offer"]);
  fresh();
});

test("registering offers what the server's marks allow, except withdrawing (that is x), and the request carries the state he chose", () => {
  fresh();
  const rich = fixture("rich");
  const cards = rich.board.flatMap((c) => c.cards);
  const offer = cards.find((c) => c.state === "offer");
  const choices = registerChoices(offer);
  assert.deepEqual(choices.map((c) => c.state), offer.marks.filter((m) => m !== "withdrawn"));
  assert.deepEqual(choices.map((c) => c.key), choices.map((_, i) => String(i + 1)));
  assert.ok(choices.every((c) => c.label && !c.label.includes("⟦")));
  assert.equal(choices.find((c) => c.state === "accepted").label, "Acepté la oferta", "the answer said the way it happened, not the name of a state");
  assert.deepEqual(registerChoices(null), []);
  assert.deepEqual(registerChoices({ marks: ["withdrawn"] }), [], "a card that can only be withdrawn has no answer to log");
  const row = { id: "card:" + offer.key, data: { card: offer.key } };
  assert.deepEqual(requestOf("register", row, { state: choices[0].state }), { path: "/api/mark", body: { key: offer.key, state: choices[0].state, note: null } });
});

test("the deadline of a new task is RFC 3339 counted from now, or none; an empty text is no task and a long one is cut", () => {
  assert.deepEqual(DUE.map((d) => d.id), ["none", "2h", "1d", "3d"]);
  assert.equal(dueAt("none", NOW), null);
  assert.equal(dueAt("2h", NOW), "2026-10-07T14:00:00Z");
  assert.equal(dueAt("1d", NOW), "2026-10-08T12:00:00Z");
  assert.equal(dueAt("3d", NOW), "2026-10-10T12:00:00Z");
  assert.equal(dueAt("nope", NOW), null);
  assert.deepEqual(taskBody("  Llamar a la agencia  ", "2h", NOW), { text: "Llamar a la agencia", due_at: "2026-10-07T14:00:00Z" });
  assert.deepEqual(taskBody("Sin fecha", "none", NOW), { text: "Sin fecha", due_at: null });
  assert.equal(taskBody("   ", "2h", NOW), null);
  assert.equal(taskBody("", "none", NOW), null);
  assert.equal(taskBody("x".repeat(500), "none", NOW).text.length, 240, "the same cut as the server's field");
  assert.equal(taskBody("a\u202eb\nc", "none", NOW).text, "a b c", "text from the keyboard goes through the same filter as text from outside");
});

test("every word of the menus and of the question after a link exists in both languages", () => {
  for (const lang of ["es", "en"]) {
    fresh(lang);
    for (const key of ["snooze.title", "withdraw.title", "register.title", "newtask.title", "newtask.text", "newtask.placeholder", "newtask.due", "newtask.add", "newtask.empty", "prompt.ask", "prompt.yes", "prompt.no", ...DUE.map((d) => "newtask.due." + d.id)]) {
      assert.ok(has("hoy." + key), `${lang}: hoy.${key}`);
    }
    assert.ok(has("menu.close"));
  }
  fresh();
});

test("the question 'did you do it?' is for what the bot could not finish, and its keys are the table's: c says yes, Esc says not yet", () => {
  assert.deepEqual([...ASKS_AFTER].sort(), ["blocked", "handoff", "manual_apply"]);
  assert.equal(KEYS.find((k) => k.id === "confirm").keys[0], "c");
  assert.equal(KEYS.find((k) => k.id === "close").display, "Esc");
});
