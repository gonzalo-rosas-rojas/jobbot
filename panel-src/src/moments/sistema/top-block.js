// The top of SISTEMA: where this is, when the engine last ran with its chip, the two figures (the next run, the sends of 24 h) and the strip of the
// night's window with its marks. What Claude cost is not a figure: the account's row says it once.
//
//   const top = createTopBlock();
//   top.els                 [header.top, div.headline, div.receipts], in the order they go in the page
//   top.patch(view)

import { h, setText } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { setData, setHidden, setTitle, stampChanged } from "../shared/util.js";
import { pct, dot, placed } from "./parts.js";

export function createTopBlock() {
  // ---- the top: where this is, and the window and STOP as the snapshot says them
  const parts = {
    eyebrow: h("p", { class: "eyebrow label" }),
    status: h("p", { class: "status label" }),
    lead: h("span", { class: "lead label" }),
    tail: h("span", { class: "tail cond" }),
    chipText: h("span", { class: "chip-text" }),
    chipStop: h("span", { class: "chip-stop" }),
    figs: h("dl", { class: "figs" }),
    stripTitle: h("p", { class: "strip-title label" }),
    stripBox: h("div", { class: "strip-box", role: "img" }),
    stripTicks: h("div", { class: "strip-ticks mono", "aria-hidden": "true" }),
    stripMarks: h("p", { class: "strip-marks label" }),
  };
  parts.h1 = h("h1", { class: "mega", id: "sistema-title", tabindex: "-1" }, parts.lead, " ", parts.tail);
  const chip = h("p", { class: "chip label" }, dot(), parts.chipText, parts.chipStop);
  const figEls = ["next", "sends"].map((id) => ({ id, n: h("dd", { class: "fig-n cond num" }), label: h("dt", { class: "fig-l label" }) }));
  // dl > div > dt + dd: the label first in the markup (what a screen reader reads first), the number first on the screen (CSS order)
  figEls.forEach((f) => parts.figs.append(h("div", { class: "fig", "data-fig": f.id }, f.label, f.n)));

  // ---- the strip: bars and marks are plain boxes placed by percent (custom properties), so nothing is stretched
  parts.stripBox.append(h("span", { class: "strip-base", "aria-hidden": "true" }));
  const stripLayers = { busy: h("div", { class: "strip-busy" }), bars: h("div", { class: "strip-bars" }), marks: h("div", { class: "strip-dots" }) };
  parts.stripBox.append(stripLayers.busy, stripLayers.bars, stripLayers.marks);

  const els = [
    h("header", { class: "top" }, parts.eyebrow, parts.status),
    h("div", { class: "headline" }, parts.h1, chip),
    h("div", { class: "receipts" },
      parts.figs,
      h("div", { class: "strip-wrap" }, parts.stripTitle, parts.stripBox, parts.stripTicks, parts.stripMarks)),
  ];

  function patchStrip(strip) {
    setText(parts.stripTitle, strip.title);
    setTitle(parts.stripBox, strip.legend);
    if (parts.stripBox.getAttribute("aria-label") !== strip.aria) parts.stripBox.setAttribute("aria-label", strip.aria);
    const ticks = strip.hours.map((x) => {
      const node = h("span", { class: "tick" }, x.label);
      node.style.setProperty("--at", pct(x.pct));
      return node;
    });
    if (stampChanged(parts.stripTicks, strip.hours.map((x) => x.label + x.pct).join("|"))) parts.stripTicks.replaceChildren(...ticks);
    if (stampChanged(stripLayers.bars, JSON.stringify([strip.busy, strip.bars, strip.marks.map((m) => [m.id, m.pct])]))) {
      stripLayers.busy.replaceChildren(...strip.busy.map((b) => placed("busy", { "--at": pct(b.pct), "--w": pct(b.w) })));
      stripLayers.bars.replaceChildren(...strip.bars.map((b) => placed("bar", { "--at": pct(b.pct), "--w": pct(b.w), "--h": pct(b.h * 100) })));
      stripLayers.marks.replaceChildren(...strip.marks.map((m) => placed("mark", { "--at": pct(m.pct) }, { "data-mark": m.id })));
    }
    const legend = strip.marks.filter((m) => m.id !== "now");
    if (stampChanged(parts.stripMarks, legend.map((m) => m.id + m.label).join("|") + "|" + (strip.empty || ""))) {
      parts.stripMarks.replaceChildren(...(strip.empty ? [h("span", { class: "none" }, strip.empty)] : []),
        ...legend.map((m) => h("span", { class: "key", "data-mark": m.id }, h("i", { class: "swatch", "aria-hidden": "true" }), m.label)));
    }
  }

  function patch(view) {
    setText(parts.eyebrow, view.eyebrow);
    setText(parts.status, [view.status, view.old].filter(Boolean).join(" · "));
    setText(parts.lead, view.head.lead);
    setText(parts.tail, view.head.tail);
    setData(chip, "tone", view.chip.tone);
    setText(parts.chipText, view.chip.text);
    setText(parts.chipStop, view.chip.stop ? t("run.stopped") : "");
    setHidden(parts.chipStop, !view.chip.stop);
    view.figures.forEach((f, i) => {
      const fe = figEls[i];
      setData(fe.n, "words", f.words ? "true" : null);
      setText(fe.label, f.label);
      setText(fe.n, f.value);
    });
    patchStrip(view.strip);
  }

  return { els, patch };
}
