# 0001. A layered Cargo workspace, enforced by a test

Status: accepted

## Context

jobbot started as one crate. One struct held the whole database API and eleven modules imported
it; the Claude client and the submit gate reached into it directly; the form filler and the handoff
module imported each other. A change to one capability touched several unrelated files, and nothing
stopped the database code from calling a job board or the browser from calling Claude.

## Decision

The code is a workspace of crates in layers:

- layer 0, `jobbot-domain`: the rules, pure (no async, no I/O);
- layer 1, `jobbot-app`: the use cases and the ports they need;
- layer 2, one crate per adapter: `jobbot-store`, `jobbot-sources`, `jobbot-claude`,
  `jobbot-browser`, `jobbot-panel`;
- layer 3, `jobbot-testkit`: test support, only ever a dev-dependency;
- layer 4, `jobbot`: the command line and the wiring.

A dependency between workspace crates must point to a strictly lower layer, so two adapters never
depend on each other. The domain may use only an allow-list of pure crates. The application may not
use the database, HTTP or browser crates. `tests/architecture.rs` checks all of this over
`cargo metadata` on every test run; each rule is a function with its own tests on made-up
workspaces, so a rule cannot quietly stop biting.

Inside each crate the code is grouped by capability (apply, discovery, inbox, learn, reporting),
not only by layer, so a change to one capability stays in one place.

## Consequences

- A path between two adapters always goes through the application; the panel cannot open the
  database itself, so opening the stores is a function of the root crate.
- A new external dependency of the domain needs an edit to the allow-list, in the same commit.
- Tests that need the real database and a use case live in the application's integration tests,
  which link the store through the testkit; an application unit test cannot, because a dev-dependency
  cycle would build the application twice and its traits would not unify.
- More manifests to keep in step; versions live once in `[workspace.dependencies]`.
