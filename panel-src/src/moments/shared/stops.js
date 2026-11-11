// The table of the six stops, in a place any moment may import: moments/registry.js re-exports it for the shell, and the view of a stop reads its
// own question from here instead of naming the key again.

/** In stop order. `key` is the number that goes there (0 is the prologue, 1-5 the others: the `go` and `prologue` keys of ui/keys.js); `name` and `ask` are i18n keys. */
export const STOPS_META = [
  { id: "prologo", key: "0", name: "tab.prologo", ask: "stop.prologo.ask" },
  { id: "hoy", key: "1", name: "tab.hoy", ask: "stop.hoy.ask" },
  { id: "procesos", key: "2", name: "tab.procesos", ask: "stop.procesos.ask" },
  { id: "cola", key: "3", name: "tab.cola", ask: "stop.cola.ask" },
  { id: "aprender", key: "4", name: "tab.aprender", ask: "stop.aprender.ask" },
  { id: "sistema", key: "5", name: "tab.sistema", ask: "stop.sistema.ask" },
];

/** The i18n key of the question a stop asks, as the table names it now. A stop the table does not have is an error that names it. */
export function askOf(id) {
  const stop = STOPS_META.find((s) => s.id === id);
  if (!stop) throw new Error(`askOf: no stop "${id}" in STOPS_META`);
  return stop.ask;
}
