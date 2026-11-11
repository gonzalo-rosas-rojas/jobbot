# 0007. Trust boundaries as types

Status: accepted

## Context

Strangers write most of what jobbot reads: the posting's title, company, description and apply
link, the page behind that link, the e-mails about an application. That text reaches a browser, a
model prompt, the terminal and files. A check that each call site must remember to run is a check
that one call site will forget.

## Decision

The checked form of each kind of strangers' input is its own type, and the code that acts on it
takes only that type:

- **`NavigableUrl`** (`jobbot_domain::navigation`) can only be made by `check`, under a policy:
  https only, no credentials, no loopback, private, link-local or reserved address however it is
  spelled, no local names. The browser's `goto` takes a `NavigableUrl`; a source test fails if
  shipped browser code calls `goto` on anything else. The page the browser lands on is checked
  again.
- **`Untrusted<T>` → `PromptText`** (`jobbot_domain::untrusted`): posting text enters a prompt only
  as a JSON string under a heading that marks it as untrusted, after control and invisible
  characters are removed and the text is cut to a length. An instruction inside a posting therefore
  cannot open a line of its own in the prompt. `serde_json` writes the string: a hand-written
  escaper would be new code to fuzz.
- **`Pii<T>`** (`jobbot_domain::privacy`) holds an e-mail address or similar: its `Debug` and
  `Display` print `***`, so it cannot reach a log by accident; the value comes out only where it is
  meant to be used.

## Consequences

- Adding a new place that opens a page or builds a prompt means taking the checked type; there is
  no unchecked path to forget.
- Company, title and location go into prompts as JSON strings too, not only the description; the
  prompt is a little longer.
- The answer bank's own contact fields stay plain strings, because they are what forms are filled
  with; the panel's privacy filter masks them in JSON.
