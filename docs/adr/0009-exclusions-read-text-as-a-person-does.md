# 0009. Exclusions read text as a person reads it

Status: accepted

## Context

The exclusion list names companies, domains and categories the candidate must never apply to; an
application to one of them is a breach that cannot be undone. A name split by a zero-width space,
a soft hyphen or a word joiner, or written in full-width letters, looks the same to a person and
did not match the list.

## Decision

`text::as_read` turns text into what a person sees: it drops the invisible format characters and
folds the full-width ASCII forms and the ideographic space to plain ASCII. Name normalisation and
the category match both start from it, so an excluded name is caught however it is disguised that
way. The submit gate checks the exclusions again before any click, and domains are checked on the
posting's and the apply link's hosts.

Added 2026-10-24: the exclusion check also folds look-alike letters from other scripts
(`text::fold_look_alikes`, Unicode TR39 confusable skeletons through the `unicode-security` crate):
a Cyrillic "В", a Greek "Β" or a mathematical bold "𝐁" in a company name, a posting or a rule is
read as "B", and accents are dropped. Only the exclusion check folds: the normalised company name
that rows store is unchanged.

## Consequences

- ASCII look-alikes are not folded ("1" for "l", "rn" for "m"): folding ASCII with the TR39 table
  would rewrite every name. Such a spelling is deliberate, not a stray character, and is caught by
  the domain only.
- The normalised company name of new rows changes for names that held invisible or full-width
  characters; an old row keeps its old form, so the per-company cap may count the two apart.
