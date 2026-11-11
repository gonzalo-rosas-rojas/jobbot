//! A synthetic data set the size of the real one, or ten times it, with no personal data: the
//! benchmarks measure it, and a demo or a public capture can show it. The same seed gives the same
//! set on every machine and with every crate version, because the generator is written out here.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Duration, Utc};

use jobbot_app::ports::store::{EmailKind, NewEmail, NewTask};
use jobbot_domain::models::Job;
use jobbot_domain::state::State;
use jobbot_store::tracker::{Tracker, TrackerError};

/// SplitMix64 (Steele, Lea and Flood; the constants of Vigna's reference): its stream is fixed by
/// its definition, not by a crate version, so a seed names one set for good.
pub(crate) struct SplitMix64(u64);

impl SplitMix64 {
    pub(crate) fn new(seed: u64) -> SplitMix64 {
        SplitMix64(seed)
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number in `0..n`; `n` is never 0.
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    fn chance(&mut self, per_mille: usize) -> bool {
        self.below(1_000) < per_mille
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            items.swap(i, self.below(i + 1));
        }
    }
}

/// How many of each record a set holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub jobs: usize,
    pub applications: usize,
    pub emails: usize,
    pub tasks: usize,
}

impl Size {
    /// The live database on 2026-10-23, rounded: 3,481 jobs, 633 applications, 17 e-mails and 35
    /// tasks.
    pub const REAL: Size = Size {
        jobs: 3_500,
        applications: 630,
        emails: 17,
        tasks: 35,
    };

    pub const fn times(self, n: usize) -> Size {
        Size {
            jobs: self.jobs * n,
            applications: self.applications * n,
            emails: self.emails * n,
            tasks: self.tasks * n,
        }
    }
}

/// The share of applications that end in each state, per mille: the live database's (withdrawn
/// 370, needs_gonza 125, scored 63, prepared 33, submitted 30, rejected 9, replied 2 and
/// technical_test 1 of 633), plus one per mille each of interview and ghosted so every column of
/// the board has rows. What the shares leave over goes to the first, the largest.
const FINAL_SHARES: [(State, usize); 10] = [
    (State::Withdrawn, 584),
    (State::NeedsGonza, 197),
    (State::Scored, 99),
    (State::Prepared, 52),
    (State::Submitted, 47),
    (State::Rejected, 14),
    (State::Replied, 3),
    (State::TechnicalTest, 2),
    (State::Interview, 1),
    (State::Ghosted, 1),
];

/// One application: the posting it is for, when it started and the moves it made from `Scored`,
/// why it went to Gonza when it did, and the screenshot of a form the pipeline filled in.
#[derive(Debug, Clone)]
pub struct SyntheticApplication {
    pub job_key: String,
    pub started: DateTime<Utc>,
    pub moves: Vec<State>,
    pub reason: Option<String>,
    pub screenshot: Option<String>,
}

impl SyntheticApplication {
    pub fn state(&self) -> State {
        self.moves.last().copied().unwrap_or(State::Scored)
    }
}

/// A whole set: every posting with its score, the applications, the e-mails and Gonza's tasks.
#[derive(Debug, Clone)]
pub struct SyntheticSet {
    pub jobs: Vec<(Job, f64)>,
    pub applications: Vec<SyntheticApplication>,
    pub emails: Vec<NewEmail>,
    pub tasks: Vec<NewTask>,
}

const SOURCES: [&str; 17] = [
    "greenhouse",
    "lever",
    "ashby",
    "workable",
    "smartrecruiters",
    "recruitee",
    "personio",
    "teamtailor",
    "breezy",
    "jazzhr",
    "remoteok",
    "weworkremotely",
    "remotive",
    "himalayas",
    "getonboard",
    "workingnomads",
    "pythonjobs",
];
const SENIORITY: [&str; 6] = ["Junior ", "", "Senior ", "Staff ", "Lead ", "Principal "];
const ROLES: [&str; 10] = [
    "Backend Engineer",
    "AI Engineer",
    "Software Engineer",
    "Full Stack Developer",
    "Data Engineer",
    "Platform Engineer",
    "ML Engineer",
    "DevOps Engineer",
    "Frontend Developer",
    "Solutions Engineer",
];
const STACKS: [&str; 6] = [
    " (Rust)",
    " (Python)",
    " (TypeScript)",
    " (Go)",
    " (Java)",
    "",
];
const LOCATIONS: [&str; 12] = [
    "Remote - Latin America",
    "Remote",
    "Remote - Americas",
    "Remote - Worldwide",
    "Santiago, Chile",
    "Buenos Aires, Argentina",
    "Sao Paulo, Brazil",
    "Mexico City, Mexico",
    "Sydney, Australia",
    "Melbourne, Australia",
    "New York, NY",
    "London, UK",
];
const WORDS: [&str; 64] = [
    "we",
    "build",
    "agents",
    "with",
    "python",
    "and",
    "rust",
    "for",
    "our",
    "platform",
    "you",
    "will",
    "design",
    "distributed",
    "systems",
    "that",
    "scale",
    "to",
    "millions",
    "of",
    "requests",
    "llm",
    "inference",
    "evaluation",
    "pipelines",
    "data",
    "postgres",
    "kubernetes",
    "aws",
    "cloud",
    "docker",
    "typescript",
    "react",
    "node",
    "graphql",
    "api",
    "microservices",
    "observability",
    "security",
    "testing",
    "ci",
    "team",
    "remote",
    "latam",
    "english",
    "years",
    "experience",
    "product",
    "customers",
    "ownership",
    "async",
    "tokio",
    "sql",
    "models",
    "prompt",
    "tooling",
    "on-call",
    "mentoring",
    "roadmap",
    "startup",
    "series",
    "funding",
    "benefits",
    "equity",
];
const TASK_KINDS: [&str; 4] = ["follow_up", "reply", "prepare", "manual"];
/// Reasons in the shapes the pipeline writes when it hands an application to Gonza.
const REASONS: [&str; 6] = [
    "handoff_host: this board takes applications on its own site",
    "missing_answers: 2 required questions have no answer in the bank",
    "page_error: HTTP 404",
    "captcha: the form asks for a captcha",
    "listing_only: the link opens a list of jobs, not a form",
    "unconfirmed: the page did not confirm the submission",
];
/// The folder of a made-up Windows user: real screenshots sit under the user's folder, which the
/// privacy filter then takes out.
const SCREENSHOTS: &str = r"C:\Users\someone\jobbot\data\screenshots";

/// The set `seed` names, of `size`, with every date before `now`. `size.applications` is at most
/// `size.jobs`: one application per posting.
pub fn generate(seed: u64, size: Size, now: DateTime<Utc>) -> SyntheticSet {
    assert!(
        size.applications <= size.jobs,
        "one application per posting at most"
    );
    let mut rng = SplitMix64::new(seed);
    let companies = (size.jobs / 6).max(1);
    let jobs: Vec<(Job, f64)> = (0..size.jobs)
        .map(|i| (posting(&mut rng, i, companies, now), score(&mut rng)))
        .collect();

    let mut ends = Vec::with_capacity(size.applications);
    for (state, share) in FINAL_SHARES {
        ends.extend(std::iter::repeat_n(
            state,
            size.applications * share / 1_000,
        ));
    }
    let left = size.applications - ends.len();
    ends.extend(std::iter::repeat_n(FINAL_SHARES[0].0, left));
    rng.shuffle(&mut ends);
    let mut order: Vec<usize> = (0..size.jobs).collect();
    rng.shuffle(&mut order);
    let applications: Vec<SyntheticApplication> = ends
        .into_iter()
        .zip(order)
        .map(|(end, i)| {
            let job = &jobs[i].0;
            let moves = history(&mut rng, end);
            let reason = moves
                .contains(&State::NeedsGonza)
                .then(|| rng.pick(&REASONS).to_string());
            let screenshot = (end == State::Prepared && rng.chance(100))
                .then(|| format!(r"{SCREENSHOTS}\{i}.png"));
            SyntheticApplication {
                job_key: job.key(),
                started: first_seen(job, now) + Duration::hours(1),
                moves,
                reason,
                screenshot,
            }
        })
        .collect();

    let answered: Vec<&SyntheticApplication> = applications
        .iter()
        .filter(|a| a.moves.contains(&State::Submitted))
        .collect();
    let emails = (0..size.emails)
        .map(|i| {
            let job_key = (!answered.is_empty()).then(|| rng.pick(&answered).job_key.clone());
            let company = rng.below(companies);
            NewEmail {
                message_id: format!("synthetic-{i:05}"),
                thread_id: format!("thread-{i:05}"),
                from_addr: format!("Recruiter {i} <recruiter{i}@company-{company:04}.example>")
                    .into(),
                subject: format!("Your application to Company {company:04}"),
                received_at: now - Duration::hours(rng.below(24 * 30) as i64),
                job_key,
                kind: *rng.pick(&EmailKind::ALL),
                summary: sentence(&mut rng),
            }
        })
        .collect();
    let tasks = (0..size.tasks)
        .map(|_| {
            let job_key =
                (!applications.is_empty()).then(|| rng.pick(&applications).job_key.clone());
            NewTask {
                text: format!("Follow up: {}", sentence(&mut rng)),
                job_key,
                kind: rng.pick(&TASK_KINDS).to_string(),
                due_at: (!rng.chance(300))
                    .then(|| now + Duration::hours(rng.below(24 * 14) as i64)),
                source_message_id: None,
                link: None,
            }
        })
        .collect();
    SyntheticSet {
        jobs,
        applications,
        emails,
        tasks,
    }
}

fn posting(rng: &mut SplitMix64, i: usize, companies: usize, now: DateTime<Utc>) -> Job {
    let source = *rng.pick(&SOURCES);
    let company = rng.below(companies);
    let location = *rng.pick(&LOCATIONS);
    let url = format!("https://{source}.jobs.example/company-{company:04}/{i}");
    let salary = rng.chance(300).then(|| {
        let min = 2_000.0 + 500.0 * rng.below(8) as f64;
        (min, min + 1_000.0 * (1 + rng.below(3)) as f64)
    });
    let days = 1 + rng.below(89) as i64;
    let hours = rng.below(24) as i64;
    Job {
        source: source.to_string(),
        external_id: i.to_string(),
        company: format!("Company {company:04}"),
        title: format!(
            "{}{}{}",
            rng.pick(&SENIORITY),
            rng.pick(&ROLES),
            rng.pick(&STACKS)
        ),
        apply_url: format!("{url}/apply"),
        url,
        location: location.to_string(),
        remote: location.starts_with("Remote"),
        description: description(rng),
        salary_min_monthly_usd: salary.map(|s| s.0),
        salary_max_monthly_usd: salary.map(|s| s.1),
        posted_at: Some(now - Duration::days(days) - Duration::hours(hours)),
    }
}

fn score(rng: &mut SplitMix64) -> f64 {
    rng.below(1_001) as f64 / 10.0
}

/// When the posting was first seen: six hours after it was posted, never after `now`.
fn first_seen(job: &Job, now: DateTime<Utc>) -> DateTime<Utc> {
    job.posted_at
        .map_or(now, |posted| (posted + Duration::hours(6)).min(now))
}

/// A description with the live database's spread: most between 800 and 9,600 characters (5,200 on
/// average), one in fifty between 12,000 and 25,000.
fn description(rng: &mut SplitMix64) -> String {
    let target = if rng.chance(20) {
        12_000 + rng.below(13_001)
    } else {
        800 + rng.below(8_801)
    };
    let mut text = String::with_capacity(target + 200);
    let mut sentences = 0;
    while text.len() < target {
        if sentences > 0 {
            text.push_str(if sentences % 4 == 0 { "\n\n" } else { " " });
        }
        text.push_str(&sentence(rng));
        sentences += 1;
    }
    text
}

fn sentence(rng: &mut SplitMix64) -> String {
    let words: Vec<&str> = (0..8 + rng.below(9)).map(|_| *rng.pick(&WORDS)).collect();
    let mut text = words.join(" ");
    if let Some(first) = text.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    text.push('.');
    text
}

/// A legal way from `Scored` to `end`.
fn history(rng: &mut SplitMix64, end: State) -> Vec<State> {
    use State::*;
    match end {
        Scored => vec![],
        Prepared => vec![Prepared],
        NeedsGonza if rng.chance(500) => vec![NeedsGonza],
        NeedsGonza => vec![Prepared, NeedsGonza],
        Withdrawn => match rng.below(3) {
            0 => vec![Withdrawn],
            1 => vec![NeedsGonza, Withdrawn],
            _ => vec![Prepared, NeedsGonza, Withdrawn],
        },
        Submitted => vec![Prepared, Submitted],
        Replied | Interview | Rejected | Ghosted => vec![Prepared, Submitted, end],
        TechnicalTest => vec![Prepared, Submitted, Replied, TechnicalTest],
        Offer => vec![Prepared, Submitted, Interview, Offer],
        Accepted => vec![Prepared, Submitted, Interview, Offer, Accepted],
    }
}

/// Writes the set through the store's own methods, the way discovery, the pipeline and the inbox
/// do: each posting on the day it was first seen, each move seven hours after the one before.
pub fn load(set: &SyntheticSet, tracker: &Tracker, now: DateTime<Utc>) -> Result<(), TrackerError> {
    for (job, score) in &set.jobs {
        let detail = BTreeMap::from([("synthetic".to_string(), *score)]);
        tracker.add_job(job, *score, &detail, first_seen(job, now))?;
    }
    for app in &set.applications {
        tracker.create_application(&app.job_key, app.started)?;
        for (step, state) in (1i64..).zip(&app.moves) {
            let at = (app.started + Duration::hours(7 * step)).min(now);
            let note = (*state == State::NeedsGonza)
                .then_some(app.reason.as_deref())
                .flatten();
            tracker.transition(&app.job_key, *state, note, at)?;
        }
        if let Some(screenshot) = &app.screenshot {
            tracker.set_screenshot(&app.job_key, Path::new(screenshot))?;
        }
    }
    for email in &set.emails {
        tracker.add_email(email, None, email.received_at)?;
    }
    for task in &set.tasks {
        tracker.add_task(task, now)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NOW, Tracker};
    use jobbot_domain::privacy::address_pattern;
    use jobbot_domain::text::host_of;

    const SMALL: Size = Size {
        jobs: 300,
        applications: 120,
        emails: 10,
        tasks: 8,
    };

    /// One application per final state share: per mille, so 1,000 applications hold them exactly.
    const THOUSAND: Size = Size {
        jobs: 1_000,
        applications: 1_000,
        emails: 17,
        tasks: 35,
    };

    #[test]
    fn the_generator_draws_the_reference_splitmix64_stream() {
        let mut rng = SplitMix64::new(0);
        assert_eq!(
            [rng.next_u64(), rng.next_u64(), rng.next_u64()],
            [
                0xe220_a839_7b1d_cdaf,
                0x6e78_9e6a_a1b9_65f4,
                0x06c4_5d18_8009_454f
            ]
        );
    }

    #[test]
    fn the_same_seed_gives_the_same_set_and_another_seed_another() {
        let a = format!("{:?}", generate(7, SMALL, *NOW));
        assert_eq!(a, format!("{:?}", generate(7, SMALL, *NOW)));
        assert_ne!(a, format!("{:?}", generate(8, SMALL, *NOW)));
    }

    #[test]
    fn the_set_has_the_size_asked_and_the_real_share_of_final_states() {
        let set = generate(1, THOUSAND, *NOW);
        assert_eq!(
            (
                set.jobs.len(),
                set.applications.len(),
                set.emails.len(),
                set.tasks.len()
            ),
            (1_000, 1_000, 17, 35)
        );
        let count = |state: State| {
            set.applications
                .iter()
                .filter(|a| a.state() == state)
                .count()
        };
        use State::*;
        assert_eq!(
            [
                Withdrawn,
                NeedsGonza,
                Scored,
                Prepared,
                Submitted,
                Rejected,
                Replied,
                TechnicalTest,
                Interview,
                Ghosted
            ]
            .map(count),
            [584, 197, 99, 52, 47, 14, 3, 2, 1, 1]
        );
        let keys: std::collections::HashSet<_> =
            set.applications.iter().map(|a| &a.job_key).collect();
        assert_eq!(keys.len(), 1_000, "one application per job at most");
        assert_eq!(Size::REAL.times(10).jobs, 35_000);
    }

    #[test]
    fn every_history_follows_the_state_machine() {
        for app in &generate(2, THOUSAND, *NOW).applications {
            let mut at = State::Scored;
            for next in &app.moves {
                assert!(
                    at.allowed_next().contains(next),
                    "{at} -> {next} in {app:?}"
                );
                at = *next;
            }
        }
    }

    #[test]
    fn the_set_carries_no_personal_data() {
        let set = generate(4, THOUSAND, *NOW);
        let mut texts: Vec<&str> = Vec::new();
        for (job, _) in &set.jobs {
            for url in [&job.url, &job.apply_url] {
                assert!(host_of(url).ends_with(".example"), "{url}");
            }
            texts.extend([job.company.as_str(), &job.title, &job.description]);
        }
        for email in &set.emails {
            texts.extend([
                email.from_addr.expose().as_str(),
                &email.subject,
                &email.summary,
            ]);
        }
        texts.extend(set.tasks.iter().map(|task| task.text.as_str()));
        let mut addresses = 0;
        for text in texts {
            for address in address_pattern().find_iter(text) {
                addresses += 1;
                assert!(address.as_str().ends_with(".example"), "{text}");
            }
        }
        assert_eq!(
            addresses,
            set.emails.len(),
            "only the senders are addresses"
        );
    }

    #[test]
    fn descriptions_have_the_real_length_spread() {
        let set = generate(5, THOUSAND, *NOW);
        let lengths: Vec<usize> = set
            .jobs
            .iter()
            .map(|(job, _)| job.description.chars().count())
            .collect();
        let average = lengths.iter().sum::<usize>() / lengths.len();
        assert!((4_500..=6_000).contains(&average), "{average}");
        assert!(lengths.iter().all(|n| (500..=26_000).contains(n)));
    }

    #[test]
    fn a_loaded_store_holds_what_the_set_holds() {
        let set = generate(3, THOUSAND, *NOW);
        let tracker = Tracker::in_memory().unwrap();
        load(&set, &tracker, *NOW).unwrap();
        for state in State::ALL {
            let expected = set
                .applications
                .iter()
                .filter(|a| a.state() == state)
                .count();
            assert_eq!(
                tracker.list_by_state(state).unwrap().len(),
                expected,
                "{state}"
            );
        }
        for (job, _) in &set.jobs {
            assert!(
                tracker.get_job(&job.key()).unwrap().is_some(),
                "{}",
                job.key()
            );
        }
        for email in &set.emails {
            assert!(tracker.email_exists(&email.message_id).unwrap());
        }
        assert_eq!(tracker.open_tasks().unwrap().len(), set.tasks.len());
        // Every application waiting for Gonza says why; prepared ones with a screenshot wait for
        // his approval, like the ones the pipeline fills in.
        let waiting = tracker.list_by_state(State::NeedsGonza).unwrap();
        assert!(!waiting.is_empty() && waiting.iter().all(|r| r.needs_reason.is_some()));
        let shots = |rows: Vec<jobbot_app::ports::store::ApplicationRow>| {
            rows.iter().filter(|r| r.screenshot_path.is_some()).count()
        };
        let expected = set
            .applications
            .iter()
            .filter(|a| a.screenshot.is_some())
            .count();
        assert!(expected > 0);
        assert_eq!(
            shots(tracker.list_by_state(State::Prepared).unwrap()),
            expected
        );
    }
}
