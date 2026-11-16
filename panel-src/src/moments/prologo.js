// The prologue (spec §2.6): the night in one screen, above HOY, always there in work mode. The copy sits on the left over a veil of
// --night-0 at 55 % (the shell gives `main#app` the veil when `data-moment="prologo"`); the sky is the real hour's, the 03:15 mark of
// the dial is the run. Four figures with their verb (the same `nightFigures` as the receipts of HOY: one function, no number written
// twice), "Anoche" in numbers, and the only voice of a person in the page: "Mientras dormías." If the engine did not run it says so.
//
// `prologoView(model, { nowMs })` is pure (Node tests it); the moment below only draws it, patching and never rebuilding.

import { t, tn, fmtDate } from "../i18n/index.js";
import { nightFigures } from "../core/model.js";
import { ago } from "../core/format.js";
import { h, setText } from "../core/dom.js";
import { HOUR_MS, DAY_MS } from "../core/time.js";
import { setHidden, setData } from "./shared/util.js";

const LONG_SPAN_MS = 20 * HOUR_MS; // a window this long or longer is a whole day: its times carry their date
const FIGURES = ["read", "above", "complete", "awaiting"];

/** What the page says of last night: it ran, it has not run since some time ago, or it has never run. */
function voiceOf({ ran, lastMs, lastRun, nowMs }) {
  if (ran) return t("prologo.voice");
  if (Number.isFinite(lastMs)) return t("prologo.norun", { ago: ago(lastRun, nowMs) });
  return t("prologo.never");
}

/** { eyebrow, title, voice, ran, figures[{ id, n, label, gold? }], base, night, go, goHint }. */
export function prologoView(m, { nowMs }) {
  const snap = m.snap;
  const night = nightFigures(snap);
  const c = snap.controls || {};
  const a = snap.activity || {};
  const lastMs = c.last_run_at ? Date.parse(c.last_run_at) : NaN;
  // Ran last night: the last run is within the last 24 hours. A clock that is wrong (a run "in the future") counts as ran: the run exists.
  const ran = Number.isFinite(lastMs) && nowMs - lastMs <= DAY_MS;
  // The window of `activity` is the last 24 hours, so two bare clock times read "18:30 → 18:30"; from a day long they carry their date.
  const span = Date.parse(a.to) - Date.parse(a.from);
  const stamp = (iso) => fmtDate(iso, span >= LONG_SPAN_MS ? "datetime" : "time");
  return {
    eyebrow: a.from && a.to ? t("prologo.eyebrow", { from: stamp(a.from), to: stamp(a.to) }) : t("prologo.eyebrow.plain"),
    title: t("prologo.title"),
    ran,
    voice: voiceOf({ ran, lastMs, lastRun: c.last_run_at, nowMs }),
    figures: [
      { id: "read", n: night.read, label: t("hoy.fig.read") },
      { id: "above", n: night.above, label: night.threshold == null ? t("hoy.fig.aboveAny") : t("hoy.fig.above", { threshold: night.threshold }) },
      { id: "complete", n: night.complete, label: t("hoy.fig.complete") },
      { id: "awaiting", n: night.awaiting, label: t("hoy.fig.awaiting"), gold: true },
    ],
    base: night.days > 0 ? tn("hoy.fig.base", night.days) : null,
    night: [tn("prologo.seen", night.newPostings), tn("prologo.moves", night.moves)].join(" · "),
    go: t("prologo.go"),
    goHint: t("prologo.go.hint"),
  };
}

/**
 * The four figures inside `dl`, each under its own data-fig. `paint(list)` writes each figure of the view to its place by id; one whose id
 * the page has no place for is left out.
 */
export function createFigures(dl) {
  // A dl reads the term before its description, so the label comes first in the markup; the page draws the number above it.
  const places = new Map(FIGURES.map((id) => [id, { label: h("dt", { class: "fig-l label" }), n: h("dd", { class: "fig-n cond num" }) }]));
  for (const [id, f] of places) dl.append(h("div", { class: "fig", "data-fig": id }, f.label, f.n));
  return {
    paint(list) {
      for (const fig of list) {
        const f = places.get(fig.id);
        if (!f) continue;
        setText(f.n, String(fig.n));
        setText(f.label, fig.label);
        if (fig.gold) setData(f.n, "gold", "true");
      }
    },
  };
}

let S = null;

function mount(host, ctx) {
  if (S) unmount();
  const parts = {
    eyebrow: h("p", { class: "eyebrow label" }),
    title: h("h1", { class: "mega cond", id: "prologo-title", tabindex: "-1" }),
    voice: h("p", { class: "voice serif" }),
    figs: h("dl", { class: "figs" }),
    base: h("p", { class: "base label" }),
    night: h("p", { class: "night label" }),
    go: h("button", { class: "go label", type: "button" }),
  };
  const figures = createFigures(parts.figs);
  parts.go.addEventListener("click", () => ctx.go("hoy"));
  const el = h("section", { class: "prologo", "aria-labelledby": "prologo-title" }, parts.eyebrow, parts.title, parts.voice, parts.figs, parts.base, parts.night, parts.go);
  host.append(el);
  S = { el, parts, figures };
}

function update(model, ctx) {
  if (!S) return;
  const v = prologoView(model, { nowMs: ctx.now() });
  const { parts, figures } = S;
  setText(parts.eyebrow, v.eyebrow);
  setText(parts.title, v.title);
  setText(parts.voice, v.voice);
  figures.paint(v.figures);
  setText(parts.base, v.base || "");
  setHidden(parts.base, !v.base);
  setText(parts.night, v.night);
  setText(parts.go, v.go);
  if (parts.go.title !== v.goHint) parts.go.title = v.goHint;
}

function unmount() {
  if (!S) return;
  S.el.remove();                                    // only its own section: the shell may have mounted the next moment in the same host
  S = null;
}

export const id = "prologo";
export { mount, update, unmount };
export default { id, mount, update, unmount };
