// The request of a verb on a row, for HOY and COLA (they each had a copy, and the copies had begun to differ). The row is `pending` from the
// first instant (R7 §2.9: it answers in under 100 ms, the server in its own time) and is freed at the end, or at once when the server says no;
// the cue of the verb sounds on a yes and the error cue on a no; and a verb that has a way back leaves a toast with "Deshacer".
//
//   const send = createSender({ session, sound, pending, onChange, requestOf, cue, undoOf, toast, t });
//   await send(actionId, row, extra)  -> "fired" | "ignored" ("ignored": the row cannot take that verb, nothing was sent)
//
// `pending` is the room's Set of row ids that wait for an answer (the room reads it to dim the row); `onChange(rowId)` hears every change of it.
// `requestOf(actionId, row, extra)` -> { path, body } | null is the room's own table of verbs. `cue` maps a verb to the sound of its yes.
// `undoOf(actionId, row)` -> { path, body, words } | null says which verbs have a way back and what it asks; `words` is the toast's own sentence
// (an i18n key), used when the server's answer has none. `toast(text, ok, { undo })` is the shell's; without it nothing offers "Deshacer".

const UNDO_CUE = "undo";

export function createSender({ session, sound, pending, onChange, requestOf, cue, undoOf, toast = null, t }) {
  const serverWords = (r) => (session.said ? session.said(r, "") : "");

  /** The row answers again: it is not waiting any more. */
  function release(rowId) {
    pending.delete(rowId);
    onChange(rowId);
  }

  /** The toast of the server's own words with "Deshacer", for a verb that has a way back. */
  function offerUndo(actionId, row, r) {
    const back = undoOf(actionId, row);
    if (!back || !toast) return;
    const undo = () => session.act(back.path, back.body, { beforeRefresh: (u) => sound.play(u.ok ? UNDO_CUE : "error") });
    toast(serverWords(r) || t(back.words), true, { undo });
  }

  return async function send(actionId, row, extra) {
    const request = requestOf(actionId, row, extra);
    if (!request) return "ignored";
    pending.add(row.id);
    onChange(row.id);
    try {
      await session.act(request.path, request.body, {
        onOk: (r) => offerUndo(actionId, row, r),
        beforeRefresh: (r) => {
          if (r.ok) {
            sound.play(cue[actionId] || "press");
          } else {
            release(row.id); // the server said no (a 409 carries its reason, the toast already has it): the row is his again
            sound.play("error");
          }
        },
      });
    } finally {
      release(row.id); // the snapshot after the 200 has already redrawn without the row, or with it: either way it is not waiting
    }
    return "fired";
  };
}
