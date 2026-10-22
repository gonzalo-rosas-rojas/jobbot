//! Small text helpers: name normalization, HTML to text, URL hosts, safe file keys
//! and whole-word matching.

use scraper::{ElementRef, Html, Node};
use url::Url;

const COMPANY_SUFFIXES: &[&str] = &[
    "llc",
    "inc",
    "ltd",
    "corp",
    "corporation",
    "co",
    "gmbh",
    "sa",
    "sas",
    "plc",
];

/// Tags that start and end a line of text.
const BLOCK_TAGS: &[&str] = &[
    "p", "div", "br", "li", "ul", "ol", "h1", "h2", "h3", "h4", "h5", "h6", "tr",
];

/// Tags whose content is code or styling, never visible text.
const SKIPPED_TAGS: &[&str] = &["script", "style"];

/// Lowercase ASCII words of a company name, without legal suffixes:
/// `"Harbor Light Shipping & Logistics Center, LLC"` becomes `"harbor light shipping and logistics center"`.
pub fn normalize(name: &str) -> String {
    let lowered = name.to_lowercase().replace('&', " and ");
    let spaced: String = lowered
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect();
    spaced
        .split_whitespace()
        .filter(|word| !COMPANY_SUFFIXES.contains(word))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Visible text of an HTML fragment, one line per block element, entities decoded.
pub fn html_to_text(html: &str) -> String {
    enum Work<'a> {
        Element(ElementRef<'a>),
        Text(&'a str),
        LineBreak,
    }

    let fragment = Html::parse_fragment(html);
    let mut raw = String::new();
    // An explicit stack instead of recursion: a posting with thousands of nested
    // tags must not overflow the thread stack.
    let mut stack = vec![Work::Element(fragment.root_element())];
    while let Some(work) = stack.pop() {
        match work {
            Work::Text(text) => raw.push_str(text),
            Work::LineBreak => raw.push('\n'),
            Work::Element(element) => {
                let name = element.value().name();
                if SKIPPED_TAGS.contains(&name) {
                    continue;
                }
                if BLOCK_TAGS.contains(&name) {
                    raw.push('\n');
                    stack.push(Work::LineBreak); // popped after the children pushed below
                }
                let children: Vec<Work> = element
                    .children()
                    .filter_map(|child| match child.value() {
                        Node::Text(text) => Some(Work::Text(text)),
                        Node::Element(_) => ElementRef::wrap(child).map(Work::Element),
                        _ => None,
                    })
                    .collect();
                stack.extend(children.into_iter().rev());
            }
        }
    }
    raw.lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Decodes HTML entities in a string that is itself escaped HTML
/// (Greenhouse sends `&lt;p&gt;Hi&lt;/p&gt;`): `"&lt;b&gt;"` becomes `"<b>"`.
pub fn decode_entities(text: &str) -> String {
    html_escape::decode_html_entities(text).into_owned()
}

/// Lowercase host of a URL, `""` when the text is not an absolute URL with a host.
pub fn host_of(url: &str) -> String {
    Url::parse(url.trim())
        .ok()
        .and_then(|parsed| {
            parsed
                .host_str()
                .map(|host| host.trim_end_matches('.').to_string())
        })
        .unwrap_or_default()
}

/// The part of a posting link that identifies the posting: lowercase, without the query, the
/// fragment, trailing slashes or a final `/application` (the form page of Ashby and Lever). Two
/// links to the same posting share it. `""` when the text is not an absolute link.
pub fn link_base(url: &str) -> String {
    let url = url.trim();
    if !url.contains("://") {
        return String::new();
    }
    let end = url.find(['?', '#']).unwrap_or(url.len());
    let base = url[..end].trim_end_matches('/');
    let base = base.strip_suffix("/application").unwrap_or(base);
    base.trim_end_matches('/').to_ascii_lowercase()
}

/// Makes a job key safe to use as a file name: every run of characters other than
/// letters, digits, `_`, `.` and `-` becomes one `_`.
pub fn safe_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut after_replacement = false;
    for c in key.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
            out.push(c);
            after_replacement = false;
        } else if !after_replacement {
            out.push('_');
            after_replacement = true;
        }
    }
    out
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}

/// Whole-word search. `text` and `term` must already be lowercase. A match needs a
/// non-alphanumeric character (or an edge) on both sides.
pub fn contains_word_lc(text: &str, term: &str) -> bool {
    if term.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(offset) = text[from..].find(term) {
        let start = from + offset;
        let end = start + term.len();
        let boundary_before = text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !is_word_char(c));
        let boundary_after = text[end..].chars().next().is_none_or(|c| !is_word_char(c));
        if boundary_before && boundary_after {
            return true;
        }
        // Resume one character after this start, so overlapping candidates are seen.
        from = start + text[start..].chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// Case-insensitive `contains_word_lc`.
pub fn contains_word(text: &str, term: &str) -> bool {
    contains_word_lc(&text.to_lowercase(), &term.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_suffix_and_ampersand() {
        assert_eq!(
            normalize("Harbor Light Shipping & Logistics Center, LLC"),
            "harbor light shipping and logistics center"
        );
    }

    #[test]
    fn normalize_of_empty_text_is_empty() {
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("  ,. "), "");
    }

    #[test]
    fn html_to_text_blocks_and_entities() {
        let html = "<p>Hello &amp; welcome</p><ul><li>One</li><li>Two</li></ul>";
        assert_eq!(html_to_text(html), "Hello & welcome\nOne\nTwo");
    }

    #[test]
    fn html_to_text_plain_and_empty_input() {
        assert_eq!(html_to_text(""), "");
        assert_eq!(html_to_text("  plain   text  "), "plain text");
        assert_eq!(html_to_text("a<br>b"), "a\nb");
    }

    #[test]
    fn html_to_text_skips_script_and_style() {
        assert_eq!(
            html_to_text("<style>p{}</style><p>Hi</p><script>alert(1)</script>"),
            "Hi"
        );
    }

    #[test]
    fn html_to_text_survives_very_deep_nesting() {
        let html = format!("{}x{}", "<div>".repeat(5000), "</div>".repeat(5000));
        assert_eq!(html_to_text(&html), "x");
    }

    #[test]
    fn decode_entities_unescapes_escaped_html() {
        assert_eq!(
            decode_entities("&lt;p&gt;Fish &amp; chips&lt;/p&gt;"),
            "<p>Fish & chips</p>"
        );
        assert_eq!(
            html_to_text(&decode_entities("&lt;p&gt;Fish &amp;amp; chips&lt;/p&gt;")),
            "Fish & chips"
        );
    }

    #[test]
    fn host_of_lowercases_and_handles_bad_input() {
        assert_eq!(
            host_of("https://Careers.Harborlight-Shipping.com/job/1?x=1"),
            "careers.harborlight-shipping.com"
        );
        assert_eq!(host_of("https://user:pw@example.com:8080/a"), "example.com");
        assert_eq!(
            host_of("https://harborlight-shipping.com./x"),
            "harborlight-shipping.com"
        );
        assert_eq!(host_of("not a url"), "");
        assert_eq!(host_of(""), "");
        assert_eq!(host_of("mailto:jobs@example.com"), "");
    }

    #[test]
    fn link_base_drops_what_does_not_identify_a_posting() {
        assert_eq!(
            link_base(" https://Jobs.AshbyHQ.com/globex/ABC/application/?utm_source=x#form "),
            "https://jobs.ashbyhq.com/globex/abc"
        );
        assert_eq!(
            link_base("https://example.com/jobs/1/"),
            "https://example.com/jobs/1"
        );
        assert_eq!(link_base("example.com/jobs/1"), "", "not an absolute link");
        assert_eq!(link_base(""), "");
    }

    #[test]
    fn safe_key_replaces_unsafe_runs() {
        assert_eq!(
            safe_key("greenhouse:anthropic/123"),
            "greenhouse_anthropic_123"
        );
        assert_eq!(safe_key("a: /b"), "a_b");
        assert_eq!(safe_key("keep-this_1.0"), "keep-this_1.0");
    }

    #[test]
    fn contains_word_needs_boundaries() {
        assert!(contains_word(
            "the Tidewater Systems team",
            "tidewater systems"
        ));
        assert!(contains_word("freight forwarding.", "freight forwarding"));
        assert!(!contains_word("freight forwarder", "freight forwarding"));
        assert!(!contains_word("the best team", "est"));
        assert!(!contains_word("anything", ""));
        assert!(contains_word("x aa", "aa"));
        assert!(!contains_word("aaa", "aa"));
    }
}
