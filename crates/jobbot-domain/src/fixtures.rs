//! Pure test data shared by the domain's own tests and the application's: a fixed clock, job
//! builders and a sample CV. It is public rather than `cfg(test)` because the application's tests
//! import it too (through `jobbot::testing`); no production code calls it.

use std::sync::LazyLock;

use chrono::{DateTime, TimeZone, Utc};

use crate::models::Job;

/// The fixed "current time" used across tests: 2026-10-01T12:00:00Z.
pub static NOW: LazyLock<DateTime<Utc>> =
    LazyLock::new(|| Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap());

/// A strong, fully populated job: a senior AI role in Latin America.
pub fn make_job() -> Job {
    Job {
        source: "test".into(),
        external_id: "1".into(),
        company: "Acme AI".into(),
        title: "Senior AI Engineer".into(),
        url: "https://example.com/jobs/1".into(),
        apply_url: "https://example.com/jobs/1/apply".into(),
        location: "Remote - Latin America".into(),
        remote: true,
        description: "Build LLM agents with python and typescript.".into(),
        salary_min_monthly_usd: None,
        salary_max_monthly_usd: None,
        posted_at: None,
    }
}

/// `make_job()` with some fields changed: `make_job_with(|j| j.company = "Acme".into())`.
pub fn make_job_with(f: impl FnOnce(&mut Job)) -> Job {
    let mut job = make_job();
    f(&mut job);
    job
}

/// A small CV master: a contact line, skills, an entry for an employer on the example exclusion
/// list (`profile/exclusions.toml`), one more entry and a project. The phone and the email are
/// made up.
pub const SAMPLE_CV: &str = "# JANE DOE
**Senior Full-Stack AI Engineer**
jane.doe@example.com • +56 9 1234 5678 • Santiago, Chile • Remote

## SUMMARY
Full-Stack AI Engineer building LLM systems and workflow platforms for logistics operations.

## TECHNICAL SKILLS
**Languages:** Python, TypeScript, SQL
**AI / LLM:** Anthropic Claude, MCP Server, RAG, Qdrant

## PROFESSIONAL EXPERIENCE

**Fractional Operations Contractor**
Harbor Light Shipping and Logistics Center LLC — Logistics company, USA | Jun 2026 – Present
• Design and build internal workflow-automation and reporting software for a multi-office logistics operation

**Senior Full-Stack AI Engineer**
stackwave.io — AI Development Agency | Apr 2025 – Mar 2026
• Built an MCP (Model Context Protocol) server with dual transport (stdio + HTTP/SSE) connecting 21 AI tools to real-time business operations
• Designed a background job system with BullMQ: 21 job types, 9 concurrent workers, exponential backoff retry

## KEY PROJECTS

**Local LLM Lab** (personal project)
• Built a fully local RAG pipeline: embeddings, hybrid BM25 + RRF retrieval, citation-grounded answers, automated evaluation
";

/// Two real bullets of `SAMPLE_CV`, word for word.
pub const SAMPLE_BULLET_MCP: &str = "Built an MCP (Model Context Protocol) server with dual transport (stdio + HTTP/SSE) connecting 21 AI tools to real-time business operations";
pub const SAMPLE_BULLET_JOBS: &str = "Designed a background job system with BullMQ: 21 job types, 9 concurrent workers, exponential backoff retry";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_job_has_the_documented_defaults() {
        let job = make_job();
        assert_eq!(job.key(), "test:1");
        assert_eq!(job.company, "Acme AI");
        assert!(job.remote && job.posted_at.is_none());
    }

    #[test]
    fn make_job_with_changes_only_what_it_is_told() {
        let job = make_job_with(|j| j.company = "Other".into());
        assert_eq!(job.company, "Other");
        assert_eq!(job.title, make_job().title);
    }

    #[test]
    fn now_is_the_fixed_test_instant() {
        assert_eq!(NOW.to_rfc3339(), "2026-10-01T12:00:00+00:00");
    }
}
