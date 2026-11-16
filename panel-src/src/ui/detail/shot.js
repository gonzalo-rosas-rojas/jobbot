// The dry-run screenshot of an application: what jobbot filled in, which is what Gonza checks before he approves. The file
// is fetched with the token (an <img> cannot send it), kept as a blob and shown through a blob URL, which is revoked when the
// application is released (ui/detail/memo.js). It is fetched once per application: a redraw of the drawer (after an action,
// a new language) shows the same picture again without asking the server. A picture that fails says so and can be tried
// again; it never takes the rest of the drawer with it.

import { h, fill } from "../../core/dom.js";
import { t } from "../../i18n/index.js";
import { memo, shotState, revokeShot } from "./memo.js";
import { actButton } from "../act.js";

/** What the slot says now, for the state of the fetch. */
function body(ctx, key) {
  const s = memo.shot;
  if (s.status === "ready") {
    const img = h("img", { class: "d-shot-img", src: s.url, alt: t("drawer.shot.alt"), onerror: () => fail(ctx, key) });
    return h("div", { class: "d-shotbox" + (s.zoom ? " zoom" : "") },
      // it scrolls (a tall capture in a short box), so the keyboard must be able to reach it: a named region in the tab order
      h("div", { class: "d-shotscroll", tabindex: 0, role: "region", "aria-label": t("drawer.shot.alt") }, img),
      actButton({ label: t(s.zoom ? "detail.shot.fit" : "detail.shot.zoom"), "aria-pressed": s.zoom, onclick: () => { s.zoom = !s.zoom; paint(ctx, key); } }));
  }
  if (s.status === "failed") {
    return h("div", { class: "cluster" },
      h("p", { class: "muted" }, t("detail.shot.failed")),
      actButton({ label: t("common.retry"), onclick: () => { start(ctx, key); paint(ctx, key); } }));
  }
  return h("p", { class: "muted" }, t("drawer.shot.loading"));
}

/** Draws the slot again, if the application it belongs to is still the one on screen. */
function paint(ctx, key) {
  if (memo.key !== key || !memo.slot || !memo.slot.isConnected) return;
  fill(memo.slot, body(ctx, key));
}

const setShot = (key, status, serial, url = null) => { memo.shot = shotState(key, status, serial, url); };

function fail(ctx, key) {
  if (memo.key !== key) return;
  revokeShot();
  setShot(key, "failed", memo.shot.serial);
  paint(ctx, key);
}

function start(ctx, key) {
  revokeShot();
  const mine = memo.shot.serial + 1;
  setShot(key, "loading", mine);
  Promise.resolve(ctx.session.blob("/api/shot?key=" + encodeURIComponent(key))).then((blob) => {
    if (memo.shot.serial !== mine || memo.key !== key) return; // closed, or another application opened: nobody wants it now
    if (!blob || (blob.type && !/^image\//i.test(blob.type))) {
      setShot(key, "failed", mine);
    } else {
      setShot(key, "ready", mine, URL.createObjectURL(blob));
    }
    paint(ctx, key);
  }, () => {
    if (memo.shot.serial === mine && memo.key === key) { setShot(key, "failed", mine); paint(ctx, key); }
  });
}

/** The section for an application whose card says it has a capture. It also starts the fetch the first time and becomes the slot the picture is painted into. */
export function showShot(ctx, card) {
  const key = card.key;
  const slot = h("div", { id: "shot-slot", class: "d-shot" });
  memo.slot = slot;
  if (memo.shot.key !== key) start(ctx, key);
  fill(slot, body(ctx, key));
  return h("section", { class: "d-sec", "aria-labelledby": "d-shot-h" }, h("h3", { id: "d-shot-h" }, t("drawer.shot.alt")), slot);
}

/** The line for an application that has none (only drawn where one could be expected). */
export function noShotView() {
  return h("p", { class: "muted small d-noshot" }, t("drawer.shot.none"));
}
