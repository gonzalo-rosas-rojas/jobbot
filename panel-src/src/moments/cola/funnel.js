// COLA's funnel of the last days (spec §5.3): five figures and the band between them. Built once, patched after: a write only when it differs.
//
//   const funnel = createFunnel();   funnel.el (section.funnel)   funnel.patch(view)   funnel.relabel()

import { h, sv, setText } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { setData, setHidden, setAttr } from "../shared/util.js";

const BAND_BOX_HALF = 15;   // half of the 30-high box the band is drawn on: where a figure of zero sits
const BAND_AMPLITUDE = 14;  // how far a figure of one reaches from that middle
const BAND_FIGURES = [0, 250, 500, 750, 1000]; // the x of each of the five figures on the 1000-wide box
const BAND_HANDLE = 125;    // the pull of each curve: half the gap between two figures

/** The band between the five figures: half of its height at each one, drawn on a 1000 x 30 box that the page stretches. */
export function bandPath(log) {
  const xs = BAND_FIGURES;
  const last = xs.length - 1;
  const top = (i) => (BAND_BOX_HALF - log[i] * BAND_AMPLITUDE).toFixed(1);
  const bot = (i) => (BAND_BOX_HALF + log[i] * BAND_AMPLITUDE).toFixed(1);
  let d = "M" + xs[0] + " " + top(0);
  for (let i = 1; i <= last; i++) d += " C" + (xs[i - 1] + BAND_HANDLE) + " " + top(i - 1) + " " + (xs[i] - BAND_HANDLE) + " " + top(i) + " " + xs[i] + " " + top(i);
  d += " L" + xs[last] + " " + bot(last);
  for (let i = last - 1; i >= 0; i--) d += " C" + (xs[i + 1] - BAND_HANDLE) + " " + bot(i + 1) + " " + (xs[i] + BAND_HANDLE) + " " + bot(i) + " " + xs[i] + " " + bot(i);
  return d + "Z";
}

export function createFunnel() {
  const parts = {
    title: h("h2", { class: "block-title label" }),
    figs: h("dl", { class: "fn" }),
    band: sv("svg", { class: "band", viewBox: "0 0 1000 30", preserveAspectRatio: "none", "aria-hidden": "true" }, sv("path", { d: "" })),
    none: h("p", { class: "none label" }),
  };
  const figEls = ["read", "above", "cv", "form", "sent"].map((id) => ({ id, n: h("dd", { class: "fig-n cond num" }), label: h("dt", { class: "fig-l label" }), sub: h("dd", { class: "fig-s label" }) }));
  figEls.forEach((f) => parts.figs.append(h("div", { class: "fig", "data-fig": f.id }, f.label, f.n, f.sub)));
  const el = h("section", { class: "funnel" }, parts.title, parts.figs, parts.band, parts.none);

  return {
    el,
    patch(view) {
      const fn = view.funnel;
      setText(parts.title, fn.title);
      setHidden(parts.figs, fn.empty);
      setAttr(parts.band, "hidden", fn.empty ? "" : null); // an SVG has no `hidden` property
      setText(parts.none, fn.empty ? fn.none : "");
      setHidden(parts.none, !fn.empty);
      fn.figs.forEach((fig, i) => {
        const f = figEls[i];
        setText(f.n, String(fig.n));
        setText(f.label, fig.label);
        setText(f.sub, fig.sub);
        setHidden(f.sub, !fig.sub);
        setData(f.n, "last", fig.last ? "true" : null);
      });
      setAttr(parts.band.firstChild, "d", bandPath(fn.log));
    },
    /** The name a screen reader gives the funnel follows the language. */
    relabel() { setAttr(el, "aria-label", t("cola.funnel.aria")); },
  };
}
