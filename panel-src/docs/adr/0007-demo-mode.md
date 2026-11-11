# 0007. The demo is the same page with `window.__DEMO__`

Status: Accepted. Decided 2026-10-07/08 (spec sections 2.5 and 8.4). Recorded 2026-10-10.

## Context

The panel needs a public face (a portfolio piece) and the browser tests need data that does not depend on Rust or on anybody's database. Both want the real page, not a mock of it, and
neither may ever hold a real company, person or e-mail.

## Decision

`node build.mjs demo` pastes `../scripts/demo/demo.js` into a copy of the page, right before the application's script. That script sets `window.__DEMO__ = { api, blob, attach, ... }`
and answers every request in memory with invented data (each text as a pair `[es, en]`); the data moves as the page is used, as the real server would. `createSession` takes the
`demo` object as an option and calls `demo.api(path, body)` instead of `fetch`. The real server never sets `__DEMO__`.

- `?scenario=calm|urgent|stop|stale|heavy|empty|window|closed` picks the mood of the board and `?lag=<ms>` makes every snapshot slow, for screenshots and for the loading tests.
- `node build.mjs demo --publish` writes the published demo page, one file: nonces removed, English by default, and the policy moved into a `<meta>` that names each inline script and style by
  SHA-256, with `connect-src 'none'` (the demo asks the network for nothing).
- The browser tests drive this build in a private Chrome (`test/helpers/page.mjs`).

## Consequences

- The same code runs for the user and for the visitor, and the whole page can be tested without the server. The price is a second producer of snapshots to keep in step with
  the contract of the Rust one (`test/demo.contract.test.mjs` and `test/fixtures.contract.test.mjs` watch it).
- `scripts/demo/demo.js` is about 60 KB of code outside `src/`: it is not under the layer rules (ADR 0004) and the demo build runs the guard without the byte budgets, only with the safety checks.
- Nothing from the private folders of the repository (the profile, the data and the CV) may appear in it (spec acceptance A12).
