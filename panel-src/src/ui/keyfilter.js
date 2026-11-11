// When a shortcut must stay quiet (R7 §2.2): while a field takes letters, inside a box that has keys of its own
// (`data-local-keys`: the drill's grades), with a browser chord held, on auto-repeat, and while an input method is
// composing. Esc and Ctrl/Cmd+K are the exceptions: they work from anywhere, fields included. Pure; tinykeys calls it as
// its `ignore` option (ui/keys.js).

// A field that takes letters: text-like inputs, text areas, selects. A checkbox, radio or slider does not.
const TEXT_INPUT = /^(text|search|email|url|tel|password|number|date|datetime-local|time|month|week)$/i;

export const typingIn = (el) =>
  !!el && (el.tagName === "TEXTAREA" || el.tagName === "SELECT" || (el.tagName === "INPUT" && TEXT_INPUT.test(el.type || "text")) || el.isContentEditable === true);

/** The element sits inside a box that reads its own keys. */
export const localKeys = (el) => !!el && typeof el.closest === "function" && el.closest("[data-local-keys]") !== null;

/** What the shortcut layer treats as "the person is busy with this element". */
export const quietFor = (el) => typingIn(el) || localKeys(el);

// Controls whose own Enter is a click: the shortcut layer must leave that Enter to the browser, or a focused stop tab, link
// or action button would open the drawer instead of doing what it says.
const ENTER_OWNERS = "button, a[href], summary, [role=button], [role=tab], [role=link], [role=menuitem], [role=option], [role=switch], [role=checkbox]";
export const takesEnter = (el) => !!el && typeof el.closest === "function" && el.closest(ENTER_OWNERS) !== null;

// The keys that move a cursor along a list keep moving while they are held (review B10); every other key fires once, however long it is held.
const REPEATABLE = new Set(["j", "k", "ArrowUp", "ArrowDown"]);

/** True when the key event must be ignored by the shortcut layer. `locked()` says a dialog or menu owns the keyboard. */
export function ignoreKey(event, { typing = quietFor, locked = () => false } = {}) {
  const key = String(event.key);
  if (event.isComposing || (event.repeat && !(REPEATABLE.has(key) && !event.ctrlKey && !event.metaKey && !event.altKey))) return true;
  if ((event.ctrlKey || event.metaKey) && !event.altKey && key.toLowerCase() === "k") return false;
  if (key === "Escape") return false;
  if (event.ctrlKey || event.metaKey || event.altKey) return true;
  return typing(event.target) || locked();
}
