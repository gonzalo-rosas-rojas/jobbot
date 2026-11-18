// How the panel talks to its server: one request with its time limit, the single retry of a POST that got no answer, and the file reads.
// It keeps no session state (no snapshot, no error, no timers of its own beyond the one of a request): everything it needs is handed in,
// so core/session.js owns what the page remembers and this owns the wire. The public demo plugs in here with its made-up server.

import { t } from "../i18n/index.js";

/** The name of an action: its path and its body, so the same one asked twice can be told apart from a different one. */
export const actionOf = (path, body) => path + "\n" + JSON.stringify(body);

export function createTransport({ fetchImpl, demo, token, timers, requestTimeoutMs }) {
  async function request(path, body, controller, key) {
    if (demo) {
      try { return await demo.api(path, body); } catch (e) {
        console.error(e);
        return { ok: false, status: 0, data: { message: t("err.network") } };
      }
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

  return { api, blob };
}
