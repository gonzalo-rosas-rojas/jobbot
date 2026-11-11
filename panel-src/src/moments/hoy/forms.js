// The questions HOY asks before a verb that needs one more answer, and the new-task form (spec §3.4): `s` postpone (until when), `x` withdraw
// (with which reason), `r` register (which answer), `n` a new task. Each is a menu (ui/menu.js) beside the row. The pure parts (the choices,
// the date of a task) are plain functions and run in Node; `createForms` joins them with the menu, the session and the model.
//
//   createForms({ menu, session, sound, announce, now, cardOf, rowEl }) -> { ask(actionId, row), newTask(from) }
//   ask(...)  resolves with what `requestOf` (actions.js) needs as `extra`, or with null when he closed the menu without choosing
//   newTask() asks for a text and an optional deadline and sends POST /api/task/add itself (there is no row to dim)

import { h } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { markPhrase } from "../../core/format.js";
import { clean } from "../shared/util.js";

const HOUR = 3600000;

/** The ways to put an application aside, as the server reads them (`until`: "1d", "3d" ...): [{ key, until, label }]. */
export const snoozeChoices = () => [
  { key: "1", until: "1d", label: t("hoy.snooze.1d") },
  { key: "2", until: "3d", label: t("hoy.snooze.3d") },
  { key: "3", until: "7d", label: t("hoy.snooze.7d") },
  { key: "4", until: "14d", label: t("hoy.snooze.14d") },
];

/** The reasons to withdraw an application; the note is written down on it (null: no note). */
export const withdrawChoices = () => [
  { key: "1", note: null, label: t("hoy.withdraw.plain") },
  { key: "2", note: t("hoy.withdraw.note.interest"), label: t("hoy.withdraw.interest") },
  { key: "3", note: t("hoy.withdraw.note.other"), label: t("hoy.withdraw.other") },
];

/** The answers that can be written down on a card: whatever its `marks` allow except withdrawing (that is `x`), said the way it happened. [{ key, state, label }] */
export const registerChoices = (card) =>
  ((card && Array.isArray(card.marks) ? card.marks : []).filter((m) => m !== "withdrawn")).map((state, i) => ({ key: String(i + 1), state, label: markPhrase(state, card.state) }));

/** The deadlines of the new-task form, as hours from now; null is "no date". */
export const DUE = [
  { id: "none", hours: null },
  { id: "2h", hours: 2 },
  { id: "1d", hours: 24 },
  { id: "3d", hours: 72 },
];

/** RFC 3339 for `/api/task/add` ("2026-10-08T15:00:00Z"), or null for no deadline. */
export function dueAt(id, nowMs) {
  const due = DUE.find((d) => d.id === id);
  if (!due || due.hours == null) return null;
  return new Date(nowMs + due.hours * HOUR).toISOString().replace(".000Z", "Z");
}

/** The body of `POST /api/task/add`, or null when the text is empty. */
export function taskBody(text, dueId, nowMs) {
  const clear = clean(text, 240);
  return clear ? { text: clear, due_at: dueAt(dueId, nowMs) } : null;
}

export function createForms({ menu, session, sound, announce, now, cardOf, rowEl }) {
  function choose(title, choices, row, extraOf) {
    return new Promise((resolve) => {
      menu.open({
        title,
        from: rowEl ? rowEl(row.id) : null,
        items: choices.map((c) => ({ key: c.key, label: c.label, run: () => resolve(extraOf(c)) })),
        onClose: (why) => { if (why !== "pick") resolve(null); },
      });
    });
  }

  function ask(actionId, row) {
    if (actionId === "snooze") return choose(t("hoy.snooze.title"), snoozeChoices(), row, (c) => ({ until: c.until }));
    if (actionId === "withdraw") return choose(t("hoy.withdraw.title"), withdrawChoices(), row, (c) => ({ note: c.note }));
    if (actionId === "register") {
      const card = row.data && row.data.card ? cardOf(row.data.card) : null;
      const choices = registerChoices(card);
      if (choices.length === 0) return Promise.resolve(null);
      return choose(t("hoy.register.title"), choices, row, (c) => ({ state: c.state }));
    }
    return Promise.resolve(null);
  }

  function newTask(from = null) {
    const input = h("input", { class: "menu-input", type: "text", maxlength: "240", "aria-label": t("hoy.newtask.text"), placeholder: t("hoy.newtask.placeholder"), autocomplete: "off" });
    const due = h("select", { class: "menu-select", "aria-label": t("hoy.newtask.due") }, ...DUE.map((d) => h("option", { value: d.id }, t("hoy.newtask.due." + d.id))));
    const form = h("form", { class: "menu-form", novalidate: "" }, input, due, h("button", { class: "menu-submit label", type: "submit" }, t("hoy.newtask.add")));
    form.addEventListener("submit", async (e) => {
      e.preventDefault();
      const body = taskBody(input.value, due.value, now());
      if (!body) {
        input.setAttribute("aria-invalid", "true");
        announce(t("hoy.newtask.empty"));
        sound.play("error");
        return;
      }
      menu.close("pick");
      await session.act("/api/task/add", body, { beforeRefresh: (r) => sound.play(r.ok ? "taskadd" : "error") });
    });
    menu.open({ title: t("hoy.newtask.title"), body: form, from });
  }

  return { ask, newTask };
}
