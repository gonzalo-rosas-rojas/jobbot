# Architecture

jobbot is a Cargo workspace of layers. Each layer is a crate, and a crate may depend only on
crates of a lower layer. The rule is not a convention: `tests/architecture.rs` reads
`cargo metadata` and fails the build when it is broken.

## Layers, lowest first

| crate | layer | holds | may depend on |
|---|---|---|---|
| `jobbot-domain` | 0 | the pure core: the job model, the application state machine and its transitions, scoring, exclusion rules, outbound-text filters, text helpers, the navigation policy, the parsing and validation of `profile/*.toml`, the execution window | `chrono`, `chrono-tz`, `html-escape`, `regex`, `scraper`, `serde`, `thiserror`, `toml`, `url`; nothing else |
| `jobbot` | 1 | the application: tracker (SQLite), sources, the Claude client, forms (Playwright), the panel (HTTP), the CLI; it re-exports the domain modules under their original paths | everything |

## Why the domain is pure

The domain decides things (is this posting excluded, what does it score, which state may follow
which). Those decisions must be testable with plain values and must not be able to reach a file,
a socket, a database or a browser, so a mistake there can never send an application. Nothing in
`jobbot-domain` reads a file: the application reads `profile/*.toml` and hands the text to the
domain (`jobbot::rules::load_criteria` calls `jobbot_domain::rules::parse_criteria`). Its errors
are typed (`thiserror`); `anyhow` lives in the application only.

## What the architecture test checks

1. **Layer rule.** Every workspace crate is in the `LAYERS` table of `tests/architecture.rs`, and
   every dependency between workspace crates points to a lower layer. (Cargo already refuses a
   cycle between packages, so the test guards the direction, which is what can still go wrong.)
2. **Domain allow-list.** The domain depends on no workspace crate and on the external crates in
   `DOMAIN_ALLOWED` only, dev-dependencies included. Adding `tokio` to the domain fails with
   `jobbot-domain depends on tokio, which is not in its allow-list`.

Both rules are plain functions over a map of manifests. Their own tests feed them made-up
workspaces (an upward dependency, an unknown crate, a missing domain), so the test cannot quietly
stop biting. The test runs `cargo metadata --no-deps --offline --locked`: it needs no network, and
it fails with cargo's own message when `Cargo.lock` is stale.

## Public paths

The application crate re-exports the domain modules at the paths they always had
(`jobbot::models`, `jobbot::text`, `jobbot::scoring`, `jobbot::tracker::State`, …), so existing
code and the integration tests did not change when the modules moved. New code may import from
`jobbot_domain` directly.

## Adding a crate

1. Create `crates/<name>/Cargo.toml` with `version`, `edition`, `rust-version`, `publish` and
   `[lints]` taken from the workspace, and dependencies written as `name.workspace = true` when
   the version is already in `[workspace.dependencies]`.
2. Add it to `members` and `default-members` in the root `Cargo.toml` (without `default-members`
   a bare `cargo test` skips it).
3. Add it to `LAYERS` in `tests/architecture.rs`, at its layer.
4. A new external dependency of the domain is added to `DOMAIN_ALLOWED` in the same commit, with
   the reason in the commit message.
5. `scripts/check.ps1` runs everything with `--workspace`.

## Still in the application crate

`paths`, `answers`, `gate`, the SM-2 scheduler of `prep`, and the types shared by `form` and
`handoff` are pure or nearly so and move next; the adapters (SQLite, sources, Claude, browser,
panel) and the use cases follow the layering in the design spec.
