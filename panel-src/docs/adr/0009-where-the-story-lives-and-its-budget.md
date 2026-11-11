# 0009. Where the story ("relato") lives and how its bytes are paid

Status: **Proposed. Waiting for a decision from Gonza (D1 of the architecture review).** Nothing below has been decided; this page records the problem and the options so that the choice is made with numbers.

## Context

The spec (sections 2.5 and 8.3) puts the story, eight acts told on the first visit and on demand, inside the same bundle, mounted lazily, because one URL served by Rust is a fixed decision
(section 2.1). But "lazily" can only mean *mounted* lazily: ADR 0001 and ADR 0002 forbid loading code later (`import(` and script injection are in the guard, and the policy allows no
script without the nonce). The story therefore ships with every page load, working mode included.

Its cost is estimated in the spec at about 35 KB gz (115 KB uncompressed). Against what was measured on 2026-10-10 (`d206b4f`):

| | now | cap | with the story |
|---|---|---|---|
| script | 450.9 KB | 480 KB | about 566 KB: over by roughly 86 KB |
| page, gzipped | 171.2 KB | 200 KB | about 206 KB: over by roughly 6 KB |

Both caps (ADR 0005) would break. Two languages always travel together: the dictionaries are about 122 KB of the script (es 61.1, en 58.2, index 2.8 at the time of the review).

## Options

| | How | For | Against |
|---|---|---|---|
| a | raise the caps, ideally per area | the cheapest | the cap stops defending anything; it is "D-20" again |
| b | load the story as a nonce'd script after the page | the main page stays as it is | the guard and the policy would need an exception (`import(` / `createElement("script")` / Trusted Types for `src`): a security decision |
| c | a separate page `/relato` with its own policy, guard and budget | clean isolation | leaves the single URL and the continuity of the world; Rust must serve it |
| d | one language per page (one built page for each; the build already has `__DEFAULT_LANG__`) | saves about 20 KB gz of the 35 | changing language reloads the page; how the server picks the language was not checked |

The review's recommendation was to measure first (per-area budgets) and, by default, to take (d) together with a moderate rise of the caps (a); to keep (c) in case the story is an
independent episode by nature; and to avoid (b). It did not know exactly what the story is beyond what the spec says, and the right choice depends on how Gonza wants it to feel
(a transition inside the world, or a page of its own).

## Decision

None yet. Until Gonza decides, no agent raises a cap, adds a loader or builds the story into the bundle for this reason: ask him.

## Consequences

To be written when the decision is taken. The consequence of not deciding is that the story cannot be built without breaking the build.
