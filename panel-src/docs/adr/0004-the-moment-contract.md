# 0004. A moment is a small contract, and `ctx` is its only door

Status: Accepted. Decided 2026-10-08 (spec sections 5 and 6, the registry of the first chassis). Recorded 2026-10-10.

## Context

The page has six stops (a prologue and five moments: HOY, PROCESOS, COLA, APRENDER, SISTEMA) that different agents built at the same time. They had to stay independent, testable without
a browser, and replaceable one at a time, while the shell keeps ONE of them mounted under the glass sheet.

## Decision

A moment is the default export of `src/moments/<id>.js`: `{ id, mount(host, ctx), update(model, ctx), unmount() }` and, when it has them, `onKey(actionId, row, event)`,
`arrive(opts)` and `drawer(detail, card, ctx)`.

- **`ctx` is the only door.** The shell builds it in `src/ui/shell.js` (language and words, sound, settings, session, cursor, navigation, drawer, announce, toast, the current model).
  A moment never reaches for the stage, the sound or the session by itself, and never imports another moment: it asks the shell (`ctx.go`, `ctx.open`).
- **Three files per big moment**: `view` is pure (model in, rows out), `actions` has no DOM (the session, the sound and the clock are handed in, so Node tests every request, every
  two-press verb and every pending row with a fake session), `render` builds the DOM once and patches it (ADR 0003).
- **Imports go one way** across the layers `core`, `i18n`, `scene`, `stage`, `sound`, `ui`, `moments` (table in `AGENT-RULES.md`).
- The words live in the i18n dictionaries and the keys a stop answers in `src/ui/wired.js`; the help promises nothing else.

## Consequences

- The part that matters most (what each verb sends to the server) is tested in Node, fast, with no browser.
- For its first two days this was a convention: only the tower's archive had a test that watched imports, and 19 edges had grown the wrong way, 13 of them to HOY's `util.js`,
  `actions.js` and `forms.js`, which five other moments import: a shared kernel that does not say so. Since 2026-10-10 `test/arch.layers.test.mjs` judges every edge of the
  import graph, fails on a new wrong one and keeps the 19 as a list of debts that can only shrink.
- A new stop touches about ten files (`AGENT-RULES.md` lists them); the ids of the stops are repeated in several tables and no single manifest feeds them.
- State in a variable of the module (`let S = null`) means one instance of a moment at a time; two instances (a test in the same process, a mini-map of a moment) would need a rewrite.
