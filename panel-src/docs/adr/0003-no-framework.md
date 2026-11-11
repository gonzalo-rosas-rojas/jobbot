# 0003. No framework: DOM built with `h()` and patched by parts

Status: Accepted. Decided with the rebuild of 2026-10-08 (the hand-written render of the first panel was kept); the evidence below is from the architecture review of 2026-10-09. Recorded 2026-10-10.

## Context

Each big moment draws a sheet of lists and cards that changes every 30 seconds (the snapshot) and on every action. The question was whether a view library would do that better than our
own code. The review read the source of the candidates against the guard of ADR 0002:

| Candidate | What the source contains | Verdict |
|---|---|---|
| Preact, Lit, Svelte 5 | the word `innerHTML` (Lit and Svelte also create a Trusted Types policy that lets everything through) | refused by the guard |
| Alpine | needs `unsafe-eval` | refused by the CSP |
| htmx | inserts HTML from strings | refused by principle |
| van.js (1.2 KB gz) | clean | would work |
| htm (0.7 KB gz) | not read in detail | could give templates over `h()` |

The size of a library (1 to 9 KB gz) is not the issue next to about 148 KB gz of script, and the whole render layer is about 66 KB minified: even a perfect rewrite would not
solve the budget (ADR 0005).

## Decision

Stay with plain ES modules. A moment builds its DOM once with `h()`/`sv()` and patches it by parts (small `setText`, `setData`, `setHidden`, `sync` helpers and a signature per row), on
top of a pure `view` and an `actions` module with no DOM (ADR 0004). Changing the way the page renders is a decision for Gonza (D2 of the review); the only experiment proposed is to
port one moment to htm over `h()` on a branch and compare lines, bytes and mount time.

## Consequences

- Verbose but predictable: the four big moments do it the same way, the logic that matters (`view`, `actions`) runs in Node with no DOM, and no library can open a hole.
- The patch helpers were copied instead of shared (HOY had the original, APRENDER a copy) until 2026-11: now they live once in `src/moments/shared/util.js`, with `clean`, the one filter for text from outside, and every moment imports them from there.
- There is no reactivity. A moment's `update(model, ctx)` recomputes its view and patches; state lives in module variables, so a moment has one instance at a time.
- Leaving this path means rewriting about 66 KB of render code and its tests, and negotiating the guard.
