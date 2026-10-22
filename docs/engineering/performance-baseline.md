# Performance baseline

Measured on 2026-10-10 at commit 95bfc76 of branch `backend/excelencia`, before any change of this branch.
Machine: Intel Core i9-14900HX, 64 GB RAM, Windows 11, rustc 1.95.0.

## Read commands (copy of a real database of 18.6 MB, release binary, 30 runs after one cold run)

Method: `scripts/bench/measure.ps1` starts the binary and reads the process's own start and exit
times, so the cost of launching it from a shell is not included. An earlier measurement of the same
commands from a shell (87 to 117 ms warm) included that launch cost and is not comparable.

| command | cold (ms) | p50 (ms) | p95 (ms) | min (ms) | max (ms) | peak memory (MB) |
|---|---|---|---|---|---|---|
| jobbot status | 71 | 21 | 33 | 19 | 37 | 11.4 |
| jobbot today | 24 | 16 | 19 | 14 | 19 | 11.7 |
| jobbot top | 25 | 22 | 39 | 17 | 39 | 11.7 |

Peak memory is read by polling the process while it runs, so it is a lower bound.

## Build and test

| item | value |
|---|---|
| `cargo build --release --locked` (crate `jobbot` only, dependencies cached) | 35 s |
| `cargo build --release --locked` (everything, no release artifacts yet) | 82 s |
| `jobbot.exe` size | 21,766,144 bytes (20.8 MB) |
| `cargo test --locked` wall time (dependencies cached) | 31.8 s |
| tests passed / ignored | 1,093 / 106 |

## After phase 2

Measured on 2026-10-10 at the head of branch `backend/excelencia` after phase 2 (navigation guard,
intents table, WAL, schema version, retry policy), same machine, same script, same copy of the real
database (18.6 MB, still in the old journal mode when copied). Two full runs of 30 are shown to
give the spread; the first run of the first command migrates the copy.

| command | run | cold (ms) | p50 (ms) | p95 (ms) | min (ms) | max (ms) | peak memory (MB) |
|---|---|---|---|---|---|---|---|
| jobbot status | 1 | 109 | 15 | 18 | 14 | 19 | 15 |
| jobbot status | 2 | 98 | 15 | 20 | 14 | 21 | 15.1 |
| jobbot today | 1 | 12 | 11 | 12 | 10 | 13 | 11.7 |
| jobbot today | 2 | 14 | 12 | 15 | 10 | 15 | 11.6 |
| jobbot top | 1 | 15 | 13 | 15 | 12 | 15 | 11.6 |
| jobbot top | 2 | 14 | 13 | 16 | 12 | 17 | 11.7 |

Against the baseline (the 10 % regression rule):

- `status`: p50 21 to 15 ms (29 % better), p95 33 to 18 and 20 ms (45 % and 39 % better). The cold run
  is 109 and 98 ms against 71 ms: it is the one open that migrates the copy (switch to WAL, stamp
  `user_version`, and the `VACUUM INTO` copy of the 18.6 MB database made before migrating, which
  the script deletes with the temporary folder). It is paid once per database. The cold runs of
  `today` and `top` come after it, on the migrated copy, and are 12 to 15 ms: lower than the
  baseline's 24 and 25 ms.
- `today`: p50 16 to 11 and 12 ms (31 % and 25 % better), p95 19 to 12 and 15 ms.
- `top`: p50 22 to 13 ms (41 % better), p95 39 to 15 and 16 ms (the baseline p95 was one slow
  outlier; the min and the p50 also moved).
- Peak memory of `status` rises from 11.4 to 15 MB: it is the peak of the migrating run (the copy
  of the database), polled from outside, so a lower bound. `today` and `top` are unchanged.

Nothing got worse by more than 10 % except the one-time migration. The causes of the improvement
were not isolated: reading in WAL mode is a plausible contributor, and the baseline was taken in a
different session, so part of the gap may be machine noise. It is reported as "no regression", not
as a gain to build on; phase 4 owns benchmarks with a regression budget.

| item | before | after phase 2 |
|---|---|---|
| `jobbot.exe` size | 21,766,144 bytes | 21,874,176 bytes (+0.5 %) |
| `cargo build --release --locked` (crate only) | 35 s | 23 s (cache state differs; not a comparison) |
| tests passed / ignored | 1,093 / 106 | 1,129 / 109 |

## After phase 3a

Measured on 2026-10-20 at the head of branch `backend/excelencia` after the extraction of
`jobbot-domain` (workspace of two crates), same machine. Each build time is one run, so a
difference of a second is inside the noise.

| item | before (start of phase 3a) | after phase 3a |
|---|---|---|
| tests passed / ignored (`cargo test --locked`) | 1,131 / 109 | 1,151 / 109 |
| tests of `jobbot-domain` alone | none (it did not exist) | 91 passed, 0.84 s |
| incremental `cargo test --no-run` after touching `src/cli.rs` | 8.3 s | 6.5 s |
| incremental `cargo test --no-run` after touching `text.rs` | 7.7 s | 7.0 s |

The 20 more tests are 7 that pin the messages of the profile loaders, 10 of the architecture test,
2 for NaN and negative numbers in `criteria.toml`, and 1 for the helper that reads the shipped
files; the six tests that moved (three fixtures, three state-machine) are counted on both sides.

The split is about structure, not speed: no build-time change is expected from moving the pure
modules (the domain is small and the application crate still dominates), and none is claimed. The
measurement is kept so a later phase can see what the extraction of the larger crates (store,
adapters, panel) does to incremental builds. Runtime behavior is unchanged; the read-command
latencies of "After phase 2" apply as they are.

## Panel snapshot

Measured by the UI session on a copy of the real database; re-measured when the benchmarks of
phase 4 exist.

`GET /api/snapshot` 38 ms release, 126 ms debug; `build_snapshot` 30 ms (pipeline 9, cards 7,
funnel 3); `redact_json` 1.3 to 1.9 ms.

## Not measured yet

- Discovery wall time: it needs the network and is run once, on request, against a copy.
- Coverage: needs `cargo-llvm-cov`, which is not installed.
- Panel memory under load: phase 4.

## Budgets

Rule: budget = max(hypothesis of the spec, 1.25 x measured p95), so no budget starts already
violated. The budgets are ceilings; what catches a regression early is the 10 % tolerance against
the figures in this file.

| item | hypothesis | measured p95 | budget |
|---|---|---|---|
| read commands, warm | <= 150 ms | 19 to 39 ms | 150 ms |
| `GET /api/snapshot`, release | <= 100 ms | 38 ms (UI session) | 100 ms |
| `GET /api/snapshot`, debug | <= 400 ms | 126 ms (UI session) | 400 ms |
| `build_snapshot` at 10x data | <= 50 ms | not measured (needs the synthetic set) | 50 ms, provisional |
| `redact_json` over about 20,000 strings | <= 5 ms | 1.3 to 1.9 ms (UI session) | 5 ms |
| regression tolerance in any benchmark | 10 % | n/a | 10 % |
