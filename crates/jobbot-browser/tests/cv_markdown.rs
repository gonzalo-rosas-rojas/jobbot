//! The CV page over generated markdown (the body of the `cv_markdown` fuzz target): whatever the
//! master holds, the page Chromium prints carries only what markdown makes; raw HTML is text.

use proptest::collection::vec;
use proptest::prelude::*;

use jobbot_browser::cv_render::markdown_to_html;
use jobbot_testkit::fuzz::cv_page;
use jobbot_testkit::hostile::MARKDOWN;

/// Lines of a CV master: headings, bullets, emphasis, links, images, code, and the hostile raw HTML.
fn master() -> impl Strategy<Value = String> {
    let raw: Vec<String> = MARKDOWN.iter().map(|(_, html)| html.to_string()).collect();
    let line = prop_oneof![
        "(#{1,3} )?[A-Za-z ]{0,20}",
        "• [A-Za-z ,]{0,20}",
        "- \\*\\*[A-Za-z]{1,8}:\\*\\* [A-Za-z, ]{0,20}",
        "\\[[a-z]{1,8}\\]\\((https|javascript|data):[a-z/.()]{0,16}\\)",
        "!\\[[a-z\" =]{0,12}\\]\\([a-z:/.]{0,16}\\)",
        "`[<>a-z/= ]{0,16}`",
        "    <[a-z]{1,6}>code",
        proptest::sample::select(raw),
        "<[a-zA-Z/!?]{1,8}( [a-z]{1,6}=[\"']?[a-z():]{0,10}[\"']?)?>",
        "\\PC{0,24}",
    ];
    vec(line, 0..16).prop_map(|lines| lines.join("\n"))
}

proptest! {
    #[test]
    fn the_cv_page_holds_only_what_markdown_makes(markdown in master()) {
        cv_page(&markdown, &markdown_to_html(&markdown));
    }
}

#[test]
fn every_hostile_line_of_the_corpus_comes_out_as_text() {
    for (name, html) in MARKDOWN {
        let markdown = format!("# Jane Doe\n\n{html}\n\nSome text {html} inline.\n");
        let page = markdown_to_html(&markdown);
        cv_page(&markdown, &page);
        assert!(page.contains("&lt;"), "{name}: {page}");
    }
}
