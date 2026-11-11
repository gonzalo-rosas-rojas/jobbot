//! Core data types shared by every module: a normalized job posting and the
//! result of fetching one source.

use chrono::{DateTime, Utc};

/// One job posting, normalized across sources.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Job {
    pub source: String,
    pub external_id: String,
    pub company: String,
    pub title: String,
    pub url: String,
    pub apply_url: String,
    pub location: String,
    pub remote: bool,
    pub description: String,
    pub salary_min_monthly_usd: Option<f64>,
    pub salary_max_monthly_usd: Option<f64>,
    pub posted_at: Option<DateTime<Utc>>,
}

impl Job {
    /// Stable identity of a posting: `"{source}:{external_id}"`.
    pub fn key(&self) -> String {
        format!("{}:{}", self.source, self.external_id)
    }
}

/// What a source returns: the jobs it could read plus one error per failure.
#[derive(Debug, Default)]
pub struct FetchResult {
    pub jobs: Vec<Job>,
    pub errors: Vec<FetchError>,
}

/// What went wrong with one part of a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchErrorKind {
    /// The request or the page failed: the network, an error status, a page that does not parse,
    /// the run's time for the boards being over.
    Failed,
    /// The host asked to be left alone (or is still resting from an earlier refusal) for this long.
    RateLimited { wait_secs: u64 },
    /// Postings dropped because they lacked an id, a title or a URL.
    Skipped { count: usize },
}

/// One failure of a source: the source, the part of it (`slug`: a board, a query, a feed), what
/// kind of failure and the reason as text. It reads as the line the run report has always printed,
/// `<source>:<slug>: <detail>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchError {
    pub source: String,
    pub slug: String,
    pub kind: FetchErrorKind,
    pub detail: String,
}

impl FetchError {
    pub fn failed(source: &str, slug: &str, detail: impl Into<String>) -> FetchError {
        FetchError::new(source, slug, FetchErrorKind::Failed, detail.into())
    }

    pub fn rate_limited(
        source: &str,
        slug: &str,
        wait_secs: u64,
        detail: impl Into<String>,
    ) -> FetchError {
        let kind = FetchErrorKind::RateLimited { wait_secs };
        FetchError::new(source, slug, kind, detail.into())
    }

    /// `count` postings of `slug` dropped for lacking an id, a title or a URL.
    pub fn skipped(source: &str, slug: &str, count: usize) -> FetchError {
        let detail = format!("skipped {count} postings missing id, title or url");
        FetchError::new(source, slug, FetchErrorKind::Skipped { count }, detail)
    }

    fn new(source: &str, slug: &str, kind: FetchErrorKind, detail: String) -> FetchError {
        FetchError {
            source: source.to_string(),
            slug: slug.to_string(),
            kind,
            detail,
        }
    }

    pub fn is_rate_limited(&self) -> bool {
        matches!(self.kind, FetchErrorKind::RateLimited { .. })
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.source, self.slug, self.detail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_key_joins_source_and_external_id() {
        let job = Job {
            source: "greenhouse".into(),
            external_id: "anthropic/123".into(),
            ..Job::default()
        };
        assert_eq!(job.key(), "greenhouse:anthropic/123");
    }

    #[test]
    fn fetch_result_starts_empty() {
        let result = FetchResult::default();
        assert!(result.jobs.is_empty() && result.errors.is_empty());
    }

    #[test]
    fn a_fetch_error_reads_as_the_line_of_the_run_report() {
        let failed = FetchError::failed("greenhouse", "acme", "HTTP status server error (500)");
        assert_eq!(failed.kind, FetchErrorKind::Failed);
        assert_eq!(
            failed.to_string(),
            "greenhouse:acme: HTTP status server error (500)"
        );

        let limited = FetchError::rate_limited(
            "himalayas",
            "rust",
            120,
            "himalayas.app:443 asked us to slow down; not asking again for 120 s",
        );
        assert_eq!(limited.kind, FetchErrorKind::RateLimited { wait_secs: 120 });
        assert_eq!(
            limited.to_string(),
            "himalayas:rust: himalayas.app:443 asked us to slow down; not asking again for 120 s"
        );

        let skipped = FetchError::skipped("lever", "acme", 3);
        assert_eq!(skipped.kind, FetchErrorKind::Skipped { count: 3 });
        assert_eq!(
            skipped.to_string(),
            "lever:acme: skipped 3 postings missing id, title or url"
        );
        assert!(
            limited.is_rate_limited() && !failed.is_rate_limited() && !skipped.is_rate_limited()
        );
    }
}
