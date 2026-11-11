// The conversation with the panel server: the token, every request, the refresh loop and what it keeps (the last
// snapshot, its model, the news since the previous one, the error). It has no DOM: the browser, the clock, the timers
// and "is the person typing" are all handed in, so it runs in Node with a fake fetch. Ported from the old page, with
// the polling made safe: one request at a time, a request that never answers cut off, a stale answer ignored, a longer wait after each failure, the
// 30 seconds counted from the END of the last answer, and a hidden page that asks nothing and reads again the moment it comes back.
//
//   const session = createSession({ token, isTyping, isVisible });
//   session.subscribe((event) => { ... });   // { type, snapshot, model, error, news, toast? }
//   await session.start();                   // first picture, then a poll every 30 s while the page is visible
//
// Event types: "snapshot" (a new picture was accepted: draw it), "error" (a refresh failed; `error.fatal` is true when
// the token is wrong), "recovered" (the server answered again with nothing new), "toast" (say `toast.message`).

import { t } from "../i18n/index.js";
import { model as buildModel } from "./model.js";
import { diffSnapshots, sinceLastVisit } from "./diff.js";

export const POLL_MS = 30000;
// Nothing changed, but the clock did: the countdowns, "3 h ago" and the days without news are worked out from the
// snapshot's own time, so an old picture is drawn again after this long.
const REDRAW_MS = 10 * 60 * 1000;
// The marks of what is new last this long; a redraw after that does not point at them again.
export const FRESH_MS = 4000;
const MAX_BACKOFF_MS = 2 * 60 * 1000;
// How long one request may take before it is given up: far more than a big base needs (7 to 12 s on his, with the debug binary), far less than "never".
const REQUEST_TIMEOUT_MS = 60 * 1000;

/** The token arrives in the address as `?t=`; a panel inside another page (or with `?embed`) leaves the history alone. */
export function parseLaunch(search, framed) {
  const params = new URLSearchParams(search || "");
  return { token: params.get("t") || "", embedded: !!framed || params.has("embed") };
}

const idle = () => ({ fresh: new Set(), lines: [], big: false });

export function createSession(options = {}) {
  const {
    fetchImpl = (...args) => globalThis.fetch(...args),
    now = Date.now,
    isTyping = () => false,
    isVisible = () => true,
    token = "",
    // The static demo hands in { api(path, body), blob(path), attach({ refresh }) } with made-up data; the real panel never has it.
    demo = null,
    timers = { setTimeout: (...args) => globalThis.setTimeout(...args), clearTimeout: (id) => globalThis.clearTimeout(id) },
    pollMs = POLL_MS,
    redrawMs = REDRAW_MS,
    freshMs = FRESH_MS,
    maxBackoffMs = MAX_BACKOFF_MS,
    requestTimeoutMs = REQUEST_TIMEOUT_MS,
    // coming back to a page that was hidden reads at once, unless the last read is this fresh (an alt-tab and back is not a reason to ask again)
    wakeMinMs = Math.min(5000, pollMs),
  } = options;

  let snapshot = null;
  let model = null;
  let error = null;
  let news = idle();
  let fresh = new Set();
  let lastKey = "";
  let inflight = null;
  let queuedForce = false;
  let failures = 0;
  let lastTryAt = 0;
  let running = false;
  let paused = false;                                // the page is hidden: no timer is armed until `wake()`
  let timer = null;
  let freshTimer = null;                             // clears `fresh`; each accepted picture replaces it, so the marks last `freshMs` from the latest
  const subscribers = new Set();

  function emit(type, extra) {
    const event = Object.assign({ type, snapshot, model, error, news }, extra);
    for (const fn of Array.from(subscribers)) {
      try { fn(event); } catch (e) { console.error(e); }
    }
  }

  /** One request, with its time limit. Always resolves to { ok, status, data }; a network failure is status 0 with a message, and a request that
   *  took longer than `requestTimeoutMs` is aborted and is status 0, `timedOut`, with its own message. */
  async function attempt(path, body, key) {
    const controller = typeof AbortController === "function" ? new AbortController() : null;
    let timer = null;
    const expired = new Promise((resolve) => {
      timer = timers.setTimeout(() => {
        if (controller) controller.abort();
        resolve({ ok: false, status: 0, timedOut: true, data: { message: t("err.timeout") } });
      }, requestTimeoutMs);
    });
    const work = request(path, body, controller, key);
    work.catch(() => {});                              // an answer that comes after the limit is nobody's
    try { return await Promise.race([work, expired]); } finally { timers.clearTimeout(timer); }
  }

  const actionOf = (path, body) => path + "\n" + JSON.stringify(body);

  // The POSTs on their way, by action (path and body): asking the same action again before it comes back joins it. `act` already gathers the same
  // action before it reaches `api`, so this layer only counts for whoever calls `api` directly with a POST.
  const pending = new Map();

  /** The request of an action. Not async on purpose: the same action asked again while its POST is out gets the same promise, not a second request.
   *  A POST is one action with one `Idempotency-Key`: if it got no answer at all (status 0) it is sent once more with the same key and bytes, so the
   *  server answers the first run instead of doing it twice. A GET carries no key and is not repeated. */
  function api(path, body) {
    if (body === undefined || demo) return attempt(path, body);
    const action = actionOf(path, body);
    const joined = pending.get(action);
    if (joined) return joined;
    const key = globalThis.crypto.randomUUID();
    const sent = (async () => {
      try {
        const first = await attempt(path, body, key);
        return first.status === 0 ? await attempt(path, body, key) : first;
      } finally {
        pending.delete(action);
      }
    })();
    pending.set(action, sent);
    return sent;
  }

  async function request(path, body, controller, key) {
    if (demo) {
      try { return await demo.api(path, body); } catch (e) { return { ok: false, status: 0, data: { message: t("err.network") } }; }
    }
    const init = { headers: { "X-Panel-Token": token } };
    if (controller) init.signal = controller.signal;
    if (body !== undefined) {
      init.method = "POST";
      init.headers["Content-Type"] = "application/json";
      init.body = JSON.stringify(body);
      if (key) init.headers["Idempotency-Key"] = key;
    }
    let res;
    try {
      res = await fetchImpl(path, init);
    } catch (e) {
      return { ok: false, status: 0, data: { message: t("err.network") } };
    }
    let data = null;
    try { data = await res.json(); } catch (e) { /* not JSON */ }
    return { ok: res.ok, status: res.status, data };
  }

  /** A file (the dry-run screenshot): its bytes, or null when it is not there. */
  async function blob(path) {
    // the demo draws its captures itself and never asks the network
    if (demo && typeof demo.blob === "function") {
      try { return (await demo.blob(path)) || null; } catch (e) { return null; }
    }
    try {
      const res = await fetchImpl(path, { headers: { "X-Panel-Token": token } });
      return res.ok ? await res.blob() : null;
    } catch (e) {
      return null;
    }
  }

  const said = (r, fallback) => (r.data && r.data.message) || fallback;

  function fail(kind, r, message) {
    failures += 1;
    error = { kind, status: r.status, message, fatal: kind === "token" };
    emit("error");
    if (!error.fatal) emit("toast", { toast: { ok: false, message } });
    return false;
  }

  function accepted() {
    failures = 0;
    if (error) {
      error = null;
      emit("recovered");
    }
    return true;
  }

  async function runOnce(force) {
    lastTryAt = now();
    const r = await api("/api/snapshot");
    if (r.status === 401 || r.status === 403) return fail("token", r, t("err.token"));
    if (!r.ok || !r.data) return fail(r.timedOut ? "timeout" : r.status === 0 ? "network" : "server", r, said(r, t("err.snapshot")));
    const data = r.data;
    // An answer older than the picture on screen (a slow one that crossed a newer one) is dropped.
    if (snapshot && Date.parse(data.generated_at) < Date.parse(snapshot.generated_at)) return accepted();
    const key = JSON.stringify(Object.assign({}, data, { generated_at: "" }));
    const aged = snapshot != null && Date.parse(data.generated_at) - Date.parse(snapshot.generated_at) >= redrawMs;
    if (!force && key === lastKey && !aged) return accepted();
    // Never redraw under a half-typed answer; the next poll tries again.
    if (!force && isTyping()) return accepted();
    let next;
    let upNext;
    try {
      next = buildModel(data);
      upNext = snapshot ? diffSnapshots(snapshot, data) : sinceLastVisit(data);
    } catch (e) {
      console.error(e);
      return fail("data", r, t("err.snapshot"));
    }
    failures = 0;
    error = null;
    lastKey = key;
    snapshot = data;
    model = next;
    news = upNext;
    fresh = news.fresh;
    emit("snapshot");
    // every picture restarts the clearing: a quick second picture keeps its marks for the whole of `freshMs`
    timers.clearTimeout(freshTimer);
    freshTimer = timers.setTimeout(() => { fresh = new Set(); }, freshMs);
    return true;
  }

  /**
   * Reads the snapshot again. One request at a time: a refresh asked while another is running joins it, and a forced
   * one makes it run once more afterwards, so a change made after the first request left is never missed. Not forced,
   * it does nothing when nothing changed and waits while the person is typing in a field.
   */
  function refresh(opt) {
    const force = typeof opt === "object" && opt !== null ? !!opt.force : !!opt;
    if (inflight) {
      if (force) queuedForce = true;
      return inflight;
    }
    inflight = (async () => {
      try {
        let ok = await runOnce(force);
        while (queuedForce) {
          queuedForce = false;
          ok = await runOnce(true);
        }
        return ok;
      } finally {
        inflight = null;
        schedule();                                     // the next read is a whole interval after the END of this one (an action's read included)
      }
    })();
    return inflight;
  }

  // The actions whose request is still out, by action: the same one asked again (a double click, also in the demo) is that action, not a second
  // one, so its toast and its callbacks (where the sounds are) run once. It is joined only until the request comes back: after that it is new.
  const acting = new Map();

  /**
   * An action: the request, a toast with what the server said, then everything is read again. `onOk(result)` runs when
   * it worked; `beforeRefresh(result)` may hold the refresh back (the page shows "approved" for a moment first).
   * Asked again while its request is out, it resolves with the same result and runs none of this a second time.
   */
  function act(path, body, opts = {}) {
    const action = actionOf(path, body);
    const joined = acting.get(action);
    if (joined) return joined;
    const run = (async () => {
      let r;
      try {
        r = await api(path, body);
      } finally {
        acting.delete(action);
      }
      const message = said(r, r.ok ? t("toast.done") : t("err.action"));
      emit("toast", { toast: { ok: r.ok, message } });
      if (r.ok && opts.onOk) opts.onOk(r);
      if (opts.beforeRefresh) await opts.beforeRefresh(r);
      await refresh({ force: true });
      return Object.assign({}, r, { message });
    })();
    acting.set(action, run);
    return run;
  }

  // ---- The 30-second loop. After a failure the wait doubles (60 s, then 2 minutes) until the server answers again.

  function schedule() {
    if (!running || paused) return;
    timers.clearTimeout(timer);
    const delay = failures === 0 ? pollMs : Math.min(maxBackoffMs, pollMs * 2 ** failures);
    timer = timers.setTimeout(tick, delay);
  }

  async function tick() {
    timer = null;
    if (!running) return;
    // Polling only while the page is visible: a hidden tab pays nothing.
    // legacy quirk: it also goes on after a 401 (the key will not come back without a new link), only less often
    if (isVisible()) {
      try { await refresh({ force: false }); } catch (e) { console.error(e); }
    }
    schedule();
  }

  /** Takes the first picture (forced) and starts polling; resolves with whether the first read worked. */
  function start() {
    if (running) return Promise.resolve(true);
    running = true;
    return refresh({ force: true }).then((ok) => { schedule(); return ok; }, (e) => { console.error(e); schedule(); return false; });
  }

  function stop() {
    running = false;
    timers.clearTimeout(timer);
    timer = null;
  }

  /** The page was hidden: the loop stops asking (and arms no timer) until `wake()`. A request already in flight is let finish. */
  function pause() {
    paused = true;
    timers.clearTimeout(timer);
    timer = null;
  }

  /** The page became visible: the loop is back, and the snapshot is read now (not at the next turn of a loop that was stopped) unless the last try is fresh. */
  function wake() {
    if (!running || !isVisible()) return Promise.resolve(false);
    paused = false;
    if (timer == null && !inflight) schedule();
    if (now() - lastTryAt < wakeMinMs) return Promise.resolve(false);
    return refresh({ force: false }).then((ok) => { schedule(); return ok; });
  }

  // the demo asks for a new picture itself when the language changes, so its free texts follow the buttons at once
  if (demo && typeof demo.attach === "function") demo.attach({ refresh });

  return {
    get snapshot() { return snapshot; },
    get model() { return model; },
    get error() { return error; },
    get news() { return news; },
    /** The marks of what is new ("mail:", "task:", "card:") for the next few seconds; empty afterwards. */
    get fresh() { return fresh; },
    refresh,
    act,
    api,
    blob,
    said,
    start,
    stop,
    pause,
    wake,
    subscribe(fn) {
      subscribers.add(fn);
      return () => subscribers.delete(fn);
    },
  };
}
