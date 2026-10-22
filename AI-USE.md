# AI use

## How this code was written

jobbot was written by Gonzalo Rosas Rojas working with Claude (Anthropic) in Claude Code. The author chose
the goals, the architecture and the rules below, reviewed the changes, and decides what ships; Claude wrote
most of the code and tests under that direction. Features were built test-first, and the layering rule in
[docs/engineering/architecture.md](docs/engineering/architecture.md) is enforced by a test, so a change that
breaks it fails the build rather than waiting for a reviewer to notice.

## How the program uses AI

Claude is optional and narrow. It is called as a child process (`claude -p`) from `src/claude.rs`, and only to
write two kinds of text: interview-preparation dossiers and freelance profile/proposal drafts. Everything that
decides or acts is ordinary Rust code with tests: scoring, the exclusion rules, the application state machine,
the answer bank, the CV tailoring (`src/cv.rs`), the run window and the daily caps.

Rules the code enforces, each with tests:

- **No invented experience.** The CV tailoring may only reorder skills already in `cv/cv-master.md` and add one
  sentence, and it uses no model. Dossier stories are kept only when their evidence is a real line of the CV.
- **Unknown answers stay unknown.** A question the answer bank cannot answer (an open "why us?", a legal
  question, self-identification such as gender or ethnicity) is left blank and flagged for the user. In
  `--live` mode the program does not click submit while a required answer is missing.
- **Nothing is sent without a human in the loop at first.** The first applications are dry runs that save a
  screenshot; the user approves them. Only then does the program submit on its own, within daily caps and a
  per-company cool-down.
- **Text from the outside world is data.** Job postings and e-mails are cleaned and length-limited, and
  nothing in them is ever interpreted as an instruction.
- **Limited exposure to the model.** Claude receives the posting and the CV with the contact line and any
  excluded employer removed, runs in an empty working directory with no tools, and the environment variables
  that choose the account or the payer are stripped.
- **A spending limit.** `profile/claude.toml` sets a daily and a per-call budget; when the daily budget is
  reached, no further call is made.
- **A kill switch.** `jobbot stop` creates a `STOP` file that every command that changes anything checks
  before acting.

## Using it yourself

The `profile/` and `cv/` files in this repository describe a fictional person. Replace them with your own
before running anything against real postings, and read the answer bank (`profile/answers.toml`) line by line:
those are the statements the program will make on your behalf.
