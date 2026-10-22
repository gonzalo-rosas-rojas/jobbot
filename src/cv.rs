//! CV tailoring (reorder skills and add one summary sentence) and PDF rendering.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::Context;
use playwright_rs::{Browser, PdfMargin, PdfOptions, Playwright};
use pulldown_cmark::{Event, Options, Parser};
use regex::Regex;

use crate::models::Job;
use crate::scoring::contains_term;

/// Opener of the placeholders (`⟦GitHub URL⟧`) that mark facts Gonza still has to supply.
const PLACEHOLDER: char = '⟦';
/// Separator between the original summary line and the sentence `tailor` appends.
const RELEVANT: &str = " Relevant to this role: ";

static SKILL_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\*\*(?P<label>[^*]+?):\*\*\s*(?P<items>.+)$").expect("valid skill-line regex")
});
static PARENTHESES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*\(.*?\)").expect("valid parentheses regex"));

#[derive(Debug, thiserror::Error)]
pub enum CvError {
    #[error("cv master still has ⟦ placeholders")]
    NotReady,
    #[error("tailored cv adds skills not in the master: {0}")]
    NewSkills(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Fails with `CvError::NotReady` while the CV master still contains a `⟦` placeholder.
/// `tailor` cannot fail (its contract returns a `String`), so callers run this first.
pub fn ensure_ready(master_md: &str) -> Result<(), CvError> {
    if master_md.contains(PLACEHOLDER) {
        Err(CvError::NotReady)
    } else {
        Ok(())
    }
}

/// The first three lines that still contain a placeholder, for the message shown to Gonza.
pub fn placeholder_lines(md: &str) -> Vec<String> {
    md.lines()
        .filter(|line| line.contains(PLACEHOLDER))
        .map(|line| line.trim().to_string())
        .take(3)
        .collect()
}

/// Splits a comma separated list, ignoring commas inside parentheses and dropping empty pieces.
pub fn split_top_level(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for ch in s.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if ch == ',' && depth == 0 {
            push_trimmed(&mut parts, &current);
            current.clear();
        } else {
            current.push(ch);
        }
    }
    push_trimmed(&mut parts, &current);
    parts
}

fn push_trimmed(parts: &mut Vec<String>, piece: &str) {
    let trimmed = piece.trim();
    if !trimmed.is_empty() {
        parts.push(trimmed.to_string());
    }
}

/// The skill name without its parenthetical detail: `RAG (embeddings, pgVector)` becomes `RAG`.
pub fn item_name(item: &str) -> String {
    PARENTHESES.replace_all(item, "").trim().to_string()
}

/// Line range (after the heading, up to the next `## ` heading) of the section called `title`.
fn section_range(lines: &[String], title: &str) -> Option<(usize, usize)> {
    let heading = format!("## {title}");
    let start = lines.iter().position(|line| line.trim() == heading)?;
    let end = (start + 1..lines.len())
        .find(|&i| lines[i].starts_with("## "))
        .unwrap_or(lines.len());
    Some((start + 1, end))
}

fn parse_skill_line(line: &str) -> Option<(String, Vec<String>)> {
    let caps = SKILL_LINE.captures(line.trim())?;
    Some((caps["label"].to_string(), split_top_level(&caps["items"])))
}

/// Skill lines of the `## TECHNICAL SKILLS` section as `(label, items)`, in file order.
pub fn parse_skills(md: &str) -> Vec<(String, Vec<String>)> {
    let lines: Vec<String> = md.lines().map(str::to_string).collect();
    let Some((start, end)) = section_range(&lines, "TECHNICAL SKILLS") else {
        return Vec::new();
    };
    lines[start..end]
        .iter()
        .filter_map(|line| parse_skill_line(line))
        .collect()
}

/// Reorders each skills line (items named in the job first) and appends
/// ` Relevant to this role: X, Y.` (at most 4 skills) to the first line of `## SUMMARY`.
/// It never adds a skill. Run `ensure_ready` first: `tailor` itself does not look for placeholders.
pub fn tailor(master_md: &str, job: &Job) -> String {
    let text = format!("{}\n{}", job.title, job.description).to_lowercase();
    let mut lines: Vec<String> = master_md.lines().map(str::to_string).collect();
    let mut matched: Vec<String> = Vec::new();
    if let Some((start, end)) = section_range(&lines, "TECHNICAL SKILLS") {
        for line in &mut lines[start..end] {
            let Some((label, items)) = parse_skill_line(line) else {
                continue;
            };
            let (hit, rest): (Vec<String>, Vec<String>) = items
                .into_iter()
                .partition(|item| contains_term(&text, &item_name(item)));
            matched.extend(hit.iter().map(|item| item_name(item)));
            let ordered: Vec<String> = hit.into_iter().chain(rest).collect();
            *line = format!("**{label}:** {}", ordered.join(", "));
        }
    }
    if !matched.is_empty()
        && let Some((start, end)) = section_range(&lines, "SUMMARY")
        && let Some(line) = lines[start..end]
            .iter_mut()
            .find(|line| !line.trim().is_empty())
    {
        let names: Vec<&str> = matched.iter().take(4).map(String::as_str).collect();
        *line = format!("{}{RELEVANT}{}.", line.trim_end(), names.join(", "));
    }
    let mut out = lines.join("\n");
    if master_md.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// The master with skills sorted and the appended sentence removed: two CVs with the same
/// canonical form differ only in skill order and in the summary sentence.
fn canonical(md: &str) -> Vec<String> {
    md.lines()
        .map(|line| {
            if let Some((label, mut items)) = parse_skill_line(line) {
                items.sort_by_key(|item| item.to_lowercase());
                format!("**{label}:** {}", items.join(", "))
            } else if let Some((before, _)) = line.split_once(RELEVANT) {
                before.trim_end().to_string()
            } else {
                line.trim_end().to_string()
            }
        })
        .collect()
}

/// Checks the invariant: the variant differs from the master only in skill order and in the
/// summary sentence, and that sentence names only skills the master already lists.
pub fn assert_no_new_skills(master_md: &str, variant_md: &str) -> Result<(), CvError> {
    ensure_ready(master_md)?;
    ensure_ready(variant_md)?;
    if canonical(master_md) != canonical(variant_md) {
        return Err(CvError::NewSkills(
            "lines other than skill order and the summary sentence changed".to_string(),
        ));
    }
    let known: HashSet<String> = parse_skills(master_md)
        .iter()
        .flat_map(|(_, items)| items)
        .map(|item| item_name(item).to_lowercase())
        .collect();
    for line in variant_md.lines() {
        let Some((_, tail)) = line.split_once(RELEVANT) else {
            continue;
        };
        for name in tail.trim_end_matches('.').split(',').map(str::trim) {
            if !known.contains(&name.to_lowercase()) {
                return Err(CvError::NewSkills(name.to_string()));
            }
        }
    }
    Ok(())
}

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

/// Turns CV markdown into a PDF file.
#[async_trait::async_trait]
pub trait Renderer: Send + Sync {
    async fn render(&self, md_text: &str, out_path: &Path) -> anyhow::Result<PathBuf>;
}

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
        let browser =
            playwright.chromium().launch().await.context(
                "launching Chromium (install it once with playwright_rs::install_browsers)",
            )?;
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
    use crate::testing::make_job_with;

    const MASTER: &str = "# JANE DOE
**Senior Full-Stack AI Engineer**

## SUMMARY
Full-Stack AI Engineer with 4+ years building production software.

## TECHNICAL SKILLS
**Languages:** TypeScript, Python, Golang, SQL
**AI / LLM:** OpenAI GPT-4, RAG (embeddings, pgVector, hybrid BM25 + RRF retrieval), MCP Server

## PROFESSIONAL EXPERIENCE
**Role**
• Did things";

    fn job() -> Job {
        make_job_with(|j| {
            j.title = "Applied AI Engineer".into();
            j.description = "We need Python and RAG experience.".into();
        })
    }

    #[test]
    fn split_top_level_respects_parentheses() {
        assert_eq!(
            split_top_level("A, B (x, y), C"),
            vec!["A", "B (x, y)", "C"]
        );
    }

    #[test]
    fn split_top_level_skips_empty_items() {
        assert_eq!(split_top_level("A,, B, "), vec!["A", "B"]);
    }

    #[test]
    fn item_name_strips_parenthetical_detail() {
        assert_eq!(item_name("RAG (embeddings, pgVector)"), "RAG");
        assert_eq!(item_name("  MCP Server "), "MCP Server");
    }

    #[test]
    fn parse_skills_keeps_labels_order_and_items() {
        let skills = parse_skills(MASTER);
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].0, "Languages");
        assert_eq!(skills[0].1, vec!["TypeScript", "Python", "Golang", "SQL"]);
        assert_eq!(skills[1].0, "AI / LLM");
        assert_eq!(skills[1].1.len(), 3);
    }

    #[test]
    fn tailor_reorders_and_adds_summary_sentence() {
        let out = tailor(MASTER, &job());
        assert!(out.contains("**Languages:** Python, TypeScript, Golang, SQL"));
        assert!(out.contains(
            "**AI / LLM:** RAG (embeddings, pgVector, hybrid BM25 + RRF retrieval), OpenAI GPT-4, MCP Server"
        ));
        assert!(out.contains("Relevant to this role: Python, RAG."));
        assert_no_new_skills(MASTER, &out).unwrap();
    }

    #[test]
    fn summary_sentence_names_at_most_four_skills() {
        let busy = make_job_with(|j| {
            j.title = "Engineer".into();
            j.description = "python typescript golang sql rag".into();
        });
        let out = tailor(MASTER, &busy);
        assert!(out.contains("Relevant to this role: TypeScript, Python, Golang, SQL."));
        assert!(!out.contains("SQL, RAG."));
    }

    #[test]
    fn tailor_without_matches_changes_nothing() {
        let chef = make_job_with(|j| {
            j.title = "Chef".into();
            j.description = "cooking".into();
        });
        assert_eq!(tailor(MASTER, &chef), MASTER);
        let with_newline = format!("{MASTER}\n");
        assert_eq!(tailor(&with_newline, &chef), with_newline);
    }

    #[test]
    fn added_skill_is_rejected() {
        let out = tailor(MASTER, &job()).replace("SQL", "SQL, Rust");
        assert!(matches!(
            assert_no_new_skills(MASTER, &out),
            Err(CvError::NewSkills(_))
        ));
    }

    #[test]
    fn summary_sentence_with_unknown_skill_is_rejected() {
        let out = tailor(MASTER, &job()).replace("Python, RAG.", "Python, Rust.");
        match assert_no_new_skills(MASTER, &out) {
            Err(CvError::NewSkills(name)) => assert_eq!(name, "Rust"),
            other => panic!("expected NewSkills, got {other:?}"),
        }
    }

    #[test]
    fn cv_with_placeholders_is_not_ready() {
        let master = format!("{MASTER}\n⟦GitHub URL⟧");
        assert!(matches!(ensure_ready(&master), Err(CvError::NotReady)));
        assert!(ensure_ready(MASTER).is_ok());
        assert_eq!(placeholder_lines(&master), vec!["⟦GitHub URL⟧"]);
    }

    #[test]
    fn assert_no_new_skills_rejects_placeholders() {
        let master = format!("{MASTER}\n⟦GitHub URL⟧");
        assert!(matches!(
            assert_no_new_skills(&master, &master),
            Err(CvError::NotReady)
        ));
    }

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
