//! Properties of the domain's rules over generated input: where the browser may go, the state
//! names, the pay reader and the HTML reader. The unit tests pin examples; these hold for whatever
//! proptest makes, and a failure prints the smallest input that breaks one.

use std::net::{Ipv4Addr, Ipv6Addr};

use jobbot_domain::navigation::{NavPolicy, NavigableUrl, check};
use jobbot_domain::scoring::parse_salary_monthly_usd;
use jobbot_domain::state::State;
use jobbot_domain::text::html_to_text;
use proptest::collection::vec;
use proptest::prelude::*;
use url::Host;

/// The IPv4 ranges the browser must never be sent to, written from the rule rather than from the
/// code: this machine, private networks, link-local, carrier-grade NAT, "this network", multicast
/// and the reserved block up to the broadcast address.
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

/// An address in one of the refused IPv4 ranges.
fn refused_address() -> impl Strategy<Value = Ipv4Addr> {
    prop_oneof![
        any::<[u8; 3]>().prop_map(|[b, c, d]| [127, b, c, d]),
        any::<[u8; 3]>().prop_map(|[b, c, d]| [10, b, c, d]),
        (16u8..=31, any::<[u8; 2]>()).prop_map(|(b, [c, d])| [172, b, c, d]),
        any::<[u8; 2]>().prop_map(|[c, d]| [192, 168, c, d]),
        any::<[u8; 2]>().prop_map(|[c, d]| [169, 254, c, d]),
        (64u8..=127, any::<[u8; 2]>()).prop_map(|(b, [c, d])| [100, b, c, d]),
        any::<[u8; 3]>().prop_map(|[b, c, d]| [0, b, c, d]),
        (224u8..=255, any::<[u8; 3]>()).prop_map(|(a, [b, c, d])| [a, b, c, d]),
    ]
    .prop_map(Ipv4Addr::from)
}

/// Every way an apply link can spell one IPv4 address: dotted, with the trailing dot the URL
/// parser drops, as one decimal or hex number, with an octal first part, and inside an IPv6
/// address that carries it (mapped, compatible, NAT64's well-known prefix, 6to4, and Teredo's
/// server or obfuscated client address next to a public other half).
fn spellings(ip: Ipv4Addr) -> Vec<String> {
    let [a, b, c, d] = ip.octets();
    let n = u32::from(ip);
    let (high, low) = (n >> 16, n & 0xffff);
    vec![
        format!("https://{ip}/"),
        format!("https://{ip}./"),
        format!("https://{n}/"),
        format!("https://0x{n:x}/"),
        format!("https://0{a:o}.{b}.{c}.{d}/"),
        format!("https://[::ffff:{ip}]/"),
        format!("https://[::{ip}]/"),
        format!("https://[64:ff9b::{ip}]/"),
        format!("https://[2002:{high:x}:{low:x}::1]/"),
        format!("https://[2001:0:{high:x}:{low:x}:0:0:b7f6:f7f7]/"),
        format!(
            "https://[2001:0:4136:e378:8000:63bf:{:x}:{:x}]/",
            high ^ 0xffff,
            low ^ 0xffff
        ),
    ]
}

fn host() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-z]{1,8}(\\.[a-z]{1,8}){0,3}\\.?",
        "[a-z]{1,8}\\.(local|internal|lan|home\\.arpa|localhost)",
        Just("localhost".to_string()),
        any::<[u8; 4]>().prop_map(|octets| Ipv4Addr::from(octets).to_string()),
        any::<u32>().prop_map(|n| n.to_string()),
        any::<u32>().prop_map(|n| format!("0x{n:x}")),
        any::<[u16; 8]>().prop_map(|segments| format!("[{}]", Ipv6Addr::from(segments))),
        any::<[u8; 4]>().prop_map(|octets| format!("[::ffff:{}]", Ipv4Addr::from(octets))),
    ]
}

/// Text shaped like an apply link: any scheme, maybe a user name and password, any kind of host,
/// maybe a port, a path, and whitespace around it.
fn address() -> impl Strategy<Value = String> {
    (
        prop_oneof![
            "https",
            "http",
            "HTTPS",
            "file",
            "ftp",
            "javascript",
            "data",
            "[a-z]{1,6}"
        ],
        prop_oneof![Just(String::new()), "[a-z]{1,5}@", "[a-z]{1,5}:[a-z]{0,5}@"],
        host(),
        prop_oneof![
            Just(String::new()),
            any::<u16>().prop_map(|port| format!(":{port}"))
        ],
        "(/[a-zA-Z0-9%._~?#=&-]{0,12}){0,3}",
        prop_oneof![Just(""), Just(" "), Just("\t\n")],
    )
        .prop_map(|(scheme, user, host, port, path, pad)| {
            format!("{pad}{scheme}://{user}{host}{port}{path}{pad}")
        })
}

fn is_loopback(host: &Host<&str>) -> bool {
    match host {
        Host::Ipv4(ip) => ip.is_loopback(),
        Host::Ipv6(ip) => {
            ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
        Host::Domain(name) => {
            let name = name.trim_end_matches('.');
            name == "localhost" || name.ends_with(".localhost")
        }
    }
}

proptest! {
    #[test]
    fn checking_any_text_never_panics(raw in any::<String>(), shaped in address()) {
        for text in [&raw, &shaped] {
            let _ = check(text, NavPolicy::STRICT);
            let _ = check(text, NavPolicy::LOCAL_TESTS);
        }
    }

    #[test]
    fn what_the_strict_check_lets_through_is_https_to_a_public_host_without_credentials(
        raw in address()
    ) {
        if let Ok(url) = check(&raw, NavPolicy::STRICT) {
            prop_assert_eq!(url.scheme(), "https");
            prop_assert!(url.username().is_empty() && url.password().is_none());
            let host = url.host().expect("an accepted address has a host");
            prop_assert!(!is_loopback(&host), "{}", url);
            match host {
                Host::Ipv4(ip) => prop_assert!(!refused_v4(ip), "{}", url),
                Host::Ipv6(ip) => {
                    if let Some(v4) = ip.to_ipv4_mapped() {
                        prop_assert!(!refused_v4(v4), "{}", url);
                    }
                }
                Host::Domain(name) => {
                    let name = name.trim_end_matches('.');
                    prop_assert!(name.contains('.'), "{}", url);
                    for local in [".local", ".internal", ".lan", ".home.arpa"] {
                        prop_assert!(!name.ends_with(local), "{}", url);
                    }
                }
            }
        }
    }

    #[test]
    fn an_address_in_a_refused_range_is_refused_however_it_is_spelled(ip in refused_address()) {
        for raw in spellings(ip) {
            prop_assert!(check(&raw, NavPolicy::STRICT).is_err(), "{} was let through", raw);
        }
    }

    #[test]
    fn the_local_test_policy_lets_through_only_this_machine_of_the_refused_ranges(
        ip in refused_address()
    ) {
        let raw = format!("http://{ip}:8080/form.html");
        prop_assert_eq!(check(&raw, NavPolicy::LOCAL_TESTS).is_ok(), ip.is_loopback(), "{}", raw);
    }

    #[test]
    fn the_local_test_policy_adds_only_this_machine_and_files_to_the_strict_one(raw in address()) {
        let strict = check(&raw, NavPolicy::STRICT);
        let local = check(&raw, NavPolicy::LOCAL_TESTS);
        if let Ok(url) = &strict {
            prop_assert_eq!(local.as_ref().ok(), Some(url));
        }
        if let (Err(_), Ok(url)) = (&strict, &local) {
            prop_assert!(
                url.scheme() == "file" || url.host().is_some_and(|host| is_loopback(&host)),
                "{}",
                url
            );
        }
    }

    #[test]
    fn a_navigable_address_reads_back_as_itself(raw in address()) {
        if let Ok(url) = NavigableUrl::parse(&raw, NavPolicy::STRICT) {
            prop_assert_eq!(NavigableUrl::parse(url.as_str(), NavPolicy::STRICT), Ok(url));
        }
    }

    #[test]
    fn only_the_names_of_the_states_are_read_as_states(
        text in prop_oneof![
            "[a-z_]{0,16}",
            "\\PC{0,16}",
            // A name with its case changed or a space around it is not the name.
            (
                proptest::sample::select(State::ALL.to_vec()),
                any::<bool>(),
                prop_oneof![Just(""), Just(" "), Just("\n")],
            )
                .prop_map(|(state, upper, pad)| {
                    let name = if upper { state.as_str().to_uppercase() } else { state.as_str().to_string() };
                    format!("{pad}{name}{pad}")
                }),
        ]
    ) {
        match State::parse(&text) {
            Some(state) => prop_assert_eq!(state.as_str(), text.as_str()),
            None => prop_assert!(State::ALL.iter().all(|state| state.as_str() != text)),
        }
    }
}

#[test]
fn every_state_reads_back_from_its_name_and_the_terminal_ones_have_no_move() {
    for state in State::ALL {
        assert_eq!(State::parse(state.as_str()), Some(state));
        let terminal = matches!(state, State::Accepted | State::Rejected | State::Withdrawn);
        assert_eq!(state.allowed_next().is_empty(), terminal, "{state}");
    }
}

fn amount() -> impl Strategy<Value = String> {
    prop_oneof![
        "[0-9]{1,7}",
        "[0-9]{1,3}(,[0-9]{3}){1,3}",
        "[0-9]{1,6}\\.[0-9]{1,3}",
        // Longer than any number an f64 holds.
        "[1-9][0-9]{310,330}",
    ]
}

/// A pay range somewhere in a posting: two amounts with or without `k`, a dash or "to", and what
/// may follow (an hourly marker, a year, "million"), between arbitrary text.
fn pay_text() -> impl Strategy<Value = String> {
    (
        "\\PC{0,12}",
        amount(),
        prop_oneof![Just(""), Just("k"), Just("K")],
        prop_oneof![Just(" - "), Just("-"), Just(" – "), Just("—"), Just(" to ")],
        prop_oneof![Just(""), Just("$"), Just("$ ")],
        amount(),
        prop_oneof![Just(""), Just("k")],
        prop_oneof![
            Just(""),
            Just("/hr"),
            Just(" per hour"),
            Just(" a year"),
            Just(" million")
        ],
        "\\PC{0,12}",
    )
        .prop_map(
            |(lead, low, low_k, dash, dollar, high, high_k, unit, rest)| {
                format!("{lead}${low}{low_k}{dash}{dollar}{high}{high_k}{unit}{rest}")
            },
        )
}

proptest! {
    #[test]
    fn a_pay_range_is_read_whole_and_in_order_or_not_at_all(
        text in prop_oneof![pay_text(), any::<String>()]
    ) {
        match parse_salary_monthly_usd(&text) {
            (None, None) => {}
            (Some(low), Some(high)) => {
                prop_assert!(low.is_finite() && high.is_finite(), "{} gave {} - {}", text, low, high);
                prop_assert!(0.0 <= low && low <= high, "{} gave {} - {}", text, low, high);
            }
            half => prop_assert!(false, "{} gave half a range: {:?}", text, half),
        }
    }
}

/// A piece of an HTML posting: words that show, words inside `script` or `style` that never do,
/// line breaks, and elements holding more pieces. Visible words start with `v`, hidden ones with
/// `h`, so the text can be checked word by word.
#[derive(Debug, Clone)]
enum Piece {
    Words(Vec<String>),
    Hidden(&'static str, Vec<String>),
    Break,
    Element(&'static str, Vec<Piece>),
}

impl Piece {
    fn html(&self, out: &mut String) {
        match self {
            Piece::Words(words) => out.push_str(&format!(" {} ", words.join(" "))),
            Piece::Hidden(tag, words) => {
                out.push_str(&format!("<{tag}>{}</{tag}>", words.join(" ")))
            }
            Piece::Break => out.push_str("<br>"),
            Piece::Element(tag, children) => {
                out.push_str(&format!("<{tag}>"));
                for child in children {
                    child.html(out);
                }
                out.push_str(&format!("</{tag}>"));
            }
        }
    }

    fn visible<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Piece::Words(words) => out.extend(words.iter().map(String::as_str)),
            Piece::Element(_, children) => children.iter().for_each(|child| child.visible(out)),
            Piece::Hidden(..) | Piece::Break => {}
        }
    }
}

fn piece() -> impl Strategy<Value = Piece> {
    let leaf = prop_oneof![
        vec("v[a-z]{1,6}", 1..4).prop_map(Piece::Words),
        (
            prop_oneof![Just("script"), Just("style")],
            vec("h[a-z]{1,6}", 1..3)
        )
            .prop_map(|(tag, words)| Piece::Hidden(tag, words)),
        Just(Piece::Break),
    ];
    leaf.prop_recursive(4, 48, 4, |inner| {
        (
            prop_oneof![
                Just("div"),
                Just("p"),
                Just("span"),
                Just("b"),
                Just("em"),
                Just("strong"),
                Just("li"),
                Just("ul"),
                Just("h2"),
                Just("section"),
            ],
            vec(inner, 0..4),
        )
            .prop_map(|(tag, children)| Piece::Element(tag, children))
    })
}

/// Each line of the text has something on it and single spaces between its words.
fn tidy(text: &str) -> bool {
    text.split('\n').all(|line| {
        !line.is_empty() && line == line.split_whitespace().collect::<Vec<_>>().join(" ")
    }) || text.is_empty()
}

proptest! {
    #[test]
    fn the_text_of_any_html_is_tidy_lines(
        html in prop_oneof![
            any::<String>(),
            "(<[a-z/!]{0,6}[^>]{0,10}>|&[a-z#0-9]{0,6};?|[a-z \\t\\r\\n]{0,6}|\\PC{0,3}){0,12}"
        ]
    ) {
        let text = html_to_text(&html);
        prop_assert!(tidy(&text), "{:?} gave {:?}", html, text);
    }

    #[test]
    fn the_visible_words_of_a_posting_come_out_in_order_and_nothing_else_does(
        pieces in vec(piece(), 1..6)
    ) {
        let mut html = String::new();
        let mut expected = Vec::new();
        for piece in &pieces {
            piece.html(&mut html);
            piece.visible(&mut expected);
        }
        let text = html_to_text(&html);
        prop_assert_eq!(text.split_whitespace().collect::<Vec<_>>(), expected, "{}", html);
        prop_assert!(tidy(&text), "{:?}", text);
    }
}
