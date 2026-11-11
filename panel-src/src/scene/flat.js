// The flat page (spec §3.11, A14): under 760 px there is no stage, no canvas and no Lenis. The page is a header with the stops as plain
// tabs (the prologue is the first one, "Anoche"), ONE sheet that holds the moment of the tab, and the bottom bar after the sheet. The
// same actions are buttons (the rows of every moment carry their own) and the three that existed only as a key in HOY and PROCESOS (`n`,
// `x`, `r`) have a touch way in: "Nueva tarea" at the top of the sheet (shell.js), and a tap on a row opens the drawer, whose common
// content registers a reply or withdraws. This is a `nav` of the shell, the twin of scene/camera.js.
//
// CSS decides what is hidden (the last media query of styles/stage.css) and this module decides who is in charge: both read the same
// number, FLAT_MAX. The page passes from one to the other when the window crosses it, without reloading.

const FLAT_MAX = 759;
const FLAT_QUERY = `(max-width: ${FLAT_MAX}px)`;

/** True for a window this narrow (CSS pixels, scrollbar included: the way a media query counts). */
export const isFlat = (width) => width <= FLAT_MAX;

/** watchFlat({ match, onChange }) -> { flat(), stop() }. `onChange(flat)` runs when the window crosses the threshold. */
export function watchFlat({ match = (query) => matchMedia(query), onChange }) {
  const query = match(FLAT_QUERY);
  const fn = () => onChange(query.matches);
  query.addEventListener("change", fn);
  return { flat: () => query.matches, stop: () => query.removeEventListener("change", fn) };
}

/** createFlat({ shell, win }) -> nav. A tab is a jump with no trip: the moment is swapped, the page goes to its top, the address is written. */
export function createFlat({ shell, win = window }) {
  return {
    start(id) {
      shell.stops.setFlat(true);
      shell.showStop(id);
      shell.stops.setActive(id);
    },
    stop() {
      shell.stops.setFlat(false);
    },
    go(id, { push = true } = {}) {
      shell.showStop(id);
      win.scrollTo(0, 0);
      shell.arrive(id, { push, manual: false });
    },
    current: () => shell.current(),
    lock() {},
    settle() {},
    cancel() {},
    repaint() {},
    resize() {},
  };
}
