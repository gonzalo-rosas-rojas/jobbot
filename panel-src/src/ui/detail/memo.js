// What the drawer keeps about the application it shows, beyond one drawing. The drawer's content is drawn again after every
// action, when the language changes and when it steps to another application; a snapshot refresh never redraws it. So the
// things that must outlive a drawing live here: the open or closed state of the folds, the picture (a blob URL that has to
// be revoked), the half-typed note, the refusal the server last gave, the answer he picked and is being asked about, and the
// one he just recorded (the receipt). Everything is about ONE application (`key`); a different one starts clean. Only the
// unsent notes are kept per application, so an accidental Esc does not lose them.

const MAX_DRAFTS = 20;

/** The state of the picture's fetch for `key` (null: none); the zoom always starts fit. status: idle | loading | ready | failed */
export function shotState(key, status, serial, url = null) {
  return { key, url, status, serial, zoom: false };
}

export const memo = {
  key: null,
  folds: new Map(), // fold id -> open?
  shot: shotState(null, "idle", 0),
  slot: null, // the element that shows the picture now (a drawing replaces it)
  refusal: null, // { key, text } what the server said to the last mark it refused
  asking: null, // { key, state } the answer he picked in "Registrar una respuesta", waiting for his yes
  noted: null, // { key, from, to } the answer that went through: the section shows its receipt until he asks for another
  seen: "", // the signature of the answer on screen, to mark a change after an action
  drafts: new Map(), // key -> the note typed and not sent
};

/** Gives back the blob URL of the picture, if there is one. */
export function revokeShot() {
  if (memo.shot.url) URL.revokeObjectURL(memo.shot.url);
}

/** Takes everything about the application that was open away; the blob URL is revoked and a late answer is ignored. */
export function release() {
  revokeShot();
  memo.shot = shotState(null, "idle", memo.shot.serial + 1);
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

/** Keeps the note typed for `key`, the newest last; an empty text forgets it. */
export function setDraft(key, text) {
  if (!text) {
    memo.drafts.delete(key);
    return;
  }
  memo.drafts.delete(key); // the newest goes last, so the oldest is the one dropped
  memo.drafts.set(key, text);
  while (memo.drafts.size > MAX_DRAFTS) memo.drafts.delete(memo.drafts.keys().next().value);
}
