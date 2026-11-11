# 0010. HTTP resilience as layers on one GET

Status: accepted

## Context

jobbot reads about twenty job boards and feeds every hour. A board that answers 429 was asked
again at once, a board that kept failing was asked again on the next run, unchanged feeds were
downloaded and parsed again, and adding a source meant editing a struct of twenty fields. The design
asked for decorators (`RateLimited`, `Retrying`, `Cached`) around each source.

## Decision

The decorators wrap one GET, not the `Source` port: one source fetch is dozens of requests, and
the behaviours belong to each request. From the outside in:

1. `Deadline`: after the run's time for the boards, no new request leaves;
2. `HostCooldown`: a host that kept refusing is left alone for as long as it asked;
3. `Cached`: a page kept from an earlier run is asked for with its `ETag` or `Last-Modified`, and a
   304 reuses it;
4. `Retrying`: a 429 or 503 is asked again after the host's own `Retry-After`, capped, or after a
   doubling wait with jitter;
5. `Network`: one HTTP client and one timeout.

Every source holds a cheap clone of one stack. With the store's `HttpMemory`, host pauses and kept
pages outlive the run, so the next run respects a pause asked of this one. Sources are rows of a
registry (name and build function); a failure is a typed `FetchError` that says whether the host
asked for a pause.

## Consequences

- A host at rest is not served from the cache: the report says it is paused rather than showing an
  old page as new.
- A failing memory warns and never fails a request; the run's own board still works.
- Requests to different hosts still go one after another: runs fit their window, and parallel
  fetching would raise the risk of 429s. The benchmarks are where that would be measured first.
