// A stable place for every pile (spec §5.2, decision 4 of §14: the order of the queue is frozen for the session, with a tombstone).
//
// The field is read by position: "the third one of ENVIADA" is where Gonza looks. So the order of each lane is worked out ONCE, when the
// session opens (what is his first, then the one that has waited longest in the stage, then the key), and from then on a refresh may
// change what a pile says but never where it is:
//   - a new application enters at the END of its lane;
//   - one that leaves the lane (it answered, it was rejected, it was withdrawn) leaves a TOMBSTONE of its own size where it was, and
//     travels to the end of its new lane (`moves` tells the page to make the trip);
//   - a tombstone lasts until the page is loaded again (this module's state lives as long as the page: the shell mounts and unmounts
//     the moment, and the places survive).
// Pure: no DOM, no clock.
//
//   const place = createPlace();
//   const { lanes, moves } = place.sync(entries, stateOf);
//     entries   [{ key, lane, days, you, state }]    every application that is in the field now
//     stateOf   (key) -> the state of a card that is no longer in the field (a closed one), or null when the snapshot does not know it
//     cut       (state) -> true when the server listed only some of the cards of that column (its total is higher than its cards): what was last seen
//               there and is now nowhere in the board is probably alive past the cut, so it leaves no tombstone, it just is not drawn
//     lanes     { [laneId]: Slot[] }     Slot = { key } | { tomb: key, to: state | null }
//     moves     [{ key, from, to }]      this sync only; `to` is null when the card left the field

/** The first order of a lane: his first, then by days in the stage (the oldest first), then by key. */
export function initialOrder(entries) {
  return entries.slice().sort((a, b) => (Number(b.you) - Number(a.you)) || (b.days - a.days) || (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
}

export function createPlace() {
  let slots = new Map(); // laneId -> Slot[]
  let started = false;
  const last = new Map(); // key -> the state it had in the last sync

  function sync(list, stateOf = () => null, cut = () => false) {
    const seen = new Map();
    for (const entry of list || []) if (!seen.has(entry.key)) seen.set(entry.key, entry);
    const entries = [...seen.values()];
    const moves = [];
    if (!started) {
      started = true;
      const byLane = new Map();
      for (const entry of entries) byLane.set(entry.lane, [...(byLane.get(entry.lane) || []), entry]);
      for (const [lane, group] of byLane) slots.set(lane, initialOrder(group).map((x) => ({ key: x.key })));
      for (const entry of entries) last.set(entry.key, entry.state);
      return { lanes: Object.fromEntries(slots), moves };
    }
    const placed = new Set();
    for (const [lane, row] of slots) {
      row.forEach((slot, i) => {
        if (slot.tomb == null) {
          const entry = seen.get(slot.key);
          if (entry && entry.lane === lane) { placed.add(slot.key); return; }
          if (!entry && !stateOf(slot.key) && cut(last.get(slot.key))) { row[i] = null; return; } // pushed out of a cut column: alive, only not listed
          row[i] = { tomb: slot.key, to: entry ? entry.state : stateOf(slot.key) };
          moves.push({ key: slot.key, from: lane, to: entry ? entry.lane : null });
        } else {
          const entry = seen.get(slot.tomb);
          if (entry && entry.lane === lane) { row[i] = { key: slot.tomb }; placed.add(slot.tomb); } // it came back: the hole closes
        }
      });
    }
    for (const [lane, row] of slots) if (row.includes(null)) slots.set(lane, row.filter(Boolean));
    for (const entry of entries) last.set(entry.key, entry.state);
    // The newcomers, in the order a first listing would give them, each at the end of its lane.
    const fresh = entries.filter((x) => !placed.has(x.key));
    const lanes = new Map();
    for (const entry of fresh) lanes.set(entry.lane, [...(lanes.get(entry.lane) || []), entry]);
    for (const [lane, group] of lanes) slots.set(lane, [...(slots.get(lane) || []), ...initialOrder(group).map((x) => ({ key: x.key }))]);
    return { lanes: Object.fromEntries(slots), moves };
  }

  return {
    sync,
    reset() { slots = new Map(); started = false; last.clear(); },
  };
}
