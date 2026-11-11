# Security

jobbot fills job application forms with a person's data, from text strangers write. It is built
on the assumption that every posting, apply page and e-mail may be hostile. The full threat model,
with the file that holds each guard, is in
[`docs/engineering/threat-model.md`](docs/engineering/threat-model.md).

## Reporting a problem

Please report a vulnerability privately, through GitHub's private vulnerability reporting on this
repository, not in a public issue. Say what input triggers it and what it reaches (a form, a
prompt, the terminal, the panel, a file).

## What counts

- A posting, apply page or e-mail that makes jobbot open a page it should refuse, type data into a
  form it should not fill, send an application the gates should stop, or apply to an excluded
  company.
- Strangers' text that reaches Claude as instructions outside its JSON string, or reaches the
  terminal, a log or a file with control or invisible characters.
- A web page that gets the local panel to act, or to read its data.
- A Claude child that runs with another account, another bill, tools, or the user's own
  `CLAUDE.md`.
- Personal data in a log, a trace or anything that leaves the machine besides the forms and the
  model call it was meant for.

## The guards, in short

Each row says what is guarded, the file that holds the guard, and the tests that keep the claim
true. `tests/security_claims.rs` reads this table and fails, naming the row, if a listed file is
gone or a listed test no longer exists. Tests marked "(Chromium)" are `#[ignore]`d in a normal run
and need a browser; the `browser` job of the CI workflow runs them weekly.

| claim | guard | tests |
|---|---|---|
| A link is opened only if it is https and carries no user name or password. | `crates/jobbot-domain/src/navigation.rs` | `crates/jobbot-domain/src/navigation.rs::other_schemes_and_plain_http_are_refused`<br>`crates/jobbot-domain/src/navigation.rs::credentials_in_the_address_are_refused` |
| No local, private, link-local or reserved address passes, however it is spelled (decimal, hex, octal, IPv4 inside IPv6, NAT64) or named (`localhost`, `.local`, `.internal`, bare names). | `crates/jobbot-domain/src/navigation.rs` | `crates/jobbot-domain/src/navigation.rs::local_and_private_hosts_are_refused`<br>`crates/jobbot-domain/src/navigation.rs::an_ipv6_address_that_carries_a_private_ipv4_one_is_refused`<br>`crates/jobbot-testkit/tests/fuzz_bodies.rs::the_address_check_keeps_its_rules_on_any_link` |
| The apply link is checked before the browser moves, and the one call that navigates takes only an address that passed the check. | `crates/jobbot-browser/src/form.rs` | `crates/jobbot-browser/src/form.rs::an_unsafe_link_is_handed_back_before_any_navigation`<br>`crates/jobbot-browser/src/form.rs::shipped_code_sends_the_browser_only_to_an_address_that_passed_the_rules` |
| The page the browser lands on is checked again, so a redirect to a private address is never filled (Chromium). | `crates/jobbot-browser/src/form.rs` | `crates/jobbot-browser/src/form.rs::a_redirect_to_a_private_address_is_never_filled` |
| Every connection of the form-filling browser goes through a proxy that refuses the local network: a page cannot reach this machine or the home network, and a name with one private answer is refused (Chromium for the first test). | `crates/jobbot-browser/src/egress.rs` | `crates/jobbot-browser/src/egress/chromium_tests.rs::a_page_cannot_reach_this_machine_through_the_proxy`<br>`crates/jobbot-browser/src/egress.rs::one_private_answer_refuses_the_name` |
| Nothing is sent outside the execution window, nor after it closes in the middle of a run. | `src/cli.rs` | `src/cli.rs::outside_window_blocks_mutating_commands`<br>`crates/jobbot-app/tests/use_cases/pipeline.rs::submit_stops_when_the_execution_window_closes_mid_run` |
| While the `STOP` file exists, nothing is sent, whatever else the gate would allow. | `crates/jobbot-app/src/apply/gate.rs` | `crates/jobbot-app/tests/use_cases/gate.rs::stop_file_wins_over_everything`<br>`src/cli.rs::stop_blocks_mutating_commands` |
| Exclusions are checked when a posting is found: an excluded one is never stored. | `crates/jobbot-app/src/discovery.rs` | `crates/jobbot-app/tests/use_cases/discovery.rs::discover_counts_and_states`<br>`crates/jobbot-app/tests/use_cases/hostile.rs::an_excluded_name_is_excluded_however_the_posting_writes_it` |
| Exclusions are checked again, by the gate, right before any send. | `crates/jobbot-app/src/apply/gate.rs` | `crates/jobbot-app/tests/use_cases/gate.rs::exclusion_is_rechecked`<br>`crates/jobbot-app/tests/use_cases/pipeline.rs::excluded_company_is_withdrawn` |
| A daily cap counts the last 24 hours, not the calendar day, and counts clicks the page did not confirm. | `crates/jobbot-app/src/apply/gate.rs` | `crates/jobbot-app/tests/use_cases/gate.rs::daily_cap_counts_the_last_24_hours_not_the_calendar_day`<br>`crates/jobbot-app/tests/use_cases/pipeline.rs::unconfirmed_clicks_count_toward_the_daily_cap` |
| A company that was just applied to rests before another application to it, even if the first click was not confirmed. | `crates/jobbot-app/src/apply/gate.rs` | `crates/jobbot-app/tests/use_cases/gate.rs::company_cooldown`<br>`crates/jobbot-app/tests/use_cases/pipeline.rs::an_unconfirmed_click_holds_back_the_next_posting_at_the_same_company` |
| An intent is written to the database before every click, and only one can be open per application, so a crash or a second run cannot send the same form twice. | `crates/jobbot-app/src/apply/pipeline.rs`<br>`crates/jobbot-store/src/tracker.rs` | `crates/jobbot-app/tests/use_cases/pipeline.rs::a_live_click_is_preceded_by_a_durable_intent_and_closes_it`<br>`crates/jobbot-app/tests/use_cases/pipeline.rs::an_interrupted_click_is_handed_to_gonza_and_never_repeated`<br>`crates/jobbot-store/src/tracker.rs::only_one_submit_intent_can_be_open_per_application` |
| A submission counts as sent only when the page confirms it; otherwise it is "unconfirmed" and goes back to the person, never retried (the first test needs Chromium). | `crates/jobbot-browser/src/form.rs`<br>`crates/jobbot-domain/src/form.rs` | `crates/jobbot-browser/src/form.rs::silent_submit_is_unconfirmed`<br>`crates/jobbot-domain/src/form.rs::a_success_phrase_that_is_denied_does_not_confirm`<br>`crates/jobbot-app/tests/use_cases/pipeline.rs::unconfirmed_submit_is_recorded_as_needs_gonza_and_never_retried` |
| Posting text reaches Claude only as JSON strings, cleaned and cut, so an instruction in a posting cannot open a line of its own in a prompt. | `crates/jobbot-domain/src/untrusted.rs` | `crates/jobbot-app/tests/use_cases/hostile.rs::a_hostile_posting_reaches_claude_only_as_json_strings_with_nothing_hidden`<br>`crates/jobbot-app/src/learn/dossier.rs::the_posting_enters_the_prompt_as_json_strings_that_cannot_pass_for_its_headings` |
| The model runs with tools, slash commands, MCP servers and session persistence switched off, within a per-call budget and a daily budget that stops the call before it is made. | `crates/jobbot-claude/src/lib.rs`<br>`crates/jobbot-app/src/claude.rs` | `crates/jobbot-claude/src/lib.rs::the_command_line_has_the_flags_the_runner_depends_on`<br>`crates/jobbot-app/tests/use_cases/claude.rs::the_daily_budget_stops_the_call_before_it_is_made` |
| The panel listens on `127.0.0.1` only, not on IPv6 loopback, with a fresh token at every launch. | `crates/jobbot-panel/src/server.rs` | `crates/jobbot-panel/src/server.rs::the_server_listens_on_loopback_only_with_a_fresh_token`<br>`crates/jobbot-panel/src/server.rs::the_panel_is_not_reachable_over_ipv6_loopback` |
| Every panel API route needs the exact `Host` and the token, refuses a foreign `Origin`, and takes POST only with a JSON body of bounded size. | `crates/jobbot-panel/src/http.rs` | `crates/jobbot-panel/src/http.rs::every_api_route_goes_through_the_same_guards_before_it_does_anything`<br>`crates/jobbot-panel/src/http.rs::a_foreign_host_is_refused_everywhere`<br>`crates/jobbot-panel/src/server.rs::a_rebinding_host_is_refused_on_the_wire` |
| The panel page carries a Content-Security-Policy with a nonce for scripts and styles and trusted types, and every script and style tag in it has the nonce. | `crates/jobbot-panel/src/http.rs`<br>`crates/jobbot-panel/src/page.html` | `crates/jobbot-panel/src/http.rs::the_page_policy_allows_own_fonts_and_images_and_refuses_every_frame`<br>`crates/jobbot-panel/src/lib.rs::every_script_and_style_tag_carries_the_nonce` |
| Everything the command line prints on standard output passes one filter: control characters become spaces and invisible characters are removed. | `src/cli.rs` | `src/cli.rs::hostile_postings_and_emails_print_as_plain_text` |
| `unsafe` code is denied across the workspace; every crate takes that setting and no shipped source file allows it. | `Cargo.toml` | `tests/architecture.rs::unsafe_is_denied_once_and_every_crate_takes_that_setting` |

The threat model names the guards the table leaves out (the e-mail import, the privacy filter
for logs and traces, the environment of the Claude child, file names made from job keys) and the
file that holds each. Supply-chain checks (`cargo deny`, `cargo audit`, locked dependencies) run
in the CI workflow; the property tests, the hostile corpus, the fuzz targets and mutation testing
are described in the threat model under "How the guards are tested".

## Supported versions

Only the latest commit of the default branch.
