// The buttons of one row of HOY or COLA (or of COLA's proposal), kept in place across repaints. The two rooms had each their copy; what differs
// is the sentence a screen reader gets (`ariaKey`), so that is a parameter.

import { t } from "../../i18n/index.js";
import { actButton, setActLabel } from "../../ui/act.js";
import { setData } from "./util.js";
import { setText } from "../../core/dom.js";

// An action with no key ("" from keyShownIfAny) is drawn with no key cap at all: to actButton an empty key means "a cap to fill later".
const buttonOf = (a) => actButton({ key: a.key === "" ? undefined : a.key, label: a.label, primary: a.primary, "data-action": a.id, title: a.title || null });

/**
 * patchActButtons(box, memo, actions, { armed, what, ariaKey })
 * `box` holds the buttons of the visible `actions`; `memo` ({ shape: "" }) remembers which set was drawn. The buttons are rebuilt only when the SET
 * changed (a verb arrived or left, a label changed with the language), and the one that had the focus gets it back; for an armed press only the
 * text of that one button changes, so the node he is pressing is the same node. `armed` is the id of the armed verb or null; `what` is the name
 * of the row the buttons belong to, and `ariaKey` the i18n key of the sentence a button says to a screen reader ("{action} {what}").
 */
export function patchActButtons(box, memo, actions, { armed, what, ariaKey }) {
  const shown = actions.filter((a) => a.visible);
  const shape = JSON.stringify(shown.map((a) => [a.id, a.key, a.label, a.primary]));
  if (shape !== memo.shape) {
    memo.shape = shape;
    const focused = box.contains(document.activeElement) ? document.activeElement.dataset.action : null;
    box.replaceChildren(...shown.map(buttonOf));
    if (focused) { const again = box.querySelector(`[data-action="${focused}"]`); if (again) again.focus({ preventScroll: true }); }
  }
  for (const b of box.querySelectorAll("button[data-action]")) {
    const a = shown.find((x) => x.id === b.dataset.action);
    if (!a) continue;
    const isArmed = armed === a.id;
    const said = isArmed ? t("act.sure") : a.label; // the name a screen reader gives is the one the eyes read
    setActLabel(b, said);
    setData(b, "armed", isArmed ? "true" : null);
    const aria = t(ariaKey, { action: said, what });
    if (b.getAttribute("aria-label") !== aria) b.setAttribute("aria-label", aria);
  }
}
