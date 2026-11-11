// How a query ranks what the palette holds (Ctrl K): pure and DOM-free. The entries come from ui/palette-items.js, in three groups (the stops, the
// applications, the settings); this module only chooses and orders them.
//
//   - With nothing typed the palette is a menu: every stop and every setting, in their order, and no application (a hundred rows are no menu). An entry
//     marked `quiet` (the second-level settings, the sound's family) waits for a word.
//   - With something typed, uFuzzy looks in each group for the words, over "label sub aliases" with no accents and no capitals (the applications:
//     "company role"), and the best rows of the three groups are taken in turn until MAX_RESULTS, so a company called "Cola" does not hide the stop.
//   - What matched comes back as parts ({ text, hit }) of the label and of the sub-line: the page turns them into text nodes and <mark> elements.
//     Nothing here ever builds markup, so a company called "<img onerror=...>" is letters, and so is the query.
//   - A query whose first word sounds like an action ("aprobar", "stop") also gets `hint`: { stop, word }, the stop where that is done. The palette
//     does not do it: it says so (spec §6.2: it finds and takes you there; the server is changed from the stops).

import uFuzzy from "@leeoniya/ufuzzy";
import { fold, partsOf } from "../moments/procesos/search.js";

export const GROUPS = ["goto", "jobs", "prefs"];
export const MAX_RESULTS = 8;
const MIN_PREFIX = 4;                                      // "apro" is the beginning of "aprobar"; "apr" could be the beginning of anything
const OUT_OF_ORDER = 4;                                    // the words of a query may come in any order (up to four of them)

// `intraMode: 1` forgives one slip inside a word ("kestrl"), as the finder of PROCESOS does.
const finder = new uFuzzy({ intraMode: 1 });

/** No accents and no capitals, one character for one character (so a range found in it fits the original). */
function plain(text) {
  let out = "";
  for (const ch of fold(text)) {
    const low = ch.toLowerCase();
    out += low.length === ch.length ? low : ch;
  }
  return out;
}

const textOf = (x) => (x == null ? "" : String(x));

/** The entries of one group that match `needle`, best first: [{ entry, ranges }]. */
function rankGroup(group, needle) {
  if (group.length === 0) return [];
  const hay = group.map((e) => plain(e.label) + " " + plain(e.sub) + " " + plain(e.words));
  let result = null;
  try {
    result = finder.search(hay, needle, OUT_OF_ORDER);
  } catch (e) {
    return [];                                             // a query the matcher cannot take finds nothing; it never breaks the page
  }
  if (!result || !Array.isArray(result[0]) || result[0].length === 0) return [];
  const [idxs, info, order] = result;
  if (info && Array.isArray(info.idx) && Array.isArray(order)) return order.map((n) => ({ entry: group[info.idx[n]], ranges: info.ranges[n] || [] }));
  return idxs.map((i) => ({ entry: group[i], ranges: [] }));  // too many hits to rank: in list order, with no marks
}

/** One row of the answer: the entry and its two lines as parts. `ranges` are in the haystack "label sub aliases". */
function rowOf(entry, ranges) {
  const label = textOf(entry.label);
  const sub = textOf(entry.sub);
  const list = ranges || [];
  return { entry, label: label ? partsOf(label, list, 0) : [], sub: sub ? partsOf(sub, list, label.length + 1) : [] };
}

/**
 * actionHint(query, verbs) -> { stop, word } | null. `verbs`: [{ stop, words }], `words` a string of the verbs that are done at that stop, in both
 * languages. Only the FIRST word of the query counts, and it counts when it is one of the verbs or (from four letters) the beginning of one.
 */
export function actionHint(query, verbs) {
  const first = plain(textOf(query).trim()).split(/\s+/)[0];
  if (!first || !Array.isArray(verbs)) return null;
  for (const verb of verbs) {
    for (const word of textOf(verb && verb.words).split(/\s+/).map(plain)) {
      if (word && (first === word || (first.length >= MIN_PREFIX && word.startsWith(first)))) return { stop: verb.stop, word: first };
    }
  }
  return null;
}

/**
 * rankEntries(entries, query, { max, verbs }) -> { rows, hint }. `entries`: [{ id, group, label, sub, words, ... }]. `rows`: [{ entry, label, sub }] in the
 * order of the groups, at most `max` when something was typed. `hint`: see `actionHint` (null when there is none); when the query is a verb and more
 * words, the rows are those of the rest ("aprobar kestrel" finds Kestrel: the verb is said, not searched, because no application is called "aprobar").
 */
export function rankEntries(entries, query, { max = MAX_RESULTS, verbs = [] } = {}) {
  const list = Array.isArray(entries) ? entries : [];
  const hint = actionHint(query, verbs);
  let asked = textOf(query).trim();
  if (hint) asked = asked.split(/\s+/).slice(1).join(" ") || asked;
  const needle = plain(asked);
  if (!needle) return { rows: list.filter((e) => e.group !== "jobs" && !e.quiet).map((e) => rowOf(e, [])), hint: null };
  const queues = GROUPS.map((g) => rankGroup(list.filter((e) => e.group === g), needle));
  const taken = [];                                        // [group, place in its queue, row]: the best of each group in turn
  for (let place = 0; taken.length < max; place++) {
    let any = false;
    queues.forEach((queue, g) => {
      if (place < queue.length && taken.length < max) { taken.push([g, place, rowOf(queue[place].entry, queue[place].ranges)]); any = true; }
    });
    if (!any) break;
  }
  taken.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  return { rows: taken.map((x) => x[2]), hint };
}
