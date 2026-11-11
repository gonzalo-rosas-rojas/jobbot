# 0002. The ports belong to the application

Status: accepted

## Context

The use cases need storage, job boards, a model, a browser and a PDF printer. If those interfaces
lived with their implementations, the application would depend on the adapters and the layers of
[0001](0001-layered-workspace.md) would run the wrong way.

## Decision

The application states what it needs as traits in `crates/jobbot-app/src/ports*`, and the records
those traits exchange live there too: `Store` (a supertrait over one repository per aggregate:
jobs, applications, intents, e-mails, tasks, meta, Claude spend, history, learning, command
attempts), `HttpMemory`, `Source`, `OpenChecker`, `ClaudeRunner`, `FormFiller`, `FormInspector` and
`Renderer`. Each adapter implements the ones it serves and re-exports them, so older paths keep
working. Data that is pure and shared by several layers (a job, a submission result, a fetch error)
is domain data; only the traits and their records are the application's.

No dependency-injection container: the root crate builds the adapters and passes them in as
`&dyn` trait objects.

## Consequences

- Use cases are tested against fakes (the testkit's fake Claude, fake sources and fillers) or the
  real SQLite stores in memory, without a browser or a network.
- A port is added only when a production caller needs it; there is no port for reading a board's
  form questions because nothing in production asks for them.
- The learning store holds a bare SQLite connection and is not `Send + Sync`; its port says so
  instead of hiding it.
