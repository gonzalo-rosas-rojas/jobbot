# 0006. What cannot be undone asks twice ("dos pulsaciones")

Status: Accepted. Decided 2026-10-08 (spec sections 3.4, 5.1 and 9.3; the first form was `twoStep` in `src/core/dom.js`). Recorded 2026-10-10.

## Context

Approving a proposal sends an application, confirming says it was sent, marking a process withdrawn or rejected closes it, and a task marked done goes away. None of these can be taken back
from the panel, and the panel is driven by keys (`a`, `h`, `x`...) where a held key repeats and a double click is one habit, not two decisions.

## Decision

A verb marked `permanent` on its row works in two presses.

1. The first press **arms** that verb on that row. The page says so at once ("¿Seguro? Pulsa otra vez" in the live region, the "arm" cue of the sound, the button drawn armed).
2. The second press **confirms** only if it is deliberate: not a repeat of a held key (`heldKey()`), and at least `ARM_MIN_MS` = 350 ms after arming. Otherwise it is ignored and nothing changes.
3. The armed state **expires** after `ARM_MAX_MS` = 5000 ms, and it is dropped when another verb is pressed on that row or the same verb on another row.
4. The armed state lives in the `actions` module of the moment, never in the text of a button, so the page can redraw under it without disarming anything.

When the page has already asked in its own words ("did you do it?"), that question is the second press and the option `confirmed` skips the first.

## Consequences

- A slip of the finger or a held key cannot send or close anything. The cost is one extra, deliberate press, on the few verbs that need it.
- The rule was written five times, not once: in `actions.js` of HOY, COLA, APRENDER and SISTEMA, and as `twoStep` in `src/core/dom.js` (used by the detail drawer). The copies had already
  drifted: COLA's helper answered `null` where the others answered `"ignored"` to a press that does nothing (nothing outside the helper read it, but the next copy might). A fault in one
  copy confirms an irreversible action on one press.
- **Update 2026-10-10.** The four moments now take the rule from `createArmer` in `src/moments/shared/armer.js`, tested once in `test/moments.shared.armer.test.mjs` (held key, double click,
  expiry, confirmation, re-arming, release, drop). Two copies are left on purpose for a later pass: `twoStep` in `src/core/dom.js` (used by `src/ui/detail/act.js` and
  `src/moments/procesos/drawer-content.js`) and COLA's lote, which is confirmed by a question drawn on screen with its own minimum of `ARM_MIN_MS`. The same test lists the only files that may
  write the timing of the rule, so a new copy fails it.
