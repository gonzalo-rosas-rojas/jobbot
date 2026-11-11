// The search of PROCESOS (`/`, spec §3.4, §5.2): uFuzzy over "company title". Pure and DOM-free. What the page highlights is a list of
// parts ({ text, hit }) that render.js turns into text nodes and <mark> elements; nothing here ever makes markup from what someone else
// wrote (a company called "<img onerror=...>" is letters, and so is the query).

import uFuzzy from "@leeoniya/ufuzzy";

/** The text without accents ("Ámbar" is "Ambar"), one character for one character, so a range found in it fits the original. */
export function fold(text) {
  let out = "";
  for (const ch of String(text == null ? "" : text)) {
    const base = ch.normalize("NFD")[0];
    out += base.length === ch.length ? base : ch;
  }
  return out;
}

// For matching only: no accents and no capitals, still the same length (a letter that would change the length stays as it is).
function plain(text) {
  let out = "";
  for (const ch of fold(text)) {
    const low = ch.toLowerCase();
    out += low.length === ch.length ? low : ch;
  }
  return out;
}

// `intraMode: 1` forgives one slip inside a word ("kestrl"); the words of the query may come in any order.
const finder = new uFuzzy({ intraMode: 1 });
const OUT_OF_ORDER = 4;

/** A text cut at the ranges [from, to, from, to...] of a longer one it begins at `offset` of: [{ text, hit }]. A range outside the text is left out. */
export function partsOf(text, ranges, offset = 0) {
  const str = String(text == null ? "" : text);
  if (str === "") return [];
  const parts = [];
  let at = 0;
  const list = Array.isArray(ranges) ? ranges : [];
  for (let i = 0; i + 1 < list.length; i += 2) {
    const from = Math.max(at, Math.min(str.length, list[i] - offset));
    const to = Math.max(from, Math.min(str.length, list[i + 1] - offset));
    if (to <= from) continue;
    if (from > at) parts.push({ text: str.slice(at, from), hit: false });
    parts.push({ text: str.slice(from, to), hit: true });
    at = to;
  }
  if (at < str.length) parts.push({ text: str.slice(at), hit: false });
  return parts;
}

/**
 * searchOf(items, query) -> null when there is nothing to look for (no filter), else a Map key -> { ranges, parts: { company, title } } of the
 * items that match, in the order of `items` (the field keeps its places; a search only hides). `items` are [{ key, company, title }].
 */
export function searchOf(items, query) {
  const needle = plain(String(query == null ? "" : query).trim());
  if (!needle) return null;
  const list = items || [];
  const hay = list.map((x) => plain(x.company) + " " + plain(x.title));
  let result = null;
  try {
    result = finder.search(hay, needle, OUT_OF_ORDER);
  } catch (e) {
    result = null; // a query the matcher cannot take finds nothing; it never breaks the page
  }
  const found = new Map();
  if (!result || !result[0]) return found;
  const [idxs, info] = result;
  const ranges = new Map();
  if (info && Array.isArray(info.idx)) info.idx.forEach((idx, n) => ranges.set(idx, info.ranges[n])); // `{}` when nothing matched
  for (const i of [...idxs].sort((a, b) => a - b)) {
    const item = list[i];
    const own = ranges.get(i) || [];
    const cut = String(item.company == null ? "" : item.company).length;
    found.set(item.key, {
      ranges: own,
      parts: { company: partsOf(item.company, own, 0), title: partsOf(item.title, own, cut + 1) },
    });
  }
  return found;
}
