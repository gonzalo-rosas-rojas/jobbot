// The bottom bar (spec §4.6): `? atajos`, the one way into the shortcut sheet that needs no key. The key it shows is read from
// the table in keys.js, so renaming it there renames it here.

import { h } from "../core/dom.js";
import { t } from "../i18n/index.js";
import { KEYS } from "./keys.js";

/** createBar({ onHelp }) -> { el, relabel() } */
export function createBar({ onHelp }) {
  const help = KEYS.find((k) => k.id === "help");
  const label = h("span", { class: "bar-label" });
  const button = h("button", { class: "bar-help label", type: "button", "aria-haspopup": "dialog", "aria-keyshortcuts": help.display, onclick: () => onHelp() },
    h("kbd", {}, help.display), label);
  const el = h("footer", { id: "bar", class: "bar" }, button);

  function relabel() {
    label.textContent = t("bar.help");
    button.setAttribute("aria-label", t("ctrl.keys.hint"));
  }
  relabel();
  return { el, relabel };
}
