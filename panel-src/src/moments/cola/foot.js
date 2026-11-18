// COLA's foot (spec §5.3): the line about the review or the cap, and the engine's switch with its state. Built once, patched after.
//
//   const foot = createFoot({ actions });   foot.el (footer.foot)   foot.patch(view)
// `actions` answers engineArmed() and isPending("engine"): the switch is drawn "¿Seguro?" while its second press is awaited, and dim while the request is out.

import { h, setText } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { setData, setHidden, setAttr } from "../shared/util.js";
import { ENGINE_REF } from "../shared/engine.js";
import { actButton, setActKey, setActLabel } from "../../ui/act.js";

export function createFoot({ actions }) {
  const parts = {
    left: h("p", { class: "foot-line label" }),
    stopBtn: actButton({ key: "", cls: "stop-btn", "data-engine": "" }),
    stopState: h("span", { class: "stop-state label" }),
  };
  const el = h("footer", { class: "foot" }, parts.left, h("span", { class: "engine-ctl" }, parts.stopBtn, parts.stopState));

  return {
    el,
    patch(view) {
      setText(parts.left, view.foot.left);
      setHidden(parts.left, !view.foot.left);
      const s = view.foot.stop;
      const armed = actions.engineArmed();
      setActKey(parts.stopBtn, s.key);
      const said = armed ? t("act.sure") : s.label;
      setActLabel(parts.stopBtn, said);
      setData(parts.stopBtn, "armed", armed ? "true" : null);
      setData(parts.stopBtn, "pending", actions.isPending(ENGINE_REF) ? "true" : null);
      // the accessible name says what the button shows, "¿Seguro?" included (as SISTEMA does)
      setAttr(parts.stopBtn, "aria-label", said);
      setText(parts.stopState, s.state);
      setData(parts.stopState, "on", s.on ? "true" : null);
    },
  };
}
