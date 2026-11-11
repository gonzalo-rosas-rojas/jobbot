//! Fuzzer bytes as a CV master, turned into the page Chromium prints (`fuzz::cv_page`).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let markdown = String::from_utf8_lossy(data);
    let page = jobbot_browser::cv_render::markdown_to_html(&markdown);
    jobbot_testkit::fuzz::cv_page(&markdown, &page);
});
