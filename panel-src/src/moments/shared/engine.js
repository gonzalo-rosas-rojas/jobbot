// The engine's switch (`p`), once, for COLA and SISTEMA. STOP is instant: it is the safety switch and stopping is the safe direction, so one press sends
// the stop with no confirmation. Quitting STOP asks twice (the first press arms it, a second deliberate one resumes), because the bot would start sending
// on its own again. Both leave a toast with "Deshacer", which needs no second press: it is the user's own undo, and it is one more press of the switch
// with its own toast. A refusal (a 409, a 500, no network) gives the page back as it was, sounds like an error and leaves no toast. While a request is
// out every press, Deshacer included, is ignored.
//
//   const engine = createEngineSwitch({ armer, session, sound, announce, t, toast, onChange });
//   await engine.press(stopped)  -> "armed" | "fired" | "ignored"      engine.state() -> { intent, pending }
//   engine.settle()   a snapshot arrived: after an answered request it is the truth, so the intent goes       engine.reset()
// `armer` is the moment's own (createArmer): the switch is armed under the ref "engine", so arming a row disarms it and the other way round.
// `intent` is STOP as the page should show it before the server's snapshot: true on, false off, null whatever the snapshot says.

export const REQUESTS = Object.freeze({ stop: { path: "/api/stop" }, resume: { path: "/api/resume" } });
const DONE = Object.freeze({ stop: "sistema.stop.done", resume: "sistema.resume.done" });

/** The request that switches the engine: resume when it is stopped, stop when it runs. */
export const engineRequest = (stopped) => ({ path: (stopped ? REQUESTS.resume : REQUESTS.stop).path, body: {} });

export function createEngineSwitch({ armer, session, sound, announce, t, toast = null, onChange = () => {} }) {
  let intent = null;
  let pending = false;
  let answered = false;    // the server said yes and the intent is waiting for a snapshot to take over

  function tell(kind) {
    const text = t(DONE[kind]);
    // with a toast, the toast is what the screen reader hears (the page has one live region): never the same sentence twice
    if (toast) toast(text, true, { undo: () => (pending ? "ignored" : send(kind === "stop" ? "resume" : "stop")) });
    else announce(text);
  }

  async function send(kind) {
    const request = engineRequest(kind === "resume");
    intent = kind === "stop";
    pending = true;
    answered = false;
    onChange();
    let ok = false;
    try {
      const r = await session.act(request.path, request.body, {
        beforeRefresh: (res) => {
          if (res.ok) {
            sound.play(kind);
          } else {
            sound.play("error");
            intent = null;                    // the server said no: the page is as it was
            pending = false;
            onChange();
          }
        },
      });
      ok = !!(r && r.ok);
    } finally {
      pending = false;
      answered = ok;
      if (!ok) intent = null;
      onChange();
    }
    if (ok) tell(kind);
    return "fired";
  }

  return {
    async press(stopped) {
      if (pending) return "ignored";
      if (!stopped) {
        armer.release("engine");              // only the switch's own arming: a row armed in the same stop stays armed
        return send("stop");
      }
      const step = armer.press("engine", "engine");
      if (step !== "confirmed") return step;
      return send("resume");
    },
    state: () => ({ intent, pending }),
    settle() {
      if (pending || !answered) return;
      answered = false;
      intent = null;
      onChange();
    },
    reset() {
      intent = null;
      pending = false;
      answered = false;
    },
  };
}
