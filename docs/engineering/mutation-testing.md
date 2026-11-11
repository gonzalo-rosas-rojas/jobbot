# Mutation testing

[cargo-mutants](https://mutants.rs/) plants one fault at a time in the domain (an operator flipped,
a comparison moved by one, a function body replaced by a constant) and runs the tests. A mutant the
tests still pass is a fault they would let through. Only the domain is mutated
(`crates/jobbot-domain/src`, without the test helpers `fixtures.rs` and `shipped.rs`; see
`.cargo/mutants.toml`): it holds every rule that decides what the program does.

## The figure

| pass | tests that run | mutants | caught | missed | timeout | unviable | caught / viable |
|---|---|---|---|---|---|---|---|
| domain only, at the start of the work | the domain's own unit and property tests | 814 | 422 | 326 | 3 | 63 | 56% |
| full, on the final code | the seven packages that test domain rules | 815 | 715 | 28 | 8 | 64 | **95.2%** |

The domain-only figure is low because many rules moved into the domain crate after their tests
were written: those tests still live with the use cases (`crates/jobbot-app/tests/use_cases/`) and
with the browser's form tests, so a domain-only run counts the faults they catch as missed. The
full pass runs the tests of `jobbot-app`, `jobbot-browser`, `jobbot-domain`, `jobbot-panel`,
`jobbot-sources`, `jobbot-store` and `jobbot-testkit`; the root package (the command line) is left
out, so a rule tested only end to end counts as missed.

A timeout is a mutated loop that never ends; the timeout stops it, so the fault does not go
unnoticed. Counting timeouts, the full pass detects 723 of 751 viable mutants (96.3%). Unviable
mutants do not compile (most replace a function with `Default::default()` for a type without a
default) and are left out of the ratio.

After that pass, four tests were added for the missed mutants in the rules that protect the
candidate (below), each checked against its own mutant: 719 of 751 caught (95.7%).

**Floor: 95% caught over viable on a full pass.** A run below it means a test lost its bite or a
rule lost its test: look at the new entries of `missed.txt` before merging.

## What the tests still miss

The rules that protect the candidate (which links the browser may open, what leaves the machine,
which consent boxes are ticked, the Hispanic or Latino answer, the exclusion list, the state
table, the gate) have no missed mutant left, except one that cannot change behaviour. The rest are
in rules where a slightly different number is not a hazard, and are listed rather than chased:

| module | missed | what they change |
|---|---|---|
| `scoring.rs` | 9 | boundaries of the salary parser and of the floor (pay exactly at the floor), the arithmetic of the stability and other signals, one `||` in the title fit: the score moves by a few points |
| `prep/sm2.rs` | 7 | the arithmetic and one boundary of the SM-2 drill scheduler: a card comes back a day sooner or later |
| `rules.rs` | 4 | edges of the criteria checks: a weight of exactly 0, the tolerance of the weight sum, a target range whose ends are equal |
| `window.rs` | 2 | `next_window_start`, which only says when the next window opens (the panel, the CLI); the check that stops a send, `in_run_window`, has none |
| `handoff.rs` | 1 | how the answer sheet groups the options of one question |
| `answers.rs` | 1 | equivalent: `||` to `&&` between the `http://` and `https://` tests in `without_urls`, which a later `contains("://")` covers anyway |

## Running it

```sh
cargo install --locked --version 25.3.1 cargo-mutants
cargo mutants -p jobbot-domain \
  --test-package jobbot-app,jobbot-browser,jobbot-domain,jobbot-panel,jobbot-sources,jobbot-store,jobbot-testkit \
  --timeout 180 --build-timeout 900 -j 2
```

- `-p jobbot-domain` is required: without it cargo-mutants looks only at the root package and
  finds nothing to mutate. `cargo mutants -p jobbot-domain` alone gives the domain-only figure.
- Set `--timeout` by hand. The configuration tests only the domain's package by default, so the
  baseline takes two seconds and the automatic timeout comes out near 20 seconds, too short once
  the seven packages run; real failures then show up as timeouts.
- A full pass takes about two and a quarter hours at two jobs on the machine it was measured on.
- Check the exit codes afterwards (`mutants.out/outcomes.json`, `process_status`). A Windows failure
  code instead of `101` (`-1073741819`, access violation; `-1073741502`, a process that could not
  start) is the machine, not the mutant: rerun those mutants alone with `--re`. In the measured
  pass, three mutants counted as caught through an access violation; rerun alone, all three were
  caught by a failing test.
- Do not stop a run and then resume it with `--iterate`: builds that fail while the run is being
  stopped are recorded as unviable, and `--iterate` skips them as if they had been tested.
