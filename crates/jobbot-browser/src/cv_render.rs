//! The CV as a PDF: the markdown becomes a single-column HTML page, which headless Chromium prints
//! with the network cut off. Implements the application's `Renderer` port.

use std::path::{Path, PathBuf};

use anyhow::Context;
use jobbot_app::ports::Renderer;
use playwright_rs::{Browser, PdfMargin, PdfOptions, Playwright};
use pulldown_cmark::{Event, Options, Parser};

/// Converts the CV markdown to a single-column HTML page (standard fonts, no tables).
pub fn markdown_to_html(md: &str) -> String {
    let source = bullets_to_list_items(md);
    // Options::empty(): no tables, no footnotes, no smart punctuation. A single newline is a
    // line break (the CV master puts several lines in one paragraph).
    let parser = Parser::new_ext(&source, Options::empty()).map(|event| match event {
        Event::SoftBreak => Event::HardBreak,
        // Raw HTML in the markdown is shown as text, never run: the page it ends up in is rendered
        // by a browser that has a network.
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
        other => other,
    });
    let mut body = String::new();
    pulldown_cmark::html::push_html(&mut body, parser);
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>CV</title>\
         <style>{CSS}</style></head><body>\n{body}</body></html>\n"
    )
}

/// `• item` lines of the master become markdown list items so the page gets real `<ul>` lists.
fn bullets_to_list_items(md: &str) -> String {
    md.lines()
        .map(|line| match line.trim_start().strip_prefix("• ") {
            Some(rest) => format!("- {rest}"),
            None => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

const CSS: &str = "body { font-family: Arial, Helvetica, sans-serif; font-size: 10pt; line-height: 1.28; color: #111; margin: 0; }
h1 { font-size: 20pt; margin: 0 0 2px 0; }
h2 { font-size: 11.5pt; border-bottom: 1px solid #999; margin: 10px 0 4px 0; text-transform: uppercase; }
p { margin: 2px 0; }
ul { margin: 2px 0 5px 18px; padding: 0; }
li { margin: 0; }";

/// Renders with headless Chromium through `playwright-rs`: markdown -> HTML -> `page.pdf`.
/// Each call starts and stops its own driver, a fixed cost of about a second per CV.
pub struct BrowserRenderer;

#[async_trait::async_trait]
impl Renderer for BrowserRenderer {
    async fn render(&self, md_text: &str, out_path: &Path) -> anyhow::Result<PathBuf> {
        if let Some(parent) = out_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let html = markdown_to_html(md_text);
        let playwright = Playwright::launch()
            .await
            .context("starting the Playwright driver")?;
        let browser = playwright
            .chromium()
            .launch_with_options(crate::launch::launch_options())
            .await
            .context("launching Chromium (install it once with playwright_rs::install_browsers)")?;
        let printed = print_pdf(&browser, &html, out_path).await;
        let _ = browser.close().await;
        printed?;
        Ok(out_path.to_path_buf())
    }
}

async fn print_pdf(browser: &Browser, html: &str, out_path: &Path) -> anyhow::Result<()> {
    let page = browser.new_page().await?;
    // The CV is self-contained (standard fonts, no images): nothing in it has a reason to reach the
    // network, so every request the page makes is aborted. `set_content` itself needs none.
    page.route("**/*", |route| async move { route.abort(None).await })
        .await?;
    page.set_content(html, None).await?;
    let options = PdfOptions::builder()
        .path(out_path.to_path_buf())
        .format("Letter")
        .print_background(true)
        .margin(PdfMargin {
            top: Some("0.5in".to_string()),
            bottom: Some("0.5in".to_string()),
            left: Some("0.65in".to_string()),
            right: Some("0.65in".to_string()),
        })
        .build();
    page.pdf(options).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_html_is_single_column_without_tables() {
        let html = markdown_to_html("# NAME\n\n## SKILLS\na | b\n--|--\n1 | 2");
        assert!(html.contains("<h1>NAME</h1>"));
        assert!(!html.contains("<table"));
        assert!(html.contains("font-family: Arial"));
    }

    #[test]
    fn bullet_characters_become_list_items() {
        let html = markdown_to_html("**Role**\n• One\n• Two");
        assert_eq!(html.matches("<li>").count(), 2);
        assert!(html.contains("<li>One</li>"));
    }

    #[test]
    fn single_newlines_become_line_breaks() {
        let html = markdown_to_html("**Languages:** Python\n**Backend:** Rust");
        assert!(html.contains("<br />"));
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn render_pdf_produces_extractable_text() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("nested").join("cv.pdf");
        let md = "# JANE DOE\n**Senior Full-Stack AI Engineer**\n\n## SUMMARY\nBuilds agents.\n• One\n• Two";
        let written = BrowserRenderer.render(md, &out).await.unwrap();
        assert_eq!(written, out);
        let bytes = std::fs::read(&out).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
        let text = pdf_extract::extract_text_from_mem(&bytes).unwrap();
        assert!(text.contains("JANE DOE"), "extracted text was: {text}");
        assert!(
            text.contains("Builds agents."),
            "extracted text was: {text}"
        );
    }

    /// Page objects in a PDF: `/Type /Page` that is not the `/Type /Pages` tree node.
    fn pdf_page_count(bytes: &[u8]) -> usize {
        const MARK: &[u8] = b"/Type /Page";
        bytes
            .windows(MARK.len() + 1)
            .filter(|w| w.starts_with(MARK) && w[MARK.len()] != b's')
            .count()
    }

    #[test]
    fn page_count_ignores_the_pages_tree_node() {
        let pdf = b"<< /Type /Pages /Count 2 >> << /Type /Page /Parent 1 0 R >> << /Type /Page >>";
        assert_eq!(pdf_page_count(pdf), 2);
    }

    #[test]
    fn raw_html_in_the_markdown_is_escaped_not_run() {
        let html = markdown_to_html(
            "# NAME\n\n<script>alert(1)</script>\n\nA <img src=\"http://169.254.169.254/x\"> B\n\n<iframe src=\"https://evil.example\"></iframe>",
        );
        for tag in ["<script", "<img", "<iframe"] {
            assert!(!html.contains(tag), "{tag} survived: {html}");
        }
        assert!(
            html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "the text is kept, as text: {html}"
        );
    }

    #[test]
    fn ordinary_markdown_still_renders_the_same_page() {
        let html = markdown_to_html("# NAME\n\n## SKILLS\n**Languages:** Python\n• One\n• Two");
        assert!(html.contains("<h1>NAME</h1>"), "{html}");
        assert!(html.contains("<h2>SKILLS</h2>"), "{html}");
        assert!(
            html.contains("<strong>Languages:</strong> Python"),
            "{html}"
        );
        assert!(
            html.contains("<li>One</li>") && html.contains("<li>Two</li>"),
            "{html}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs chromium"]
    async fn rendering_the_cv_never_touches_the_network() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("cv.pdf");
        let md = format!("# NAME\n\n![tracking pixel]({}/pixel.png)\n", server.uri());
        BrowserRenderer.render(&md, &out).await.unwrap();
        assert!(out.exists(), "the CV is still rendered");
        assert!(
            pdf_extract::extract_text(&out).unwrap().contains("NAME"),
            "and it still has its text"
        );
        let seen = server.received_requests().await.unwrap_or_default();
        assert!(seen.is_empty(), "the CV page made requests: {seen:?}");
    }
}
