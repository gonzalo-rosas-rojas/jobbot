// The capture of the dry run (spec §4.6, §5.3): the plane, tilted and at the right of the sheet, that he judges a proposal by. The server
// keeps the screenshot of the form as the bot filled it (`GET /api/shot?key=`); the token travels in a header, so the picture comes through
// `session.blob` and is shown from a `blob:` address (the CSP allows `img-src blob:` and nothing else). The page paints it as paper (CSS:
// warm, a little darker, never the lightest thing of the stop) and the caption says what it is: a dry run, and that nothing was sent.
// Its two soft shadows (a dark one and a warm glow, 140 and 120 px of blur) come in AFTER the picture, one at a time (data-halo 1 and 2 on the
// figure): each is a big blur to rasterize the first time, and together with the paper in the frame of the arrival they made frames of 70 ms and more.
//
//   const shot = createShot({ session });
//   shot.el                         figure.plane-wrap (the render appends it inside the section: it is positioned at the right of the sheet)
//   shot.show({ key, preparedAt })  the capture of this proposal (fetched once, kept while it is the one on screen)
//   shot.show({ key: null })        the proposal has no capture: says so      shot.show(null)  nothing on screen: hides the plane
//   shot.destroy()                  gives back the address of the picture

import { h } from "../../core/dom.js";
import { t, fmtDate } from "../../i18n/index.js";
import { setText, setData, setHidden } from "../shared/util.js";

const GRACE_MS = 3000;
const HALO_AT = [260, 700]; // ms after the picture is loaded: the dark shade, then the warm one

export function createShot({ session }) {
  const caption = h("figcaption", { class: "plane-cap label" });
  const plane = h("div", { class: "plane" });
  const note = h("p", { class: "plane-note label" });
  const retry = h("button", { class: "plane-retry label", type: "button" }, t("common.retry"));
  const shades = ["dark", "warm"].map((k) => h("div", { class: "plane-shade", "data-k": k, "aria-hidden": "true" }));
  const stack = h("div", { class: "plane-stack" }, ...shades, plane);
  const el = h("figure", { class: "plane-wrap", "data-state": "none", "data-halo": "0" }, caption, stack, note, retry);
  el.hidden = true;
  let haloTimers = [];

  const S = { key: null, preparedAt: null, status: "none", url: null, size: "", seq: 0, sig: "", dead: false };

  // An address is given back a moment AFTER the picture stops using it: a picture still being fetched from it (he walked past this proposal
  // with j before it was drawn) would otherwise fail with a console error. destroy() gives back what is left at once.
  const dying = new Set();
  function revoke() {
    if (S.url) {
      const url = S.url;
      dying.add(url);
      setTimeout(() => { if (dying.delete(url)) URL.revokeObjectURL(url); }, GRACE_MS);
    }
    S.url = null;
    S.size = "";
  }

  /** The shadows follow the plane: they come in one by one once the first picture is loaded, stay while proposals change, and go with the plane. */
  function halo(shown, loaded) {
    if (!shown) {
      for (const timer of haloTimers) clearTimeout(timer);
      haloTimers = [];
      setData(el, "halo", "0");
      return;
    }
    if (!loaded || haloTimers.length) return;
    haloTimers = HALO_AT.map((ms, i) => setTimeout(() => { if (!S.dead) setData(el, "halo", String(i + 1)); }, ms));
  }

  function paint() {
    const sig = [S.key, S.status, S.url, S.size, S.preparedAt, t("cola.shot.none")].join("|");
    if (sig === S.sig) return;
    S.sig = sig;
    el.hidden = S.status === "off";
    setData(el, "state", S.status);
    const at = S.preparedAt && Number.isFinite(Date.parse(S.preparedAt)) ? fmtDate(S.preparedAt, "datetime") : "";
    setText(caption, S.status === "ok" && at ? (S.size ? t("cola.shot.caption", { at, size: S.size }) : t("cola.shot.caption.plain", { at })) : "");
    setHidden(caption, !caption.textContent);
    setText(note, S.status === "none" ? t("cola.shot.none") : S.status === "error" ? t("cola.shot.error") : "");
    setHidden(note, !note.textContent);
    setText(retry, t("common.retry"));
    setHidden(retry, S.status !== "error");
    if (S.status === "ok" && S.url) {
      const have = plane.firstChild;
      if (!have || have.dataset.src !== S.url) { // the picture is made once per address: a new caption does not reload it
        const img = h("img", { class: "shot", src: S.url, alt: t("drawer.shot.alt"), decoding: "async" });
        img.dataset.src = S.url;
        img.addEventListener("load", () => {
          if (S.dead || img.parentNode !== plane) return;
          const size = img.naturalWidth && img.naturalHeight ? img.naturalWidth + "×" + img.naturalHeight : "";
          if (size !== S.size) { S.size = size; paint(); }
        });
        img.addEventListener("error", () => { if (!S.dead && img.parentNode === plane) { revoke(); S.status = "error"; paint(); } });
        plane.replaceChildren(img);
      }
    } else if (S.status === "loading") {
      plane.replaceChildren(h("div", { class: "shot-skel", "aria-hidden": "true" }), h("span", { class: "sr" }, t("drawer.shot.loading")));
    } else {
      plane.replaceChildren();
    }
    setHidden(plane, S.status === "none" || S.status === "error");
    setHidden(stack, S.status === "none" || S.status === "error");
    halo(S.status === "loading" || S.status === "ok", S.status === "ok" && S.size !== "");
  }

  async function load(key) {
    const mine = ++S.seq;
    S.status = "loading";
    paint();
    let blob = null;
    try { blob = typeof session.blob === "function" ? await session.blob("/api/shot?key=" + encodeURIComponent(key)) : null; } catch (e) { blob = null; }
    if (S.dead || mine !== S.seq) return; // another proposal is on screen now: do not even make the address
    if (blob) { S.url = URL.createObjectURL(blob); S.status = "ok"; } else S.status = "error";
    paint();
  }

  retry.addEventListener("click", () => { if (S.key) load(S.key); });

  return {
    el,
    show(spec) {
      if (!spec) {
        S.seq += 1;
        revoke();
        S.key = null;
        S.status = "off";
        paint();
        return;
      }
      S.preparedAt = spec.preparedAt || null;
      if (!spec.key) {
        S.seq += 1;
        revoke();
        S.key = null;
        S.status = "none";
        paint();
        return;
      }
      if (S.key === spec.key && (S.status === "ok" || S.status === "loading" || S.status === "error")) { S.sig = ""; paint(); return; }
      revoke();
      S.key = spec.key;
      load(spec.key);
    },
    state: () => ({ key: S.key, status: S.status, size: S.size }),
    destroy() {
      S.dead = true;
      halo(false, false);
      S.seq += 1;
      revoke();
      for (const url of dying) URL.revokeObjectURL(url);
      dying.clear();
    },
  };
}
