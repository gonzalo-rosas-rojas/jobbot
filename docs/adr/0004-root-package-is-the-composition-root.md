# 0004. The root package `jobbot` is the composition root

Status: accepted

## Context

The design called the top crate `jobbot-cli`. The binary, the scripts, the scheduled task's command
line and the documentation all call it `jobbot`. Before the split, the root crate also held most of
the logic.

## Decision

The root package keeps the name `jobbot` and plays the part of the composition root. Only
`src/main.rs` and `src/cli.rs` hold code: they parse the command, open the adapters, hand them to
the use cases and print. Every other root module is a re-export of a crate below, except a short
list of wiring functions (opening the panel's stores). `tests/architecture.rs` reads `src/` and
fails when another root module defines logic.

The daily run's sequencing (discover, prepare, submit, dossiers, digest, with the window and STOP
checked between steps) stays in the command line, because it sequences the command line's own
commands and their exit codes; the record of each run is written by the application.

The performance benchmarks live in the root package's `benches/`, the one place that may use every
crate.

## Consequences

- The crate name differs from the design diagram.
- A handful of tests that need several adapters at once stay in the root.
- A new command adds a function to `cli.rs`, never logic to a root module.
