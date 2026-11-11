// The address says where you are, so Back and Forward walk the stops (spec §3.7): `#prologo`, `#hoy`, `#procesos`, `#cola`,
// `#aprender`, `#sistema`, plus `/<job key>` while the drawer is open. A jump by key or by the bar and the opening of the drawer
// each add one history entry (pushState); everything else (the camera settling after a scroll, a stop reached by the wheel)
// only rewrites the current one (replaceState). A panel shown inside another page (`embedded`) never touches the history.
// The route is { stop, job } and it is read with `routeOfHash` of core/store.js, the one that knows what a good address is.

import { hashOf, routeOfHash } from "../core/store.js";

/** createRouter({ embedded, win }) -> { write(stop, jobKey, { push }), back(), listen(fn), read() }. `win` is injectable for Node. */
export function createRouter({ embedded = false, win = window } = {}) {
  let ignore = 0;                                  // pops that we caused ourselves (closing the drawer takes its entry back)

  /** What the address says now: { stop, job }, or null. A plain function, so `listen` never depends on how the router is called. */
  const read = () => {
    const route = routeOfHash(win.location.hash);
    return route && { stop: route.room, job: route.job };
  };

  return {
    /** Writes the address for a stop (and the open application, if any); true when a new history entry was added. */
    write(stop, jobKey = null, { push = false } = {}) {
      if (embedded) return false;
      const hash = hashOf(stop, stop === "prologo" ? null : jobKey);
      if (win.location.hash === hash) return false;
      const url = win.location.pathname + hash;
      if (push) win.history.pushState(null, "", url);
      else win.history.replaceState(null, "", url);
      return push;
    },
    /** Takes back the history entry the drawer added, so that "Back" does not open it again. */
    back() {
      ignore += 1;
      win.history.back();
    },
    /** `fn({ stop, job })` for every Back or Forward that was not ours; no address at all is HOY. Returns the function that stops listening. */
    listen(fn) {
      const onPop = () => {
        if (ignore > 0) { ignore -= 1; return; }
        const route = read();
        // An address that names no stop (a skip link's #app, a stray anchor) is not a place to go.
        if (!route && win.location.hash && win.location.hash !== "#") return;
        fn(route || { stop: "hoy", job: null });
      };
      win.addEventListener("popstate", onPop);
      return () => win.removeEventListener("popstate", onPop);
    },
    read,
  };
}
