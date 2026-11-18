// Where a room looks a card up: the snapshot keeps the same application in the columns of the board, among the proposals that wait for his
// yes and among the ones that wait for him. The list of those places is written here once; what a room does with a key that is in two of
// them is its own rule, and the two rules are named below: HOY uses the index (the last one wins) because it draws what waits for him, and
// PROCESOS the single lookup (the first one wins) because it draws the board.

/** Every card of the places a card can be in, in the order of the sheet: the board's columns, the proposals, the ones that need him. */
export function cardLists(snap) {
  const board = (snap.board || []).flatMap((col) => col.cards || []);
  return [...board, ...((snap.awaiting_approval || {}).cards || []), ...((snap.needs_gonza || {}).cards || [])];
}

/** The cards of a snapshot by key. A key that is in two places is the card of the LAST one (a proposal or a stuck one wins over its board column). HOY: its rows and its forms both read it, so they see the same card. */
export function indexCards(snap) {
  const map = new Map();
  for (const c of cardLists(snap)) map.set(c.key, c);
  return map;
}

/** A card of the model by its key, or null. A key that is in two places is the card of the FIRST one (its board column wins). PROCESOS, which draws the board. */
export function findCard(model, key) {
  if (!model) return null;
  return cardLists(model.snap).find((c) => c.key === key) || null;
}
