# 0003. Typed errors where a caller decides on the kind, `anyhow` elsewhere

Status: accepted

## Context

Callers of the store need to tell an illegal state move from a missing record or a database
failure; callers of the Claude runner need to tell a budget stop from a wrong account; discovery
needs to tell a board that asked for a pause from one that failed. A string error cannot be matched.
Elsewhere, the use cases and the command line report an error and stop, and a typed error per
function would add code without adding a decision.

## Decision

Errors are typed wherever a caller decides something on the kind:

- in the domain: `FetchError` (`Failed`, `RateLimited { wait_secs }`, `Skipped`), `RulesError`,
  `ScheduleError` and `BankError` for the profile, the execution window and the question bank;
- in the ports, as part of their contract: `StoreError` (illegal transition, unknown key, grade
  out of range, backend) and `ClaudeError` with `ShapeError` (budget, account, reply shape);
- in the store crate: `TrackerError` and `PrepError`, which its port implementations map to
  `StoreError`;
- `ConfigError` for Claude's settings.

They derive `thiserror` (`FetchError` writes its `Display` by hand). A wrapping variant keeps its
cause as `source`, not in its message. `anyhow` stays in the
application and the command line, and in the adapter functions whose callers only report the
failure: launching and closing the browser, the source crate's HTTP helpers and config loader, and
opening the panel. The ports the browser serves (`FormFiller`, `FormInspector`, `Renderer`) return
`anyhow::Result` for the same reason: the pipeline records the failure on the application and moves
on.

## Consequences

- A caller of those `anyhow` functions cannot match on the kind; none needs to today, and a function
  can move to a typed error when a caller appears.
- The design document asked for `anyhow` only in the command line. The application and the
  report-only adapter functions keep it, because forcing types there would have been churn without
  a caller.
