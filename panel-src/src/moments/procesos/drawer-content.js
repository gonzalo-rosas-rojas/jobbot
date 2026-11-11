// The drawer of one application, PROCESOS' own content (spec §6.3, board/relay2/procesos-1920.png): the score with its six weights, the five
// layers as tilted plates with their labels (once and large), the dated trail from the posting to the send with his steps in gold, the
// follow-up rule over 45 days with "hoy · día N", the mails of that company and the next step with its keys.
//
//   drawerView(detail, card, m, nowMs) -> the page of one application as data (pure: no DOM, everything counted from the model)
//   drawerContent(detail, card, ctx, { run }) -> Node        the same, drawn; `run(actionId, { confirmed })` does a verb on this card
//
// The common drawer (ui/drawer.js, T20) fetches `GET /api/job?key=` and asks for the content of the application it opens; PROCESOS gives it
// `moment.drawer(detail, card, ctx)`. A drawer that does not ask keeps showing the common content, and nothing here depends on it.
// What the snapshot does not carry stays out: who did each move (an event has no author: his steps are the ones that left "needs you" and a by-hand send),
// and the CV and dossier files (`GET /api/file` does not exist yet).

import { h, sv, twoStep } from "../../core/dom.js";
import { t, has, fmtDate, fmtMoney } from "../../i18n/index.js";
import { layerOf, SENT_STATES, CLOSED } from "../../core/model.js";
import { kindLabel, stateLabel, factorBars, factorName, pointsOf } from "../../core/format.js";
import { actButton } from "../../ui/act.js";
import { clean, safeHref } from "../shared/util.js";
import { CADENCE, cadenceOf, dayOf } from "./cadence.js";
import { entriesOf, actionsOf, gateOf, layersOf, nextStepText, keyDisplay } from "./view.js";

/** The six factors by name, in the order the bars come in (core/format.js factorBars); a test imports it. */
export const FACTORS = ["fit", "compensation", "interview_style", "stability", "learning", "other"];
const PLATES = ["aviso", "puntaje", "cv", "respuestas", "compuerta"];
// the order of the buttons of the last block: the one that closes the matter first, then the answer, the posting, the pause, the exit, the note
const VERBS = ["confirm", "register", "link", "snooze", "withdraw", "newtask"];

/** A date as the page writes it, or "" when it is not one (a bad date never prints as "Invalid Date" or as 1970). */
const dateText = (iso, style = "day") => (iso && Number.isFinite(Date.parse(iso)) ? fmtDate(iso, style) : "");
const money = (x) => fmtMoney(Math.round(x), { digits: 0 });

/** The bars of the score, drawn like the other two places that draw them (core/format.js factorBars); an array is no breakdown. */
export function barsOf(scoreDetail) {
  if (Array.isArray(scoreDetail)) return [];
  return factorBars(scoreDetail).map((b) => ({ ...b, label: has("procesos.factor." + b.id) ? t("procesos.factor." + b.id) : clean(factorName(b.id), 40) }));
}

/** The place, the pay: "Remoto, cualquier país · US$ 6.000 – 7.500 al mes", only the pieces that exist. */
function pieceLine(detail, card) {
  const place = clean(detail.location, 80);
  const remote = detail.remote && !/remot/i.test(place) ? t("procesos.drawer.remote") : "";
  const where = place && remote ? t("procesos.drawer.place", { location: place, remote }) : place || remote;
  const positive = (x) => (x !== null && x > 0 ? x : null);
  const lo = positive(pointsOf(detail.salary_min));
  const hi = positive(pointsOf(detail.salary_max));
  const pay = lo !== null && hi !== null ? t("procesos.drawer.salary", { from: money(lo), to: money(hi) })
    : lo !== null ? t("procesos.drawer.salary.min", { from: money(lo) })
      : hi !== null ? t("procesos.drawer.salary.max", { to: money(hi) }) : "";
  return [clean(card.title, 90), clean(card.source, 24), where, pay].filter(Boolean);
}

/** The five plates: each is done, here or still to come, says what is on it and, once it passed, when; the ones he took part in are his. */
export function platesOf(detail, card, { threshold = null, events = [] } = {}) {
  const layers = layersOf(card);
  const layer = layerOf(card);
  const gate = gateOf(card);
  const sentAt = events.find((e) => e.to === "submitted");
  const gotOut = events.some((e) => e.from === "needs_gonza");
  const at = (id) => layers.find((l) => l.id === id);
  const text = {
    aviso: () => {
      const date = dateText(detail.first_seen || card.first_seen);
      const source = clean(card.source, 24) || t("layer.aviso").toLowerCase();
      return date ? t("procesos.plate.aviso", { source, date }) : t("procesos.plate.aviso.nodate", { source });
    },
    puntaje: () => (threshold == null ? t("procesos.plate.puntaje", { score: at("puntaje").w }) : t("procesos.plate.puntaje.threshold", { score: at("puntaje").w, threshold })),
    cv: () => t(card.has_cv ? "procesos.plate.cv.done" : "procesos.plate.cv.todo"),
    respuestas: () => t("procesos.plate.respuestas." + at("respuestas").state),
    compuerta: () => t("procesos.plate.compuerta." + gate),
  };
  return PLATES.map((id) => {
    const mark = at(id);
    const you = (mark.state === "here" && layer.who === "you")
      || (id === "respuestas" && mark.state === "done" && gotOut)
      || (id === "compuerta" && (gate === "clear" || gate === "hollow"));
    return {
      id,
      name: t("layer." + id),
      state: mark.state,
      gate: id === "compuerta" ? gate : null,
      w: id === "puntaje" ? mark.w : null,
      text: text[id](),
      when: id === "compuerta" && sentAt ? dateText(sentAt.at, "datetime") : "",
      you,
    };
  });
}

/** The moves of the application, oldest first, with the ones that were his marked (leaving "needs you", or sending by hand). */
export function trailOf(detail, card) {
  const gate = gateOf(card);
  const list = (Array.isArray(detail.events) ? detail.events : []).filter((e) => e && e.to);
  return list
    .map((e, i) => ({ e, i, ms: Date.parse(e.at) }))
    .sort((a, b) => (Number.isFinite(a.ms) ? a.ms : Infinity) - (Number.isFinite(b.ms) ? b.ms : Infinity) || a.i - b.i)
    .map(({ e }) => ({
      at: e.at,
      from: e.from || null,
      to: e.to,
      when: dateText(e.at, "datetime"),
      text: e.from ? t("procesos.trail.move", { from: stateLabel(e.from), to: stateLabel(e.to) }) : t("procesos.trail.first", { to: stateLabel(e.to) }),
      note: clean(e.note, 140),
      you: e.from === "needs_gonza" || (e.to === "submitted" && gate === "hollow"),
    }));
}

/** The ruler of the follow-ups over the 45 days after the send, or null when the application has not gone out or is no longer waiting. */
function rulerOf(card, nowDay) {
  if (card.state !== "submitted" && card.state !== "ghosted") return null;
  const [a0, a1] = CADENCE.ask;
  const [i0, i1] = CADENCE.insist;
  const pct = (d) => Math.min(100, Math.max(0, (d / CADENCE.close) * 100));
  return {
    day: nowDay,
    at: pct(nowDay),
    bands: [{ from: pct(a0), to: pct(a1), label: `${a0}–${a1}` }, { from: pct(i0), to: pct(i1), label: `${i0}–${i1}` }],
    close: CADENCE.close,
    labels: { send: t("procesos.ruler.send"), close: t("procesos.ruler.close", { n: CADENCE.close }), now: t("procesos.ruler.now", { n: nowDay }) },
    rule: t("procesos.ruler.rule", { ask: `${a0}–${a1}`, insist: `${i0}–${i1}`, close: CADENCE.close, median: `${CADENCE.median[0]}–${CADENCE.median[1]}` }),
  };
}

function mailsOf(m, key, nowMs) {
  const list = ((((m && m.snap) || {}).today || {}).emails || []).filter((x) => x.job_key === key);
  return list
    .slice()
    .sort((a, b) => Date.parse(b.received_at) - Date.parse(a.received_at))
    .map((x) => ({ kind: kindLabel(x.kind), subject: clean(x.subject, 120) || t("common.noSubject"), summary: clean(x.summary, 260), when: dateText(x.received_at, "datetime"), from: clean(x.from, 70) }));
}

/** What the last block says on top (whose turn it is and why) and the buttons under it. */
function nextOf(e, card, cad, task) {
  const you = !!e && e.turn === "you";
  let what;
  if (!e) what = CLOSED.includes(card.state) ? stateLabel(card.state) : t("procesos.drawer.next.wait");
  else if (e.turn === "snoozed") what = t("procesos.turn.snoozed");
  else if (cad.kind === "ask" || cad.kind === "insist") what = t("procesos.drawer.next.step", { step: cad.step, day: cad.day, from: cad.window[0], to: cad.window[1] });
  else if (cad.kind === "decide") what = t("procesos.drawer.next.decide", { day: cad.day });
  else if (cad.kind === "offer") what = t("procesos.drawer.next.offer");
  else if (task) what = t("procesos.drawer.next.task");
  else if (card.state === "prepared" && you) what = t("procesos.drawer.next.approve");
  else if (card.state === "needs_gonza") what = t("procesos.drawer.next.reason");
  else what = t("procesos.drawer.next.wait");
  const verbs = e ? actionsOf(e) : [];
  const actions = VERBS.map((id) => (id === "newtask" ? (e ? { id, permanent: false } : null) : verbs.find((v) => v.id === id))).filter(Boolean)
    .map((v) => ({ id: v.id, label: t("procesos.drawer.act." + v.id), key: keyDisplay(v.id), permanent: v.permanent }));
  const primary = actions.find((a) => a.id === "confirm") || actions.find((a) => a.id === "register") || actions[0] || null;
  return {
    you,
    head: you ? t("procesos.drawer.next.turn", { what }) : what,
    title: e ? nextStepText(e, cad) : "",
    actions: actions.map((a) => ({ ...a, primary: !!primary && a.id === primary.id })),
  };
}

/**
 * drawerView(detail, card, model, nowMs) -> {
 *   key, company, eyebrow, sub[], score { total, threshold, bars[], none }, plates[5], trail[], after { head, ruler } | null, mails[], next { you, head, title, actions[] },
 *   dossier, gaps[], posting }
 * `detail` may be null (the application is still being read): the page then shows what the card knows.
 */
export function drawerView(detail, card, m, nowMs) {
  const d = detail || {};
  const c = card || d.card || {};
  const snap = (m && m.snap) || {};
  const entry = m ? entriesOf(m, nowMs).find((e) => e.key === c.key) || null : null;
  const cad = cadenceOf(c, { hasTask: !!(entry && entry.task) });
  const day = dayOf(c);
  const threshold = pointsOf((snap.pipeline || {}).threshold);
  const trail = trailOf(d, c);
  const sentAt = trail.find((x) => x.to === "submitted");
  const sent = SENT_STATES.includes(c.state);
  const waiting = c.state === "submitted" || c.state === "ghosted";
  const afterHead = [sentAt && sentAt.when ? t("procesos.drawer.after", { date: sentAt.when }) : t("procesos.drawer.after.nodate"), waiting ? t("procesos.drawer.after.noreply") : ""].filter(Boolean).join(", ");
  const mails = mailsOf(m, c.key, nowMs);
  return {
    key: c.key,
    company: clean(c.company, 60),
    eyebrow: [stateLabel(c.state), t("procesos.drawer.day", { n: day })],
    sub: pieceLine(d, c),
    posting: safeHref(c.url) || safeHref(d.apply_url),
    score: { total: Math.max(0, Math.min(100, Math.round(Number(c.score) || 0))), threshold, bars: barsOf(d.score_detail), none: barsOf(d.score_detail).length === 0 },
    plates: platesOf(d, c, { threshold, events: trail }),
    trail,
    after: sent && waiting ? { head: afterHead, ruler: rulerOf(c, day) } : null,
    mails,
    next: nextOf(entry, c, cad, entry && entry.task),
    dossier: d.dossier ? String(d.dossier).slice(0, 4000) : "",
    gaps: (Array.isArray(d.gaps) ? d.gaps : []).map((g) => ({ topic: clean(g && g.topic, 80), seen: !!(g && g.seen) })).filter((g) => g.topic),
  };
}

// ---- the DOM

const PLATE_W = 150;
const PLATE_H = 30;

function plateFace(plate) {
  const w = PLATE_W;
  const hh = PLATE_H;
  const pts = [[0, hh * 0.58], [w * 0.94, 0], [w, hh * 0.44], [w * 0.06, hh]].map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(" ");
  const cy = hh * 0.5;
  const glyph = [];
  if (plate.id === "aviso") glyph.push(sv("line", { x1: w * 0.2, y1: cy + 1, x2: w * 0.7, y2: cy - 3, class: "pl-ink" }), sv("line", { x1: w * 0.2, y1: cy + 6, x2: w * 0.55, y2: cy + 3, class: "pl-ink dim" }));
  // the score plate carries the number itself (an HTML text over it), so nothing is drawn behind that text
  if (plate.id === "cv") for (let k = 0; k < 5; k++) glyph.push(sv("circle", { cx: w * 0.28 + k * 13, cy: cy + 1 - k * 0.8, r: 2.4, class: "pl-dot" }));
  if (plate.id === "respuestas") glyph.push(sv("line", { x1: w * 0.2, y1: cy + 1, x2: w * 0.75, y2: cy - 3, class: "pl-dash" }));
  if (plate.id === "compuerta") glyph.push(sv("line", { x1: w * 0.22, y1: cy + 3, x2: w * 0.52, y2: cy + 1, class: plate.gate === "hollow" ? "pl-gate hollow" : "pl-gate" }));
  return sv("svg", { viewBox: `0 0 ${w} ${hh}`, class: "pl-svg", "aria-hidden": "true", focusable: "false" }, sv("polygon", { points: pts, class: "pl-face" }), ...glyph);
}

function plateRow(plate) {
  return h("li", { class: "plate", "data-layer": plate.id, "data-state": plate.state, "data-you": plate.you ? "true" : null },
    h("span", { class: "pl-art" }, plateFace(plate), plate.id === "puntaje" ? h("span", { class: "pl-num cond num" }, String(plate.w)) : null),
    h("span", { class: "pl-guide", "aria-hidden": "true" }),
    h("span", { class: "pl-text" },
      h("span", { class: "pl-name label" }, plate.name, plate.you ? h("span", { class: "pl-you" }, " · " + t("procesos.plate.you")) : null),
      h("span", { class: "pl-state wrap-any" }, plate.text),
      plate.when ? h("span", { class: "pl-when num" }, plate.when) : null));
}

function rulerBox(r) {
  const at = (pct) => ({ "--at": pct.toFixed(2) + "%" });
  return h("div", { class: "ruler", role: "img", "aria-label": `${r.labels.now} · ${r.rule}` },
    h("i", { class: "rl-line" }),
    h("i", { class: "rl-done", style: { "--to": r.at.toFixed(2) + "%" } }),
    ...r.bands.map((b) => h("i", { class: "rl-band", style: { "--from": b.from.toFixed(2) + "%", "--w": (b.to - b.from).toFixed(2) + "%" } })),
    ...r.bands.map((b) => h("span", { class: "rl-tick num mono", style: at((b.from + b.to) / 2) }, b.label)),
    h("span", { class: "rl-tick rl-start mono" }, r.labels.send),
    h("span", { class: "rl-tick rl-end mono" }, r.labels.close),
    h("i", { class: "rl-now", style: at(r.at) }),
    h("span", { class: "rl-now-l mono num", style: at(r.at) }, r.labels.now));
}

/** Draws the page of one application. `run(actionId, { confirmed })` does a verb on it (the moment's own actions: the same two steps as the list). */
export function drawerContent(detail, card, ctx, { run = null } = {}) {
  const m = ctx && ctx.model ? ctx.model() : null;
  const nowMs = ctx && ctx.now ? ctx.now() : Date.now();
  const v = drawerView(detail, card, m, nowMs);
  const doVerb = (id, opts) => (run ? run(id, opts) : undefined);

  const bars = v.score.none
    ? h("p", { class: "pd-none" }, t("procesos.drawer.score.none"))
    : h("ul", { class: "bars", "aria-label": t("procesos.drawer.score.aria") }, v.score.bars.map((b, i) => h("li", { "data-factor": b.id },
      h("span", { class: "bk" }, b.label),
      h("i", { class: "bm", style: { "--k": b.k.toFixed(3) }, "data-top": i === 0 ? "true" : null }),
      h("span", { class: "bv num" }, b.text))));

  const buttons = v.next.actions.map((a) => {
    // the page's own two presses (the list's actions are not asked: `confirmed`), so the drawer makes the two sounds itself
    const sound = ctx && ctx.sound;
    const onclick = a.permanent
      ? twoStep(a.label, () => doVerb(a.id, { confirmed: true }), sound ? { onArm: () => sound.play("arm"), onConfirm: () => sound.play("confirm") } : {})
      : () => doVerb(a.id);
    return actButton({ key: a.key, label: a.label, primary: a.primary, "data-act": a.id, onclick });
  });

  return h("div", { class: "detail-body pd", "data-job": v.key },
    h("p", { class: "pd-eyebrow label" }, v.eyebrow.join(", ")),
    h("h2", { id: "detail-title", class: "pd-title wrap-any" }, v.company),
    v.sub.length ? h("p", { class: "pd-sub wrap-any" }, v.sub.join(" · ")) : null,
    h("section", { class: "pd-score", "aria-label": t("procesos.drawer.score.aria") },
      h("b", { class: "n cond num" }, String(v.score.total)),
      bars),
    h("section", { class: "pd-sec pd-stack" },
      h("h3", { class: "pd-h label" }, t("drawer.journey")),
      h("ol", { class: "plates", "aria-label": t("procesos.layers.aria") }, v.plates.map(plateRow))),
    h("section", { class: "pd-sec pd-trail" },
      h("h3", { class: "pd-h label" }, t("drawer.history")),
      v.trail.length === 0
        ? h("p", { class: "pd-none" }, t("procesos.trail.none"))
        : h("ol", { class: "trail" }, v.trail.map((x) => h("li", { "data-you": x.you ? "true" : null },
          h("time", { class: "tr-when mono num", datetime: x.at }, x.when),
          h("span", { class: "tr-text" }, x.text),
          x.note ? h("span", { class: "tr-note wrap-any" }, x.note) : null)))),
    v.after ? h("section", { class: "pd-sec pd-after" },
      h("h3", { class: "pd-h label" }, v.after.head),
      v.after.ruler ? rulerBox(v.after.ruler) : null,
      v.after.ruler ? h("p", { class: "pd-rule label" }, v.after.ruler.rule) : null) : null,
    h("section", { class: "pd-sec pd-mails" },
      h("h3", { class: "pd-h label" }, t("procesos.drawer.mails", { n: v.mails.length })),
      v.mails.length === 0
        ? null
        : h("ul", { class: "mails" }, v.mails.map((x) => h("li", {},
          h("span", { class: "ml-head label" }, [x.when, x.kind].filter(Boolean).join(" · ")),
          h("span", { class: "ml-subject wrap-any" }, x.subject),
          x.summary ? h("span", { class: "ml-summary wrap-any" }, x.summary) : null)))),
    v.gaps.length ? h("section", { class: "pd-sec pd-gaps" },
      h("h3", { class: "pd-h label" }, t("procesos.drawer.gaps")),
      h("ul", { class: "gaps" }, v.gaps.map((g) => h("li", { class: "wrap-any", "data-seen": g.seen ? "true" : null }, g.topic)))) : null,
    v.dossier ? h("details", { class: "pd-fold", "data-fold": "dossier" }, h("summary", { class: "label" }, t("procesos.drawer.dossier")), h("pre", { class: "pd-pre wrap-any" }, v.dossier)) : null,
    h("section", { class: "pd-next", "data-you": v.next.you ? "true" : null },
      h("p", { class: "pd-next-head label" }, v.next.head),
      v.next.title ? h("h3", { class: "pd-next-title wrap-any" }, v.next.title) : null,
      h("div", { class: "acts", role: "group", "aria-label": t("procesos.drawer.acts.aria") }, buttons)));
}
