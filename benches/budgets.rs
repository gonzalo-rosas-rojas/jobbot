//! `cargo bench --bench budgets`: the performance budgets, measured on the synthetic set
//! (`jobbot_testkit::synthetic`: the live data's size, and ten times it for the larger
//! benchmarks; never the live data). Prints one line per benchmark and exits 1 when
//! `benches/budgets.toml` is breached; `cargo bench --bench budgets -- --baseline` writes this run
//! into that file as the new baseline, keeping the tolerance and the ceilings.

use std::hint::black_box;
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use serde_json::Value;

use jobbot_app::paths::Paths;
use jobbot_app::privacy::UserPaths;
use jobbot_app::profile::{load_criteria, load_exclusions};
use jobbot_domain::exclusion::check_exclusion;
use jobbot_domain::scoring::score_job;
use jobbot_domain::state::State;
use jobbot_domain::text::html_to_text;
use jobbot_panel::Panel;
use jobbot_panel::snapshot::build_snapshot;
use jobbot_testkit::bench::{
    Timing, baseline_toml, best_of, breaches, measure, measure_with, parse_budgets,
};
use jobbot_testkit::synthetic::{Size, SyntheticSet, generate, load};
use jobbot_testkit::{NOW, PrepStore, Tracker, real_paths};

/// The one seed every run uses: the same set on every machine.
const SEED: u64 = 20_261_023;
/// Every benchmark runs this many rounds and keeps its lowest p50 and p95 (`best_of`).
const ROUNDS: usize = 3;

fn main() -> ExitCode {
    let write_baseline = std::env::args().any(|arg| arg == "--baseline");
    let budgets_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("benches/budgets.toml");
    let budgets = match std::fs::read_to_string(&budgets_path)
        .map_err(|err| err.to_string())
        .and_then(|text| parse_budgets(&text).map_err(|err| err.to_string()))
    {
        Ok(budgets) => budgets,
        Err(err) => {
            eprintln!("{}: {err}", budgets_path.display());
            return ExitCode::FAILURE;
        }
    };

    let started = Instant::now();
    let real = generate(SEED, Size::REAL, *NOW);
    let ten = generate(SEED, Size::REAL.times(10), *NOW);
    println!(
        "synthetic sets: {} and {} postings, made in {:.1} s",
        real.jobs.len(),
        ten.jobs.len(),
        started.elapsed().as_secs_f64()
    );

    let mut timings = domain(&real, &ten);
    let started = Instant::now();
    let ten_store = loaded(&ten);
    println!(
        "ten times the live data loaded into SQLite in {:.1} s",
        started.elapsed().as_secs_f64()
    );
    timings.extend(store(&ten_store));
    timings.extend(snapshot(ten_store, loaded(&real)));

    println!(
        "\n{:<28} {:>8} {:>11} {:>11}",
        "benchmark", "samples", "p50 ms", "p95 ms"
    );
    for timing in &timings {
        println!(
            "{:<28} {:>8} {:>11.3} {:>11.3}",
            timing.name,
            timing.samples,
            timing.p50.as_secs_f64() * 1_000.0,
            timing.p95.as_secs_f64() * 1_000.0
        );
    }

    if write_baseline {
        if let Err(err) = std::fs::write(&budgets_path, baseline_toml(&budgets, &timings)) {
            eprintln!("{}: {err}", budgets_path.display());
            return ExitCode::FAILURE;
        }
        println!("\nnew baseline written to {}", budgets_path.display());
        return ExitCode::SUCCESS;
    }
    let found = breaches(&budgets, &timings);
    if found.is_empty() {
        println!("\nwithin budget ({})", budgets_path.display());
        ExitCode::SUCCESS
    } else {
        println!("\nover budget:");
        for line in &found {
            println!("  {line}");
        }
        println!(
            "(timings move with the machine's load: run again on a quiet machine before trusting a breach)"
        );
        ExitCode::FAILURE
    }
}

/// The pure rules, on the live data's size (the state machine on ten times it).
fn domain(real: &SyntheticSet, ten: &SyntheticSet) -> Vec<Timing> {
    let profile = real_paths().profile_dir;
    let criteria = load_criteria(&profile.join("criteria.toml")).expect("the shipped criteria");
    let exclusions =
        load_exclusions(&profile.join("exclusions.toml")).expect("the shipped exclusions");
    let jobs: Vec<_> = real.jobs.iter().map(|(job, _)| job).collect();
    let mut next = 0;
    let mut job = || {
        next = (next + 1) % jobs.len();
        jobs[next]
    };
    let score = best_of(ROUNDS, || {
        measure("domain/score_job", 200, 3_000, || {
            black_box(score_job(black_box(job()), &criteria));
        })
    });
    let exclusion = best_of(ROUNDS, || {
        measure("domain/check_exclusion", 200, 3_000, || {
            black_box(check_exclusion(black_box(job()), &exclusions));
        })
    });
    // A posting as a board sends it: paragraphs, a list and entities around the plain text.
    let pages: Vec<String> = jobs
        .iter()
        .take(200)
        .map(|job| {
            let paragraphs = job.description.replace("\n\n", "</p>\n<p>");
            format!("<div><p>{paragraphs}</p><ul><li>Rust &amp; Python</li></ul></div>")
        })
        .collect();
    let mut page = 0;
    let html = best_of(ROUNDS, || {
        measure("domain/html_to_text", 50, 1_000, || {
            page = (page + 1) % pages.len();
            black_box(html_to_text(black_box(&pages[page])));
        })
    });
    let moves = best_of(ROUNDS, || {
        measure("domain/state_moves_10x", 5, 200, || {
            let mut legal = 0usize;
            for app in &ten.applications {
                let mut at = State::Scored;
                for next in &app.moves {
                    legal += usize::from(at.allowed_next().contains(next));
                    at = *next;
                }
            }
            black_box(legal);
        })
    });
    vec![score, exclusion, html, moves]
}

fn loaded(set: &SyntheticSet) -> Tracker {
    let tracker = Tracker::in_memory().expect("an in-memory database");
    load(set, &tracker, *NOW).expect("the synthetic set loads");
    tracker
}

/// The queries the read commands and the panel run, on ten times the live data.
fn store(tracker: &Tracker) -> Vec<Timing> {
    vec![
        best_of(ROUNDS, || {
            measure("store/top_scored_10x", 10, 300, || {
                black_box(tracker.top_scored(50).unwrap());
            })
        }),
        best_of(ROUNDS, || {
            measure("store/list_by_state_10x", 3, 100, || {
                for state in State::ALL {
                    black_box(tracker.list_by_state(state).unwrap());
                }
            })
        }),
        best_of(ROUNDS, || {
            measure("store/counts_10x", 10, 300, || {
                black_box(tracker.counts().unwrap());
            })
        }),
    ]
}

/// The panel's snapshot on ten times the live data, and on the live data's size the body of
/// `GET /api/snapshot` as the route makes it (build, serialize, the privacy filter) without HTTP.
fn snapshot(ten: Tracker, real: Tracker) -> Vec<Timing> {
    let folder = tempfile::tempdir().expect("a temporary data folder");
    let panel = |tracker: Tracker| {
        let mut paths = Paths::new(folder.path());
        paths.profile_dir = real_paths().profile_dir;
        Panel::new(
            paths,
            tracker,
            PrepStore::in_memory().expect("an in-memory prep store"),
            Box::new(|| *NOW),
        )
    };
    let ten = panel(ten);
    let real = panel(real);
    let user = UserPaths::new(Some(r"C:\Users\someone"), None, None)
        .with_own_addresses(["someone@mail.example".to_string()]);

    let build = best_of(ROUNDS, || {
        measure("snapshot/build_10x", 3, 40, || {
            black_box(build_snapshot(&ten).unwrap());
        })
    });
    let body = best_of(ROUNDS, || {
        measure("snapshot/body_1x", 5, 100, || {
            let json = serde_json::to_value(build_snapshot(&real).unwrap())
                .unwrap()
                .to_string();
            let value: Value = serde_json::from_str(&json).unwrap();
            black_box(user.redact_json(value).to_string());
        })
    });
    let value = serde_json::to_value(build_snapshot(&real).unwrap()).unwrap();
    println!(
        "snapshot of the live data's size: {} bytes, {} string values",
        value.to_string().len(),
        strings_in(&value)
    );
    let redact = best_of(ROUNDS, || {
        measure_with(
            "privacy/redact_json_1x",
            10,
            300,
            || value.clone(),
            |value| {
                black_box(user.redact_json(value));
            },
        )
    });
    vec![build, body, redact]
}

/// The string values of a JSON document (not its keys): what the privacy filter reads.
fn strings_in(value: &Value) -> usize {
    match value {
        Value::String(_) => 1,
        Value::Array(items) => items.iter().map(strings_in).sum(),
        Value::Object(map) => map.values().map(strings_in).sum(),
        _ => 0,
    }
}
