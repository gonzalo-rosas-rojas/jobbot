// The account block of SISTEMA: who Claude is signed in as, where its settings live, how it gets in, its version, and what it cost in the last 24 hours
// with its meter. An account that nothing pins down is a real warning.
//
//   const block = createAccountBlock();
//   block.el               section.block.account
//   block.patch(view)

import { h, setText } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { setData, setHidden } from "../shared/util.js";
import { setVar, pct, dot, makeRow } from "./parts.js";

export function createAccountBlock() {
  const acc = { account: makeRow("account"), folder: makeRow("folder"), access: makeRow("access"), cli: makeRow("cli"), spend: makeRow("spend") };
  const accDetail = h("span", { class: "detail" });
  const accFix = h("span", { class: "fix gold" });
  const accText = h("span", { class: "acc-text" });
  const accWho = h("span", { class: "acc-who" });
  const accAnchor = h("span", { class: "acc-anchor" });
  // an account that nothing pins down is a real warning: a square, its name in the alert colour, and in plain words what is missing
  const looseTitle = h("span", { class: "loose-title" });
  const looseWhy = h("span", { class: "loose-why" });
  const accLoose = h("span", { class: "loose", role: "note" }, looseTitle, looseWhy);
  acc.account.value.append(dot(), accText, accAnchor, accWho);
  acc.account.sub.append(accDetail, accFix, accLoose);
  const meter = h("div", { class: "meter", "aria-hidden": "true" }, h("span", { class: "meter-fill" }));
  const stagesList = h("p", { class: "stages label" });
  const overNote = h("span", { class: "over" });
  const spendText = h("span", { class: "spend-text" });
  acc.spend.value.append(spendText, meter, stagesList, overNote);
  const accountTitle = h("h2", { class: "block-title label" });
  const accountBlock = h("section", { class: "block account", "data-block": "account" }, accountTitle,
    h("dl", { class: "rows" }, acc.account.el, acc.folder.el, acc.access.el, acc.cli.el, acc.spend.el));

  function patch(view) {
    const a = view.account;
    setText(accountTitle, t("sistema.account.title"));
    setText(acc.account.label, t("sistema.row.account"));
    setData(acc.account.el, "tone", a.state);
    setText(accText, a.text);
    setText(accWho, a.who || "");
    setText(accDetail, a.detail || "");
    setHidden(accDetail, !a.detail);
    setText(accFix, a.fix || "");
    setHidden(accFix, !a.fix);
    const loose = !!a.anchor && a.anchor.state === "loose";
    setData(acc.account.el, "anchor", a.anchor ? a.anchor.state : null);
    setText(accAnchor, a.anchor && !loose ? a.anchor.text : "");
    setText(looseTitle, loose ? a.anchor.text : "");
    setText(looseWhy, loose ? a.anchor.why : "");
    setHidden(accLoose, !loose);
    setHidden(acc.account.sub, !a.detail && !a.fix && !loose);
    for (const [key, row, label] of [["folder", acc.folder, "sistema.row.folder"], ["access", acc.access, "sistema.row.access"], ["version", acc.cli, "sistema.row.cli"]]) {
      setText(row.label, t(label));
      setText(row.value, a[key] || "");
      setHidden(row.el, !a[key]);
    }
    setText(acc.spend.label, t("sistema.row.spend"));
    const s = view.spend;
    setData(acc.spend.el, "over", s && s.over ? "true" : "false");
    setText(spendText, s ? s.text : t("sistema.fig.nodata"));
    setHidden(meter, !s || s.ratio == null);
    if (s && s.ratio != null) setVar(meter, "--w", pct(s.ratio * 100));
    setText(stagesList, s ? s.stages.map((x) => x.text).join(" · ") : "");
    setHidden(stagesList, !s || s.stages.length === 0);
    setText(overNote, s && s.over ? s.overText : "");
    setHidden(overNote, !(s && s.over));
  }

  return { el: accountBlock, patch };
}
