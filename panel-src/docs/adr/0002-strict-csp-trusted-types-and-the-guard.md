# 0002. Strict CSP, Trusted Types and a guard that refuses the page

Status: Accepted. Decided 2026-10-07/08 (spec sections 9.6 and 10), in force since `3562fc6`. Recorded 2026-10-10.

## Context

The panel is the most privileged page on this machine: it carries the token that approves and sends applications, and it shows text that comes from outside (company names,
job descriptions, e-mail, messages of the server). It must never call out except to its own `/api/*`. A single `innerHTML` with a hostile job title would be enough to
break both promises.

## Decision

Three layers, none optional.

1. **CSP with a nonce per load**: `default-src 'none'; script-src 'nonce-X'; style-src 'nonce-X'; connect-src 'self'; img-src 'self' blob:; font-src 'self'; base-uri 'none';
   form-action 'none'; frame-ancestors 'none'; require-trusted-types-for 'script'`. Rust sends it (`../crates/jobbot-panel/src/http.rs`), `tools/serve.mjs` sends the same for development.
2. **Trusted Types with no policy**: a string handed to `innerHTML`, `eval` or a script `src` throws. Our DOM is built only by `h()` and `sv()` in `src/core/dom.js`: every child
   that is not a node is a text node, dangerous tags and URL schemes are refused, `on*` takes functions only, `style` takes an object.
3. **`guard.mjs`**: the built page and the bundle are refused if they contain any of 30 strings (`innerHTML`, `eval(`, `new Function`, `import(`, `new Worker(`, `WebSocket`,
   `sendBeacon`, `<iframe`, `<link`, ...) or one of 3 patterns (`fetch` of an absolute address, `setTimeout` with a string, `createElement` of a script or an iframe).
   `build.mjs` does not write the page when the guard says no, and there is no `--force`. The Rust tests of `../crates/jobbot-panel/src/lib.rs` hold the same rules from the other side.

## Consequences

- Defence in depth: a library that does `innerHTML` cannot be bundled, because the guard finds the word in the bundle. This is what rules out most frameworks (ADR 0003).
- The guard is a string search, so it can refuse a harmless library that merely contains a word. That is accepted: the answer is another library or our own code, not a hole.
- `window.__app` (the handle the browser tests use) exists only with `?debug=1`.
- Weakening any layer (a Trusted Types policy "so a library works", `unsafe-eval`, an external resource, a looser guard) is a decision about security, and it belongs to Gonza, not to
  whoever hits the wall first.
