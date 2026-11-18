# 0015. Dates move off chrono, in a change of their own

Status: accepted

## Context

On 2026-11-17 the maintainer of `chrono` and `chrono-tz` filed two advisories against his own
crates, RUSTSEC-2026-0543 and RUSTSEC-2026-0555: both are unmaintained, every version is affected
and there is no upgrade. He had announced it in January
([chronotope/chrono#1768](https://github.com/chronotope/chrono/issues/1768)). Neither advisory
names a flaw; they say that none would be fixed.

jobbot keeps every instant as `chrono::DateTime<Utc>` and reads the candidate's hours in
`America/Santiago` through `chrono-tz`: 84 source files name one of the two crates. `cargo deny
check` treats an unmaintained crate as an error, so the dependency check went red the day the
advisories were issued, on a tree that had not changed.

What the advisories mean for this program, most concrete first:

- `chrono-tz` compiles the time zone database into the binary and will no longer follow it. Chile
  has moved the dates of its clock changes several times. After the next such change, the run
  window (the hours in which jobbot may act) would be read one hour off until the zone data comes
  from somewhere else.
- A parsing or arithmetic flaw found in `chrono` later would stay open.

## Decision

- jobbot moves to [`jiff`](https://crates.io/crates/jiff), one of the two crates the advisory
  recommends. The other, `time`, has no IANA time zones of its own, and the run window needs one.
- The move is a change of its own, not a line in another one: it touches the stored text of every
  instant, the JSON the panel reads and the window arithmetic, and each of those is pinned by tests
  that must pass unchanged before and after.
- Until it lands, the two advisories are ignored by id in `deny.toml` and `.cargo/audit.toml`, each
  with its reason. The class stays an error: the next unmaintained crate turns the check red again.
- A change to Chile's clock rules does not wait for the move. If one is announced first, the zone
  data is replaced first.

## Consequences

- The dependency check is green on a known, written-down exception instead of red on every run.
- Two ignores now sit in the list that have nothing to do with tests-only code; the note beside
  them points here so that nobody reads them as "harmless".
- Until the move, the program depends on a time zone table frozen at `chrono-tz` 0.10.4.

## Where it stands (2026-11-18)

**The net is in place.** Every date that leaves the program or comes back in is pinned by a test
whose expected value is a literal written by hand, not something the date library computes, so the
tests keep meaning the same after the library changes. They live next to what they pin:
`crates/jobbot-store/tests/date_text.rs`, `crates/jobbot-sources/tests/date_formats.rs`,
`crates/jobbot-app/tests/use_cases/dates.rs`, `crates/jobbot-panel/src/snapshot/date_text_tests.rs`
and the test modules of `window.rs`, `server.rs`, `scheduler.rs` and `src/cli/`. The tests of the
crates that depend on `jobbot-testkit` build their input instants with `jobbot_testkit::utc_at`,
so the move rewrites that one function for them.

**The move waits for jiff 1.x to be two weeks old.** jiff 1.0.0 was published on 2026-11-15 and
1.0.1 on 2026-11-18. jobbot does not take a release that is days old into the binary that sends
applications, and moving to 0.2 now would mean moving twice. No flaw is named in either advisory,
so the written exception holds until then.

**What the move must keep, because the compiler will not notice if it does not:**

- Every stored and served instant is `2026-03-01T12:00:00Z`: UTC, `Z`, cut to the second (not
  rounded), never a fraction. The store compares these as text in SQL (`at >= ?1`, `ORDER BY`,
  `MAX`, and plain equality in the undo of a mark). jiff prints the fraction when there is one; a
  naive swap would put `…:00Z` and `…:00.123456789Z` in the same column, and a fraction sorts
  *before* the whole second. One function writes this text for the store, the drill and the page.
- The values kept in `meta` (last run, last discovery, last mail import, last account check) are
  written with `+00:00` and zero, three, six or nine digits of fraction, and two of them reach the
  page as they are. They stay as they are in the move; making them `Z` is a separate decision.
- What is read back: `Z` or an offset, `T` or a blank, any number of fraction digits; a date
  without a time or a zone is an error, never a guess. The tables in the tests say which shapes are
  taken and which refused, for stored text, for what the page and the command line send, for
  `Retry-After` and for each job board.
- Chile's clock: the offset by season, the day that changes at 03:00 or 04:00 UTC, the hour that
  repeats in April and the one that does not exist in September, the run window's edges to the
  millisecond, "next Monday at 00:00 in Santiago", and five years added to 29 February.
- Three things the tests pin as they are today and the move may change on purpose, each updated
  with its reason when it does: seconds so large they read as the year 57720, the wording of the
  parser's own error inside a refused clock time (the program's message is pinned, the library's is
  not), and which exotic spellings of a date are accepted.

The zone data then comes from jiff's bundled copy of the IANA database, so a change to Chile's
rules becomes a dependency update.
