// The trip of a pile that changed lane (the shared element of spec §3.3): where each one stands before the page is patched, and, once it is
// patched, the slide from there to its new place. Measured after the move and played with the page's own clock (the Web Animations API).
//
//   const before = positionsBefore(view.moves, piles);   // BEFORE patching: id -> the box of each pile that is going to move
//   ... patch ...
//   travelTo(before, piles, reduced);                    // AFTER: slides each one from its old box to its new one

import { rowIdOf } from "./view.js";

export const TRAVEL_MS = 420;
const TRAVEL_EASE = "cubic-bezier(0.2, 0, 0, 1)"; // --ease-jump
const TRAVEL_MAX_SCREENS = 1.5; // a pile that would travel farther than this many screens is too far to follow with the eye: it simply arrives

/** The box of each pile named by `moves` that is on screen now: Map id -> DOMRect. */
export function positionsBefore(moves, piles) {
  const before = new Map();
  for (const m of moves) {
    const p = piles.get(rowIdOf(m.key));
    if (p && p.el.isConnected) before.set(p.id, p.el.getBoundingClientRect());
  }
  return before;
}

/** The pile that changed lane goes from where it was to where it is (a shared element): measured after the move, played with the page's own clock. */
export function travelTo(before, piles, reduced) {
  if (reduced()) return;
  for (const [id, from] of before) {
    const pile = piles.get(id);
    if (!pile || !pile.el.isConnected || typeof pile.el.animate !== "function") continue;
    const to = pile.el.getBoundingClientRect();
    const dx = from.left - to.left;
    const dy = from.top - to.top;
    if ((dx === 0 && dy === 0) || Math.abs(dy) > window.innerHeight * TRAVEL_MAX_SCREENS) continue;
    pile.el.dataset.travel = "true";
    const anim = pile.el.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], { duration: TRAVEL_MS, easing: TRAVEL_EASE });
    const done = () => { delete pile.el.dataset.travel; };
    anim.addEventListener("finish", done);
    anim.addEventListener("cancel", done);
  }
}
