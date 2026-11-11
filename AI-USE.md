# AI use

## How this code was written

jobbot was written by Jane Doe Garcia working with Claude (Anthropic) in Claude Code. The
author set the goals, approved the specifications and plans, and decides what runs and what is
published; Claude wrote most of the code, the tests and the documents under that direction, test
first, often working through an approved plan without stopping, and each plan ends with an
independent review. Two things keep that honest without relying on anyone's attention: the
layering rule is a test (`tests/architecture.rs`), and the rules that protect the candidate are
covered by property tests, a hostile-input corpus, fuzz targets and mutation testing
([`docs/engineering/mutation-testing.md`](docs/engineering/mutation-testing.md)).

## How the program uses AI

Claude is optional and narrow. It is called as a child process (`claude -p`, in
`crates/jobbot-claude`) to write text only: interview dossiers, freelance profile and proposal
drafts, and a connection check. Everything that decides or acts is ordinary Rust with tests:
scoring, exclusions, the application state machine, the answer bank, the form planner, the CV
tailoring, the execution window and the gate.

Rules the code enforces:

- **No invented experience.** CV tailoring uses no model: it may only reorder skills the CV master
  already lists and add one sentence. A dossier story is kept only when its evidence is a real line
  of the CV.
- **Unknown answers stay unknown.** A question the answer bank does not cover (an open "why us?", a
  legal question, a self-identification question the user left empty) is left blank and flagged.
  In `--live` mode the program does not click submit while a required answer is missing.
- **A gate before every send.** `crates/jobbot-app/src/apply/gate.rs` checks, in order: the `STOP`
  file, platforms that forbid bots, listing-only pages, the exclusion list, stale postings, the
  daily cap (a lower one until the first applications are reviewed), the per-company cooldown, the
  warm-up hold and manual approval during the review phase. Only the gate can make the token that
  a live send requires, so sending without it does not compile.
- **A person in the loop first.** The default is a dry run with a screenshot; `--assist` fills a
  visible browser and leaves the click to the person; the first applications need approval.
- **Never twice.** An intent is written before every irreversible act. A click whose outcome
  nobody recorded (a run that stopped halfway) is counted as attempted and handed to the person;
  the program never clicks it again.
- **Strangers' text is data.** Postings and e-mails reach the model as JSON strings, cleaned and
  cut; they are never read as instructions, and links are checked before the browser opens them.
- **Limited exposure to the model.** Claude receives the posting and the CV without its contact
  line and without excluded employers, runs with no tools in an empty folder outside the
  repository, and without the environment variables that choose the account or the payer.
- **A spending limit.** `profile/claude.toml` sets a daily and a per-call budget; at the daily
  budget no further call is made.
- **A kill switch.** `jobbot stop` creates the `STOP` file: every command that could send or call
  Claude refuses to start while it exists, and no flag overrides it.

## Using it yourself

The `profile/` and `cv/` files in this repository describe a fictional person. Replace them with
your own before running anything against real postings, and read `profile/answers.toml` line by
line: those are the statements the program will make on your behalf.
