// The closed list of icons (spec §4.7) and the one way to draw one. The shapes are a sprite that tools/icons.mjs builds from
// Lucide and the page template inlines; the page only points at a symbol of it. The list lives HERE and the tool reads it,
// so a name cannot be drawn without being in the sprite, and nothing is in the sprite that the code cannot name. An icon is
// never decoration: it stands next to a word or a key that says the same thing.

import { sv } from "../core/dom.js";

export const ICONS = [
  "sun", "moon", "clock", "alarm-clock", "calendar",            // time and theme
  "search", "keyboard", "command", "list", "funnel",            // finding and keys
  "arrow-up", "arrow-down", "arrow-left", "arrow-right",         // direction
  "check", "x", "plus", "minus", "pause", "play", "refresh-cw",  // actions
  "send", "mail", "external-link", "file-text", "folder",        // things
  "triangle-alert", "info", "settings", "user", "star",          // state
  "volume-2", "volume-x",                                        // sound
];

/** <svg class="icon [cls]"><use href="#i-name"/></svg>. 16 px in rows, 20 px with `cls = "icon-lg"`; the colour is the text's. */
export function icon(name, cls = "") {
  if (!ICONS.includes(name)) throw new TypeError(`icon(): "${name}" is not one of the ${ICONS.length} icons of ui/icons.js`);
  return sv("svg", { class: ["icon", cls], "aria-hidden": "true", focusable: "false" }, sv("use", { href: "#i-" + name }));
}
