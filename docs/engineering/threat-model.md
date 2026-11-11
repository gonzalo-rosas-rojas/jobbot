# Threat model

jobbot finds job postings, prepares a CV and a dossier for each, fills application forms in a
headless browser and keeps the record of every application. It runs on one Windows PC, for one
person, from a scheduled task and from a local web panel. This document says what it protects,
from whom, where strangers' input comes in, what stops each threat in the code, and what is left.
It was written by hand from the code; every mitigation names the file that holds it.

## What is protected

| asset | where it lives | why it matters |
|---|---|---|
| The candidate's personal data: name, e-mail, phone, address, work history, answers to form questions | `profile/answers.toml`, `cv/cv-master.md`, the SQLite database | typed into forms on the open web; a leak or a wrong form is permanent |
| The candidate's reputation | every application sent | a wrong, duplicate or unwanted application cannot be taken back |
| Contract obligations: companies and categories the candidate must never apply to | `profile/exclusions.toml` | applying to one of them breaches a contract |
| The Claude subscription and its spending | `%LOCALAPPDATA%\jobbot\claude-config`, the spend ledger in SQLite | another account billed, or a budget burned by a loop |
| The PC itself | everything the program can reach | a browser or a child process driven by a stranger |

## Who can attack, and how

1. **The author of a posting.** Writes the title, company, description and apply link that the
   program reads, scores, pastes into prompts, prints and opens. Can write instructions for a
   model, invisible or control characters, markup, and any link.
2. **The page behind an apply link.** Decides what the browser lands on, where it redirects, which
   fields the form asks for and how they are labelled.
3. **The sender of an e-mail.** The candidate's job e-mails are imported from a JSON file (jobbot
   never reads the mailbox); their senders control subjects, bodies and anything presented as an
   attachment.
4. **A web page open in the candidate's browser.** Can try to reach the local panel (cross-site
   requests, DNS rebinding).
5. **The model.** Claude reads strangers' text; what it writes back may have been steered by it.
6. **Another process or user on the PC, and the environment of the scheduled task.** Can set
   variables that pick another Claude account or bill, or place a `CLAUDE.md` where a child would
   read it.
7. **The supply chain.** The crates the program builds from.

## Entry points and their guards

### Postings (sources) — threat 1

- Every source goes through one HTTP stack with per-host rate limits, retries that honour
  `Retry-After` within a cap, and cooldowns stored in SQLite (`crates/jobbot-sources/src/http.rs`,
  `sources/base.rs`).
- Exclusion is checked on the apply and posting hosts, on the company name and on the title and
  description, matched as a person reads the text: invisible characters do not split a name and
  full-width letters fold to ASCII (`crates/jobbot-domain/src/exclusion.rs`, `text::as_read`).
  The submit gate checks it again before any click (`crates/jobbot-app/src/apply/gate.rs`).
- HTML is turned into text without `script` or `style` content, with an explicit stack so deep
  nesting cannot overflow (`crates/jobbot-domain/src/text.rs`).
- A pay amount that is not a finite number is not pay (`scoring.rs`).

### The browser — threats 1 and 2

- The apply link must pass `navigation::check` before the browser moves and again where it lands:
  https only, no user name or password, no loopback, private, link-local, carrier-grade NAT,
  "this network", multicast or reserved address, however it is spelled (decimal, hex, octal,
  IPv4 inside IPv6, NAT64), no local names (`localhost`, `.local`, `.internal`, `.lan`,
  `.home.arpa`, bare names). The only way to make the `NavigableUrl` the browser takes is that
  check, and a source test fails if shipped code calls `goto` on anything else
  (`crates/jobbot-domain/src/navigation.rs`, `crates/jobbot-browser/src/form.rs`).
- Every connection the filler's and the inspector's Chromium make goes through a proxy inside
  jobbot on `127.0.0.1` that takes only `CONNECT host:443`: it looks a name up once, refuses it if
  any answer breaks the same rules (`navigation::connect_host`, `is_public_ip`), and connects to
  exactly the addresses it judged (an attempt every 250 ms, families alternating, so an IPv6 that
  does not route does not stall the site). Chromium is launched with the bypass list `<-loopback>`
  (it removes Chromium's implicit bypass, so this machine's addresses go through the proxy too;
  named in code, not left to Playwright, which skips it when an environment variable says so), with
  `--host-resolver-rules=MAP * ^NOTFOUND , EXCLUDE 127.0.0.1` (it makes no lookup of its own),
  with WebRTC kept to proxied traffic and with QUIC off. This closes DNS rebinding and every
  request a page makes to this machine or the home network (navigations, redirects, popups,
  frames, subresources, forms, beacons, WebSockets, WebRTC). If the proxy does not start, the
  browser does not either; if it stops, the browser reaches nothing. When the page itself is
  refused, the posting's reason says so (`unsafe_url: the address check refused <host> (…)`); every
  refusal of the address check, a frame's or a script's too, is printed once per host and reason as
  an `aviso:` line on stderr, cleaned, at most 20 per session. Proven with a real Chromium: a
  probe page that reaches local listeners without the proxy reaches nothing through it, and WebRTC
  sends no UDP (`crates/jobbot-browser/src/egress.rs`, `src/launch.rs`, `src/egress/chromium_tests.rs`).
- The form planner decides from the field scan alone: a box the page ticked is never clicked
  again; a box that stands alone is ticked only when it is required and matches a consent phrase,
  and in a group only the options the bank's answer names are ticked; a pre-ticked box that signs
  the candidate up for more is reported to the candidate to untick; a field with no approved answer stays
  empty and is reported (`crates/jobbot-domain/src/form.rs`).
- The CV is printed from HTML in which raw HTML from the markdown is text, in a page whose every
  network request is aborted (`crates/jobbot-browser/src/cv_render.rs`).
- Chromium runs with its sandbox, so a renderer that a page takes over is still confined. Playwright
  passes `--no-sandbox` unless the sandbox is asked for, and until 2026-10-24 jobbot did not ask.
  Every launch takes its options from one place, which asks for the sandbox and gives the launch
  30 s instead of Playwright's three minutes. A Chromium test reads the running browser's command
  line and fails on `--no-sandbox` (`crates/jobbot-browser/src/launch.rs`).

### Sending — threats 1, 2 and 5

- Nothing is sent outside the execution window or while the `STOP` file exists; a fixed gate runs
  before every submission: STOP, platforms that forbid automation, listing-only pages, exclusions,
  stale postings, a rolling daily cap that counts unconfirmed clicks too, a per-company cooldown,
  a warm-up hold for priority companies, and manual approval until enough submissions were
  reviewed (`crates/jobbot-app/src/apply/gate.rs`, `crates/jobbot-domain/src/window.rs`).
- An intent is written before the click and closed after it, and only one can be open per
  application, so a crash or a second run cannot send twice (`jobbot-store`, `INTENT_SUBMIT`).
- The state machine refuses every move its table does not list (`crates/jobbot-domain/src/state.rs`,
  `Tracker::transition`).

### Prompts and the model — threats 1 and 5

- Posting text reaches Claude as JSON strings under a heading that marks it untrusted, cleaned of
  control and invisible characters and cut to a length: an instruction in a posting cannot open a
  line of its own in the prompt (`crates/jobbot-domain/src/untrusted.rs`,
  `crates/jobbot-app/src/learn/dossier.rs`, `freelance.rs`).
- The child runs with tools switched off, no slash commands, no MCP servers, no session
  persistence, a per-call budget and a daily budget, and a JSON schema for its answer
  (`crates/jobbot-claude/src/lib.rs`, `crates/jobbot-app/src/claude.rs`).
- What Claude writes is cleaned and capped; a dossier story whose evidence is not a line of the CV
  is dropped; tailoring a CV only reorders the master's own skills and never adds text from a
  posting (`learn/dossier.rs`, `apply/cv.rs`).

### E-mails — threat 3

- The import file is limited in size and count and each message is validated on its own: every
  text becomes one line without control or invisible characters, cut to its cap; ids are never cut;
  dates must be in range; unknown fields (an `attachments` list, a `link`) are ignored, nothing in a
  file is opened or run, and no task made from an e-mail carries a link
  (`crates/jobbot-app/src/inbox.rs`).

### The panel — threat 4

- It listens on `127.0.0.1` only. Every request must carry the exact `Host` of that socket
  (against DNS rebinding); every API call needs the per-launch token, compared in constant time,
  and is refused with a foreign `Origin`; a POST must be `application/json`, which a hostile page
  cannot send without a preflight the server never answers; bodies are size-limited and read only
  after the headers pass (`crates/jobbot-panel/src/http.rs`).
- The page has a nonce-only Content-Security-Policy with trusted types, `frame-ancestors 'none'`
  and `X-Frame-Options: DENY`; data reaches it only as JSON from the API.
- Every action goes through the same functions as the command line, and none starts discovery, a
  browser or Claude. A repeated `Idempotency-Key` gets the first reply and runs once.

### The terminal and the logs — threats 1 and 3

- Every line the command line prints on standard output passes one filter: control characters
  other than the newline become spaces and invisible characters are removed, so an escape sequence
  in a posting cannot move the cursor, recolour or hide text (`src/cli.rs`, `PlainText`).
- Standard error is not filtered. The lines written there carry the program's own words, hosts and
  URLs (`aviso:` from the HTTP stack and the database); the two that carry page text, the
  `--assist` line (company, title, questions left) and the `[omitido]` line (a field's label),
  clean it where they are built (`crates/jobbot-browser/src/form.rs`, `plain`, `plain_line`). The
  scheduled task logs standard error with standard output (`scripts/run-daily.cmd`), so its
  warnings skip the privacy filter; none carries personal data today.
- Output that is not a terminal (the scheduled task's log) and the trace go through the privacy
  filter, which replaces the user's folders and own addresses; the trace keeps only jobbot's own
  spans, whose fields are counts and outcomes (`crates/jobbot-app/src/privacy.rs`). Fields that
  hold an e-mail address are `Pii<T>`, which prints `***` (`crates/jobbot-domain/src/privacy.rs`).

### The Claude child's environment — threat 6

- The child gets its own config folder under `%LOCALAPPDATA%\jobbot`, never inherits a variable
  that picks another account or bill (`ANTHROPIC_*`, `CLAUDE_CODE_USE_*`, `AWS_*`, OAuth tokens,
  another `CLAUDE_CONFIG_DIR`), runs with `CLAUDE_CODE_DISABLE_CLAUDE_MDS=1` and telemetry off,
  and works in a folder outside the repository. A preflight checks the login before any call and
  stops on the wrong account (`crates/jobbot-claude/src/lib.rs`, `crates/jobbot-app/src/paths.rs`).
- File names made from job keys keep only letters, digits, `_`, `.` and `-` (and the dossier's add a
  hash), so a posting cannot name a path outside its folder (`text::safe_key`, `outbound::key_slug`).

### The supply chain — threat 7

- Versions live once in `[workspace.dependencies]` and `Cargo.lock` is committed; `cargo deny`
  (licenses, sources, bans, advisories) and `cargo audit` run clean; `unsafe` is denied across the
  workspace; the domain may depend only on an allow-list of crates, enforced by
  `tests/architecture.rs`.
- The browser driver is the exception: playwright-rs's build script downloads Node.js and
  playwright-core over HTTPS and checks no hash, and both run whenever the browser opens. The cache
  is keyed by driver version, so only the first build after a version change downloads.
  `scripts/verify-playwright-driver.ps1` compares the cached copy with Node's SHASUMS256 and npm's
  integrity, file by file; run it after that build. Last run 2026-10-24, with playwright-rs 0.20.1:
  node.exe 24.21.0 and all 115 files of playwright-core 1.64.0 matched.

## How the guards are tested

The guards a reviewer would check first are listed in the table in
[`SECURITY.md`](../../SECURITY.md), each with its file and the tests that hold it;
`tests/security_claims.rs` fails if a listed file or test goes missing. The rest of this section
is the kinds of test behind them.

Unit tests pin examples; property tests (`proptest`) hold the rules over generated input (the
address check, the state machine, the e-mail import, the source parsers, the pay and HTML readers);
a hostile corpus (`crates/jobbot-testkit/src/hostile.rs`) runs strangers' text at its worst through
the real entry points; fuzz targets (`fuzz/`) cover the form planner, the e-mail import, links and
the CV page; mutation testing measures how many deliberate faults in the domain the tests catch
(`docs/engineering/mutation-testing.md`).

## What is left

- Names spelled with ASCII look-alikes ("1" for "l", "rn" for "m") are not folded; letters from
  other scripts are (ADR 0009). Such a spelling of an excluded company is caught by its domain only.
- Mapped, compatible, well-known NAT64, 6to4 and Teredo addresses are judged by the IPv4 addresses
  they carry; the local-use NAT64 prefix (`64:ff9b:1::/48`) is refused whatever it carries.
- The driver's Node.js is older than its line's newest security release (24.21.0 against 24.22.1
  on 2026-10-24, with playwright-rs 0.20.1; `verify-playwright-driver.ps1` exits 2 while that
  holds). Rated low: jobbot makes no HTTP request from Node (no request context, no `route.fetch`,
  no downloads); Chromium fetches the pages. The fix is the next playwright-rs update.
  playwright-rs also takes `PLAYWRIGHT_NODE_EXE` to run its driver on another Node.
- Chromium, the part that does read hostile pages, is the build Playwright 1.64 bundles
  (156.0.8078.4, through playwright-rs 0.20.1, taken on 2026-10-24 for that reason). It is an early
  build of its line and gets no fixes of its own: it ages until the next playwright-rs update,
  which is taken when it moves Chromium. Driving the installed Chrome instead (`channel: chrome`)
  would stay current, but would tie live submission to a browser that updates itself outside the
  versions Playwright is tested with.
- The browser's proxy is now the only thing between a page and the home network: Chromium's own
  Local Network Access check treats a proxied request as public and steps aside. A bug in the
  proxy's decision would open what it closes; the decision is small, pure and unit-tested, and the
  splice is `tokio::io::copy_bidirectional`. Pages on a port other than 443 do not open (with an
  `aviso:` line). Any local process can use the proxy while a run holds it open (no credential;
  its port is on Chromium's command line); it reaches only public hosts on 443, which the PC's
  processes can reach anyway, unless a firewall restricts some account's outbound traffic.
  Service-worker traffic is expected to take the same path but is not proven with Chromium (a
  service worker needs an https origin the tests cannot serve through the strict proxy). The CV
  printer and the Chromium tests' own sessions launch without the proxy: the printer renders the
  candidate's own CV, and the tests serve their pages from this machine. The panel resists
  rebinding by itself (exact `Host` and a per-start token).
- Form labels reach the answer sheet as the page wrote them; the planner's rules hold, but a label
  can still be misleading text.
- A Claude reply steered by a posting may put markdown structure inside a dossier section; it is
  shown as Claude wrote it, cleaned of hidden characters.
- The CV page does not judge link schemes: the master is the candidate's own text.
- The fuzz targets run on Linux only and have not run yet; the property tests run the same bodies
  on Windows.
- The PC is trusted. Malware with the user's rights can read the database, the profile and the
  Claude login, as it could any file of that user.
