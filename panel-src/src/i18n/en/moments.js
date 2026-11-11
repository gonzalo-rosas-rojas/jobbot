// English, the words of the moments that are not HOY (HOY has its own scope, ./hoy.js): the question each stop answers and the prologue.
// The Spanish twin is ../es/moments.js: same keys, same {variables}.
// Labels are written as they read (spec §4.2): one that stands alone starts with a capital.
export default {
  // ---- the question of each stop (spec §2.4); the name is the `tab.*` key of common.js and hud.js
  "stop.prologo.ask": "What did the bot do last night?",
  "stop.hoy.ask": "What is mine today?",
  "stop.procesos.ask": "Where is each application and what comes next?",
  "stop.cola.ask": "What goes out tonight?",
  "stop.aprender.ask": "Which answer am I still missing?",
  "stop.sistema.ask": "Is the engine alive and which account does it run on?",

  // ---- the prologue (spec §2.6): the night in one screen
  "prologo.eyebrow": "The night on one screen, {from} to {to}",
  "prologo.eyebrow.plain": "The night on one screen",
  "prologo.title": "Last night",
  "prologo.voice": "While you slept.",
  "prologo.norun": "There was no run last night. Last one: {ago}.",
  "prologo.never": "There was no run last night: there has not been one yet.",
  "prologo.seen.zero": "no new postings",
  "prologo.seen.one": "{n} new posting",
  "prologo.seen.other": "{n} new postings",
  "prologo.moves.zero": "no moves",
  "prologo.moves.one": "{n} move",
  "prologo.moves.other": "{n} moves",
  "prologo.go": "Go to Today",
  "prologo.go.hint": "Also with the 1 key",

  // ---- the small menu of ui/menu.js
  "menu.close": "close",
};
