//! Live smoke check: hits every source in `profile/sources.toml` once and prints what came back.
//!
//! Manual only. It needs network access, so it is never part of `cargo test`.
//! Run from the project root: `cargo run --bin smoke`.

use jobbot::paths::Paths;
use jobbot::sources::{build_sources, load_sources_config};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let paths = Paths::new(std::env::current_dir()?);
    let cfg = load_sources_config(&paths.profile_dir.join("sources.toml"))?;
    let client = reqwest::Client::builder()
        .user_agent("jobbot/0.1")
        .build()?;

    for source in build_sources(&cfg, &client) {
        let result = source.fetch().await;
        println!(
            "{}: {} jobs, {} errors",
            source.name(),
            result.jobs.len(),
            result.errors.len()
        );
        for error in result.errors.iter().take(5) {
            println!("  ERR {error}");
        }
        if let Some(job) = result.jobs.first() {
            println!(
                "  e.g. {} | {} | {} | remote: {} | salary: {:?}..{:?}",
                job.company,
                job.title,
                job.location,
                job.remote,
                job.salary_min_monthly_usd,
                job.salary_max_monthly_usd
            );
        }
    }
    Ok(())
}
