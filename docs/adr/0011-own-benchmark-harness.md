# 0011. An own benchmark harness with a budgets file

Status: accepted

## Context

The design sets performance budgets as medians and tails per call (p50 and p95), and asks that a
regression of more than 10 % fail a check against a versioned file. The usual tool, `criterion`,
reports per-sample means and medians, has no budget gate (the same checker would be written
anyway), and would have added about ten crates to the build. The measurements also need data of the
live size and ten times it, without any personal data.

## Decision

- `jobbot_testkit::bench` times one call per sample and reports p50 and p95 by nearest rank, the
  same method as the process-level script `scripts/bench/measure.ps1`.
- `cargo bench --bench budgets` measures the domain rules, the store queries, the panel snapshot
  and the privacy filter, and exits 1 when `benches/budgets.toml` is breached: a p50 more than the
  tolerance (10 %) over its baseline, or a p95 over the ceiling the design sets (snapshot build
  50 ms on ten times the data, snapshot body 100 ms, privacy filter 5 ms). `-- --baseline` writes a
  run as the new baseline and keeps the tolerance and the ceilings.
- Each benchmark keeps the best of three rounds: noise only adds time, so the minimum is the
  steadiest figure.
- `jobbot_testkit::synthetic` builds the data from a seed with SplitMix64 written out in the
  testkit rather than the `rand` crate, because `rand` does not promise the same stream across
  versions and a seed must always name the same data set. The shares of each application state
  follow the live database's proportions.
- The store benchmarks use SQLite in memory; disk and WAL costs are measured by the process-level
  script.

## Consequences

- No HTML reports or change-point statistics; `criterion` can still be added over the same
  synthetic set.
- A busy machine can breach a budget; the output says to rerun on a quiet one.
- When the 10x snapshot breached its ceiling, the code was fixed (one pass over the events, capped
  columns reading only their cards) rather than the ceiling raised.
