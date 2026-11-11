// The text system: Spanish and English, one flat dictionary per scope and language, merged here.
//
//   t("act.approve")                          one string
//   t("news.card", { company, state })        {name} is replaced; a name missing from vars stays visible as {name}
//   tn("time.ago.days", 3)                    picks time.ago.days.one / .other (or .zero when n is 0 and it exists)
//
// A missing key never throws: it shows as ⟦key⟧ (and is warned about once), so a gap is visible on screen and in
// tests. Keys are `scope.thing.detail` in English words. Nothing here touches the DOM at import time.

import { local } from "../core/store.js";
import esCommon from "./es/common.js";
import esHoy from "./es/hoy.js";
import esProcesos from "./es/procesos.js";
import esCola from "./es/cola.js";
import esAprender from "./es/aprender.js";
import esSistema from "./es/sistema.js";
import esShell from "./es/shell.js";
import esDetail from "./es/detail.js";
import esStage from "./es/stage.js";
import esHud from "./es/hud.js";
import esKeys from "./es/keys.js";
import esMoments from "./es/moments.js";
import esSound from "./es/sound.js";
import enCommon from "./en/common.js";
import enHoy from "./en/hoy.js";
import enProcesos from "./en/procesos.js";
import enCola from "./en/cola.js";
import enAprender from "./en/aprender.js";
import enSistema from "./en/sistema.js";
import enShell from "./en/shell.js";
import enDetail from "./en/detail.js";
import enStage from "./en/stage.js";
import enHud from "./en/hud.js";
import enKeys from "./en/keys.js";
import enMoments from "./en/moments.js";
import enSound from "./en/sound.js";

export const LANGS = ["es", "en"];
const DICTS = {
  es: Object.assign({}, esCommon, esHoy, esProcesos, esCola, esAprender, esSistema, esShell, esDetail, esStage, esHud, esKeys, esMoments, esSound),
  en: Object.assign({}, enCommon, enHoy, enProcesos, enCola, enAprender, enSistema, enShell, enDetail, enStage, enHud, enKeys, enMoments, enSound),
};
// Number and date rules: es-CL for Spanish (Gonza's), en-GB for English (day before month, 24-hour clock).
const LOCALES = { es: "es-CL", en: "en-GB" };

export const isLang = (value) => value === "es" || value === "en";

// The build decides the default: the Rust-served panel is Spanish, the public demo is built with
// __DEFAULT_LANG__ = "en". "auto" asks the browser. Without a define (tests, labs) it is Spanish.
function buildDefault() {
  return typeof __DEFAULT_LANG__ === "undefined" ? "es" : __DEFAULT_LANG__;
}

let current = isLang(buildDefault()) ? buildDefault() : "es";
let zone; // undefined: the browser's time zone
const listeners = new Set();
const warned = new Set();

export const lang = () => current;

function pageLang(next) {
  try {
    if (typeof document !== "undefined" && document.documentElement) document.documentElement.lang = next;
  } catch (e) { /* no document */ }
}

/**
 * The language to open in: `?lang=` in the address, then the saved choice, then the build's default, which is
 * "es" or "en" and, only when the build says "auto", the browser's (Spanish if it starts with "es", else English).
 * Every input can be given, so tests need no browser.
 */
export function detectLang(env = {}) {
  const search = env.search !== undefined ? env.search : safe(() => location.search, "");
  const asked = new URLSearchParams(search).get("lang");
  if (isLang(asked)) return asked;
  const saved = env.saved !== undefined ? env.saved : local.get("lang");
  if (isLang(saved)) return saved;
  const def = env.def !== undefined ? env.def : buildDefault();
  if (isLang(def)) return def;
  const nav = env.navLang !== undefined ? env.navLang : safe(() => navigator.language, "");
  return String(nav || "").toLowerCase().startsWith("es") ? "es" : "en";
}

function safe(read, fallback) {
  try { return read(); } catch (e) { return fallback; }
}

/** Boot: picks the language and puts it on <html>. It is not saved, so a later change of default still applies. */
export function initLang(env) {
  current = detectLang(env);
  pageLang(current);
  return current;
}

/** Chooses a language on purpose: saved, put on <html>, and every listener told when it changed. False for a bad value. */
export function setLang(next) {
  if (!isLang(next)) return false;
  const changed = next !== current;
  current = next;
  local.set("lang", next);
  pageLang(next);
  if (changed) for (const fn of Array.from(listeners)) call(fn, next);
  return true;
}

function call(fn, value) {
  try { fn(value); } catch (e) { console.error(e); }
}

export function onLang(fn) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

// ---- Words

const hole = /\{([A-Za-z_]\w*)\}/g;

function fillIn(text, vars) {
  if (!vars) return text;
  return text.replace(hole, (whole, name) => (Object.hasOwn(vars, name) && vars[name] != null ? String(vars[name]) : whole));
}

export function has(key) {
  return typeof DICTS[current][key] === "string";
}

export function t(key, vars) {
  const text = DICTS[current][key];
  if (typeof text !== "string") {
    if (!warned.has(key)) {
      warned.add(key);
      console.warn("i18n: missing key " + key + " (" + current + ")");
    }
    return "⟦" + key + "⟧";
  }
  return fillIn(text, vars);
}

const rules = {};
const pluralRules = () => rules[current] || (rules[current] = new Intl.PluralRules(LOCALES[current]));

/** Plural: `key.zero` for 0 when it exists, else the CLDR category of the language (`one`), else `key.other`. */
export function tn(key, n, vars) {
  const dict = DICTS[current];
  let form = n === 0 && dict[key + ".zero"] != null ? "zero" : pluralRules().select(n);
  if (dict[key + "." + form] == null) form = "other";
  return t(key + "." + form, Object.assign({ n }, vars));
}

// ---- Numbers and dates for the active language

export const getTimeZone = () => zone;
/** An IANA name ("UTC", "America/Santiago") or undefined for the browser's own; tests pin it. */
export function setTimeZone(tz) {
  zone = tz || undefined;
}

const made = new Map();
function formatter(kind, options, tz) {
  const id = kind + "|" + current + "|" + (tz || zone || "") + "|" + JSON.stringify(options);
  let f = made.get(id);
  if (!f) {
    const full = kind === "date" ? Object.assign({ timeZone: tz || zone }, options) : options;
    f = kind === "date" ? new Intl.DateTimeFormat(LOCALES[current], full) : new Intl.NumberFormat(LOCALES[current], full);
    made.set(id, f);
  }
  return f;
}

export function fmtNumber(n, options) {
  return formatter("number", options || {}).format(n);
}

/** 0.456 becomes "46%"; no value (a ratio of nothing) is "n/a". */
export function fmtPercent(x) {
  return x == null ? t("common.na") : Math.round(x * 100) + "%";
}

// "$1.84" in English; in Spanish the full "US$1,84", because in Chile a bare "$" means pesos.
export function fmtMoney(usd, { digits = 2 } = {}) {
  const display = current === "es" ? "symbol" : "narrowSymbol";
  return formatter("number", { style: "currency", currency: "USD", currencyDisplay: display, minimumFractionDigits: digits, maximumFractionDigits: digits }).format(Number(usd));
}

const DATE_STYLES = {
  short: { weekday: "short", day: "numeric", month: "short" },
  long: { weekday: "long", day: "numeric", month: "long", year: "numeric" },
  day: { day: "numeric", month: "short" },
  time: { hour: "2-digit", minute: "2-digit", hourCycle: "h23" },
  datetime: { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit", hourCycle: "h23" },
};

/** `short` "Wed 7 Oct", `long`, `day` "7 Oct", `time` "14:05", `datetime` "7 Oct, 14:05". A bad date comes back as given. */
export function fmtDate(iso, style = "short", tz) {
  try {
    return formatter("date", DATE_STYLES[style] || DATE_STYLES.short, tz).format(new Date(iso));
  } catch (e) {
    return String(iso == null ? "" : iso);
  }
}

/** How long ago, in words ("3 min ago", "2 days ago"); a time in the future or not a time is "just now". */
export function fmtRelative(iso, nowMs) {
  const minutes = Math.floor((nowMs - Date.parse(iso)) / 60000);
  if (!(minutes >= 0)) return t("time.ago.now");
  if (minutes < 60) return t("time.ago.min", { n: Math.max(minutes, 1) });
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return t("time.ago.hours", { n: hours });
  return tn("time.ago.days", Math.floor(hours / 24));
}
