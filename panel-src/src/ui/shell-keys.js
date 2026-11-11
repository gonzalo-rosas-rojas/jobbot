// What each key of the table (ui/keys.js) does on the page. The shell hands these to `installKeys`; this file is the one place that
// says who owns a verb:
//   the page   1-5, 0, Ctrl K, ?, t, m, l and Esc: they mean the same everywhere (spec §3.4). Ctrl K opens the palette, unless the help or a menu
//              has the keyboard (one layer at a time; the sound's sheet "Escuchar los sonidos" gives way to it); with the palette open it only
//              brings the focus back to its field. A slash is NOT the
//              palette's: PROCESOS and COLA keep it for their own finder (the key table has one owner for it)
//   the stop   every other verb goes to the moment on screen: `onKey(verb, row, event)` with the row the cursor is on (the element, or
//              null), and the moment answers `true` when the key was its own. A moment that does not know a verb leaves the key to the
//              page's default for it: j and k move the keyboard cursor, Enter opens the application under it.
//   the drawer j and k change the application shown while it is open, and Esc closes it (after the help, the palette and the menu).

import { momentOf } from "../moments/registry.js";
import { STOP_IDS } from "../scene/poses.js";

// The verbs of a row or of a stop (spec §3.4): all of them travel to the moment.
export const MOMENT_VERBS = ["move", "open", "approve", "confirm", "resolve", "withdraw", "snooze", "register", "done", "newtask", "link", "filter", "find", "field", "batch", "engine"];

/**
 * createKeyHandlers({ stop, cursor, drawer, help, menu, palette, tools, sheet, go, cancelJump }) -> { [verb]: (key, event) => void }
 * `sheet` is the sound's sheet ({ isOpen, close }); a page without one (a lab) leaves it out.
 * `stop()` is the stop on screen; `go(id)` travels; `cancelJump()` stops a trip in the air.
 */
export function createKeyHandlers({ stop, cursor, drawer, help, menu, palette, tools, sheet = null, go, cancelJump }) {
  function byDefault(verb, key) {
    if (verb === "move") cursor.move(key === "j" ? 1 : -1);
    else if (verb === "open") {
      const row = cursor.current();
      const job = cursor.jobOf(row);
      if (job) drawer.open(job, row);
    }
  }

  const toMoment = (verb) => (key, event) => {
    if (verb === "move" && drawer.isOpen()) { drawer.step(key === "j" ? 1 : -1); return; }
    const moment = momentOf(stop());
    if (moment && typeof moment.onKey === "function" && moment.onKey(verb, cursor.current(), event) === true) return;
    byDefault(verb, key);
  };

  const handlers = {
    go: (key) => go(STOP_IDS[Number(key)]),
    prologue: () => go("prologo"),
    search: () => {
      if (help.isOpen() || menu.isOpen()) return;
      if (sheet && sheet.isOpen()) sheet.close();
      palette.open();
    },
    help: () => help.toggle(),
    theme: () => tools.theme(),
    sound: () => tools.sound(),
    lang: () => tools.lang(),
    // One layer per press, in the order of the spec: the sound sheet, the help, the palette, the menu, the drawer, and last the cursor. A trip in the air stops first.
    close: () => {
      cancelJump();
      if (sheet && sheet.isOpen()) sheet.close();
      else if (help.isOpen()) help.close();
      else if (palette.isOpen()) palette.close();
      else if (menu.isOpen()) menu.close();
      else if (drawer.isOpen()) drawer.close();
      else cursor.clear();
    },
  };
  for (const verb of MOMENT_VERBS) handlers[verb] = toMoment(verb);
  return handlers;
}
