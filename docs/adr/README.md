# Architecture decision records

Each record states one decision that shaped the code: the problem, what was decided, and what it
costs. A record is not edited once accepted; a later decision that changes it gets its own record
and the old one is marked as superseded.

| # | decision | status |
|---|---|---|
| [0001](0001-layered-workspace.md) | A layered Cargo workspace, enforced by a test | accepted |
| [0002](0002-ports-belong-to-the-application.md) | The ports belong to the application | accepted |
| [0003](0003-typed-errors-at-library-edges.md) | Typed errors at the library edges, `anyhow` above them | accepted |
| [0004](0004-root-package-is-the-composition-root.md) | The root package `jobbot` is the composition root | accepted |
| [0005](0005-private-state-under-one-root.md) | Private state under one root, `JOBBOT_HOME` | accepted |
| [0006](0006-intent-before-every-irreversible-act.md) | An intent before every irreversible act | accepted |
| [0007](0007-trust-boundaries-as-types.md) | Trust boundaries as types | accepted |
| [0008](0008-one-filter-for-printed-text.md) | One filter for everything the command line prints | accepted |
| [0009](0009-exclusions-read-text-as-a-person-does.md) | Exclusions read text as a person reads it | accepted |
| [0010](0010-http-resilience-on-one-get.md) | HTTP resilience as layers on one GET | accepted |
| [0011](0011-own-benchmark-harness.md) | An own benchmark harness with a budgets file | accepted |
| [0012](0012-hard-tests.md) | Properties, a hostile corpus, fuzz bodies and mutation testing | accepted |
| [0013](0013-panel-guards.md) | The local panel's guards | accepted |

The architecture as a whole is described in [`ARCHITECTURE.md`](../../ARCHITECTURE.md), the
threats and their guards in [`docs/engineering/threat-model.md`](../engineering/threat-model.md).
