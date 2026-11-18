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
