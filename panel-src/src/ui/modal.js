// What the palette, the help and the sound sheet do the same way as modal dialogs: the veil behind the box (a press on it closes, the wheel
// over it never moves the page behind). Esc (closeOnEscape) is shared by the palette and the sound sheet; the help does not use it: its Esc
// arrives through the page's global handler (shell-keys.js, "close"). What each one does differently stays in its own file: where the Tab key goes, what else
// a key does, and what the dialog hands back when it closes (the focus goes back through ui/focus-return.js).

import { h } from "../core/dom.js";

/**
 * The veil that holds `dialog`: `cls` is its class, `onClose` runs when a press lands on the veil itself and not on the dialog.
 * It is `data-lenis-prevent` and its wheel is the `scroller`'s or nobody's: Lenis ignores the attribute and the browser scrolls what the wheel
 * is over, so a wheel over the veil, or over `scroller` when `holdsWhenItFits` and it has nothing to scroll, must not move the page behind
 * (the stage's camera).
 */
export function modalVeil({ cls, dialog, onClose, scroller = dialog, holdsWhenItFits = true }) {
  const veil = h("div", { class: cls, "data-lenis-prevent": "", onmousedown: (e) => { if (e.target === veil) onClose(); } }, dialog);
  veil.addEventListener("wheel", (e) => {
    const outside = !scroller.contains(e.target);
    if (outside || (holdsWhenItFits && scroller.scrollHeight <= scroller.clientHeight)) e.preventDefault();
  }, { passive: false });
  return veil;
}

/** Esc closes the dialog and nothing else hears it (the page's own Esc closes the drawer). True when the key was Esc. */
export function closeOnEscape(e, close) {
  if (e.key !== "Escape") return false;
  e.preventDefault();
  e.stopPropagation();
  close();
  return true;
}
