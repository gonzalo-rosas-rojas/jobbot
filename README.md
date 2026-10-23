# jobbot

[![CI](https://github.com/GonzaloRosas/jobbot/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/GonzaloRosas/jobbot/actions/workflows/ci.yml)

**Walkthrough:** [The Night Shift](https://gonzalorosas.github.io/jobbot-showcase/), a scroll-driven story of one
night of jobbot (18:00 to 09:00) on fictional data, ending in a live demo of its panel (an earlier build, in
Spanish). Source: [jobbot-showcase](https://github.com/GonzaloRosas/jobbot-showcase).

[![The Night Shift, scrolling from the hero to the panel, the five layers and the dawn](https://raw.githubusercontent.com/GonzaloRosas/jobbot-showcase/main/docs/hero.gif)](https://gonzalorosas.github.io/jobbot-showcase/)

A local job-search assistant written in Rust. It finds postings from public sources, drops the ones you
must not apply to, scores the rest, tailors your CV, fills application forms in a real browser, and keeps
a tracker. It is built around one rule: **it never says anything you have not told it to say, and it
never sends anything without a way for you to stop it.**

I built it for my own job search by directing [Claude Code](https://claude.com/claude-code): I chose the
goals, the architecture and the rules, and reviewed the changes; Claude wrote most of the code and tests (see
[AI-USE.md](AI-USE.md)). It is a personal tool, not a product: it targets Windows, and part of its output is
in Spanish, my native language. The profile and CV in this repository describe a **fictional person** so that
the test suite can run; replace them with your own.

## What it does

```text
discover  ->  prepare  ->  submit  ->  digest
 sources      tailored     dry run,      daily summary,
 + filters    CV + PDF     assisted or   tracker, dossiers
 + scoring                 live
```

1. **`discover`** reads public sources (Greenhouse, Lever and Ashby boards, plus RemoteOK, Remotive, Get on
   Board, Himalayas, Jobicy, We Work Remotely, Working Nomads, 4 Day Week, Workable search, Hacker News "Who is
   hiring?", python.org and a few others), removes duplicates and excluded companies, checks that the job can
   be done from your region, scores each posting against `profile/criteria.toml`, and queues the good ones in
   SQLite. Postings that expire or turn out to be out of region are withdrawn on the next run.
2. **`prepare`** adapts the master CV to each posting: it reorders skills you already listed and adds one
   sentence. It adds nothing you did not write, and it uses no language model for this. The PDF is rendered
   with Chromium.
3. **`submit`** fills the application form with Playwright. The default is a **dry run**: it fills what the
   answer bank knows, saves a screenshot, and sends nothing. `--assist` fills the form in a visible browser and
   leaves the click (and any CAPTCHA) to you. `--live` submits, but only what you approved, and only if the page
   confirms it.
4. **`digest`** writes the daily summary. **`run-daily`** chains everything and is what the scheduled task runs.
5. **`dossier`** and **`freelance`** use Claude to write an interview-preparation dossier or a freelance
   proposal draft, from the posting and your CV (never from anything else). **`prep`** is a spaced-repetition
   drill for interview questions.

## Design rules

These are the parts I would want a reviewer to check.

- **Unknown means blank, never invented.** The answer bank (`profile/answers.toml`) answers what you decided
  to answer. An open question ("why this company?"), a legal question, or self-identification such as gender
  or ethnicity is left empty and marked in red in the screenshot. `--live` refuses to click submit while a
  required answer is missing.
- **One work-rights rule lives in code.** A posting that is open only because `extra_open_places` in
  `profile/criteria.toml` names its place (Australia in the example) is answered as a move there: "authorized
  to work there?", "citizen or permanent resident?" and "hold a valid visa?" get "No", "will you need
  sponsorship or a visa?" gets "Yes", and a visa-status question gets one fixed sentence (`bank_for_job` in
  `src/pipeline.rs`, the answers in `src/answers.rs`). An empty list turns it off.
- **A human reviews the first applications.** The first five go through dry run or `--assist`. Only after that
  does the program submit on its own, within daily caps and a 90-day cool-down per company.
- **Exclusions are tested data.** `profile/exclusions.toml` lists companies, domains and categories the
  program must never apply to or write about (for example a former employer). They are matched on whole words
  and domain suffixes, they are stripped from the CV before anything reaches a model, and tests keep it so.
- **Idle hours, a kill switch and a lock.** Inside the busy hours of `profile/schedule.toml` the hourly task
  does nothing, and discover, prepare, submit, run-daily, dossier, freelance and claude-check refuse to start
  unless given `--ignore-window`. `jobbot stop` creates a `STOP` file: while it exists those seven commands
  refuse to start (run-daily also looks again between steps, submit before each application), and no flag
  overrides it. Commands that only keep records (approve, mark, record, task, inbox import, the panel) keep
  working, and nothing approved under STOP is sent until you resume. A lock prevents two runs from overlapping.
- **Postings and e-mails are data, not instructions.** They are cleaned and length-limited, and nothing in
  them is ever executed or obeyed.
- **A spending limit on the model.** `profile/claude.toml` caps Claude calls per day and per call. The calls
  use your own subscription through `claude -p`, in an empty directory, with no tools, and the program refuses
  to run if the logged-in account is not the one you expect.
- **No CAPTCHA solving, no bot against sites that forbid it.** LinkedIn, Upwork and similar platforms are
  never automated; Ashby forms (which flag automated browsers) are filled for you to review and sent by hand.

## Architecture

A Cargo workspace in layers, with the dependency rule enforced by a test (`tests/architecture.rs`) rather
than by convention:

| crate | layer | holds |
|---|---|---|
| `crates/jobbot-domain` | 0 | the pure core: job model, application state machine, scoring, exclusion rules, outbound-text filters, parsing of `profile/*.toml`, the run window. No I/O, no async runtime, no database. |
| `jobbot` (root) | 1 | the application: SQLite tracker, sources, the Claude client, forms (Playwright), the local panel, the CLI. |

The domain decides (is this excluded, what does it score, which state may follow which); the application
acts. A mistake in the first cannot send an application. Details in
[docs/engineering/architecture.md](docs/engineering/architecture.md).

## Quick start

Requirements: Rust 1.95 (pinned in `rust-toolchain.toml`), and on Windows the Visual Studio Build Tools with C++.

```powershell
cargo build --release
target\release\jobbot.exe install-browsers   # downloads Chromium once, about 250 MB
cargo test --workspace                       # the suite runs offline against fixtures
```

Then make it yours:

1. Replace `cv/cv-master.md` with your CV (keep the headings).
2. Fill `profile/answers.toml`. Every value in it is something the program will say on your behalf.
3. Edit `profile/criteria.toml` (pay floor, threshold, titles, skills), `profile/exclusions.toml`,
   `profile/sources.toml` and `profile/schedule.toml`.
4. Optionally run `jobbot claude-login` for the model-backed features; put the expected account in
   `profile/claude.local.toml` (git-ignored).

## Commands

```text
jobbot discover [--rescore]     find, filter, score and queue postings
jobbot top --n 20               review the best-scored postings
jobbot prepare                  tailor the CV per posting and render PDFs
jobbot submit [--assist|--live] dry run by default; --assist fills a visible browser; --live sends approved ones
jobbot approve KEY...           approve dry-run applications
jobbot resolve KEY...           put a "needs you" application back in the queue
jobbot confirm KEY...           mark as sent something you sent by hand
jobbot digest                   daily summary
jobbot run-daily                discover + prepare + submit + dossiers + digest
jobbot status                   counts by state, the last run, the verified Claude account
jobbot results [--last 20]      latest submit results (mode, state, answers filled, what was missing)
jobbot stop | resume            kill switch
jobbot mark KEY STATE           record a reply by hand (replied, interview, offer, rejected, ...)
jobbot record ...               record an application you sent yourself
jobbot inbox import FILE        import job e-mails summarised elsewhere (the program never touches your mail)
jobbot today                    your list for today
jobbot task add TEXT | done ID  add a to-do to today's list, or mark one done
jobbot dossier KEY...           interview-preparation dossier (calls Claude)
jobbot freelance profile|proposal PLATFORM   draft only; nothing is posted anywhere
jobbot prep ...                 spaced-repetition interview drill
jobbot panel                    local web panel (applications and review)
jobbot claude-check | claude-login
jobbot install-browsers
```

### Inbox file

`jobbot inbox import FILE` reads a JSON array of at most 500 messages (2 MB). Each message needs `message_id`,
`thread_id`, `from`, `subject`, `received_at` (RFC 3339), `kind` (`ack`, `rejection`, `recruiter`,
`interview_invite`, `technical_test`, `action_required` or `other`) and `summary`; `company`, `job_key` and
`task` (`{"text": ..., "due_at": ...}`) are optional. Every text is cleaned and cut to a fixed length, and a
message already imported is skipped.

### Scheduling

Scheduling on Windows is two scripts: `scripts/schedule.ps1` registers an hourly task, `\jobbot\run-daily`,
that exits immediately outside the run window, and `scripts/unschedule.ps1` removes it. Neither runs by itself.

## Tests and tooling

- **1,175 tests** run offline with a plain `cargo test`: sources against recorded fixtures, forms against
  local HTML pages, Claude against a fake `claude` executable. Another 109 are marked `#[ignore]`: 107 need a
  real Chromium (form filling, PDF rendering, the panel; two of them act only when `JOBBOT_SCAN_URLS` /
  `JOBBOT_REAL_URLS` list live pages), one asks a logged-in `claude` (a real call) and one queries the Windows
  Task Scheduler, read-only. After `install-browsers`,
  `cargo test -- --ignored --test-threads=1 --skip real_claude --skip real_scheduler` runs the browser ones.
- CI (`.github/workflows/ci.yml`) runs `cargo fmt --check`, `cargo clippy -D warnings` and `cargo test` on
  Windows (and, informationally, Linux), plus `cargo deny` and `cargo audit`. Actions are pinned by commit.
- `unsafe_code` is denied for the workspace and `dbg!`, `todo!` and `unimplemented!` are denied by clippy.
- About 60,000 lines of Rust in 81 files, tests included.

## Limits, honestly

- Windows first. The scheduler scripts are PowerShell; Linux is a CI job that is allowed to fail.
- Much of the CLI and panel text is in Spanish, and so are the interview-practice questions in
  `src/prep/questions.toml` and the CLI help (the doc comments in `src/cli.rs`). The example profile and the
  scripts are in English, as are most code comments.
- Times are Santiago (Chile) time: the CLI, the digest, the dossiers, the freelance drafts and the panel show
  them in that zone, and the dates of `jobbot today` and of the digest follow it. The zone is fixed in the code;
  only the busy hours in `profile/schedule.toml` take any IANA zone.
- The first build downloads the Playwright driver (Node.js from nodejs.org and playwright-core from npm) into
  the user cache. playwright-rs checks the TLS connection but no checksum; to build against a driver you checked
  yourself, point `PLAYWRIGHT_DRIVER_CACHE_DIR` at it.
- Greenhouse, Lever and Ashby forms are handled; other application systems are listed for you to open by hand.
- The local panel's page is a single built HTML file (`src/panel/page.html`); its front-end sources are not part
  of this repository.
- This is the code of a tool one person uses on their own accounts. It is published so the engineering can be
  read, not as a service.

## License

[MIT](LICENSE), except the third-party fonts, code and icons listed in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md), which keep their own licences (the fonts: SIL Open Font License
1.1, texts in `licenses/fonts/`).
