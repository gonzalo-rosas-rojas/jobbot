//! The bodies of the fuzz targets under `fuzz/`: each takes the raw bytes a fuzzer makes, runs one
//! entry point that reads strangers' input, and panics when a rule breaks (a panic is what a fuzzer
//! reports). cargo-fuzz runs them on Linux; here the property tests of this crate and of the browser
//! crate run the same bodies, so the code a fuzzer would run is compiled and exercised on Windows.

use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::path::Path;
use std::sync::LazyLock;

use jobbot_app::actions::earliest_date;
use jobbot_app::inbox::{import, parse_items};
use jobbot_domain::answers::{AnswerBank, parse_answers};
use jobbot_domain::fixtures::NOW;
use jobbot_domain::form::{
    FieldInfo, FillValue, blocker_in_text, claims_not_hispanic, plan_fields,
};
use jobbot_domain::navigation::{NavPolicy, NavigableUrl, check};
use jobbot_domain::text::{host_of, link_base};
use jobbot_store::tracker::Tracker;
use scraper::Html;
use url::Host;

use crate::hostile::hidden_chars;

/// A made-up answer bank in the shape of the real one (no personal data).
static BANK: LazyLock<AnswerBank> = LazyLock::new(|| {
    parse_answers(
        r#"
consent_checkboxes = ["privacy policy", "terms of service"]

[[entry]]
patterns = ["first name"]
value = "Jane"

[[entry]]
patterns = ["email"]
value = "jane.doe@example.com"

[[entry]]
patterns = ["sponsorship", "visa"]
value = "Yes"
alternatives = ["Yes, I will need sponsorship"]

[[entry]]
patterns = ["country"]
value = "Chile"
alternatives = ["CL"]

[[entry]]
patterns = ["hispanic", "latino"]
value = "Hispanic or Latino"

[[entry]]
patterns = ["start date"]
value = "2026-11-01"

[[entry]]
patterns = ["years of experience"]
value = "8"

[[entry]]
patterns = ["skills"]
value = "Python, TypeScript"
alternatives = ["Python", "TypeScript"]
"#,
    )
    .expect("the made-up bank parses")
});

const TAGS: [&str; 5] = ["input", "select", "textarea", "button", "div"];
const KINDS: [&str; 10] = [
    "text", "email", "file", "checkbox", "radio", "date", "number", "combobox", "tel", "hidden",
];
const LABELS: [&str; 12] = [
    "First name",
    "Email",
    "Resume/CV",
    "Cover letter",
    "Do you need visa sponsorship?",
    "Country",
    "I agree to the privacy policy",
    "Send me marketing emails and SMS",
    "Are you Hispanic or Latino?",
    "Start date",
    "Years of experience",
    "",
];
const OPTIONS: [&str; 8] = [
    "",
    "Yes",
    "No",
    "Yes, I will need sponsorship",
    "Hispanic or Latino",
    "Not Hispanic or Latino",
    "Decline to self identify",
    "Chile",
];

/// The fields a page scan could return, read from fuzzer bytes: records split by `0xFF`; in each,
/// the first five bytes pick the tag, the type, the label, the option and the flags (required,
/// checked, group), and the rest is free text used when a pick falls past its list.
pub fn fields(data: &[u8]) -> Vec<FieldInfo> {
    data.split(|&byte| byte == 0xFF)
        .take(64)
        .enumerate()
        .map(|(idx, record)| {
            let byte = |i: usize| usize::from(record.get(i).copied().unwrap_or(0));
            let free = String::from_utf8_lossy(record.get(5..).unwrap_or_default()).into_owned();
            let label = LABELS
                .get(byte(2) % (LABELS.len() + 1))
                .map_or_else(|| free.clone(), |label| label.to_string());
            let option = OPTIONS
                .get(byte(3) % (OPTIONS.len() + 1))
                .map_or(free, |option| option.to_string());
            FieldInfo {
                idx,
                tag: TAGS[byte(0) % TAGS.len()].to_string(),
                kind: KINDS[byte(1) % KINDS.len()].to_string(),
                label,
                required: byte(4) & 1 == 1,
                option,
                name: format!("group-{}", (byte(4) >> 4) & 3),
                checked: byte(4) & 2 == 2,
            }
        })
        .collect()
}

/// Plans a generated form with the made-up bank: every action names a field of the page and fits
/// its kind, a field is acted on once, a box already ticked is never clicked again, a lone box is
/// ticked only when required, "Not Hispanic or Latino" is never ticked, and every box listed to
/// untick was ticked by the page.
pub fn form_plan(data: &[u8]) {
    let fields = fields(data);
    let plan = plan_fields(&fields, &BANK, Path::new("cv.pdf"));
    let field = |idx: usize| {
        fields
            .iter()
            .find(|field| field.idx == idx)
            .unwrap_or_else(|| panic!("the plan names a field the page does not have: {idx}"))
    };
    let mut acted = HashSet::new();
    for action in &plan.actions {
        let target = field(action.idx);
        assert!(
            acted.insert(action.idx),
            "field {} acted on twice",
            action.idx
        );
        match &action.value {
            FillValue::File(path) => {
                assert_eq!(target.kind, "file", "{target:?}");
                assert_eq!(path, Path::new("cv.pdf"));
            }
            FillValue::Check => {
                assert!(
                    matches!(target.kind.as_str(), "checkbox" | "radio"),
                    "{target:?}"
                );
                assert!(!target.checked, "a second click would undo it: {target:?}");
                if target.kind == "checkbox" && target.option.is_empty() {
                    assert!(
                        target.required,
                        "a lone box that is not required: {target:?}"
                    );
                }
                assert!(!claims_not_hispanic(&target.option), "{target:?}");
            }
            FillValue::Text(_) | FillValue::Select(_) => {
                assert!(
                    !matches!(target.kind.as_str(), "file" | "checkbox" | "radio"),
                    "{target:?}"
                );
            }
        }
    }
    for idx in &plan.kept {
        field(*idx);
    }
    for idx in &plan.to_untick {
        assert!(
            field(*idx).checked,
            "untick a box the page did not tick: {idx}"
        );
    }
    let unique: HashSet<&String> = plan.missing.iter().collect();
    assert_eq!(unique.len(), plan.missing.len(), "{:?}", plan.missing);
    let _ = blocker_in_text(&String::from_utf8_lossy(data));
}

/// One plain line: nothing hidden and no line break.
fn one_line(text: &str) -> bool {
    hidden_chars(text).is_empty() && !text.contains('\n')
}

/// Reads fuzzer bytes as an import file and imports it into an empty store: a file that is not one
/// is refused whole, and an import counts every element once, prints and stores one plain line per
/// text, and gives no task a link.
pub fn email_import(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    let Ok(items) = parse_items(&text) else {
        return;
    };
    let tracker = Tracker::in_memory().expect("an in-memory store opens");
    let report = import(&tracker, &items, *NOW).expect("only the store can fail an import");
    assert_eq!(
        report.imported + report.skipped + report.rejected + report.linked,
        items.len(),
        "{report:?}"
    );
    for line in &report.lines {
        assert!(one_line(line), "{line:?}");
    }
    for email in tracker.recent_emails(earliest_date()).expect("emails read") {
        for text in [
            &email.message_id,
            &email.thread_id,
            email.from_addr.expose(),
            &email.subject,
            &email.summary,
        ] {
            assert!(one_line(text), "{email:?}");
        }
    }
    for task in tracker.open_tasks().expect("tasks read") {
        assert!(one_line(&task.text), "{task:?}");
        assert_eq!(task.link, None, "{task:?}");
    }
}

/// The IPv4 ranges the browser must never be sent to (the rule, not the code).
fn refused_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    a == 127
        || a == 10
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 169 && b == 254)
        || (a == 100 && (64..=127).contains(&b))
        || a == 0
        || a >= 224
}

/// Reads fuzzer bytes as an apply link: what the strict check accepts is https to a public host
/// without credentials and reads back as itself, and the local test policy accepts everything the
/// strict one does. The other readers of links take it without panicking.
pub fn address(data: &[u8]) {
    let raw = String::from_utf8_lossy(data);
    let strict = check(&raw, NavPolicy::STRICT);
    let local = check(&raw, NavPolicy::LOCAL_TESTS);
    if let Ok(url) = &strict {
        assert_eq!(url.scheme(), "https", "{url}");
        assert!(
            url.username().is_empty() && url.password().is_none(),
            "{url}"
        );
        match url.host().expect("an accepted address has a host") {
            Host::Ipv4(ip) => assert!(!refused_v4(ip), "{url}"),
            Host::Ipv6(ip) => {
                assert!(!ip.is_loopback() && !ip.is_unspecified(), "{url}");
                if let Some(v4) = ip.to_ipv4_mapped() {
                    assert!(!refused_v4(v4), "{url}");
                }
            }
            Host::Domain(name) => {
                let name = name.trim_end_matches('.');
                assert!(name.contains('.') && !name.ends_with(".localhost"), "{url}");
            }
        }
        assert_eq!(local.as_ref().ok(), Some(url), "{url}");
        let navigable = NavigableUrl::parse(&raw, NavPolicy::STRICT).expect("it passed the check");
        assert_eq!(
            NavigableUrl::parse(navigable.as_str(), NavPolicy::STRICT),
            Ok(navigable.clone())
        );
    }
    let _ = host_of(&raw);
    let _ = link_base(&raw);
}

/// The elements and attributes a CV page body may hold: what markdown itself makes. Raw HTML in
/// the markdown must come out as text.
const CV_ELEMENTS: [&str; 20] = [
    "html",
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "li",
    "strong",
    "em",
    "a",
    "img",
    "br",
    "code",
    "pre",
    "blockquote",
    "hr",
];
const CV_ATTRIBUTES: [&str; 6] = ["href", "src", "alt", "title", "start", "class"];

/// Checks the page `markdown_to_html` made from `markdown`: its body holds only elements and
/// attributes markdown makes, so no script, style, frame or event handler came from raw HTML.
pub fn cv_page(markdown: &str, page: &str) {
    let body = page
        .split_once("<body>")
        .and_then(|(_, rest)| rest.rsplit_once("</body>"))
        .map(|(body, _)| body)
        .unwrap_or_else(|| panic!("the page has a body: {page:?}"));
    let fragment = Html::parse_fragment(body);
    for element in fragment
        .root_element()
        .descendants()
        .filter_map(scraper::ElementRef::wrap)
    {
        let name = element.value().name();
        assert!(
            CV_ELEMENTS.contains(&name),
            "<{name}> came from {markdown:?}"
        );
        for (attribute, _) in element.value().attrs() {
            assert!(
                CV_ATTRIBUTES.contains(&attribute),
                "{attribute} on <{name}> came from {markdown:?}"
            );
        }
    }
}
