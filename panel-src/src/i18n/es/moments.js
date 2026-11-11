// Spanish, the words of the moments that are not HOY (HOY has its own scope, ./hoy.js): the question each stop answers and the prologue.
// The English twin is ../en/moments.js: same keys, same {variables}.
// Labels are written as they read (spec §4.2): one that stands alone starts with a capital. Warm and plain, "tú".
export default {
  // ---- the question of each stop (spec §2.4); the name is the `tab.*` key of common.js and hud.js
  "stop.prologo.ask": "¿Qué hizo el bot anoche?",
  "stop.hoy.ask": "¿Qué me toca hoy?",
  "stop.procesos.ask": "¿Dónde está cada postulación y qué sigue?",
  "stop.cola.ask": "¿Qué sale esta noche?",
  "stop.aprender.ask": "¿Qué respuesta me sigue faltando?",
  "stop.sistema.ask": "¿Está vivo el motor y con qué cuenta corre?",

  // ---- the prologue (spec §2.6): the night in one screen
  "prologo.eyebrow": "La noche en una pantalla, de {from} a {to}",
  "prologo.eyebrow.plain": "La noche en una pantalla",
  "prologo.title": "Anoche",
  "prologo.voice": "Mientras dormías.",
  "prologo.norun": "Anoche no hubo corrida. Última: {ago}.",
  "prologo.never": "Anoche no hubo corrida: todavía no hay ninguna.",
  "prologo.seen.zero": "ningún aviso nuevo",
  "prologo.seen.one": "{n} aviso nuevo",
  "prologo.seen.other": "{n} avisos nuevos",
  "prologo.moves.zero": "ningún movimiento",
  "prologo.moves.one": "{n} movimiento",
  "prologo.moves.other": "{n} movimientos",
  "prologo.go": "Ir a HOY",
  "prologo.go.hint": "También con la tecla 1",

  // ---- the small menu of ui/menu.js
  "menu.close": "cerrar",
};
