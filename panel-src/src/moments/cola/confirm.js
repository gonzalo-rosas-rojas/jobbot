// The drawn confirmation of COLA's lote (spec §5.3): a line under the proposal that names what ↵ would send and takes ↵ or Esc. It is a part of the page, not a
// box with keys of its own, so while it is open the keyboard is inside it. Built once, patched after.
//
//   const confirm = createConfirm({ actions });
//   confirm.el                    div.confirm (hidden while no lote is drawn)
//   confirm.patch(batch)          `batch` is the view's: the words of the two keys and the note; the names and the question are the picks drawn when it opened
//   confirm.contains(node)        whether `node` is inside it
//   confirm.focusOk()             puts the keyboard on its yes
//   confirm.returnFocusTo(node)   when the lote has closed with the keyboard still inside, `node` takes it back (call before patch hides the line)

import { h, setText } from "../../core/dom.js";
import { setHidden } from "../shared/util.js";
import { batchNamesOf, batchQuestionOf } from "./view.js";

export function createConfirm({ actions }) {
  const parts = {
    ask: h("span", { class: "gold" }),
    names: h("span", { class: "names" }),
    note: h("span", { class: "note" }),
    ok: h("button", { class: "ck", type: "button", "data-batch": "confirm" }, h("kbd", { class: "k" }), " ", h("span", {})),
    no: h("button", { class: "ck", type: "button", "data-batch": "cancel" }, h("kbd", { class: "k" }), " ", h("span", {})),
  };
  const el = h("div", { class: "confirm label", role: "group" });
  el.append(parts.ask, " ", parts.names, " ", parts.note, " ", parts.ok, " ", parts.no);
  el.hidden = true;

  return {
    el,
    patch(batch) {
      const open = actions.batch.isOpen();
      const picks = open ? actions.batch.picks() : [];
      // the drawn confirmation names what was drawn when it opened, so what he sees is what ↵ sends
      setHidden(el, !open);
      if (!open) return;
      setText(parts.ask, batchQuestionOf(picks));
      setText(parts.names, batchNamesOf(picks));
      setText(parts.note, batch.note ? " · " + batch.note + " · " : " · ");
      setText(parts.ok.firstChild, batch.enter);
      setText(parts.ok.lastChild, batch.confirm);
      setText(parts.no.firstChild, batch.esc);
      setText(parts.no.lastChild, batch.cancel);
    },
    contains: (node) => !!node && el.contains(node),
    focusOk() { if (!el.hidden) parts.ok.focus({ preventScroll: true }); },
    returnFocusTo(node) {
      if (!actions.batch.isOpen() && !el.hidden && el.contains(document.activeElement)) node.focus({ preventScroll: true });
    },
  };
}
