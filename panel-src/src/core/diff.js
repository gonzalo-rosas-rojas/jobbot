// What changed since the last picture, so the page can say so instead of redrawing in silence, and what happened since
// the last visit. Both return { fresh, lines, big }: `fresh` is a Set of marks ("mail:<id>", "task:<id>", "card:<key>")
// that the rooms use to highlight what is new, `lines` the announcements in the active language, and `big` says an
// interview or an offer arrived (the ceremony). Ported from the old page.

import { t, tn } from "../i18n/index.js";
import { local } from "./store.js";
import { LIVE, CLOSED } from "./model.js";
import { kindLabel, stateLabel } from "./format.js";

export const mailId = (mail) => mail.received_at + "|" + mail.subject;

const nothing = () => ({ fresh: new Set(), lines: [], big: false });

export function diffSnapshots(prev, next) {
  const out = nothing();
  if (!prev) return out;
  const seenMail = new Set(prev.today.emails.map(mailId));
  const seenTask = new Set(prev.today.tasks.map((x) => x.id));
  const was = new Map();
  for (const col of prev.board) for (const c of col.cards) was.set(c.key, c.state);
  for (const mail of next.today.emails) {
    if (seenMail.has(mailId(mail))) continue;
    out.fresh.add("mail:" + mailId(mail));
    out.lines.push(t("news.mail", { who: mail.company || t("news.mail.anon"), kind: kindLabel(mail.kind) }));
  }
  for (const task of next.today.tasks) {
    if (seenTask.has(task.id)) continue;
    out.fresh.add("task:" + task.id);
    out.lines.push(t("news.task"));
  }
  for (const col of next.board) {
    for (const c of col.cards) {
      const before = was.get(c.key);
      if (!before || before === c.state) continue;
      out.fresh.add("card:" + c.key);
      out.lines.push(t("news.card", { company: c.company, state: stateLabel(c.state) }));
      if (c.state === "interview" || c.state === "offer") out.big = true;
    }
  }
  return out;
}

/** The moment of the last visit as the page stored it (ms), or 0 when there is none. */
export function lastVisit() {
  return Date.parse(local.get("lastVisit") || "") || 0;
}

let held = null;

/**
 * Boot asks for this once, before anything can call `rememberVisit`: the last visit as it was stored when this page started. A page hidden before its
 * first picture saves a new `lastVisit`, and without this the news of the time away would be compared with that, and vanish.
 */
export function holdVisit() {
  if (held === null) held = lastVisit();
  return held;
}

/** On the first picture of a visit: what happened since the last time the page was open (the visit held at boot, when boot held one). */
export function sinceLastVisit(snap, last = held ?? lastVisit()) {
  const out = nothing();
  if (!(last > 0)) return out;
  for (const mail of snap.today.emails) if (Date.parse(mail.received_at) > last) out.fresh.add("mail:" + mailId(mail));
  for (const col of snap.board) {
    // legacy quirk: only the cards of the applications sent or closed count; a newly scored, prepared or stuck one is not "news"
    if (!LIVE.includes(col.state) && !CLOSED.includes(col.state)) continue;
    for (const c of col.cards) if (Date.parse(c.updated_at) > last) out.fresh.add("card:" + c.key);
  }
  if (out.fresh.size) out.lines.push(tn("news.sinceVisit", out.fresh.size));
  return out;
}

export function rememberVisit(nowMs = Date.now()) {
  local.set("lastVisit", new Date(nowMs).toISOString());
}
