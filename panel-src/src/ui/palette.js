// The command palette (Ctrl K, or the search key of the HUD): a modal over the stage that finds and takes you there. It is a combobox (the field)
// that owns a listbox (the rows) in three groups: IR A (the stops), the applications (found by company and role) and the settings and the help.
// What is in it and how a query ranks it is ui/palette-items.js and ui/palette-rank.js (pure); this module only shows it and listens.
//
// It changes nothing on the server and says so when it is asked to: a query that sounds like an action shows a line that says where that is
// done. A match is a <mark> of text, never markup (what someone else wrote stays letters). Like the help it is a modal dialog: the page behind it is
// `inert` (the shell does that through `onToggle(open)`, after the focus is inside when it opens and before it goes back when it closes), Tab
// stays inside, Esc or a click outside closes it and the focus goes back to where it was. The arrows move the active row and wrap at the ends;
// Enter runs it (the dialog closes first, so what the row does finds the page awake); Ctrl K again only brings the focus back to the field.
//
//   createPalette({ root, source, sound, onToggle }) -> { open, close, isOpen, refresh }
//   source()  -> { entries, verbs }, asked each time it opens and each time the shell says there is news (`refresh`), so the applications are
//                the ones of the last snapshot even when that snapshot arrives while it is open

import { h } from "../core/dom.js";
import { t, tn } from "../i18n/index.js";
import { icon } from "./icons.js";
import { createFocusReturn } from "./focus-return.js";
import { modalVeil, closeOnEscape } from "./modal.js";
import { KEYS } from "./keys.js";
import { rankEntries, GROUPS } from "./palette-rank.js";

const TITLE = { goto: "palette.goto", jobs: "palette.group.jobs", prefs: "palette.group.prefs" };

/** The parts of a line (ui/palette-rank.js) as nodes: text, and a <mark> where the query matched. */
const nodesOf = (parts) => parts.map((p) => (p.hit ? h("mark", {}, p.text) : p.text));

/** What a list of rows looks like, as one string: two lists with the same look would be drawn the same. */
const lookOf = (rows) => JSON.stringify(rows.map(({ entry: e }) => [e.id, e.label, e.sub, e.tag, e.key]));

export function createPalette({ root = document.body, source, sound, onToggle = () => {} }) {
  let box = null;
  let input = null;
  let list = null;
  let note = null;
  let empty = null;
  let live = null;
  let closeButton = null;
  let entries = [];
  let verbs = [];
  let shown = [];
  let at = 0;
  const focusReturn = createFocusReturn();

  const isOpen = () => box !== null;

  function showActiveRow() {
    const options = Array.from(list.querySelectorAll('[role="option"]'));
    options.forEach((el, i) => el.setAttribute("aria-selected", String(i === at)));
    if (options[at]) input.setAttribute("aria-activedescendant", options[at].id);
    else input.removeAttribute("aria-activedescendant");
    if (options[at] && typeof options[at].scrollIntoView === "function") options[at].scrollIntoView({ block: "nearest" });
  }

  function option(row, i) {
    const e = row.entry;
    return h("div", {
      id: "pal-" + i, role: "option", class: "palette-item", "aria-selected": false,
      onmousedown: (event) => event.preventDefault(),            // a click on a row must not take the focus out of the field
      onclick: () => run(i),
      onmousemove: () => { if (at !== i) { at = i; showActiveRow(); } },
    },
    h("span", { class: "palette-main" },
      h("span", { class: "palette-label" }, nodesOf(row.label)),
      row.sub.length ? h("span", { class: "palette-sub" }, nodesOf(row.sub)) : null),
    e.tag || e.key
      ? h("span", { class: "palette-trail" }, e.tag ? h("span", { class: "palette-tag" }, e.tag) : null, e.key ? h("kbd", { class: "palette-key" }, e.key) : null)
      : null);
  }

  /** The three groups as blocks: each with its rows, or (applications, nothing typed) the line that says they wait for a word. */
  function groupBlocks(rows, typed) {
    const blocks = [];
    let i = 0;
    for (const group of GROUPS) {
      const mine = rows.filter((r) => r.entry.group === group);
      const first = i;
      i += mine.length;
      if (mine.length) {
        blocks.push(h("div", { class: "palette-group", role: "group", "aria-labelledby": "pal-g-" + group },
          h("div", { id: "pal-g-" + group, class: "palette-group-title label" }, t(TITLE[group])),
          mine.map((row, j) => option(row, first + j))));
      } else if (group === "jobs" && !typed) {
        // the applications wait for a word: said, not listed (a hundred rows are no menu); not part of the list for a screen reader, the field's name says it
        blocks.push(h("div", { class: "palette-group palette-group-wait", "aria-hidden": "true" },
          h("div", { class: "palette-group-title label" }, t(TITLE.jobs)),
          h("div", { class: "palette-wait" }, t("palette.jobs.hint"))));
      }
    }
    return blocks;
  }

  /** What a screen reader hears after each query: how many rows, and where an action is done when the query names one. */
  const liveText = (rows, line, typed) => (!typed ? "" : [rows.length ? tn("palette.count", rows.length) : t("palette.empty"), line].filter(Boolean).join(". "));

  const ranked = () => rankEntries(entries, input.value, { verbs });

  function paint({ rows, hint } = ranked()) {
    const typed = input.value.trim() !== "";
    shown = rows;
    at = rows.length ? Math.min(at, rows.length - 1) : 0;
    list.replaceChildren(...groupBlocks(rows, typed));
    list.hidden = rows.length === 0;
    input.setAttribute("aria-expanded", String(rows.length > 0));
    empty.hidden = !(typed && rows.length === 0);
    const line = hint ? t("palette.note", { stop: t("tab." + hint.stop) }) : "";
    note.textContent = line;
    note.hidden = line === "";
    live.textContent = liveText(rows, line, typed);
    showActiveRow();
  }

  /**
   * What the page knows now, while the palette is open: a snapshot that arrives must not leave "nothing matches" standing over an application
   * that exists. It draws again only when the rows would be other rows, so a snapshot that changes nothing he sees moves nothing under the
   * mouse and says nothing again to a screen reader; the active row stays on the same entry, or goes to the first when that entry is gone,
   * and the list stays where he had scrolled it (drawing brings the active row into view, which is right for a key and wrong for news).
   */
  function refresh() {
    if (!box) return;
    const before = lookOf(shown);
    const activeId = shown[at] ? shown[at].entry.id : null;
    ({ entries, verbs } = source());
    const result = ranked();
    if (lookOf(result.rows) === before) { shown = result.rows; return; }   // the same rows to the eye; the entries are the new ones
    at = Math.max(0, result.rows.findIndex((row) => row.entry.id === activeId));
    const scrolledTo = list.scrollTop;
    paint(result);
    list.scrollTop = scrolledTo;
  }

  function close() {
    if (!box) return;
    box.remove();
    box = input = list = note = empty = live = closeButton = null;
    onToggle(false);
    sound.play("close");
    focusReturn.restore();
  }

  function run(i) {
    const row = shown[i];
    if (!row) return;
    close();
    row.entry.run();
  }

  function onKey(e) {
    if (e.isComposing) return;
    if (closeOnEscape(e, close)) return;
    if (e.key === "Tab") { e.preventDefault(); (document.activeElement === input ? closeButton : input).focus(); return; }   // two stops in the dialog: the field and the button
    if (e.target !== input) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (shown.length) at = (at + (e.key === "ArrowDown" ? 1 : shown.length - 1)) % shown.length;
      showActiveRow();
    } else if (e.key === "Enter") { e.preventDefault(); run(at); }
  }

  function open() {
    if (box) { input.focus(); return; }
    focusReturn.remember();
    ({ entries, verbs } = source());
    at = 0;
    input = h("input", {
      id: "palette-input", class: "palette-input", type: "text", role: "combobox", "aria-expanded": true, "aria-controls": "palette-list", "aria-autocomplete": "list",
      "aria-label": t("palette.aria"), autocomplete: "off", autocapitalize: "off", spellcheck: "false", enterkeyhint: "go", placeholder: t("palette.placeholder"),
      oninput: () => { at = 0; paint(); },
    });
    closeButton = h("button", { class: "palette-close label", type: "button", onclick: () => close() }, t("common.close"));
    note = h("p", { class: "palette-note", hidden: true });
    list = h("div", { id: "palette-list", class: "palette-list", role: "listbox", "aria-label": t("palette.aria") });
    empty = h("p", { class: "palette-empty", hidden: true }, t("palette.empty"));
    live = h("div", { class: "sr", role: "status", "aria-live": "polite" });
    const foot = h("div", { class: "palette-foot label" },
      h("span", {}, h("kbd", {}, "↑"), h("kbd", {}, "↓"), t("palette.foot.move")),
      h("span", {}, h("kbd", {}, "↵"), t("palette.foot.pick")),
      h("span", {}, h("kbd", {}, KEYS.find((k) => k.id === "close").display), t("palette.foot.close")));
    const dialog = h("div", { class: "palette", role: "dialog", "aria-modal": "true", "aria-label": t("palette.aria"), onkeydown: onKey },
      h("div", { class: "palette-head" }, h("label", { class: "palette-field" }, icon("search"), input), closeButton),
      note, live, list, empty, foot);
    box = modalVeil({ cls: "palette-back", dialog, onClose: close, scroller: list }); // the wheel is the list's or nobody's
    root.append(box);
    sound.play("open");
    paint();
    input.focus();
    onToggle(true);
  }

  return { open, close, isOpen, refresh };
}
