//! Moved to `jobbot_app::apply::cv`. Re-exported here so every `crate::cv::...` path
//! keeps working.
//! The test left here prints the master CV with the real browser renderer (an adapter).

pub use jobbot_app::apply::cv::*;
pub use jobbot_browser::cv_render::{BrowserRenderer, markdown_to_html};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::make_job_with;

    /// Pages of a PDF: its `/Type /Page` objects (not the `/Type /Pages` tree node).
    fn pdf_page_count(bytes: &[u8]) -> usize {
        const MARK: &[u8] = b"/Type /Page";
        bytes
            .windows(MARK.len() + 1)
            .filter(|w| w.starts_with(MARK) && w[MARK.len()] != b's')
            .count()
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn the_master_cv_fits_two_pages() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("master.pdf");
        let md = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/cv/cv-master.md"))
            .unwrap();
        BrowserRenderer.render(&md, &out).await.unwrap();
        let pages = pdf_page_count(&std::fs::read(&out).unwrap());
        assert!(pages <= 2, "the master CV prints on {pages} pages");

        // A tailored copy adds the "Relevant to this role" sentence; the longest skills make the
        // longest sentence.
        let job = make_job_with(|j| {
            j.title = "Senior Full-Stack AI Engineer".to_string();
            j.description = "Next.js (App Router, RSC, Server Actions), React Hook Form, \
                Server-Sent Events, Webhook Systems, Anthropic Claude, Prompt Engineering"
                .to_string();
        });
        let tailored = out.with_file_name("tailored.pdf");
        BrowserRenderer
            .render(&tailor(&md, &job), &tailored)
            .await
            .unwrap();
        let pages = pdf_page_count(&std::fs::read(&tailored).unwrap());
        assert!(pages <= 2, "a tailored CV prints on {pages} pages");
    }
}
