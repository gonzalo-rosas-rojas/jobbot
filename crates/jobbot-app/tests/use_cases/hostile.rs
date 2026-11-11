//! The hostile corpus (`jobbot_testkit::hostile`) through the real entry points: the exclusion
//! check, the browser's address check, the readers of a posting, the prompts for Claude, the
//! dossier file, CV tailoring, the e-mail import, a mark from the panel and the file names a job
//! key becomes. What a stranger writes must stay text: never instructions out of place, never a
//! hidden character, never an address the browser opens.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::TimeDelta;

use jobbot_app::actions::mark;
use jobbot_app::apply::cv::tailor;
use jobbot_app::freelance::build_proposal_prompt;
use jobbot_app::inbox::import;
use jobbot_app::learn::dossier::{
    RawDossier, RawGap, RawQuestion, RawStory, assemble, build_dossier_prompt, dossier_path,
    render_markdown,
};
use jobbot_domain::exclusion::check_exclusion;
use jobbot_domain::handoff::sheet_path;
use jobbot_domain::navigation::{NavPolicy, check};
use jobbot_domain::rules::{Exclusions, parse_criteria};
use jobbot_domain::scoring::{parse_salary_monthly_usd, score_job};
use jobbot_domain::state::State;
use jobbot_domain::text::{html_to_text, safe_key};
use jobbot_testkit::hostile::{LINKS, MARK, disguises, emails, hidden_chars, postings, texts};
use jobbot_testkit::{NOW, SAMPLE_CV, Tracker, make_job_with, real_paths};

/// Made-up rules in the shape of the real exclusion list.
fn rules() -> Exclusions {
    Exclusions {
        names: vec!["Blocked Industries".into()],
        domains: vec!["blocked.example".into()],
        categories: vec!["gambling".into()],
    }
}

#[test]
fn an_excluded_name_is_excluded_however_the_posting_writes_it() {
    for (how, company) in disguises("Blocked Industries") {
        let job = make_job_with(|job| job.company = company.clone());
        let result = check_exclusion(&job, &rules());
        assert!(result.excluded, "{how}: {company:?} was not excluded");
    }
    for (how, word) in disguises("gambling") {
        let job = make_job_with(|job| job.description = format!("A {word} platform."));
        let result = check_exclusion(&job, &rules());
        assert!(result.excluded, "{how}: {word:?} was not excluded");
    }
}

#[test]
fn an_excluded_domain_is_excluded_however_the_link_writes_it() {
    for link in [
        "https://BLOCKED.example/jobs/1",
        "https://careers.blocked.example./apply",
        "https://jobs.blocked.example:443/x?y=1",
    ] {
        let job = make_job_with(|job| job.apply_url = link.into());
        assert!(check_exclusion(&job, &rules()).excluded, "{link}");
    }
}

#[test]
fn the_browser_opens_none_of_the_odd_links() {
    for (name, link) in LINKS {
        assert!(
            check(link, NavPolicy::STRICT).is_err(),
            "{name}: {link:?} would be opened"
        );
    }
    for (name, job) in postings() {
        assert!(
            check(&job.apply_url, NavPolicy::STRICT).is_err(),
            "{name}: {:?}",
            job.apply_url
        );
    }
}

#[test]
fn the_readers_of_a_posting_take_every_hostile_one() {
    let criteria = std::fs::read_to_string(real_paths().profile_dir.join("criteria.toml")).unwrap();
    let criteria = parse_criteria(&criteria).unwrap();
    for (name, job) in postings() {
        let score = score_job(&job, &criteria);
        assert!(score.total.is_finite(), "{name}: {}", score.total);
        let _ = check_exclusion(&job, &rules());
        let _ = parse_salary_monthly_usd(&job.description);
        let text = html_to_text(&job.description);
        assert!(
            !text.contains("fetch(") && !text.contains("display:none"),
            "{name}: script or style reached the text: {text:?}"
        );
    }
}

/// The lines of `prompt` that hold `MARK` start with one of `fields`: the hostile text stayed
/// inside the JSON string of its field instead of opening a line of its own.
fn marked_lines_outside(prompt: &str, fields: &[&str]) -> Vec<String> {
    prompt
        .lines()
        .filter(|line| line.contains(MARK))
        .filter(|line| !fields.iter().any(|field| line.starts_with(field)))
        .map(|line| line.chars().take(120).collect())
        .collect()
}

#[test]
fn a_hostile_posting_reaches_claude_only_as_json_strings_with_nothing_hidden() {
    let fields = [
        "Company: \"",
        "Title: \"",
        "Location: \"",
        "Description: \"",
    ];
    for (name, job) in postings() {
        let prompt = build_dossier_prompt(&job, SAMPLE_CV);
        assert_eq!(
            marked_lines_outside(&prompt, &fields),
            Vec::<String>::new(),
            "{name}"
        );
        assert_eq!(hidden_chars(&prompt), Vec::<char>::new(), "{name}");
        assert_eq!(prompt.matches("\n## TASK\n").count(), 1, "{name}");
    }
    for (name, text) in texts() {
        let prompt = build_proposal_prompt("upwork", &text, SAMPLE_CV);
        assert_eq!(
            marked_lines_outside(&prompt, &["\""]),
            Vec::<String>::new(),
            "{name}"
        );
        assert_eq!(hidden_chars(&prompt), Vec::<char>::new(), "{name}");
        assert_eq!(prompt.matches("\n## TASK\n").count(), 1, "{name}");
    }
}

#[test]
fn the_dossier_file_keeps_the_posting_on_its_title_line_and_what_claude_wrote_clean() {
    for (name, job) in postings() {
        let hostile = texts().into_iter().find(|(n, _)| *n == name).unwrap().1;
        let raw = RawDossier {
            company_summary: hostile.clone(),
            likely_questions: vec![RawQuestion {
                question: hostile.clone(),
                why: hostile.clone(),
            }],
            stories: vec![RawStory {
                title: hostile.clone(),
                evidence: "Python, TypeScript, SQL".into(),
                story: hostile.clone(),
            }],
            gaps: vec![RawGap {
                topic: hostile.clone(),
                question: hostile.clone(),
                answer: hostile.clone(),
            }],
        };
        let empty = render_markdown(&job, &assemble(RawDossier::default(), SAMPLE_CV), *NOW);
        let first_line = empty.lines().next().unwrap();
        assert!(
            empty.lines().skip(1).all(|line| !line.contains(MARK)),
            "{name}: the posting broke out of the title line: {first_line:?}"
        );
        let full = render_markdown(&job, &assemble(raw, SAMPLE_CV), *NOW);
        assert_eq!(hidden_chars(&full), Vec::<char>::new(), "{name}");
    }
}

#[test]
fn tailoring_never_copies_a_word_of_the_posting_into_the_cv() {
    for (name, mut job) in postings() {
        // Naming skills of the CV makes tailoring rewrite the summary and the skills lines.
        job.description
            .push_str(" We use Python, TypeScript and RAG.");
        let cv = tailor(SAMPLE_CV, &job);
        assert!(cv.contains("Relevant to this role:"), "{name}");
        assert!(!cv.contains(MARK), "{name}");
        assert_eq!(cv.lines().count(), SAMPLE_CV.lines().count(), "{name}");
        assert_eq!(hidden_chars(&cv), Vec::<char>::new(), "{name}");
    }
}

#[test]
fn hostile_emails_are_stored_as_plain_lines_and_open_nothing() {
    let tracker = Tracker::in_memory().unwrap();
    let job = make_job_with(|job| job.company = "Acme AI".into());
    let sent_at = *NOW - TimeDelta::days(5);
    tracker
        .add_job(&job, 80.0, &BTreeMap::new(), sent_at)
        .unwrap();
    tracker.create_application(&job.key(), sent_at).unwrap();
    for state in [State::Prepared, State::Submitted] {
        tracker
            .transition(&job.key(), state, None, sent_at)
            .unwrap();
    }
    let (names, items): (Vec<_>, Vec<_>) = emails(*NOW).into_iter().unzip();

    let report = import(&tracker, &items, *NOW).unwrap();

    assert_eq!(report.rejected, 0, "{:#?}", report.lines);
    assert_eq!(report.imported, items.len(), "{:#?}", report.lines);
    for line in &report.lines {
        assert_eq!(hidden_chars(line), Vec::<char>::new(), "{line:?}");
        assert!(!line.contains('\n'), "{line:?}");
    }
    let one_line = |what: &str, text: &str| {
        assert_eq!(hidden_chars(text), Vec::<char>::new(), "{what}: {text:?}");
        assert!(!text.contains('\n'), "{what}: {text:?}");
    };
    for email in tracker.recent_emails(*NOW - TimeDelta::days(30)).unwrap() {
        one_line("message id", &email.message_id);
        one_line("from", email.from_addr.expose());
        one_line("subject", &email.subject);
        one_line("summary", &email.summary);
    }
    let tasks = tracker.open_tasks().unwrap();
    assert!(!tasks.is_empty(), "the corpus makes tasks: {names:?}");
    for task in tasks {
        one_line("task", &task.text);
        assert_eq!(
            task.link, None,
            "an e-mail never gives a task a link: {task:?}"
        );
    }
}

#[test]
fn a_hostile_note_from_a_web_page_is_stored_on_one_plain_line() {
    for (name, text) in texts() {
        let tracker = Tracker::in_memory().unwrap();
        let job = make_job_with(|job| job.external_id = name.into());
        tracker.add_job(&job, 80.0, &BTreeMap::new(), *NOW).unwrap();
        tracker.create_application(&job.key(), *NOW).unwrap();
        for state in [State::Prepared, State::Submitted] {
            tracker.transition(&job.key(), state, None, *NOW).unwrap();
        }

        let outcome = mark(&tracker, &job.key(), "rejected", Some(&text), *NOW).unwrap();

        assert!(outcome.ok, "{name}: {}", outcome.message);
        let note = tracker
            .get_application(&job.key())
            .unwrap()
            .unwrap()
            .note
            .unwrap_or_default();
        assert_eq!(hidden_chars(&note), Vec::<char>::new(), "{name}: {note:?}");
        assert!(!note.contains('\n'), "{name}: {note:?}");
    }
}

#[test]
fn a_hostile_job_key_names_a_file_inside_its_folder() {
    let dir = Path::new("data").join("handoff");
    for (name, job) in postings() {
        let key = job.key();
        let safe = safe_key(&key);
        assert!(
            safe.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')),
            "{name}: {safe:?}"
        );
        for path in [sheet_path(&dir, &key), dossier_path(&dir, &key)] {
            assert_eq!(path.parent(), Some(dir.as_path()), "{name}: {path:?}");
        }
    }
}
