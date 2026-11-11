// What the server says in its own words, said in his: "Aprobada: greenhouse:7345211" becomes "Aprobada: Brisa Mobility — Full-stack Engineer" and
// "Pospuesta hasta 2026-10-12 18:03" becomes "Pospuesta hasta lun 12 oct, 18:03" (es) or "Mon 12 Oct, 6:03 PM" (en). Every text of the server that
// reaches him goes through here (the toast, the live region, the banner: ui/shell.js), so a moment never has to know.
//
// Pure: a text and the cards of the model in, a text out. The text is cut into words and spaces; a word is a key when, taken out of the punctuation
// that surrounds it, the model has a card by that name (so "test:22" or "xtest:2" are not "test:2", and a key inside a tag is not a word of its own);
// a word is a date when it is a day of the calendar (and an hour next to it, or after a "T"). Nothing else is read, changed or removed, and what comes
// out is a string for `textContent`: no markup is ever made.

import { SERVER_TZ } from "./time.js";

const SEPARATOR = " — ";
const MAX_ROLE = 32;                                  // a role longer than this is left out: "Brisa Mobility", not the whole title
const LEAD = new Set(["(", "[", "{", "\"", "'", "«", "¿", "¡"]);
const TRAIL = new Set([")", "]", "}", "\"", "'", "»", ",", ".", ";", ":", "!", "?", "…"]);
const LOCALE = { es: "es-CL", en: "en-US" };

/** The cards of a snapshot by key: the board, what needs him, what awaits his approval and what is put aside. One index per snapshot. */
const indexes = new WeakMap();
export function cardIndex(snap) {
  if (!snap || typeof snap !== "object") return new Map();
  let index = indexes.get(snap);
  if (index) return index;
  index = new Map();
  const add = (card) => { if (card && card.key != null && !index.has(card.key)) index.set(card.key, { company: card.company, title: card.title }); };
  for (const column of snap.board || []) for (const card of column.cards || []) add(card);
  for (const list of [snap.needs_gonza, snap.awaiting_approval, snap.snoozed]) for (const card of (list && list.cards) || []) add(card);
  indexes.set(snap, index);
  return index;
}

const clean = (x) => (x == null ? "" : String(x).trim());

/** "Company — Role", or the company alone when the role is long, or whichever of the two exists; null when there is nothing to say. */
function labelOf(card) {
  if (!card) return null;
  const company = clean(card.company);
  const role = clean(card.title);
  if (company && role) return role.length <= MAX_ROLE ? company + SEPARATOR + role : company;
  return company || role || null;
}

/** [lead, core, trail]: the word without the punctuation around it. */
function split(word) {
  let a = 0;
  let b = word.length;
  while (a < b && LEAD.has(word[a])) a += 1;
  while (b > a && TRAIL.has(word[b - 1])) b -= 1;
  return [word.slice(0, a), word.slice(a, b), word.slice(b)];
}

// ---- dates

const isDigits = (s, n) => s.length === n && [...s].every((c) => c >= "0" && c <= "9");

/** "2026-10-12" -> { y, m, d } when it is a real day of the calendar, else null. */
function parseDay(s) {
  if (s.length !== 10 || s[4] !== "-" || s[7] !== "-") return null;
  const y = s.slice(0, 4);
  const m = s.slice(5, 7);
  const d = s.slice(8, 10);
  if (!isDigits(y, 4) || !isDigits(m, 2) || !isDigits(d, 2)) return null;
  const day = { y: Number(y), m: Number(m), d: Number(d) };
  const check = new Date(Date.UTC(day.y, day.m - 1, day.d));
  return check.getUTCFullYear() === day.y && check.getUTCMonth() === day.m - 1 && check.getUTCDate() === day.d ? day : null;
}

/** "18:03", "18:03:07" or "18:03:07.250" -> { h, mi, s }, else null. */
function parseHour(s) {
  const parts = s.split(":");
  if (parts.length < 2 || parts.length > 3) return null;
  const [h, mi, rest = "0"] = parts;
  const sec = rest.split(".");
  if (!isDigits(h, 2) || !isDigits(mi, 2) || sec.length > 2 || !sec[0] || ![...sec[0]].every((c) => c >= "0" && c <= "9") || sec[0].length > 2) return null;
  if (sec.length === 2 && (!sec[1] || ![...sec[1]].every((c) => c >= "0" && c <= "9"))) return null;
  const out = { h: Number(h), mi: Number(mi), s: Number(sec[0]) };
  return out.h < 24 && out.mi < 60 && out.s < 60 ? out : null;
}

/** The end of "18:03:00-03:00": the hour and its offset in minutes (null: none written). `undefined` when what follows is not a zone. */
function parseHourZone(s) {
  const z = s.search(/[Zz+-]/);                        // the hour has none of these: the first one starts the zone
  if (z < 0) return { time: parseHour(s), offset: null };
  const time = parseHour(s.slice(0, z));
  const zone = s.slice(z);
  if (zone === "Z" || zone === "z") return { time, offset: 0 };
  const sign = zone[0] === "-" ? -1 : 1;
  const digits = zone.slice(1).replace(":", "");
  if (!(digits.length === 2 || digits.length === 4) || ![...digits].every((c) => c >= "0" && c <= "9")) return undefined;
  const hours = Number(digits.slice(0, 2));
  const minutes = digits.length === 4 ? Number(digits.slice(2)) : 0;
  return hours < 24 && minutes < 60 ? { time, offset: sign * (hours * 60 + minutes) } : undefined;
}

const zones = new Map();
function zoneFormat(tz, locale, options) {
  const make = () => new Intl.DateTimeFormat(locale, Object.assign({ timeZone: tz || undefined }, options));
  if (!tz) return make();                              // the browser's own clock: not kept, the zone may change under a page that stays open
  const id = tz + "|" + locale + "|" + JSON.stringify(options);
  if (!zones.has(id)) zones.set(id, make());
  return zones.get(id);
}

/** Minutes the wall clock of `tz` is ahead of UTC at the instant `ms`. */
function offsetAt(ms, tz) {
  const p = Object.fromEntries(zoneFormat(tz, "en-US", { year: "numeric", month: "numeric", day: "numeric", hour: "numeric", minute: "numeric", second: "numeric", hourCycle: "h23" })
    .formatToParts(new Date(ms)).map((x) => [x.type, x.value]));
  return (Date.UTC(Number(p.year), Number(p.month) - 1, Number(p.day), Number(p.hour), Number(p.minute), Number(p.second)) - Math.floor(ms / 1000) * 1000) / 60000;
}

/** The instant a wall clock of `tz` shows (the offset is looked up twice: it may change between the guess and the answer). */
function wallToMs(day, time, tz) {
  const guess = Date.UTC(day.y, day.m - 1, day.d, time.h, time.mi, time.s);
  const first = guess - offsetAt(guess, tz) * 60000;
  return guess - offsetAt(first, tz) * 60000;
}

/** "lun 12 oct, 18:03" in Spanish, "Mon 12 Oct, 6:03 PM" in English. `hour: false` is only the day. */
function write(ms, lang, tz, hour) {
  const english = lang === "en";
  const options = { weekday: "short", day: "numeric", month: "short" };
  if (hour) Object.assign(options, { hour: "numeric", minute: "2-digit", hourCycle: english ? "h12" : "h23" });
  const p = Object.fromEntries(zoneFormat(tz, LOCALE[english ? "en" : "es"], options).formatToParts(new Date(ms)).map((x) => [x.type, x.value.replace(/\.$/, "")]));
  const day = `${p.weekday} ${p.day} ${p.month}`;
  if (!hour) return day;
  return english ? `${day}, ${p.hour}:${p.minute} ${p.dayPeriod}` : `${day}, ${p.hour.padStart(2, "0")}:${p.minute}`;
}

/**
 * The date that starts at word `i` of `parts` (words at even places, spaces at odd ones): { text, used } where `used` is how many parts it took, or null.
 * "2026-10-12T21:03:00Z" and "2026-10-12 18:03" (the server's own, a day and an hour) are dates; so is a day alone.
 */
function dateAt(parts, i, lang, tz, serverTz) {
  const [lead, core, trail] = split(parts[i]);
  const cut = core.indexOf("T");
  const dayText = cut < 0 ? core : core.slice(0, cut);
  const day = parseDay(dayText);
  if (!day) return null;
  if (cut >= 0) {
    const read = parseHourZone(core.slice(cut + 1));
    if (!read || !read.time) return null;
    const ms = read.offset == null ? wallToMs(day, read.time, serverTz) : Date.UTC(day.y, day.m - 1, day.d, read.time.h, read.time.mi, read.time.s) - read.offset * 60000;
    return { text: lead + write(ms, lang, tz, true) + trail, used: 1 };
  }
  // a day, then one space and an hour: the server's wall clock
  const space = parts[i + 1];
  const next = parts[i + 2];
  if (trail === "" && space && space.length === 1 && next) {
    const [, hourText, after] = split(next);
    const time = parseHour(hourText);
    if (time) return { text: lead + write(wallToMs(day, time, serverTz), lang, tz, true) + after, used: 3 };
  }
  return { text: lead + write(Date.UTC(day.y, day.m - 1, day.d, 12), lang, "UTC", false) + trail, used: 1 };
}

/**
 * humanize(text, cards, { lang, tz, serverTz }) -> text. `cards`: a Map (or an object) of key -> { company, title }; see `cardIndex`. `lang`: "es" or "en".
 * `tz`: the clock of the page (the browser's own when absent); `serverTz`: where the server's dates with no zone were written (Santiago).
 */
export function humanize(text, cards, { lang = "es", tz, serverTz = SERVER_TZ } = {}) {
  const source = text == null ? "" : String(text);
  const find = !cards ? () => undefined
    : cards instanceof Map ? (key) => cards.get(key)
    : (key) => (Object.prototype.hasOwnProperty.call(cards, key) ? cards[key] : undefined);
  const parts = source.split(/(\s+)/);                 // words at even places, the spaces between them at odd ones
  let out = "";
  for (let i = 0; i < parts.length; i++) {
    const word = parts[i];
    if (i % 2 === 1 || word === "") { out += word; continue; }
    const date = dateAt(parts, i, lang, tz, serverTz);
    if (date) { out += date.text; i += date.used - 1; continue; }
    const [lead, core, trail] = split(word);
    const label = core ? labelOf(find(core)) : null;
    out += label ? lead + label + trail : word;
  }
  return out;
}
