// The address says where you are, so Back and Forward walk the stops (spec §3.7): `#prologo`, `#hoy`, `#procesos`, `#cola`,
// `#aprender`, `#sistema`, plus `/<job key>` while the drawer is open. A jump by key or by the bar and the opening of the drawer
// each add one history entry (pushState); everything else (the camera settling after a scroll, a stop reached by the wheel)
// only rewrites the current one (replaceState). A panel shown inside another page (`embedded`) never touches the history.
// The route is { stop, job } and it is read with `routeOfHash` of core/route.js, the one that knows what a good address is.

import { hashOf, routeOfHash } from "../core/route.js";

/** createRouter({ embedded, win }) -> { pushStop(stop, jobKey), replaceStop(stop, jobKey), back(), listen(fn), read() }. `win` is injectable for Node. */
export function createRouter({ embedded = false, win = window } = {}) {
  let ignore = 0;                                  // pops that we caused ourselves (closing the drawer takes its entry back)

  /** What the address says now: { stop, job }, or null. A plain function, so `listen` never depends on how the router is called. */
  const read = () => {
    const route = routeOfHash(win.location.hash);
    return route && { stop: route.room, job: route.job };
  };

  /** The address that says `stop` (and the open application, if any), or null when there is nothing to write: embedded, or it already says that. */
  function addressFor(stop, jobKey) {
    if (embedded) return null;
    const hash = hashOf(stop, stop === "prologo" ? null : jobKey);
    return win.location.hash === hash ? null : win.location.pathname + hash;
  }

  return {
    /** A jump or the opening of the drawer: the address for a stop is a NEW history entry. True when it was added. */
    pushStop(stop, jobKey = null) {
      const url = addressFor(stop, jobKey);
      if (url === null) return false;
      win.history.pushState(null, "", url);
      return true;
    },
    /** Everything else (the camera settling, a step inside the drawer, a close): the address for a stop rewrites the current entry. Always false: no entry was added. */
    replaceStop(stop, jobKey = null) {
      const url = addressFor(stop, jobKey);
      if (url !== null) win.history.replaceState(null, "", url);
      return false;
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
