//! CV tailoring: reorder the skills and add one summary sentence, never a skill the master lacks.
//! Printing the PDF is the `Renderer` port's job (the browser adapter implements it).

use std::collections::HashSet;
use std::sync::LazyLock;

use jobbot_domain::models::Job;
use jobbot_domain::scoring::contains_term;
use regex::Regex;

pub use crate::ports::Renderer;

/// Opener of the placeholders (`⟦GitHub URL⟧`) that mark facts Gonza still has to supply.
const PLACEHOLDER: char = '⟦';
/// Separator between the original summary line and the sentence `tailor` appends.
const RELEVANT: &str = " Relevant to this role: ";

static SKILL_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\*\*(?P<label>[^*]+?):\*\*\s*(?P<items>.+)$").expect("valid skill-line regex")
});
static PARENTHESES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*\(.*?\)").expect("valid parentheses regex"));
static SEEKING_REMOTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"Seeking remote roles in (?P<what>[^.]+)\.").expect("valid seeking regex")
});

/// The master as sent to a job in a place Gonza would move to (`extra_open_relocation`): the item
/// `Remote` of the contact line becomes `Open to relocating to {place}`, and "Seeking remote roles
/// in X." becomes "Seeking roles in X in {place}, on site or remote." (his words, 2026-10-21: "mas
/// que dispuesto a mudarme", "presencial o remoto me sirve"). Nothing else changes, and a master
/// without those words comes back as it is. Tailor the result, and check it against the result.
pub fn relocating_to(master_md: &str, place: &str) -> String {
    let mut out = master_md
        .lines()
        .map(|line| {
            if line.contains('@') && line.contains(" • ") {
                line.split(" • ")
                    .map(|item| {
                        if item.trim() == "Remote" {
                            format!("Open to relocating to {place}")
                        } else {
                            item.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" • ")
            } else {
                SEEKING_REMOTE
                    .replace(line, |caps: &regex::Captures| {
                        format!(
                            "Seeking roles in {} in {place}, on site or remote.",
                            &caps["what"]
                        )
                    })
                    .into_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if master_md.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[derive(Debug, thiserror::Error)]
pub enum CvError {
    #[error("cv master still has ⟦ placeholders")]
    NotReady,
    #[error("tailored cv adds skills not in the master: {0}")]
    NewSkills(String),
    #[error(transparent)]
    Store(#[from] crate::ports::store::StoreError),
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

#[cfg(test)]
mod tests {
    use jobbot_domain::fixtures::make_job_with;

    use super::*;

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
    fn relocating_changes_the_remote_item_and_the_seeking_sentence_only() {
        let master = "# JANE DOE
me@example.com • +56 9 0000 0000 • Santiago, Chile • Remote • github.com/someone

## SUMMARY
Builds things. Seeking remote roles in applied AI and full-stack AI engineering.

## TECHNICAL SKILLS
**Languages:** Python, Remote Sensing
";
        let moved = relocating_to(master, "Australia");

        assert!(moved.contains(
            "me@example.com • +56 9 0000 0000 • Santiago, Chile • Open to relocating to Australia • github.com/someone"
        ), "{moved}");
        assert!(moved.contains(
            "Builds things. Seeking roles in applied AI and full-stack AI engineering in Australia, on site or remote."
        ), "{moved}");
        assert!(
            moved.contains("**Languages:** Python, Remote Sensing"),
            "skills are never touched"
        );
        assert!(moved.ends_with('\n'));
        assert_eq!(
            relocating_to(MASTER, "Australia"),
            MASTER,
            "a master without those words comes back as it is"
        );
        // The tailored variant is checked against the relocated master, and passes.
        let variant = tailor(&moved, &job());
        assert_no_new_skills(&moved, &variant).unwrap();
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
}
