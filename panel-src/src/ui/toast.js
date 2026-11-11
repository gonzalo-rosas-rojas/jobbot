// One message at a time, bottom right, for four seconds (spec §6.4): what an action said, a refresh that failed, a setting that
// changed. "Deshacer" rides on the message when the action can be taken back (approve, dismiss, snooze). An error never goes
// away by itself: it has a close button and the next message replaces it. The toast is NOT a live region, because the page has
// exactly one (#live, spec §3.8): `say` is how the shell lets that region read the same sentence.
// `polite` is for good news nobody asked for (a milestone): it never takes the place of an error, it waits until it is closed.

import { h } from "../core/dom.js";
import { t } from "../i18n/index.js";
import { icon } from "./icons.js";

export const TOAST_MS = 4000;

/** createToast({ say, ms, set, clear }) -> { el, show(text, ok = true, { undo } = {}), polite(text, ok), hide() }. `set`/`clear` are injectable timers. */
export function createToast({ say = () => {}, ms = TOAST_MS, set = (fn, n) => setTimeout(fn, n), clear = (id) => clearTimeout(id) } = {}) {
  const text = h("span", { class: "toast-text" });
  const undoButton = h("button", { class: "toast-undo", type: "button", hidden: true });
  const closeButton = h("button", { class: "toast-close", type: "button", hidden: true }, icon("x"));
  const el = h("div", { id: "toast", class: "toast" }, text, undoButton, closeButton);
  let timer = 0;
  let undo = null;
  let isError = false;
  let pending = null;                                  // a polite message waiting behind an error

  function hide() {
    clear(timer);
    timer = 0;
    undo = null;
    isError = false;
    el.classList.remove("show");
    const next = pending;
    pending = null;
    if (next) show(next.text, next.ok);
  }

  function show(message, ok = true, { undo: run = null } = {}) {
    clear(timer);
    pending = null;
    text.textContent = message;
    undo = run;
    isError = ok === false;
    undoButton.hidden = !run;
    undoButton.textContent = run ? t("toast.undo") : "";
    closeButton.hidden = !isError;
    closeButton.setAttribute("aria-label", t("common.close"));
    el.classList.remove("ok", "bad");
    el.classList.add("show", isError ? "bad" : "ok");
    say(message);
    timer = isError ? 0 : set(hide, ms);               // an error stays until it is closed or replaced
  }

  undoButton.addEventListener("click", () => {
    const run = undo;
    hide();
    if (run) run();
  });
  closeButton.addEventListener("click", hide);

  return {
    el,
    show,
    polite(message, ok = true) {
      if (isError && el.classList.contains("show")) pending = { text: message, ok };
      else show(message, ok);
    },
    hide,
  };
}
