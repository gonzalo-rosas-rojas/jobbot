# 0005. Two byte budgets that break the build, and how they moved

Status: Accepted. The caps are in `guard.mjs`; the second raise of the page cap is decision D-20 of the spec (section 14), taken without Gonza present and reversible. Recorded 2026-10-10.

## Context

Nobody notices a page growing 2 KB at a time. The page is served from `127.0.0.1`, so its bytes never cross a network, but a cap is still the defence against growth that nobody decided,
and the published demo (ADR 0007) is a different case that will need a tighter budget of its own.

## Decision

`guard.mjs` refuses to let `build.mjs` write the page when:

- the JS bundle is longer than **480 KB** (`JS_BUDGET`; it is the length of the MINIFIED bundle, whatever the old comment in the file said), or
- the whole page (script, style and icon sprite) is longer than **200 KB gzipped** (`HTML_GZ_BUDGET`).

A cap is raised only by writing down, in the commit and here, what was measured and why. Nothing is cut silently to fit, and nothing is raised silently to fit.

History of the caps:

| Date | Cap | Why |
|---|---|---|
| 2026-10-07 | JS 800 KB, then 1000 KB | the first tower: three.js alone was about 530 KB |
| 2026-10-09 (`fd8bbb5`) | JS **480 KB**, page **110 KB gz** | the rebuilt page: the plan for HOY and a prologue with the story inside (spec A6) |
| 2026-10-09 (`8c3f8ff`) | page **160 KB gz** | decision D-20: the five real moments, the drawer and the palette measured 149.2 KB gz; 150 plus 10 of room |
| 2026-10-09 (`fa6e8ba`) | page **200 KB gz** | the sound of the actions and of the background (33 cues, the player, the detector, the sheet "Escuchar los sonidos", two languages): about 13 KB gz, measured 169.9 KB |

Measured on 2026-10-10 (`d206b4f`): script 450.9 KB of 480, page 171.2 KB gz of 200.

## Consequences

- Every raise so far came after the measurement, not before: the cap follows the product and is not a plan. The next big addition does not fit (ADR 0009).
- Two global numbers say that the page is too big but not where it grew. **Update 2026-10-10:** `node build.mjs` now also prints the script by area (each layer, each moment, each dictionary,
  each library; `tools/budgets.mjs`) and refuses to write the page when an area, or the whole script, grew past its margin over the baseline in `budgets.json` (the larger of 3 KB and 5 % per area;
  6 KB and 1.5 % for the whole). Growth that is meant is accepted with `node build.mjs --update-budgets`, which shows up as a diff of `budgets.json` with its reason in the commit. This adds a
  tripwire per area; it moves neither global cap. Styles are not in it yet (104 KB of the page).
- The caps measure what the browser downloads from the local server. They say nothing about the cost of mounting a moment on a slow CPU, which is measured separately.
