// Everything the panel remembers, and the one place that touches web storage.
//
// Two shelves, both under the `panel.` prefix:
//   keep   sessionStorage, JSON. What Gonza was doing in this visit (room, filter, folds); gone with the tab.
//   local  localStorage, plain text. Choices that outlive the visit: theme, language, last visit, sound.
// Storage can be missing or throw (private windows, blocked site data, previews), so every access sits in a
// try/catch and falls back to a Map that lives as long as the page. Nothing here runs at import time.

const PREFIX = "panel.";
const memory = { keep: new Map(), local: new Map() };
let override = null;

/** Tests give fake shelves ({ local, session }, each with getItem/setItem/removeItem); `null` restores the real ones. */
export function useStorage(next) {
  override = next || null;
  memory.keep.clear();
  memory.local.clear();
}

function shelf(kind) {
  try {
    if (override) return override[kind === "keep" ? "session" : "local"] || null;
    return (kind === "keep" ? globalThis.sessionStorage : globalThis.localStorage) || null;
  } catch (e) {
    return null; // reading the property itself can throw when site data is blocked
  }
}

function read(kind, name) {
  try {
    const box = shelf(kind);
    if (box) return box.getItem(PREFIX + name);
  } catch (e) { /* fall through to memory */ }
  return memory[kind].has(name) ? memory[kind].get(name) : null;
}

function write(kind, name, text) {
  try {
    const box = shelf(kind);
    if (box) { box.setItem(PREFIX + name, text); return; }
  } catch (e) { /* fall through to memory */ }
  memory[kind].set(name, text);
}

function drop(kind, name) {
  memory[kind].delete(name);
  try {
    const box = shelf(kind);
    if (box) box.removeItem(PREFIX + name);
  } catch (e) { /* nothing to remove */ }
}

export const keep = {
  get(name) {
    try { return JSON.parse(read("keep", name) || "null"); } catch (e) { return null; }
  },
  set(name, value) {
    write("keep", name, JSON.stringify(value));
  },
};

export const local = {
  get: (name) => read("local", name),
  set: (name, text) => write("local", name, String(text)),
  remove: (name) => drop("local", name),
};

// ---- Theme: the system's until someone chooses; the choice is kept on this machine

export const THEMES = ["auto", "light", "dark"];

export function nextTheme(current) {
  const at = THEMES.indexOf(current);
  return THEMES[(at < 0 ? 0 : at + 1) % THEMES.length];
}

export function loadTheme() {
  const saved = local.get("theme");
  return saved === "light" || saved === "dark" ? saved : "auto";
}

export function saveTheme(name) {
  if (name === "auto") local.remove("theme");
  else local.set("theme", name);
}
