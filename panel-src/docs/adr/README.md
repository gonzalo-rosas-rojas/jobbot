# Architecture decision records of panel-src

One page per decision that shapes this folder, in the short form of Michael Nygard: context, decision, consequences. They were written on 2026-10-10 from the
design spec (not published here; its section 14 holds the numbered decisions such as D-20) and from the commits, so each one says
when the thing was really decided. The spec stays the place for the idea; these pages are the place for "why is it like this, and what does it cost".

| # | Decision | Status |
|---|---|---|
| [0001](0001-esbuild-to-one-inline-page.md) | esbuild bundles everything into ONE inline page | Accepted |
| [0002](0002-strict-csp-trusted-types-and-the-guard.md) | Strict CSP, Trusted Types and a guard that refuses the page | Accepted |
| [0003](0003-no-framework.md) | No framework: DOM built with `h()` and patched by parts | Accepted |
| [0004](0004-the-moment-contract.md) | A moment is a small contract, and `ctx` is its only door | Accepted |
| [0005](0005-budgets-and-their-history.md) | Two byte budgets that break the build, and how they moved | Accepted |
| [0006](0006-two-presses-for-what-cannot-be-undone.md) | What cannot be undone asks twice ("dos pulsaciones") | Accepted |
| [0007](0007-demo-mode.md) | The demo is the same page with `window.__DEMO__` | Accepted |
| [0008](0008-dependencies-enter-through-check-lock.md) | Code that is not ours enters only through `check-lock.mjs` | Accepted |
| [0009](0009-where-the-story-lives-and-its-budget.md) | Where the story ("relato") lives and how its bytes are paid | Proposed, waiting for Gonza (D1) |

How to add one: copy the shape of any page, take the next number, keep it under a page, and write the date of the decision (not only of the record). Do not edit the
decision of an accepted record: write a new one that supersedes it and change the old status to `Superseded by NNNN`. A record is written or reviewed by a human or by an
agent whose brief says so; an agent does not decide something that belongs to Gonza by writing it down here.
