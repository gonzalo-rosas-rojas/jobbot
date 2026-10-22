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

/// What a source returns: the jobs it could read plus one message per failure.
#[derive(Debug, Default)]
pub struct FetchResult {
    pub jobs: Vec<Job>,
    pub errors: Vec<String>,
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
}
