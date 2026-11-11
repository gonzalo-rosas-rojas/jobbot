// The keyboard's place in a list: j and k move it over the rows of the stop on screen (`[data-card]` and `[data-task]`), Enter
// opens what it is on. The row it stands on carries `data-cursor="true"`. It is shared state (`view.cursor`): a moment's redraw
// replaces every node, so the shell calls `apply()` after each `update` and the cursor finds its row again by key.
// Moving the cursor may scroll the LIST the row is in (an element with data-lenis-prevent), never the page: scrolling the page
// would move the camera, and the camera belongs to the scene engine alone.

const rowKey = (el) => (el.dataset.card ? "c:" + el.dataset.card : "t:" + el.dataset.task);

/** createCursor({ view, sound, reduced, root }) -> { key, set(key), follow(job, origin), clear(), move(step), apply(), rows(), current(), jobOf(row), reveal(row) } */
export function createCursor({ view, sound, reduced = () => false, root = () => document.getElementById("app") }) {
  /** The rows the person can see, outermost first: a card inside a card counts once; nothing inside an SVG. */
  function rows() {
    const host = root();
    if (!host) return [];
    return Array.from(host.querySelectorAll("[data-card], [data-task]")).filter((el) => {
      if (el.closest("svg") || el.getClientRects().length === 0) return false;
      const outer = el.parentElement && el.parentElement.closest("[data-card], [data-task]");
      return !outer;
    });
  }

  function apply() {
    const host = root();
    if (!host) return;
    for (const el of host.querySelectorAll('[data-cursor="true"]')) el.removeAttribute("data-cursor");
    if (!view.cursor) return;
    const el = rows().find((row) => rowKey(row) === view.cursor);
    if (el) el.setAttribute("data-cursor", "true");
    else view.cursor = null;
  }

  /** Brings a row into view inside its own scroller, by the least amount; a row that is in no scroller is where it is. */
  function reveal(el) {
    const box = el.closest("[data-lenis-prevent]");
    if (!box) return;
    const a = el.getBoundingClientRect();
    const b = box.getBoundingClientRect();
    const over = a.top < b.top ? a.top - b.top : a.bottom > b.bottom ? a.bottom - b.bottom : 0;
    if (over) box.scrollBy({ top: over, behavior: reduced() ? "auto" : "smooth" });
  }

  function move(step) {
    const list = rows();
    if (list.length === 0) return;
    let at = list.findIndex((row) => rowKey(row) === view.cursor);
    at = at < 0 ? (step > 0 ? 0 : list.length - 1) : Math.min(list.length - 1, Math.max(0, at + step));
    view.cursor = rowKey(list[at]);
    apply();
    sound.play("step");
    reveal(list[at]);
  }

  /** The row under the cursor, or null. */
  function current() {
    return view.cursor ? rows().find((row) => rowKey(row) === view.cursor) || null : null;
  }

  /**
   * The drawer shows this application: the cursor is its row, brought into view. A row that is not drawn here (a filter hides it, or it is of
   * another stop) leaves the cursor on none, never on the one before. `origin` is the row the application was opened from (an element, or the
   * id a moment gives its row in `data-row`): when that is the row the cursor is on, the cursor stays, because a task's row stands for its
   * application without carrying the application's key.
   */
  function follow(job, origin = null) {
    const here = current();
    const opener = origin == null ? null : typeof origin === "string" ? rows().find((row) => row.dataset.row === origin) || null : origin;
    const fromHere = !!here && !!opener && (opener === here || (typeof here.contains === "function" && here.contains(opener)));
    if (!fromHere) { view.cursor = "c:" + job; apply(); }
    const row = current();
    if (row) reveal(row);
  }

  return {
    get key() { return view.cursor; },
    set(key) { view.cursor = key; apply(); },
    follow,
    clear() { view.cursor = null; apply(); },
    move,
    apply,
    rows,
    current,
    /** Brings a row into view inside its own list (the drawer's j and k use it), never the page. */
    reveal,
    /** The key of the application a row belongs to, or null for a task row. */
    jobOf: (row) => (row && row.dataset.card) || null,
  };
}
