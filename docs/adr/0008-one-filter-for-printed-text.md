# 0008. One filter for everything the command line prints

Status: accepted

## Context

`jobbot top`, `today`, `status` and the digest print company names and titles straight from
postings. A posting can hold terminal escape sequences (move the cursor, recolour, hide text),
control characters and invisible format characters. Cleaning each print site would leave the next
new command unprotected.

## Decision

`cli::run_in` wraps the output of every command in one writer, `PlainText`: control characters
other than the newline become spaces and invisible characters are removed, a line at a time. The
digest's lines are also cleaned where they are built, because the digest is written to a file too.

Two more filters sit behind it, for privacy rather than safety: when standard output is not a
terminal (the scheduled task's log, a pipe) a line filter replaces the user's folders and own
addresses; the `JOBBOT_TRACE` writer keeps only jobbot's own spans, whose fields are counts and
outcomes, because a library's span can carry a page link with a token in it.

## Consequences

- jobbot's own output can never carry a tab or an escape sequence; none does.
- Output is held until each newline; no command prompts for input, so nothing waits on a partial
  line.
- A person who pipes jobbot into a pager sees the user's folder replaced, because a pipe is treated
  as a log.
- Standard error stays outside the writer: the few lines written there clean any page text where
  they are built, and none of them carries personal data. A new warning that prints strangers' text
  must do the same.
