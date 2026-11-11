// The six stops of the page and the module that draws each one (spec §5, §6.1). A moment is, default exported, `{ id, mount(host, ctx),
// update(model, ctx), unmount(), onKey?(actionId, row, event), arrive?(opts), drawer?(detail, card, ctx) }`; the shell mounts it when its stop
// is reached and unmounts it when the stop is left (the others are `inert` and empty). `ctx` is built in ui/shell.js: t, tn, lang, sound,
// settings, session, flags, cursor, view, menu, open(jobKey, origin), go(stopId, opts), reduced(), flat(), now(), announce(text), toast(text, ok, opts),
// drawer { open, close, isOpen, step, reload, key }, model(), themeName(), chooseTheme(name). AGENT-RULES.md explains the contract.
//
// To REPLACE a moment: write its module in this folder and change its line in MOMENTS. To ADD a stop that is not enough: MOMENTS and STOPS_META
// (the table of names, keys and questions that the bar, the dial marks, the palette and the help read; it lives in shared/stops.js and is
// re-exported below) are two of the places that name a stop;
// STOP_IDS and MARKS (scene/poses.js), ROOM_IDS (core/store.js and sound/recipes.js), WIRED (ui/wired.js), the key table (ui/keys.js) and the
// shell's lists of stops (ui/shell.js) are the others. AGENT-RULES.md has the whole list.

import aprender from "./aprender.js";
import cola from "./cola.js";
import hoy from "./hoy.js";
import procesos from "./procesos.js";
import prologo from "./prologo.js";
import sistema from "./sistema.js";

export const MOMENTS = {
  prologo,
  hoy,
  procesos,
  cola,
  aprender,
  sistema,
};

export const momentOf = (id) => MOMENTS[id] || null;

// The table lives in shared/stops.js so that the views can read their question from it without importing this file (which imports them).
export { STOPS_META, askOf } from "./shared/stops.js";
