// The camera of a desktop window (spec §3.2): Lenis smooths the native scroll of the 6 x 100 vh document, the engine (scene/engine.js)
// turns it into one number `pos` (0..5 stops), and ONE reader paints what that number means: the stage (sky, skyline, dial, beam), the
// moment under the sheet, the sheet's opacity (it fades while the camera travels and is solid on a stop). It is a `nav` of the shell:
// `start(id)`, `stop()`, `go(id)`, `lock(open)`, `cancel()`, `repaint()`. Under 760 px the page has no stage at all and this is not built
// (scene/flat.js is the nav then); the page can pass from one to the other without reloading, and `stop()` leaves nothing behind.

import Lenis from "lenis";
import { createEngine } from "./engine.js";
import { STOP_IDS } from "./poses.js";
import { sheetView } from "./sheetview.js";
import { typingIn } from "../ui/keyfilter.js";

/**
 * createCamera({ shell, stage, clock, reduced, canSnap, win }) -> nav. `stage` is stage/stage.js; `clock.hora()` the hour of the sky; `canSnap()` says
 * whether the engine may glide a page that rests between two stops to the nearest one (not while the drawer, a menu or the help is open).
 */
export function createCamera({ shell, stage, clock, reduced, canSnap = () => true, win = window }) {
  const app = shell.app;
  let lenis = null;
  let engine = null;
  let detach = null;
  let watch = null;
  let opacity = "";
  let veiled = false;        // the sheet is below half on a trip: it takes no click
  // `inert` on the whole sheet costs a style recalculation and a layout of its subtree (35 ms at CPU x4 for the 1,000 nodes of PROCESOS, and it fell
  // in the frame before the mount: the second long frame of a trip, ACEPTACION A7). While the camera travels a click guard does the same job for free.
  const veil = (e) => { if (veiled && app.contains(e.target)) { e.preventDefault(); e.stopPropagation(); } };

  /** The one painter: called by the engine on every frame with work, and by `repaint()`. */
  function paint(view) {
    const pos = view ? view.pos : engine.S.pos;
    // The sheet is read from the position, except on a long trip, where it goes from the stop it leaves to the one it aims at and mounts nothing in
    // between (sheetview.js). Calm motion has no snap: the sheet must be readable wherever the page rests.
    const sheet = sheetView({ pos, job: engine.S.jump, floorHeight: win.innerHeight, reduced: reduced() });
    shell.showStop(STOP_IDS[sheet.index], { fade: reduced() });
    const o = sheet.opacity;
    const text = o >= 0.999 ? "" : o.toFixed(3);
    const travelling = !!engine.S.jump;
    if (text !== opacity) {
      opacity = text;
      app.style.opacity = text;
    }
    veiled = travelling && o < 0.5;                              // a sheet that is fading is not a place to click...
    if (!travelling && app.inert !== o < 0.5) app.inert = o < 0.5;   // ...and one that rests faded between two stops is not either
    const drawn = stage.render({ pos, hora: clock.hora(), reduced: reduced() });
    shell.setDialBig(drawn ? !drawn.layout.docked : null);
  }

  function resize() {
    stage.resize();
    repaint();
  }

  function repaint() {
    if (!engine) return;
    engine.S.dirty = true;
    engine.wake();
  }

  /** The fonts arrived or the window changed: the marks are measured again and a page at rest keeps its stop. */
  function refit() {
    if (!engine || engine.S.jump) return;
    stage.resize();
    engine.measure();
    engine.place(engine.current());
  }

  return {
    get S() { return engine && engine.S; },
    get engine() { return engine; },
    get lenis() { return lenis; },
    start(id) {
      if (engine) return;
      lenis = new Lenis({ autoRaf: false, lerp: 0.1, smoothWheel: true, syncTouch: false });
      app.addEventListener("click", veil, true);
      engine = createEngine({
        lenis, stops: STOP_IDS, reduced, canSnap,
        onFrame: paint,
        onArrive: (stopId, info) => shell.arrive(stopId, info),
      });
      detach = engine.bind(win, { typing: typingIn });
      win.addEventListener("resize", resize);
      shell.showStop(id);
      stage.resize();
      engine.measure();
      engine.place(id);
      paint();
      // the sheet grows and shrinks with its moment, and the beam is cut at its edge: any change of its box is a new picture
      if (typeof ResizeObserver === "function") { watch = new ResizeObserver(repaint); watch.observe(app); }
      if (win.document.fonts && win.document.fonts.ready) win.document.fonts.ready.then(refit);
    },
    stop() {
      if (!engine) return;
      if (watch) watch.disconnect();
      watch = null;
      win.removeEventListener("resize", resize);
      app.removeEventListener("click", veil, true);
      veiled = false;
      detach();
      engine.destroy();
      lenis.destroy();
      engine = lenis = detach = null;
      opacity = "";
      app.style.opacity = "";
      app.inert = false;
      shell.setDialBig(null);
      win.scrollTo(0, 0);
    },
    go(id, { push = true } = {}) { if (engine) engine.jumpTo(id, { push }); },
    current: () => (engine ? engine.current() : null),
    /** The drawer holds the camera still while it is open (§3.6): the wheel and the touch no longer move the page. */
    lock(open) { if (!lenis) return; if (open) lenis.stop(); else { lenis.start(); engine.snap(); } },
    /** Something that held the snap back closed (the help): put a page that rests between two stops on the nearest. */
    settle() { if (engine) engine.snap(); },
    cancel() { if (engine) engine.cancelJump(); },
    repaint,
    resize,
  };
}
