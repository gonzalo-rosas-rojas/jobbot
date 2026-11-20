# Known limits and debt: where an application came from, and "send anyway"

Two things arrived together in November 2026:

- every application records the channel it came from (`applications.origin`, schema version 5), so
  results can be counted per channel;
- a card parked for its region can be sent on by one deliberate act, "Send anyway" in the panel or
  `jobbot resolve <key> --send-outside-region` in the terminal. A plain resolve of such a card is
  refused and writes nothing.

The work was reviewed five times and stopped at a bar: nothing below can send an application the
person did not decide to send, or lose one without a card. What follows was seen, judged below that
bar and left on purpose.

## Limits in the behaviour

- **The decision records what the posting says when the request arrives, not what the page
  showed.** If a refresh changes the location between the moment the card was drawn and the press,
  the event carries the new location (`actions::resolve`). Closing it means sending what the page
  showed with the request and refusing when it differs. Not built: the window is the seconds
  between a discover run and a press, and the person is reading the card.
- **A cleared application that waits for another reason keeps that card when its posting moves.**
  `prune_ineligible` parks a cleared application again when a fresh fetch says something else than
  what was cleared. One that already sits in `needs_gonza` for another reason is left as it is and
  nothing is written, so the changed location is seen only when that card is worked.
- **A database that cannot be written at start-up skips one repair in silence.**
  `repair_blank_company_norms_if_possible` lets a busy, locked or read-only database pass and
  retries at the next start. It logs nothing when it does; a `tracing::debug!` there would say why
  a blank company name is still blank.
- **Messages of the server are in Spanish only.** The panel translates its own words; the sentences
  an action answers (`SEND_ANYWAY_REFUSAL` and its neighbours) are not keyed for translation.

## Debt in the code, not in the behaviour

- `src/cli/marks.rs` (`cmd_resolve`) knows a refusal was the region one by comparing the message
  with `SEND_ANYWAY_REFUSAL`. `Outcome` should carry the kind of refusal, so that rewording the
  sentence cannot break the terminal's hint.
- `cmd_resolve` repeats the loop of its sibling commands (open the tracker, act on each key, print
  a line per key, keep the worst exit code).
- `region_override` and `had_region_override` (`crates/jobbot-store/src/tracker/applications.rs`)
  build nearly the same query over `events`; one helper taking the note condition would hold the
  shared part.
- `prune_ineligible` (`crates/jobbot-app/src/discovery.rs`) is long: deciding what happens to one
  row and writing it are in the same loop body. Splitting the decision into a function that
  returns what to do would let it be tested without a store.
- The ids of the actions (`send_anyway`, `resolve`, `approve`...) are written by hand on both sides:
  in Rust where a card lists what it offers, and in JavaScript in the key table, the labels and the
  requests. Nothing checks that the two lists agree; a test reading one from the other would.
- In the panel, the table from a verb to its sound (`CUE`) and the function from a verb to its
  request (`requestOf`) exist once per room (`moments/cola/actions.js` and
  `moments/hoy/actions.js`; `moments/aprender/actions.js` has its own `requestOf`). COLA and HOY
  differ in a few verbs only.
- `KEYLESS_ACTIONS` (`panel-src/src/ui/keys.js`) is the list of actions drawn with no key. An
  action missing from both that list and the key table throws when its card is drawn, which is the
  intent, but the guard that draws every action a room can label exists for HOY only.
- The test-only table of `RegionSignal` variants in `crates/jobbot-domain/src/region.rs` (`VARIANTS`,
  `ALL` and the slots) is kept in step by hand, with a test that fails when they disagree.

## After merging

A binary built before this change knows schema version 4. Once this version has opened a database
(it copies it to `backups/` first), the older binary refuses that file as written by a newer build,
so rebuild the binary right after merging.
