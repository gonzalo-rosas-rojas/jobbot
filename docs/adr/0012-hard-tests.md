# 0012. Properties, a hostile corpus, fuzz bodies and mutation testing

Status: accepted

## Context

Example-based tests pin the cases their author thought of. jobbot's rules face input written by
strangers (links, posting text, e-mails, form pages), and the cases that matter are the ones nobody
thought of. The machine it is developed on runs Windows, where `cargo-fuzz` does not work.

## Decision

- **Property tests** (`proptest`, a dev-dependency) state each guard as a rule over generated
  input: the address check refuses every refused range in every spelling, the state machine takes
  exactly the moves its table lists, the e-mail import stores only clean text within its caps and a
  second import stores nothing new, the source readers never panic and read their own output back.
  `proptest` keeps its defaults: a random seed per run, 256 cases, and a failure file next to the
  test; shrunk failures that found real defects are committed.
- **A hostile corpus** (`jobbot_testkit::hostile`) holds strangers' text at its worst (instructions
  to a model, escape sequences, invisible and bidirectional characters, markup, huge text, refused
  links, disguised names, fake attachments) and runs it through the real entry points: exclusion,
  scoring, prompts, dossiers, the terminal, the e-mail import, file names.
- **Fuzz bodies in the testkit.** Each fuzz target in `fuzz/` is one line of glue over a function
  in `jobbot_testkit::fuzz` that states the target's invariants. `proptest` runs the same functions
  on Windows with arbitrary bytes, bytes shaped like each input and the hostile corpus with bytes
  flipped, so the code a fuzzer runs is compiled and checked by `cargo test` everywhere. The
  `fuzz/` crate is its own workspace, excluded from the main one, and needs nightly and Linux.
- **Mutation testing** (`cargo mutants -p jobbot-domain`) puts deliberate faults in the domain and
  counts how many the tests catch; the measured score is recorded as a floor in
  `docs/engineering/mutation-testing.md`.
- Every new property or corpus test is checked by breaking the code it guards and watching it fail.

## Consequences

- A rare failing case shows once and is rerun only if its line in the failure file is committed;
  the failure prints the input.
- The fuzz glue has not run yet; a compile error in it would show on the first Linux run. It was
  type-checked against a stub of the macro.
- The form pages are fuzzed as the field scan Rust receives, not as HTML through Chromium.
