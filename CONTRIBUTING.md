# Contributing

jobbot is a personal tool, so changes are reviewed with one question first: does this keep the
candidate safe? Read [`ARCHITECTURE.md`](ARCHITECTURE.md) and [`SECURITY.md`](SECURITY.md) before a
change of any size. A security problem is reported privately, as `SECURITY.md` says, never in an
issue.

## Before you send a change

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

`scripts/check.ps1` runs the same three and, when they are installed, `cargo deny check` and
`cargo audit`. All of it must pass; CI runs it on Windows and, as information, on Linux.

A change under `panel-src/` also needs `npm ci` and `npm run check` there (CI runs both on Linux).
`npm run check` builds the page, runs the tests that need no browser, and fails if
`crates/jobbot-panel/src/page.html`, the copy the binary embeds, is not the page the sources build:
after a change to the sources, run `node build.mjs --install`. The tests that start Chromium are
`#[ignore]`d; CI runs them on Windows by hand and every Monday, with the command in the checks table
of the README.

## How the code is organised

- The workspace is layered and `tests/architecture.rs` enforces it: the domain is pure, the
  application owns the ports, adapters never depend on each other, and the command line only wires
  and prints. A change that needs to break a layer needs an ADR in [`docs/adr/`](docs/adr/README.md)
  first.
- A new external dependency goes into `[workspace.dependencies]` with an exact reason in the commit
  message, must pass `cargo deny`, and a dependency of the domain also goes into `DOMAIN_ALLOWED`.
  `unsafe` code is denied.
- Errors: typed (`thiserror`) where a caller decides on the kind, `anyhow` in the application and
  the command line ([ADR 0003](docs/adr/0003-typed-errors-at-library-edges.md)).

## Tests

- Write the failing test first, and watch it fail for the right reason.
- A rule goes next to its code as a unit test; a use case goes in
  `crates/jobbot-app/tests/use_cases/`, against the real stores in memory and the fake Claude.
- Input from strangers (postings, apply pages, e-mails, links) gets a case in the hostile corpus
  (`crates/jobbot-testkit/src/hostile.rs`) or a property test, not only an example.
- No test touches the network, a real browser or a real Claude account; the Chromium tests are
  `#[ignore]`d and run by hand.
- A test that checks the values of someone's own profile (their answers, figures or exclusions)
  belongs in a file named `profile_audit*`: those files are kept out of public copies.
- A public copy is made by `cargo xtask export-public`, which can rewrite private names in the
  copied text before scanning it, and then runs the copy's own test suite before its single
  commit; a copy that does not pass is not committed.

## Commits

One change per commit, a message that says what changed and why, and the paths named explicitly
when committing. Code, tests and documents carry no real person's, employer's or client's data:
the example profile and CV are fictional on purpose, and a profile audit, the one place where a
real profile's values are checked, never leaves a private copy.
