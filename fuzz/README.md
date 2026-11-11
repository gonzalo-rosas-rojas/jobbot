# Fuzz targets

Four cargo-fuzz targets over the entry points that read strangers' input:

| target | input | what must hold |
|---|---|---|
| `form_plan` | a page scan (the fields a form shows) | every planned action names a field and fits its kind; a field is acted on once; a box already ticked is never clicked again; a lone box is ticked only when required; "Not Hispanic or Latino" is never ticked |
| `email_import` | an e-mail import file | a file that is not one is refused whole; every element is counted once; every stored text is one plain line; no task gets a link |
| `address` | an apply link | what the strict check accepts is https to a public host without credentials and reads back as itself |
| `cv_markdown` | a CV master | the printed page holds only what markdown makes: raw HTML comes out as text |

The bodies live in `crates/jobbot-testkit/src/fuzz.rs`. The property tests in
`crates/jobbot-testkit/tests/fuzz_bodies.rs` and `crates/jobbot-browser/tests/cv_markdown.rs` run the
same bodies on every platform, so `cargo test` keeps them compiled and checked; this folder only adds
the libFuzzer glue.

cargo-fuzz needs a nightly toolchain and libFuzzer, so it runs on Linux (on Windows it is fragile).
This folder is its own workspace; the main workspace excludes it.

```sh
cargo install cargo-fuzz --locked
cargo +nightly fuzz run form_plan -- -max_total_time=600
cargo +nightly fuzz run email_import -- -max_total_time=600
cargo +nightly fuzz run address -- -max_total_time=600
cargo +nightly fuzz run cv_markdown -- -max_total_time=600
```

A crash leaves its input under `artifacts/<target>/`. Turn it into an example test next to the
property test of that body before fixing the code.
