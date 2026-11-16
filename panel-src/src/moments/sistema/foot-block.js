// The foot of SISTEMA: the keys of the page's own preferences (language, theme, sound), which also live in the palette.
//
//   const foot = createFootBlock();
//   foot.el                 footer.foot
//   foot.patch(view)

import { h, setText } from "../../core/dom.js";
import { stampChanged } from "../shared/util.js";

export function createFootBlock() {
  const footTitle = h("span", { class: "foot-title label" });
  const footKeys = h("span", { class: "foot-keys" });
  const foot = h("footer", { class: "foot" }, footTitle, footKeys);

  function patch(view) {
    setText(footTitle, view.foot.title);
    if (!stampChanged(footKeys, JSON.stringify(view.foot.keys))) return;
    footKeys.replaceChildren(...view.foot.keys.map((k) => h("span", { class: "pref" }, h("kbd", {}, k.key), " ", h("span", { class: "pref-l label" }, k.label))));
  }

  return { el: foot, patch };
}
