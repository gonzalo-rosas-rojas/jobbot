// "¿Lo hiciste?" (spec §5.1): an item the bot could not finish (a CAPTCHA, a form: `blocked`, `handoff`, `manual_apply`) has `o` as its main verb, which
// opens the posting in another tab. When he comes back to this tab the page asks, in one line, whether he sent it: `c` says yes (the same as "Ya la
// envié", without the second press: this question IS the confirmation) and Esc says not yet. Nothing is asked if he never left the page.
//
//   const prompt = createPrompt({ host, run, announce, doc });   prompt.afterLink(row)   prompt.cancel()   prompt.pending()   prompt.destroy()
// `run(actionId, row, { confirmed: true })` is `actions.run` (actions.js); `host` is where the line is put (the HOY section).

import { h } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { KEYS } from "../../ui/keys.js";
import { quietFor } from "../../ui/keyfilter.js";
import { actButton } from "../../ui/act.js";
import { clean } from "../shared/util.js";

/** The kinds of item whose main verb sends him to another page to do something by hand. */
export const ASKS_AFTER = new Set(["blocked", "handoff", "manual_apply"]);
const YES = KEYS.find((k) => k.id === "confirm").keys[0];
const NO = KEYS.find((k) => k.id === "close").display;

export function createPrompt({ host, run, announce, doc = document }) {
  let row = null;     // the item he went to do
  let shown = null;   // the line on screen

  function hide() {
    if (shown) { shown.remove(); shown = null; }
    doc.removeEventListener("keydown", onKey, true);
  }

  function cancel() {
    row = null;
    hide();
  }

  function yes() {
    const target = row;
    cancel();
    if (target) run("confirm", target, { confirmed: true });
  }

  function onKey(e) {
    if (!shown || e.ctrlKey || e.metaKey || e.altKey || e.repeat || quietFor(e.target)) return;
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation(); // not also the page's Esc
      cancel();
    } else if (String(e.key).toLowerCase() === YES) {
      e.preventDefault();
      e.stopPropagation();
      yes();
    }
  }

  function show() {
    if (!row || shown) return;
    const who = clean(row.company || row.text, 60);
    shown = h("div", { class: "prompt", role: "group", "aria-label": t("hoy.prompt.ask", { company: who }) },
      h("span", { class: "prompt-text" }, t("hoy.prompt.ask", { company: who })),
      actButton({ key: YES, label: t("hoy.prompt.yes"), primary: true, "data-prompt": "yes", onclick: yes }),
      actButton({ key: NO, label: t("hoy.prompt.no"), "data-prompt": "no", onclick: cancel }));
    host.prepend(shown);
    doc.addEventListener("keydown", onKey, true);
    announce(t("hoy.prompt.ask", { company: who }));
  }

  function onVisible() {
    if (!doc.hidden) show();
  }
  doc.addEventListener("visibilitychange", onVisible);

  return {
    /** Called after `o` opened the posting of `item`: it will be asked about when the tab is visible again. */
    afterLink(item) {
      if (!ASKS_AFTER.has(item.kindId)) return;
      row = item;
      hide();
    },
    cancel,
    pending: () => row,
    isShown: () => shown !== null,
    show, // when the page is already visible again (a window that opened beside this one)
    destroy() { cancel(); doc.removeEventListener("visibilitychange", onVisible); },
  };
}
