//! jobbot's application layer. It depends on the domain only: what the use cases need from the
//! outside world (a browser, a PDF renderer, ...) is a port here, a trait the adapter crates
//! implement and the composition root (the `jobbot` binary) plugs in.

pub mod paths;
pub mod ports;
pub mod profile;
pub mod reporting {
    pub mod digest;
    pub mod status;
    pub mod today;
}
pub mod apply {
    pub mod attachment;
    pub mod cv;
    pub mod gate;
    pub mod handoff;
    pub mod pipeline;
    pub mod question_review;
    pub mod results;
}
pub mod actions;
pub mod claude;
pub mod discovery;
pub mod inbox;
pub mod learn {
    pub mod dossier;
}
pub mod freelance;
pub mod privacy;

/// Records counts on the current span, in the fields its `instrument` declared empty. Every use
/// case runs in a span named after it whose fields are counts and outcomes, never a key, a name or
/// a text; nothing is printed unless the binary installs a subscriber (`JOBBOT_TRACE`).
pub(crate) fn record_counts(counts: &[(&str, usize)]) {
    let span = tracing::Span::current();
    for &(field, count) in counts {
        span.record(field, count);
    }
}
