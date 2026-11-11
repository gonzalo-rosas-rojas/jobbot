// What the drawer keeps about the application it shows, beyond one drawing. The drawer's content is drawn again after every
// action, when the language changes and when it steps to another application; a snapshot refresh never redraws it. So the
// things that must outlive a drawing live here: the open or closed state of the folds, the picture (a blob URL that has to
// be revoked), the half-typed note, the refusal the server last gave, the answer he picked and is being asked about, and the
// one he just recorded (the receipt). Everything is about ONE application (`key`); a different one starts clean. Only the
// unsent notes are kept per application, so an accidental Esc does not lose them.

const MAX_DRAFTS = 20;

export const memo = {
  key: null,
  folds: new Map(), // fold id -> open?
  shot: { key: null, url: null, status: "idle", serial: 0, zoom: false }, // status: idle | loading | ready | failed
  slot: null, // the element that shows the picture now (a drawing replaces it)
  refusal: null, // { key, text } what the server said to the last mark it refused
  asking: null, // { key, state } the answer he picked in "Registrar una respuesta", waiting for his yes
  noted: null, // { key, from, to } the answer that went through: the section shows its receipt until he asks for another
  seen: "", // the signature of the answer on screen, to mark a change after an action
  drafts: new Map(), // key -> the note typed and not sent
};

/** Takes everything about the application that was open away; the blob URL is revoked and a late answer is ignored. */
export function release() {
  const shot = memo.shot;
  if (shot.url) URL.revokeObjectURL(shot.url);
  memo.shot = { key: null, url: null, status: "idle", serial: shot.serial + 1, zoom: false };
  memo.slot = null;
  memo.key = null;
  memo.folds.clear();
  memo.refusal = null;
  memo.asking = null;
  memo.noted = null;
  memo.seen = "";
}

/** Makes `key` the application on screen (the one before is released first). */
export function adopt(key) {
  if (memo.key === key) return false;
  release();
  memo.key = key;
  return true;
}

export function draftOf(key) {
  return memo.drafts.get(key) || "";
}

export function keepDraft(key, text) {
  if (!text) {
    memo.drafts.delete(key);
    return;
  }
  memo.drafts.delete(key); // the newest goes last, so the oldest is the one dropped
  memo.drafts.set(key, text);
  while (memo.drafts.size > MAX_DRAFTS) memo.drafts.delete(memo.drafts.keys().next().value);
}
