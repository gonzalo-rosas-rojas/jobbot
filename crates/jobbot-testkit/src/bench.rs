//! The benchmark harness behind `cargo bench --bench budgets`: each sample times one call, the
//! result is its p50 and p95 (nearest rank, the method of `scripts/bench/measure.ps1`), and a run
//! is held against `benches/budgets.toml`. Medians and tails, not averages: one slow call in a
//! hundred is what a person waits for.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde::Deserialize;

/// What one benchmark measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timing {
    pub name: String,
    pub samples: usize,
    pub p50: Duration,
    pub p95: Duration,
}

/// The baseline of one benchmark, in milliseconds, and the ceiling for its p95 when it has one.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub p50_ms: f64,
    pub p95_ms: f64,
    #[serde(default)]
    pub max_p95_ms: Option<f64>,
}

/// `benches/budgets.toml`: how much slower than its baseline a p50 may get, and every benchmark's
/// budget by name.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budgets {
    pub tolerance: f64,
    pub bench: BTreeMap<String, Budget>,
}

pub fn parse_budgets(text: &str) -> Result<Budgets, toml::de::Error> {
    toml::from_str(text)
}

/// The nearest-rank percentile `p` (0 to 1) of durations sorted from fastest to slowest.
pub fn percentile(sorted: &[Duration], p: f64) -> Duration {
    let rank = (p * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

/// Times `samples` calls of `call`, one by one, after `warmup` calls that are not kept.
pub fn measure(name: &str, warmup: usize, samples: usize, mut call: impl FnMut()) -> Timing {
    measure_with(name, warmup, samples, || (), |()| call())
}

/// Like `measure`, with a fresh input from `setup` for every call; making it is not timed.
pub fn measure_with<T>(
    name: &str,
    warmup: usize,
    samples: usize,
    mut setup: impl FnMut() -> T,
    mut call: impl FnMut(T),
) -> Timing {
    assert!(samples > 0, "a benchmark needs at least one sample");
    for _ in 0..warmup {
        call(setup());
    }
    let mut times: Vec<Duration> = (0..samples)
        .map(|_| {
            let input = setup();
            let start = Instant::now();
            call(input);
            start.elapsed()
        })
        .collect();
    times.sort();
    Timing {
        name: name.to_string(),
        samples,
        p50: percentile(&times, 0.50),
        p95: percentile(&times, 0.95),
    }
}

/// The lowest p50 and the lowest p95 of `rounds` runs of one benchmark. The machine only ever adds
/// time (another process, a clock that has not sped up yet), so the lowest is the steadiest
/// figure from one run to the next.
pub fn best_of(rounds: usize, mut run: impl FnMut() -> Timing) -> Timing {
    let mut best = run();
    for _ in 1..rounds {
        let next = run();
        best.p50 = best.p50.min(next.p50);
        best.p95 = best.p95.min(next.p95);
    }
    best
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

/// What makes a run fail, one line each: a benchmark without a budget, a p50 more than `tolerance`
/// over its baseline, a p95 over its ceiling, and last a budget whose benchmark is gone.
pub fn breaches(budgets: &Budgets, timings: &[Timing]) -> Vec<String> {
    let mut found = Vec::new();
    for timing in timings {
        let Some(budget) = budgets.bench.get(&timing.name) else {
            found.push(format!("{}: no budget in budgets.toml", timing.name));
            continue;
        };
        let (p50, p95) = (millis(timing.p50), millis(timing.p95));
        let allowed = budget.p50_ms * (1.0 + budgets.tolerance);
        if p50 > allowed {
            found.push(format!(
                "{}: p50 {p50:.3} ms is more than {:.0} % over its baseline {:.3} ms",
                timing.name,
                budgets.tolerance * 100.0,
                budget.p50_ms
            ));
        }
        if let Some(ceiling) = budget.max_p95_ms
            && p95 > ceiling
        {
            found.push(format!(
                "{}: p95 {p95:.3} ms is over its ceiling of {ceiling} ms",
                timing.name
            ));
        }
    }
    for name in budgets.bench.keys() {
        if !timings.iter().any(|timing| &timing.name == name) {
            found.push(format!("{name}: a budget but no benchmark"));
        }
    }
    found
}

/// `budgets.toml` with this run as the new baseline: the tolerance and the ceilings stay.
pub fn baseline_toml(budgets: &Budgets, timings: &[Timing]) -> String {
    let mut text = format!(
        "# Performance budgets. Written by `cargo bench --bench budgets -- \
         --baseline`;\n# p50_ms and p95_ms are that run's, in milliseconds. A later run fails when \
         a p50 is more\n# than `tolerance` over its baseline or a p95 is over its `max_p95_ms` \
         ceiling.\ntolerance = {:?}\n",
        budgets.tolerance
    );
    for timing in timings {
        text.push_str(&format!(
            "\n[bench.\"{}\"]\np50_ms = {:.3}\np95_ms = {:.3}\n",
            timing.name,
            millis(timing.p50),
            millis(timing.p95)
        ));
        if let Some(ceiling) = budgets.bench.get(&timing.name).and_then(|b| b.max_p95_ms) {
            text.push_str(&format!("max_p95_ms = {ceiling:?}\n"));
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn timing(name: &str, p50: u64, p95: u64) -> Timing {
        Timing {
            name: name.to_string(),
            samples: 10,
            p50: ms(p50),
            p95: ms(p95),
        }
    }

    const BUDGETS: &str = "tolerance = 0.10\n\n\
        [bench.\"a\"]\np50_ms = 10.0\np95_ms = 12.0\nmax_p95_ms = 20.0\n\n\
        [bench.\"b\"]\np50_ms = 1.0\np95_ms = 2.0\n";

    #[test]
    fn a_percentile_is_the_nearest_rank_of_the_sorted_samples() {
        let sorted: Vec<Duration> = (1..=20).map(ms).collect();
        assert_eq!(percentile(&sorted, 0.50), ms(10));
        assert_eq!(percentile(&sorted, 0.95), ms(19));
        assert_eq!(percentile(&[ms(7)], 0.95), ms(7));
    }

    #[test]
    fn measure_times_each_call_after_the_warm_up() {
        let mut calls = 0;
        let timing = measure("count", 3, 10, || calls += 1);
        assert_eq!(calls, 13);
        assert_eq!((timing.name.as_str(), timing.samples), ("count", 10));
        assert!(timing.p50 <= timing.p95);
    }

    #[test]
    fn the_best_of_several_rounds_keeps_the_lowest_p50_and_p95() {
        let mut rounds = [timing("x", 9, 10), timing("x", 7, 14), timing("x", 8, 11)].into_iter();
        let best = best_of(3, || rounds.next().unwrap());
        assert_eq!(best, timing("x", 7, 10));
        assert!(rounds.next().is_none(), "every round ran");
    }

    #[test]
    fn measure_with_leaves_the_setup_out_of_the_timing() {
        let timing = measure_with("setup", 0, 5, || std::thread::sleep(ms(20)), |()| {});
        assert!(timing.p95 < ms(20), "{:?}", timing.p95);
    }

    #[test]
    fn a_run_within_its_budgets_breaches_nothing() {
        let budgets = parse_budgets(BUDGETS).unwrap();
        assert!(breaches(&budgets, &[timing("a", 11, 19), timing("b", 1, 5)]).is_empty());
    }

    #[test]
    fn a_slower_median_or_a_p95_over_its_ceiling_is_a_breach() {
        let budgets = parse_budgets(BUDGETS).unwrap();
        let found = breaches(&budgets, &[timing("a", 12, 21), timing("b", 1, 1)]);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].starts_with("a: p50 12.000 ms"), "{found:?}");
        assert!(
            found[1].starts_with("a: p95 21.000 ms") && found[1].contains("20"),
            "{found:?}"
        );
    }

    #[test]
    fn a_benchmark_without_a_budget_and_a_budget_without_a_benchmark_are_breaches() {
        let budgets = parse_budgets(BUDGETS).unwrap();
        let found = breaches(&budgets, &[timing("a", 10, 12), timing("c", 1, 1)]);
        assert_eq!(
            found,
            [
                "c: no budget in budgets.toml",
                "b: a budget but no benchmark"
            ]
        );
    }

    #[test]
    fn the_versioned_budgets_file_reads_and_keeps_the_specs_ceilings() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../benches/budgets.toml");
        let budgets = parse_budgets(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(budgets.tolerance, 0.10);
        let ceiling = |name: &str| budgets.bench[name].max_p95_ms;
        assert_eq!(
            [
                ceiling("snapshot/build_10x"),
                ceiling("snapshot/body_1x"),
                ceiling("privacy/redact_json_1x")
            ],
            [Some(50.0), Some(100.0), Some(5.0)]
        );
    }

    #[test]
    fn a_new_baseline_keeps_the_tolerance_and_the_ceilings() {
        let budgets = parse_budgets(BUDGETS).unwrap();
        let text = baseline_toml(&budgets, &[timing("a", 9, 11), timing("b", 2, 3)]);
        let again = parse_budgets(&text).unwrap();
        assert_eq!(again.tolerance, 0.10);
        assert_eq!(
            again.bench["a"],
            Budget {
                p50_ms: 9.0,
                p95_ms: 11.0,
                max_p95_ms: Some(20.0)
            }
        );
        assert_eq!(again.bench["b"].max_p95_ms, None);
    }
}
