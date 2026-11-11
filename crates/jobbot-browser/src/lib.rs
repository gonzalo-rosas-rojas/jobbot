//! jobbot's browser: the Playwright code behind three of the application's ports. `form` fills an
//! application form (`FormFiller`), `handoff` dry-runs a form Gonza sends himself and lists its
//! questions (`FormInspector`), `cv_render` prints the CV to PDF (`Renderer`). What to answer is
//! decided by the pure planner in the domain; this crate only drives the page.
//!
//! It is an adapter: it knows Chromium and nothing about the database, the job boards, Claude or
//! the panel (`tests/architecture.rs` of the application enforces that). Its Chromium tests are
//! `#[ignore]`d: `cargo test -p jobbot-browser -- --ignored --test-threads=1`.

pub mod cv_render;
mod egress;
pub mod form;
pub mod handoff;
mod launch;

#[cfg(test)]
mod test_support {
    use std::path::{Path, PathBuf};

    use jobbot_domain::answers::{AnswerBank, parse_answers};

    /// The workspace root: where `profile/`, `cv/` and `target/` are.
    pub fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("the crate sits two folders under the workspace root")
            .to_path_buf()
    }

    /// This crate's HTML fixtures: offline copies of real forms.
    pub fn fixtures_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
    }

    /// An answer bank file, read like the application reads `profile/answers.toml`.
    pub fn load_bank(path: &Path) -> anyhow::Result<AnswerBank> {
        Ok(parse_answers(&std::fs::read_to_string(path)?)?)
    }
}
