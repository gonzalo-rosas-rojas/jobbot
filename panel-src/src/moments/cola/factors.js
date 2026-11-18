// The two lists of the proposal in focus that come from what the server says of it: the six bars of its score (they load after the proposal does) and the
// path of the five layers it went through. Each is rebuilt only when what it says changes.
//
//   const factors = createFactors();   factors.bars (ul)   factors.path (ol)
//   factors.patchBars(focus)   factors.patchPath(focus)   factors.relabel()

import { h } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { setAttr } from "../shared/util.js";

const SKELETON_BARS = 6;

export function createFactors() {
  const bars = h("ul", { class: "bars", "aria-label": t("cola.factors.aria") });
  const path = h("ol", { class: "path mono", "aria-label": t("cola.path.aria") });
  let barsSig = "";
  let pathSig = "";

  return {
    bars,
    path,
    patchBars(f) {
      const sig = JSON.stringify([f.factors, f.factorsError, f.factorsNone, t("common.retry")]);
      if (sig === barsSig) return;
      barsSig = sig;
      bars.setAttribute("aria-busy", String(f.factors.state === "loading"));
      if (f.factors.state === "loading") {
        bars.replaceChildren(...Array.from({ length: SKELETON_BARS }, () => h("li", { class: "f-bar skel", "aria-hidden": "true" }, h("span", { class: "f-name" }), h("i", { class: "track" }), h("span", { class: "f-val" }))));
      } else if (f.factors.state === "error") {
        bars.replaceChildren(h("li", { class: "bars-msg label", role: "alert" }, f.factorsError, " ", h("button", { class: "plane-retry label", type: "button", "data-retry": "" }, t("common.retry"))));
      } else if (f.factors.state === "none") {
        bars.replaceChildren(h("li", { class: "bars-msg label" }, f.factorsNone));
      } else {
        bars.replaceChildren(...f.factors.bars.map((b) => h("li", { class: "f-bar", "data-factor": b.id },
          h("span", { class: "f-name label" }, b.label),
          h("i", { class: "track" }, h("b", { style: { "--k": b.k.toFixed(3) } })),
          h("span", { class: "f-val mono num" }, b.text))));
      }
    },
    patchPath(f) {
      const sig = JSON.stringify([f.path, t("layer.aviso")]);
      if (sig === pathSig) return;
      pathSig = sig;
      path.replaceChildren(...f.path.map((l) => h("li", { "data-layer": l.id, "data-state": l.state, "aria-current": l.state === "here" ? "step" : null },
        h("b", { class: "l-name" }, t("layer." + l.id)),
        h("span", { class: "l-note" }, l.note, l.here ? [" · ", h("span", { class: "gold" }, l.here)] : null))));
    },
    /** The names that screen readers read for the two lists: they follow the language. */
    relabel() {
      setAttr(bars, "aria-label", t("cola.factors.aria"));
      setAttr(path, "aria-label", t("cola.path.aria"));
    },
  };
}
