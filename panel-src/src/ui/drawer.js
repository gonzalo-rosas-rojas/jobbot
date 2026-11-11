// The drawer (`aside#detail`): where one application opens, beside the list that holds it (spec §3.6, §6.3). This module is the behaviour
// around the content, not the content (ui/detail.js draws that):
//   - `open(key, origin)` reads /api/job?key=, shows it, moves the focus in, adds ONE history entry (`#<stop>/<key>`) so Back closes it and
//     tells the shell (`onOpenChange(true)`: the camera stops, the keys 0-5 stop travelling). It is NOT modal: nothing behind it goes inert,
//     the list stays alive with its cursor, and the keys of a row keep working there.
//   - `close()` (Esc, the button, Back): the focus goes back to the element that opened it and the history entry is taken back.
//   - `step(dir)` (j/k, ] [): the next or previous application among the ones behind the drawer, "n of N".
//   - it is NOT redrawn by a snapshot refresh (that would lose the picture, the note and the folds); `reload()` reads the same application
//     again and redraws it in place (the detail's actions call it), `rerender()` redraws what it has (a language change).
// It sits right of the sheet on a wide screen (--drawer wide, drawer.css) and covers the page on a narrow one.
//
//   const drawer = createDrawer({ el, session, router, stop, sound, getCtx, jobKeys, toast, onShow, onOpenChange, content });
//   el              the `aside#detail` element           stop()      the stop that is on screen ("hoy"), for the address
//   router          ui/router.js                         jobKeys()   the keys of the applications behind it, in the order j/k walks
//   getCtx()        what ui/detail.js needs: { session, sound, drawer, model, go, announce, reduced }
//   toast(text, ok) says an error                        onShow(key, origin) another application was read and is now shown (opened, or j/k): the list moves its cursor to its row
//   onOpenChange(open)  the drawer opened or closed: the shell stops or starts the camera
//   content(detail, card, ctx)  optional: the stop's own page of the application (PROCESOS has one, `moment.drawer`), a Node; null or
//                   undefined, or a throw, leaves the common content of ui/detail.js, which is what every other stop shows. Its root takes the
//                   class `detail-body` (the j/k slide moves that node) and `data-job="<key>"` (the drawer finds the application by it).

import { h, fill, focusMark, restoreFocus } from "../core/dom.js";
import { t } from "../i18n/index.js";
import { renderDetail, releaseDetail } from "./detail.js";
import { actButton } from "./act.js";

const CLEAR_MS = 320; // the slide out is on screen this long, then the content goes
const ORIGINS = ["data-card", "data-open"]; // the elements a row puts its application's key on

export function createDrawer({ el, session, router, stop, sound, getCtx, jobKeys, toast, onShow = null, onOpenChange = null, content = null }) {
  const state = { key: null, detail: null, returnTo: null, pushed: false, stepped: false };
  let serial = 0; // a newer request (or a close) cancels an answer that is still on its way
  let clearing = 0;

  const isOpen = () => el.classList.contains("open");

  // The element the drawer grows out of (and goes back into): the row of that application on the stop that is on screen.
  function originOf(key) {
    if (!key) return null;
    for (const attr of ORIGINS) {
      for (const node of document.querySelectorAll("[" + attr + '="' + CSS.escape(key) + '"]')) if (node.getClientRects().length > 0) return node;
    }
    return null;
  }

  function bar(key) {
    const keys = jobKeys();
    const at = keys.indexOf(key);
    const seq = at >= 0 && keys.length > 1
      ? h("span", { class: "seq" },
        actButton({ id: "seq-prev", label: "‹", title: t("drawer.prev.hint"), "aria-label": t("drawer.prev"), onclick: () => step(-1) }),
        h("span", { class: "num", "aria-live": "polite" }, t("drawer.seq", { at: at + 1, total: keys.length })),
        actButton({ id: "seq-next", label: "›", title: t("drawer.next.hint"), "aria-label": t("drawer.next"), onclick: () => step(1) }))
      : null;
    return h("div", { class: "detail-bar" }, seq, actButton({ id: "close-detail", label: t("common.close"), onclick: () => close() }));
  }

  function paint() {
    const detail = state.detail;
    if (!detail) return;
    let body = null;
    if (content) {
      try {
        body = content(detail, detail.card, getCtx()) || null;
      } catch (e) {
        console.error(e);
      }
    }
    if (!body) {
      try {
        body = renderDetail(detail, detail.card, getCtx());
      } catch (e) {
        console.error(e);
        body = h("p", { class: "muted" }, t("err.job"));
      }
    }
    el.setAttribute("aria-label", t("drawer.aria"));
    fill(el, bar(state.key), body);
  }

  /** From one application to the next the content slides the way you went. A cut under reduced motion. */
  function slide(node, dir) {
    if (!node || typeof node.animate !== "function") return;
    const ctx = getCtx();
    if (ctx.reduced && ctx.reduced()) return;
    node.animate([{ opacity: 0, transform: `translateX(${dir === -1 ? -14 : 14}px)` }, { opacity: 1, transform: "none" }], { duration: 180, easing: "cubic-bezier(0.23, 1, 0.32, 1)" });
  }

  async function open(key, origin, dir) {
    if (!key) return;
    const same = state.key === key && state.detail != null;
    const opener = document.activeElement; // who has the focus NOW, before the read: that is who the focus goes back to on close
    const shownKey = state.key;
    state.key = key;
    const mine = ++serial;
    const r = await session.api("/api/job?key=" + encodeURIComponent(key));
    if (mine !== serial) return; // another application was asked for while this one loaded: that one wins
    if (!r.ok || !r.data) {
      toast(session.said(r, t("err.job")), false);
      if (isOpen()) state.key = shownKey; // a drawer already open keeps what it shows (the note being written, the folds) and says the error; only a first opening is left closed
      else { state.key = null; router.write(stop(), null); }
      return;
    }
    const wasOpen = isOpen();
    state.detail = r.data;
    // the list's cursor is the row of what the drawer shows, however it was opened (a click, the palette, an address, j/k); a re-read of the same
    // application moves nothing
    if (onShow && !same) onShow(key, origin);
    if (same) {
      const mark = focusMark(document.activeElement, el);
      paint();
      restoreFocus(mark, el);
      // the control that had the focus may be gone (an approved application has no Approve button): keep it in the drawer
      if (isOpen() && !el.contains(document.activeElement)) el.focus({ preventScroll: true });
      return;
    }
    clearTimeout(clearing);
    const kept = wasOpen ? focusMark(document.activeElement, el) : null; // stepping with the buttons: the focus stays on the same one
    paint();
    el.scrollTop = 0; // a new application starts at its top, not where the last one was scrolled to
    if (wasOpen) {
      restoreFocus(kept, el);
      if (!el.contains(document.activeElement)) el.focus({ preventScroll: true });
      const title = el.querySelector("h2");
      if (title && title.textContent) getCtx().announce(title.textContent);
      slide(el.querySelector(".detail-body"), dir);
      router.write(stop(), key);
      return;
    }
    const active = opener;
    const given = origin instanceof Element && origin.isConnected ? origin : null; // a moment may name its row by an id, which is not an element
    state.returnTo = active && active !== document.body && !el.contains(active) ? active : given || originOf(key);
    state.stepped = false;
    el.inert = false;
    el.classList.add("open");
    if (onOpenChange) onOpenChange(true);
    state.pushed = router.write(stop(), key, { push: true });
    el.focus({ preventScroll: true });
    sound.play("open");
  }

  /** Closes the drawer. `fromPop`: the address already moved (Back). `silent`: the stop is changing, leave the history alone. */
  function close({ fromPop = false, silent = false } = {}) {
    if (!state.key && !isOpen()) return;
    serial += 1;
    const was = isOpen();
    const back = state.returnTo;
    const origin = originOf(state.key);
    state.key = null;
    state.detail = null;
    state.returnTo = null;
    el.classList.remove("open");
    el.inert = true;
    if (was) {
      if (onOpenChange) onOpenChange(false);
      sound.play("close");
    }
    // after j/k the list's cursor went with the drawer: Esc returns to the row of the application on screen, not to the one it was opened from
    const first = state.stepped ? origin : back;
    const target = first && first.isConnected ? first : back && back.isConnected ? back : origin;
    if (target && target.focus && !silent) target.focus({ preventScroll: true });
    clearTimeout(clearing);
    clearing = setTimeout(() => {
      if (isOpen()) return;
      el.replaceChildren();
      releaseDetail(); // the capture's blob URL goes only once the content that shows it is gone
    }, CLEAR_MS);
    if (silent) { state.pushed = false; return; }
    if (state.pushed && !fromPop) {
      // the address had a step for the drawer: take it back, so "Back" does not open it again
      state.pushed = false;
      router.back();
    } else {
      state.pushed = false;
      router.write(stop(), null);
    }
  }

  function step(dir) {
    const keys = jobKeys();
    const at = keys.indexOf(state.key);
    if (keys.length === 0 || (at >= 0 && keys.length < 2)) return;
    // an application that is not one of the rows (opened from a task, an address or a link): j goes to the first row and k to the last
    const next = at < 0 ? keys[dir > 0 ? 0 : keys.length - 1] : keys[(at + dir + keys.length) % keys.length];
    state.stepped = true;
    open(next, null, dir);
  }

  /** Reads the same application again and redraws it where it is (after an action). */
  function reload() {
    if (state.key) return open(state.key);
    return Promise.resolve();
  }

  /** Draws what it already has again, for a new language. */
  function rerender() {
    if (!state.detail) return;
    const mark = focusMark(document.activeElement, el);
    const top = el.scrollTop;
    paint();
    el.scrollTop = top;
    restoreFocus(mark, el);
  }

  return {
    open,
    close,
    step,
    reload,
    rerender,
    isOpen,
    get key() { return state.key; },
  };
}
