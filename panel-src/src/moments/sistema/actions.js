// What the one key of SISTEMA does (spec §3.4: `p`, "parar / reanudar el motor"). It has no DOM: the session, the sound, the clock and the
// timers are handed in, so the whole of it runs in Node with a fake session. The switch itself (STOP at once, quitting STOP in two presses,
// the Deshacer toast, a refusal that leaves the page as it was) is the one COLA also uses: ../shared/engine.js.
//
//   const actions = createActions({ session, sound, announce, t, toast, onChange, now, timers, held });
//   await actions.run(view.stop)  -> "armed" | "fired" | "ignored"          actions.state() -> { intent, pending, armed }
//   actions.settle()   a snapshot arrived: after an answered action it is the truth, so the intent goes   actions.destroy()

import { heldKey } from "../../core/dom.js";
import { createArmer } from "../shared/armer.js";
import { createEngineSwitch, REQUESTS } from "../shared/engine.js";

export { REQUESTS };

export function createActions({ session, sound, announce, t, toast = null, onChange = () => {}, now = Date.now, timers = { set: (fn, ms) => setTimeout(fn, ms), clear: (id) => clearTimeout(id) }, held = heldKey }) {
  const armer = createArmer({ announce, sound, t, onChange: () => onChange(), now, timers, held });
  const engine = createEngineSwitch({ armer, session, sound, announce, t, toast, onChange: () => onChange() });

  return {
    run: (stop) => engine.press(!!(stop && stop.on)),
    state: () => ({ ...engine.state(), armed: armer.any }),
    settle: engine.settle,
    destroy() {
      armer.drop();
      engine.reset();
    },
  };
}
