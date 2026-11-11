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
//   createPalette({ root, source, sound, onToggle }) -> { open, close, isOpen }
//   source()  -> { entries, verbs }, asked each time it opens (so the applications are the ones of the last snapshot)

import { h } from "../core/dom.js";
import { t, tn } from "../i18n/index.js";
import { icon } from "./icons.js";
import { rankEntries, GROUPS } from "./palette-rank.js";

const TITLE = { goto: "palette.goto", jobs: "palette.group.jobs", prefs: "palette.group.prefs" };

/** The parts of a line (ui/palette-rank.js) as nodes: text, and a <mark> where the query matched. */
const nodesOf = (parts) => parts.map((p) => (p.hit ? h("mark", {}, p.text) : p.text));

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
  let from = null;

  const isOpen = () => box !== null;

  function mark() {
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
      onmousemove: () => { if (at !== i) { at = i; mark(); } },
    },
    h("span", { class: "palette-main" },
      h("span", { class: "palette-label" }, nodesOf(row.label)),
      row.sub.length ? h("span", { class: "palette-sub" }, nodesOf(row.sub)) : null),
    e.tag || e.key
      ? h("span", { class: "palette-trail" }, e.tag ? h("span", { class: "palette-tag" }, e.tag) : null, e.key ? h("kbd", { class: "palette-key" }, e.key) : null)
      : null);
  }

  function paint() {
    const query = input.value;
    const typed = query.trim() !== "";
    const { rows, hint } = rankEntries(entries, query, { verbs });
    shown = rows;
    at = rows.length ? Math.min(at, rows.length - 1) : 0;
    const blocks = [];
    let i = 0;
    for (const group of GROUPS) {
      const mine = rows.filter((r) => r.entry.group === group);
      if (mine.length) {
        blocks.push(h("div", { class: "palette-group", role: "group", "aria-labelledby": "pal-g-" + group },
          h("div", { id: "pal-g-" + group, class: "palette-group-title label" }, t(TITLE[group])),
          mine.map((row) => option(row, i++))));
      } else if (group === "jobs" && !typed) {
        // the applications wait for a word: said, not listed (a hundred rows are no menu); not part of the list for a screen reader, the field's name says it
        blocks.push(h("div", { class: "palette-group palette-group-wait", "aria-hidden": "true" },
          h("div", { class: "palette-group-title label" }, t(TITLE.jobs)),
          h("div", { class: "palette-wait" }, t("palette.jobs.hint"))));
      }
    }
    list.replaceChildren(...blocks);
    list.hidden = rows.length === 0;
    input.setAttribute("aria-expanded", String(rows.length > 0));
    empty.hidden = !(typed && rows.length === 0);
    const line = hint ? t("palette.note", { stop: t("tab." + hint.stop) }) : "";
    note.textContent = line;
    note.hidden = line === "";
    live.textContent = !typed ? "" : [rows.length ? tn("palette.count", rows.length) : t("palette.empty"), line].filter(Boolean).join(". ");
    mark();
  }

  function close() {
    if (!box) return;
    box.remove();
    box = input = list = note = empty = live = closeButton = null;
    onToggle(false);
    sound.play("close");
    if (from && from.isConnected && from.focus) from.focus({ preventScroll: true });
    from = null;
  }

  function run(i) {
    const row = shown[i];
    if (!row) return;
    close();
    row.entry.run();
  }

  function onKey(e) {
    if (e.isComposing) return;
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); close(); return; }
    if (e.key === "Tab") { e.preventDefault(); (document.activeElement === input ? closeButton : input).focus(); return; }   // two stops in the dialog: the field and the button
    if (e.target !== input) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (shown.length) at = (at + (e.key === "ArrowDown" ? 1 : shown.length - 1)) % shown.length;
      mark();
    } else if (e.key === "Enter") { e.preventDefault(); run(at); }
  }

  function open() {
    if (box) { input.focus(); return; }
    from = document.activeElement;
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
      h("span", {}, h("kbd", {}, "Esc"), t("palette.foot.close")));
    const dialog = h("div", { class: "palette", role: "dialog", "aria-modal": "true", "aria-label": t("palette.aria"), onkeydown: onKey },
      h("div", { class: "palette-head" }, h("label", { class: "palette-field" }, icon("search"), input), closeButton),
      note, live, list, empty, foot);
    box = h("div", { class: "palette-back", "data-lenis-prevent": "", onmousedown: (e) => { if (e.target === box) close(); } }, dialog);
    // the wheel is the list's or nobody's: over the veil, or over a list that has nothing to scroll, it must not move the page behind (the stage's camera)
    box.addEventListener("wheel", (e) => { if (!list.contains(e.target) || list.scrollHeight <= list.clientHeight) e.preventDefault(); }, { passive: false });
    root.append(box);
    sound.play("open");
    paint();
    input.focus();
    onToggle(true);
  }

  return { open, close, isOpen };
}
