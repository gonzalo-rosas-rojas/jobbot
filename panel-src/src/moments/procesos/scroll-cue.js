// The cue under PROCESOS' list: how many piles are below, where the thumb of the scroll track is, and the lift that keeps the row under the
// cursor out of the fades at the edges of the list. It reads and writes the page's own nodes (handed in), and owns two observers and a scroll listener.
//
//   const cue = createScrollCue({ scroller, viewbox, cueEl, thumb, observed: [field, tbody], reduced, raf });
//   cue.queue()      measures on the next frame (once, however many times it is asked for)
//   cue.destroy()    removes the scroll listener and disconnects the observers

import { t } from "../../i18n/index.js";
import { setData } from "../shared/util.js";
import { setText } from "../../core/dom.js";

const EDGE_PX = 2;         // a list this close to an edge counts as at it, and one this little taller than its box does not scroll
const SLACK_PX = 1;        // the cursor's row is lifted out of the fades only when it is more than this far into one
const MIN_THUMB_PCT = 6;   // the thumb of the scroll track never gets smaller than this share of the track
const FADE = 64;           // the height of the fade at the bottom of the list (css), in px
const FADE_TOP = 22;       // and of the one at the top

export function createScrollCue({ scroller, viewbox, cueEl, thumb: thumbEl, observed, reduced, raf }) {
  let queued = false;

  // All the reads first, all the writes after: a write between two reads makes the next read recalculate styles on the spot, and a pile of PROCESOS
  // has about a thousand elements (that was 16 ms at CPU x4 for each read that followed a write, in the frame right after a trip's swap: ACEPTACION A7).
  function measure() {
    queued = false;
    const box = scroller;
    const max = box.scrollHeight - box.clientHeight;
    const can = max > EDGE_PX;
    const top = box.scrollTop;
    const atEnd = !can || top >= max - EDGE_PX;
    let below = 0;
    let thumb = null;
    if (can) {
      const bottom = box.getBoundingClientRect().bottom;
      for (const node of box.querySelectorAll("[data-card]")) if (node.getBoundingClientRect().top >= bottom - EDGE_PX) below += 1;
      thumb = { top: ((top / box.scrollHeight) * 100).toFixed(2) + "%", height: Math.max(MIN_THUMB_PCT, (box.clientHeight / box.scrollHeight) * 100).toFixed(2) + "%" };
    }
    setData(viewbox, "canScroll", can ? "true" : "false");
    setData(viewbox, "atStart", top <= EDGE_PX ? "true" : "false");
    setData(viewbox, "atEnd", atEnd ? "true" : "false");
    if (can) {
      setText(cueEl, [below > 0 ? t("procesos.cue.below", { n: below }) : "", t("procesos.legend.wheel")].filter(Boolean).join(" · "));
      thumbEl.style.setProperty("top", thumb.top);
      thumbEl.style.setProperty("height", thumb.height);
    }
  }
  const queue = () => { if (!queued) { queued = true; raf(measure); } };
  scroller.addEventListener("scroll", queue, { passive: true });
  const resizer = typeof ResizeObserver === "function" ? new ResizeObserver(queue) : null;
  if (resizer) { resizer.observe(scroller); for (const node of observed) resizer.observe(node); }

  // The bottom of the list fades out (css) and the shared cursor only brings a row to the EDGE of the list, which is inside the fade: so the
  // row under the cursor is lifted out of it, and a row that is walked to is always clear to read.
  const lifter = typeof MutationObserver === "function" ? new MutationObserver(() => {
    if (viewbox.dataset.canScroll !== "true") return;
    const row = scroller.querySelector('[data-cursor="true"]');
    if (!row) return;
    const at = row.getBoundingClientRect();
    const box = scroller.getBoundingClientRect();
    const under = box.bottom - FADE - at.bottom; // negative: the row is in the fade at the bottom
    const above = box.top + FADE_TOP - at.top;   // positive: the row is in the fade at the top
    let top = 0;
    if (under < -SLACK_PX) top = -under;
    else if (above > SLACK_PX) top = -above;
    if (top) scroller.scrollBy({ top, behavior: reduced() ? "auto" : "smooth" });
  }) : null;
  if (lifter) lifter.observe(scroller, { subtree: true, attributes: true, attributeFilter: ["data-cursor"] });

  return {
    queue,
    destroy() {
      scroller.removeEventListener("scroll", queue);
      if (resizer) resizer.disconnect();
      if (lifter) lifter.disconnect();
    },
  };
}
