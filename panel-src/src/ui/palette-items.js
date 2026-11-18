// What the command palette holds (Ctrl K): pure and DOM-free, in the language of the page. Three kinds of entry, in this order:
//   goto   the six stops, with their number (Anoche is 0), each the way to a place
//   jobs   every application of the snapshot (the board, what needs him, what awaits his approval, what is put aside): company and role
//   prefs  the settings and the help: the theme, the language, the sound, calm motion, the keys
// An entry is { id, group, label, sub, words, key, tag, current, run }: `label` and `sub` are what the row says, `words` the aliases that find it
// (the aliases are in both languages, so "approve" finds COLA in the Spanish page; the verbs done at a stop are among them, so the stop is the row
// that answers "aprobar"), `key` the key cap on the right, `tag` a word on the right (the
// state of an application, "here" on the stop he is on). `run` calls what the shell gave in `run`, and nothing else: there is no entry that approves,
// snoozes, stops or resumes anything. The palette finds and takes you there; the server is changed from the stops (spec §6.2).
//
//   buildEntries({ stops, cards, current, state, run }) -> entries
//   stops    the table of ui/stops (STOPS_META, in moments/shared/stops.js and re-exported by moments/registry.js): [{ id, key, name, ask }]
//   cards    cardsOf(snapshot)
//   current  the id of the stop on screen
//   state    { theme: "dark" | "light", lang: "es" | "en", sound: bool, motion: "auto" | "calm" | "full" }
//   run      { go(stopId), job(key), theme(), lang(), sound(), motion(), help() }
//   soundEntries  the entries about the sound (ui/sound-entries.js), in place of the single "sound" one; without them the single one is offered

import { t, has } from "../i18n/index.js";

const trimmed = (x) => (x == null ? "" : String(x).trim());

const cardsIn = (list) => (list && Array.isArray(list.cards) ? list.cards : []);

/** The applications of a snapshot, once each by key, in the order the board gives them: the board first, then what needs him, awaits his approval and is put aside. */
export function cardsOf(snap) {
  const out = [];
  if (!snap || typeof snap !== "object") return out;
  const seen = new Set();
  const add = (card) => {
    if (!card || typeof card !== "object" || card.key == null || seen.has(card.key)) return;
    const company = trimmed(card.company);
    const title = trimmed(card.title);
    if (!company && !title) return;                       // nothing to find it by
    seen.add(card.key);
    out.push({ key: card.key, company, title, state: trimmed(card.state) });
  };
  for (const column of Array.isArray(snap.board) ? snap.board : []) for (const card of cardsIn(column)) add(card);
  for (const list of [snap.needs_gonza, snap.awaiting_approval, snap.snoozed]) for (const card of cardsIn(list)) add(card);
  return out;
}

const doesWords = (id) => (has("palette.does." + id) ? t("palette.does." + id) : "");

/** [{ stop, words }]: the verbs that are done at each stop (the palette says where, it does not do them). Only the stops that have some. */
export function actionVerbs(stops) {
  return (stops || []).filter((s) => has("palette.does." + s.id)).map((s) => ({ stop: s.id, words: t("palette.does." + s.id) }));
}

/** One entry per stop, with its number; the one on screen says so. */
function gotoEntries(stops, current, run) {
  return (stops || []).map((s) => {
    const here = s.id === current;
    return { id: "go:" + s.id, group: "goto", label: t(s.name), sub: t(s.ask), words: t("palette.alias." + s.id) + " " + doesWords(s.id), key: s.key, tag: here ? t("palette.here") : "", current: here, run: () => run.go(s.id) };
  });
}

/** One entry per application: its company and role, and its state as the tag. */
function jobEntries(cards, run) {
  return (cards || []).map((card) => ({
    id: "job:" + card.key, group: "jobs", label: card.company || card.title, sub: card.company ? card.title : "", words: "",
    key: "", tag: card.state && has("state." + card.state) ? t("state." + card.state) : "", current: false, run: () => run.job(card.key),
  }));
}

/** The settings and the help: theme, language, sound (the single entry or the family), calm motion, the keys. */
function prefEntries(state, run, soundEntries) {
  const theme = state.theme === "light" ? "light" : "dark";
  const other = state.lang === "en" ? "es" : "en";
  return [
    { id: "pref:theme", group: "prefs", label: t("cmd.theme", { name: t("theme." + (theme === "light" ? "dark" : "light")) }), sub: t("palette.current") + ": " + t("theme." + theme), words: t("palette.alias.theme"), key: "t", tag: "", current: false, run: () => run.theme() },
    { id: "pref:lang", group: "prefs", label: t("cmd.lang", { name: t("lang." + other) }), sub: t("palette.current") + ": " + t("lang." + (state.lang === "en" ? "en" : "es")), words: t("palette.alias.lang"), key: "l", tag: "", current: false, run: () => run.lang() },
    ...(soundEntries || [{ id: "pref:sound", group: "prefs", label: t(state.sound ? "cmd.sound.off" : "cmd.sound.on"), sub: "", words: t("palette.alias.sound"), key: "m", tag: "", current: false, run: () => run.sound() }]),
    { id: "pref:motion", group: "prefs", label: t(state.motion === "calm" ? "cmd.motion.auto" : "cmd.motion.calm"), sub: "", words: t("palette.alias.motion"), key: "", tag: "", current: false, run: () => run.motion() },
    { id: "pref:help", group: "prefs", label: t("ctrl.keys.hint"), sub: "", words: t("palette.alias.help"), key: "?", tag: "", current: false, run: () => run.help() },
  ];
}

export function buildEntries({ stops, cards, current, state, run, soundEntries = null }) {
  return [...gotoEntries(stops, current, run), ...jobEntries(cards, run), ...prefEntries(state, run, soundEntries)];
}
