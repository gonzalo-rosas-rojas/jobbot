# 0006. An intent before every irreversible act

Status: accepted

## Context

Submitting an application cannot be undone. A run that dies between the click and the record of
the result (a crash, a closed laptop, a killed process) left no trace of the click, and the next run
would click again: a duplicate application to the same company. The local panel has the same
problem in a smaller form: a button pressed twice, or a request the browser repeats, ran its action
twice.

## Decision

- **Intents.** Before the submit click the pipeline writes an intent (`kind`, application key,
  time) in a durable transaction, and closes it with the outcome afterwards. Only one intent of a
  kind can be open per application: `begin_intent` returns nothing when one is, and the caller must
  not act. At the start of every submit stage, each intent still open is a click whose outcome
  nobody recorded: the application is stamped as attempted (so the daily cap and the company
  cooldown count it) and handed to the candidate with a reason. It is never clicked again by the
  program.
- **The gate first, enforced by a type.** Every submission passes the fixed gate (STOP, platforms
  that forbid automation, listing-only pages, exclusions, stale postings, the daily cap, the
  company cooldown, manual approval while in review) before its intent is written. When the gate
  lets an application through it hands out a `GateApproval`, a token with a private field that
  only the gate can make; the form port sends only in `FillMode::Live(GateApproval)`, and the send
  uses the token up. Code that did not run the gate cannot send: it does not compile.
- **Command attempts.** A POST to the panel may carry an `Idempotency-Key`. The first request with a
  key runs and its reply is stored; a repeat gets the stored reply with `Idempotent-Replayed: true`,
  an unfinished one gets 409, and the same key on a different body gets 422. Attempts are kept a
  week. The command line takes no attempt id: a command typed once, or started by the scheduler,
  has no layer that repeats it behind the user's back.
- Neither form submissions nor Claude calls are retried automatically: a retry would duplicate an
  application or spending.

## Consequences

- A crash at the wrong moment costs one application that a person has to check by hand, never a
  duplicate.
- The request body is stored to recognise a reused key (bodies are small and kept a week).
- The page does not send the key yet; the server accepts requests without one, so the change is
  additive.
