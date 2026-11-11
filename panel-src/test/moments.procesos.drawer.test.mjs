import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { setLang, setTimeZone, fmtDate, fmtMoney } from "../src/i18n/index.js";
import { model } from "../src/core/model.js";
import { drawerView, barsOf, platesOf, trailOf } from "../src/moments/procesos/drawer-content.js";

const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/snapshot.${name}.json`, import.meta.url), "utf8"));
const fresh = (lang = "es") => { setLang(lang); setTimeZone("America/Santiago"); };
const cardOf = (snap, key) => snap.board.flatMap((c) => c.cards).find((c) => c.key === key) || [...snap.awaiting_approval.cards, ...snap.needs_gonza.cards].find((c) => c.key === key);
const nowOf = (snap) => Date.parse(snap.generated_at);
const strings = (value, out = []) => {
  if (typeof value === "string") out.push(value);
  else if (Array.isArray(value)) value.forEach((v) => strings(v, out));
  else if (value && typeof value === "object") Object.values(value).forEach((v) => strings(v, out));
  return out;
};
const detailOf = (card, extra = {}) => ({
  card,
  location: "Remote, anywhere",
  remote: true,
  apply_url: card.url,
  first_seen: "2026-09-26T04:14:00Z",
  posted_at: null,
  salary_min: 6000,
  salary_max: 7500,
  score_detail: { fit: 26, compensation: 15, stability: 17, learning: 12, interview_style: 9, other: 8 },
  gaps: [],
  events: [
    { at: "2026-09-29T21:03:00Z", from: "prepared", to: "submitted", note: null },
    { at: "2026-09-27T05:30:00Z", from: "scored", to: "prepared", note: "Tailored CV" },
    { at: "2026-09-26T04:20:00Z", from: null, to: "scored", note: null },
  ],
  dossier: null,
  description: "A posting.",
  ...extra,
});
const viewOf = (snap, key, extra) => drawerView(detailOf(cardOf(snap, key), extra), cardOf(snap, key), model(snap), nowOf(snap));

test("the six weights are as long as their points, in a fixed order, with any name the server adds after them", () => {
  fresh();
  const bars = barsOf({ other: 8, fit: 26, learning: 12, brand_new: 4, stability: 17, interview_style: 9, compensation: 15 });
  assert.deepEqual(bars.map((b) => b.id), ["fit", "compensation", "interview_style", "stability", "learning", "other", "brand_new"], "COLA's order (Encaje, Sueldo, Entrevista, Estabilidad, Aprendizaje, Otro), then the names the page does not know");
  assert.deepEqual(bars.map((b) => b.label).slice(0, 3), ["Encaje", "Sueldo", "Entrevista"]);
  assert.equal(bars[6].label, "brand_new", "a name the page does not know shows as it came");
  assert.deepEqual(barsOf(null), []);
  assert.deepEqual(barsOf([1, 2]), [], "an array is no breakdown");
  assert.deepEqual(barsOf({ fit: "x", other: NaN }), [], "a point that is not a number is not a bar");
  assert.equal(barsOf({ "<img onerror=x>\u202e": 3 })[0].label.includes("\u202e"), false);
  assert.equal(barsOf({ ["A".repeat(100)]: 3 })[0].label.length <= 40, true);
  fresh("en");
  assert.equal(barsOf({ fit: 1 })[0].label, "Fit");
  fresh();
});

test("the five plates of an application that went out: all done, with the date of each and the gate as its label says", () => {
  fresh();
  const snap = fixture("rich");
  const v = viewOf(snap, "fx:1");
  assert.deepEqual(v.plates.map((p) => p.id), ["aviso", "puntaje", "cv", "respuestas", "compuerta"]);
  assert.deepEqual(v.plates.map((p) => p.name), ["Aviso", "Puntaje", "CV", "Respuestas", "Revisión final"]);
  assert.ok(v.plates.every((p) => p.state === "done"));
  assert.equal(v.plates[0].text, `greenhouse, ${fmtDate("2026-09-26T04:14:00Z", "day")}`);
  assert.equal(v.plates[1].text, "91 de 100, umbral 60", "the threshold is the pipeline's, not a number written here");
  assert.equal(v.plates[1].w, 91);
  assert.equal(v.plates[2].text, "a la medida");
  assert.equal(v.plates[4].text, "la envió jobbot", "it went out by itself");
  assert.equal(v.plates[4].gate, "dim");
  assert.equal(v.plates[4].when, fmtDate("2026-09-29T21:03:00Z", "datetime"), "the day it went out, in Santiago");
  assert.match(v.plates[4].when, /18:03$/, "21:03 in Greenwich is 18:03 in Santiago");
  assert.ok(v.plates.every((p) => !p.you), "nothing of his is on it");
  const noThreshold = fixture("rich");
  delete noThreshold.pipeline.threshold;
  assert.equal(viewOf(noThreshold, "fx:1").plates[1].text, "91 de 100");
});

test("the plates he took part in are his: the gate he approved or sent by hand, the answers he gave, and the layer that waits for him", () => {
  fresh();
  const snap = fixture("rich");
  const approved = fixture("rich");
  cardOf(approved, "fx:1").approved = true;
  const a = viewOf(approved, "fx:1");
  assert.deepEqual(a.plates.map((p) => p.you), [false, false, false, false, true]);
  assert.equal(a.plates[4].text, "aprobada por ti");
  const byHand = fixture("rich");
  cardOf(byHand, "fx:1").source = "manual";
  assert.equal(viewOf(byHand, "fx:1").plates[4].text, "la enviaste a mano");
  assert.equal(viewOf(byHand, "fx:1").plates[4].gate, "hollow");
  assert.equal(viewOf(byHand, "fx:1").plates[4].you, true);
  const answered = viewOf(snap, "fx:1", { events: [{ at: "2026-09-28T08:00:00Z", from: "needs_gonza", to: "prepared", note: null }, { at: "2026-09-29T21:03:00Z", from: "prepared", to: "submitted", note: null }] });
  assert.equal(answered.plates[3].you, true, "he got it out of 'needs you': those were his answers");
  // the one that waits for him: a stuck form
  const stuck = viewOf(snap, "fx:9");
  const here = stuck.plates.filter((p) => p.state === "here");
  assert.equal(here.length, 1);
  assert.equal(here[0].you, true);
  assert.equal(here[0].id, "compuerta", "a hand-off waits at the gate");
  // one that waits for the robot is not his
  const scored = viewOf(snap, "fx:15");
  assert.deepEqual(scored.plates.map((p) => p.state), ["done", "done", "here", "todo", "todo"]);
  assert.ok(scored.plates.every((p) => !p.you));
  assert.equal(scored.plates[2].text, "falta el CV");
  assert.equal(scored.plates[4].text, "espera tu aprobación");
  assert.equal(platesOf({}, { state: "scored", score: 80 }).length, 5, "a bare card still has five plates");
});

test("the trail is dated, oldest first whatever order the server sends, and his steps are the ones that left 'needs you' or went out by his hand", () => {
  fresh();
  const snap = fixture("rich");
  const v = viewOf(snap, "fx:1");
  assert.deepEqual(v.trail.map((x) => x.to), ["scored", "prepared", "submitted"]);
  assert.equal(v.trail[0].text, "Puntuada", "the first one has no origin: it just is");
  assert.equal(v.trail[1].text, "Puntuada → Preparada");
  assert.equal(v.trail[1].note, "Tailored CV");
  assert.equal(v.trail[2].when, fmtDate("2026-09-29T21:03:00Z", "datetime"));
  assert.ok(v.trail.every((x) => !x.you));
  const stuck = trailOf(detailOf(cardOf(snap, "fx:9"), { events: [{ at: "2026-09-30T10:00:00Z", from: "needs_gonza", to: "prepared", note: null }, { at: "2026-09-29T10:00:00Z", from: "prepared", to: "needs_gonza", note: "Form asked for a phone number" }] }), cardOf(snap, "fx:9"));
  assert.deepEqual(stuck.map((x) => x.you), [false, true]);
  const byHand = fixture("rich");
  cardOf(byHand, "fx:1").source = "manual";
  assert.equal(viewOf(byHand, "fx:1").trail.at(-1).you, true, "he sent it himself");
  // broken events: no date, no destination, hostile note
  const bad = trailOf({ events: [null, { at: "nope", from: null, to: "scored", note: "<b>x</b>\u0000" + "n".repeat(300) }, { at: "2026-09-29T10:00:00Z", to: "prepared" }, { at: "x" }] }, cardOf(snap, "fx:1"));
  assert.equal(bad.length, 2);
  assert.equal(bad[0].to, "prepared", "the one with a date first");
  assert.equal(bad[1].when, "", "no date is no text, never 'Invalid Date'");
  assert.ok(bad[1].note.length <= 140 && !bad[1].note.includes("\u0000"));
  assert.deepEqual(trailOf({}, cardOf(snap, "fx:1")), []);
  assert.deepEqual(trailOf({ events: "x" }, cardOf(snap, "fx:1")), []);
});

test("the ruler of the follow-ups: only while it waits for an answer, with today's day on the 45 and the rule as words", () => {
  fresh();
  const snap = fixture("rich");
  const v = viewOf(snap, "fx:1");
  assert.equal(v.after.head, `Después del envío: ${fmtDate("2026-09-29T21:03:00Z", "datetime")}, sin respuesta`);
  const r = v.after.ruler;
  assert.equal(r.day, 12);
  assert.ok(Math.abs(r.at - (12 / 45) * 100) < 1e-9);
  assert.deepEqual(r.bands.map((b) => b.label), ["8–10", "15–17"]);
  assert.ok(Math.abs(r.bands[0].from - (8 / 45) * 100) < 1e-9 && Math.abs(r.bands[1].to - (17 / 45) * 100) < 1e-9);
  assert.equal(r.labels.now, "hoy, día 12");
  assert.equal(r.labels.close, "45: cierra");
  assert.equal(r.rule, "Seguimiento a los 8–10 y 15–17 días; sin noticias a los 45; la mediana de respuesta es de 6–7 días");
  assert.equal(viewOf(snap, "fx:2").after, null, "an answered one has no follow-up to measure");
  assert.equal(viewOf(snap, "fx:15").after, null, "one that has not gone out has no after");
  const old = fixture("rich");
  cardOf(old, "fx:6").days_in_state = 80;
  assert.equal(viewOf(old, "fx:6").after.ruler.at, 100, "past the 45 the marker stays at the end");
  const nodate = viewOf(snap, "fx:1", { events: [] });
  assert.equal(nodate.after.head, "Después del envío, sin respuesta");
  fresh("en");
  assert.equal(viewOf(snap, "fx:1").after.ruler.labels.now, "today, day 12");
  fresh();
});

test("the mails are the ones of that application, newest first, and what a stranger wrote stays text", () => {
  fresh();
  const snap = fixture("rich");
  const v = viewOf(snap, "fx:2");
  assert.equal(v.mails.length, 1);
  assert.equal(v.mails[0].subject, "Can we talk on Wednesday?");
  assert.equal(v.mails[0].kind, "Reclutador");
  assert.equal(viewOf(snap, "fx:1").mails.length, 0);
  const evil = fixture("rich");
  evil.today.emails.push({ received_at: "2026-10-07T16:00:00Z", from: "<script>@x.example", company: "X", job_key: "fx:2", kind: "weird", subject: "\u202eClick\n" + "S".repeat(300), summary: null });
  const w = viewOf(evil, "fx:2");
  assert.equal(w.mails[0].subject.length <= 120, true);
  assert.equal(/[\u202e\n]/.test(w.mails[0].subject), false);
  assert.equal(w.mails[0].kind, "weird", "a kind the page does not know shows as it came");
  assert.equal(w.mails[0].summary, "");
  const none = drawerView(null, cardOf(evil, "fx:2"), null, nowOf(evil));
  assert.deepEqual(none.mails, [], "without a model there are no mails, and no crash");
});

test("the next step: whose turn, what is on it and the buttons with their keys; the permanent one is marked", () => {
  fresh();
  const snap = fixture("rich");
  const yours = viewOf(snap, "fx:2");
  assert.equal(yours.next.you, true);
  assert.equal(yours.next.title, "Reply to the Beta Test Co recruiter", "the task that is on it");
  assert.match(yours.next.head, /^te toca: /);
  assert.deepEqual(yours.next.actions.map((a) => a.id), ["register", "link", "snooze", "withdraw", "newtask"]);
  assert.deepEqual(yours.next.actions.map((a) => a.key), ["r", "o", "s", "x", "n"]);
  assert.equal(yours.next.actions.find((a) => a.primary).id, "register");
  const stuck = viewOf(snap, "fx:9");
  assert.equal(stuck.next.actions[0].id, "confirm", "a stuck form: he says he sent it, first");
  assert.equal(stuck.next.actions[0].permanent, true);
  assert.equal(stuck.next.actions[0].primary, true);
  assert.equal(stuck.next.actions[0].key, "c");
  const sent = viewOf(snap, "fx:1");
  assert.equal(sent.next.you, false);
  assert.equal(sent.next.head, "espera a la empresa");
  assert.equal(sent.next.title, "Esperar respuesta");
  const window = fixture("rich");
  cardOf(window, "fx:1").days_in_state = 9;
  const ask = viewOf(window, "fx:1");
  assert.equal(ask.next.head, "seguimiento 1: hoy es el día 9 de 8–10");
  assert.equal(ask.next.title, "Preguntar cómo va");
  const closed = viewOf(snap, "fx:17");
  assert.deepEqual(closed.next.actions, [], "a closed one has nothing to do");
  assert.equal(closed.next.head, "Rechazada");
  const away = fixture("rich");
  cardOf(away, "fx:2").snoozed_until = new Date(nowOf(away) + 3 * 86400000).toISOString();
  assert.equal(viewOf(away, "fx:2").next.head, "pospuesta");
});

test("the header: who, where, how much; the link only when it is http(s); the eyebrow has the stage and the day", () => {
  fresh();
  const snap = fixture("rich");
  const v = viewOf(snap, "fx:1");
  assert.equal(v.company, "Alpha Test Co");
  assert.deepEqual(v.eyebrow, ["Enviada", "día 12"]);
  assert.deepEqual(v.sub, ["Backend Engineer", "greenhouse", "Remote, anywhere", `${fmtMoney(6000, { digits: 0 })} – ${fmtMoney(7500, { digits: 0 })} al mes`], "the place already says remote, so it is not said twice");
  assert.deepEqual(viewOf(snap, "fx:1", { location: "Santiago", remote: true }).sub[2], "Santiago, remoto");
  assert.deepEqual(viewOf(snap, "fx:1", { location: "Santiago", remote: false }).sub[2], "Santiago");
  assert.deepEqual(viewOf(snap, "fx:1", { location: null, remote: true }).sub[2], "remoto");
  assert.equal(v.posting, "https://example.com/fx/1");
  const odd = viewOf(snap, "fx:1", { location: "Remote, anywhere", remote: true, salary_min: null, salary_max: 9000, apply_url: "javascript:1" });
  assert.match(odd.sub.at(-1), /^hasta .* al mes$/);
  const bad = drawerView(detailOf(cardOf(snap, "fx:1"), { salary_min: 0, salary_max: 0, location: null, remote: false }), { ...cardOf(snap, "fx:1"), url: "javascript:alert(1)" }, model(snap), nowOf(snap));
  assert.deepEqual(bad.sub, ["Backend Engineer", "greenhouse"], "no place and no pay: nothing is written for them");
  assert.equal(bad.posting, "https://example.com/fx/1", "the card's link is not http(s): the detail's own application link is");
  const none = drawerView(detailOf(cardOf(snap, "fx:1"), { apply_url: "ftp://x" }), { ...cardOf(snap, "fx:1"), url: "javascript:alert(1)" }, model(snap), nowOf(snap));
  assert.equal(none.posting, null);
  assert.equal(viewOf(snap, "fx:1", { dossier: "Line one\nLine two" }).dossier, "Line one\nLine two");
  assert.deepEqual(viewOf(snap, "fx:1", { gaps: [{ topic: "Queues", seen: true }, { topic: "", seen: false }, null] }).gaps, [{ topic: "Queues", seen: true }]);
});

test("while the detail is still being read the page shows what the card knows; and no string of it is ever a missing word, in either language", () => {
  fresh();
  const snap = fixture("rich");
  const early = drawerView(null, cardOf(snap, "fx:1"), model(snap), nowOf(snap));
  assert.equal(early.score.none, true);
  assert.equal(early.score.total, 91);
  assert.equal(early.trail.length, 0);
  assert.equal(early.plates.length, 5);
  assert.equal(drawerView(null, null, null, 0).plates.length, 5, "not even a card is a crash");
  for (const lang of ["es", "en"]) {
    fresh(lang);
    for (const name of ["rich", "heavy"]) {
      const s = fixture(name);
      const m = model(s);
      for (const c of s.board.flatMap((col) => col.cards).slice(0, 60)) {
        for (const text of strings(drawerView(detailOf(c), c, m, nowOf(s)))) assert.ok(!/⟦|NaN|undefined|\bnull\b/.test(text), `${lang}/${name}/${c.key}: ${text.slice(0, 80)}`);
      }
    }
  }
  fresh();
});
