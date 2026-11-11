// The two-press rule of what cannot be undone, once (docs/adr/0006-two-presses-for-what-cannot-be-undone.md). The first press ARMS a target and says so (the live
// region, the "arm" cue); the second DELIBERATE press goes through. A held key and a double click are one press, not two (ARM_MIN_MS); an armed press forgets itself
// after ARM_MAX_MS. HOY, COLA, APRENDER and SISTEMA each had their own copy of this and the copies had drifted; the rule is the same for all of them, so it lives here.
//
//   const armer = createArmer({ announce, sound, t, onChange });
//   armer.press(ref, id)             -> "armed" | "ignored" | "confirmed"   ("confirmed" has played the "confirm" cue and disarmed: now do the thing)
//   armer.armedOf(ref)               -> the action armed on that target, or null
//   armer.isArmed(ref, id?)          armer.any (is anything armed at all)
//   armer.release(ref)               a target that goes on to another verb forgets its arming
//   armer.disarm()                   drops it and tells `onChange(ref)` once, so the render can paint that target again
//   armer.drop()                     drops it without telling anyone (the moment is going away)
// `ref` is whatever names the target (a row id, or "engine"); `onChange(ref)` hears every arming and disarming. `now`, `timers` and `held` are the clock, the timers and the
// held-key probe, handed in so that Node can test all of it with a fake clock.

import { ARM_MIN_MS, ARM_MAX_MS, heldKey } from "../../core/dom.js";

export function createArmer({ announce, sound, t, onChange = () => {}, now = Date.now, timers = { set: (fn, ms) => setTimeout(fn, ms), clear: (id) => clearTimeout(id) }, held = heldKey }) {
  let armed = null; // { ref, id, at, timer }

  function disarm() {
    if (!armed) return;
    const was = armed;
    armed = null;
    timers.clear(was.timer);
    onChange(was.ref);
  }

  function arm(ref, id) {
    disarm();
    armed = { ref, id, at: now(), timer: timers.set(() => disarm(), ARM_MAX_MS) };
    announce(t("act.sure"));
    sound.play("arm");
    onChange(ref);
  }

  return {
    press(ref, id) {
      const mine = armed && armed.ref === ref && armed.id === id;
      if (!mine) { arm(ref, id); return "armed"; }
      if (held() || now() - armed.at < ARM_MIN_MS) return "ignored";   // a held key or a double click is one press, not two
      disarm();
      sound.play("confirm");
      return "confirmed";
    },
    release(ref) { if (armed && armed.ref === ref) disarm(); },
    disarm,
    isArmed: (ref, id) => !!armed && armed.ref === ref && (id == null || armed.id === id),
    armedOf: (ref) => (armed && armed.ref === ref ? armed.id : null),
    /** Armed at all (SISTEMA has one target). */
    get any() { return armed !== null; },
    /** Timer cleared without telling anyone (a moment that is going away). */
    drop() { if (armed) timers.clear(armed.timer); armed = null; },
  };
}
