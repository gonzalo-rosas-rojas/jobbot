# 0001. esbuild bundles everything into ONE inline page

Status: Accepted. Decided 2026-10-07 with the first build, restated in the spec (section 8.3). Recorded 2026-10-10.

## Context

The Rust binary serves the panel from `127.0.0.1` and embeds one file, `../crates/jobbot-panel/src/page.html`, with `include_str!`. The file is committed, so `cargo build` never needs Node.
On every load the server fills two placeholders in the text: `__NONCE__` (a fresh nonce) and `__ASSETS__` (the per-launch path of the fonts). The policy of the page
(ADR 0002) allows scripts and styles only by that nonce, with no `'self'` and no `'strict-dynamic'`: a script file, a `blob:` script or a dynamic `import()` would be blocked.

## Decision

`build.mjs` bundles `src/main.js` with esbuild (exact version in `package.json`; IIFE, es2022, minified, no legal comments), transforms the CSS the same way, and puts script,
style and the icon sprite into `page.template.html`. The result has exactly two nonce'd `<script>` tags (a few lines in the head that choose the theme and language before
the first paint, and the application) and one nonce'd `<style>`. `guard.mjs` judges the result before it is written (ADR 0002, ADR 0005); `--install` writes
`../crates/jobbot-panel/src/page.html`. Other modes build a lab, the demo (ADR 0007) or a page under another name for a parallel agent.

## Consequences

- One request, a policy that is easy to state, a Rust build with no Node, a build of about 0.2 to 0.3 seconds. A faster bundler would buy nothing.
- No code splitting and no lazy loading of code: everything ships at once and `import(` is forbidden by the guard. A feature that is big and rarely used still pays its
  bytes (ADR 0009 is the case that matters).
- `../crates/jobbot-panel/src/page.html` is a generated file under git: its diffs are huge and nobody edits it by hand. It is regenerated on purpose, not as a side effect of a change.
- The unminified development build is larger than the JS cap and does not pass the budget; `--dev` is for reading, not for installing.
