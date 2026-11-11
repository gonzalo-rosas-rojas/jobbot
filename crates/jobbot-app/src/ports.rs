//! The ports: what the use cases ask of the outside world. Each one belongs to the side that uses
//! it, so an adapter depends on this crate and never the other way round.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use jobbot_domain::answers::AnswerBank;
use jobbot_domain::form::{SubmitMode, SubmitResult};
use jobbot_domain::handoff::FormInspection;
use jobbot_domain::models::{FetchResult, Job};

use crate::apply::gate::GateApproval;

mod claude;
pub mod store;

pub use claude::{AccountSeen, ClaudeError, ClaudeReply, ClaudeRequest, ClaudeRunner, ShapeError};

/// How far `FormFiller::submit` goes: a dry run (fill, screenshot, never click), assist (fill and
/// leave the form for a person to send), or live (send), which takes the gate's approval.
#[derive(Debug)]
pub enum FillMode {
    DryRun,
    Assist,
    Live(GateApproval),
}

impl FillMode {
    /// The browser's mode for this fill.
    pub fn submit_mode(&self) -> SubmitMode {
        match self {
            FillMode::DryRun => SubmitMode::DryRun,
            FillMode::Assist => SubmitMode::Assist,
            FillMode::Live(_) => SubmitMode::Live,
        }
    }
}

/// Fills the application form of `job` and, according to `mode`, submits it or waits for Gonza
/// to. Anything it cannot answer or must not bypass comes back as `NeedsGonza` with a reason;
/// `Submitted` means the page confirmed the submission.
#[async_trait]
pub trait FormFiller: Send + Sync {
    async fn submit(
        &self,
        job: &Job,
        cv_pdf: &Path,
        bank: &AnswerBank,
        mode: FillMode,
        shots_dir: &Path,
    ) -> anyhow::Result<SubmitResult>;
}

/// Opens a form like a dry run (fills what the bank answers, takes the screenshot, never clicks
/// anything) and also lists every question the form asked, which `FormFiller` does not report.
#[async_trait]
pub trait FormInspector: Send + Sync {
    async fn inspect(
        &self,
        job: &Job,
        cv_pdf: &Path,
        bank: &AnswerBank,
        shots_dir: &Path,
    ) -> anyhow::Result<FormInspection>;
}

/// Turns CV markdown into a PDF file.
#[async_trait]
pub trait Renderer: Send + Sync {
    async fn render(&self, md_text: &str, out_path: &Path) -> anyhow::Result<PathBuf>;
}

/// A place that publishes job postings (a Greenhouse board, the RemoteOK feed, ...).
///
/// `fetch` never panics and never returns an error. Every failure (HTTP status, HTML instead of
/// JSON, network down) becomes one line `"<source>:<slug>: <reason>"` in `FetchResult::errors`,
/// so a broken board cannot stop the other boards or the other sources.
#[async_trait]
pub trait Source: Send + Sync {
    /// Short stable name used in job keys and error lines, for example `"greenhouse"`.
    fn name(&self) -> &str;
    async fn fetch(&self) -> FetchResult;
}

/// What the source says about a posting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenStatus {
    /// The source still lists the posting.
    Open,
    /// The source says the posting is gone.
    Closed,
    /// No clear answer. Treat it as open.
    Unknown,
}

/// Anything that can say whether a posting is still open: the network in production, a fake in
/// the submit tests.
#[async_trait]
pub trait OpenChecker: Send + Sync {
    async fn check(&self, job: &Job) -> OpenStatus;
}
