// The screen-reader voice of the shell: one polite live region, off screen. A room change, a count that moved and the
// news of a snapshot are said here so that what the lit windows and the camera show is also said in words.

import { h } from "../core/dom.js";

export function createAnnouncer() {
  const el = h("div", { id: "live", class: "sr", role: "status", "aria-live": "polite", "aria-atomic": "true" });
  let timer = 0;
  return {
    el,
    /** Says `text`. Cleared first, so the same sentence twice in a row is still read. */
    say(text) {
      clearTimeout(timer);
      el.textContent = "";
      timer = setTimeout(() => { el.textContent = text; }, 30);
    },
  };
}
