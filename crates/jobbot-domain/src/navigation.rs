//! Where the browser may be sent. A job posting is text written by someone else: its apply link
//! decides which page the browser opens and where Gonza's data is typed, so the link is checked
//! before the browser moves and again where the browser lands (a redirect can change the host).

use std::net::{Ipv4Addr, Ipv6Addr};

use url::{Host, Url};

/// How strict a check is. The program always uses `STRICT`; the Chromium tests serve their pages
/// from this machine and use `LOCAL_TESTS`, which lets loopback hosts and `file:` pages through and
/// nothing else (private networks stay refused there too).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavPolicy {
    allow_local: bool,
}

impl NavPolicy {
    pub const STRICT: NavPolicy = NavPolicy { allow_local: false };
    pub const LOCAL_TESTS: NavPolicy = NavPolicy { allow_local: true };
}

/// Why an address was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NavRefusal {
    #[error("not an absolute web address")]
    NotAWebAddress,
    #[error("the scheme `{0}` is not allowed (https only)")]
    Scheme(String),
    #[error("the address carries a user name or a password")]
    Credentials,
    #[error("the address has no host")]
    NoHost,
    #[error("`{0}` is a local or private address")]
    PrivateHost(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    Public,
    Loopback,
    Private,
}

/// The address if the browser may open it under `policy`, otherwise the rule it breaks.
pub fn check(raw: &str, policy: NavPolicy) -> Result<Url, NavRefusal> {
    let url = Url::parse(raw.trim()).map_err(|_| NavRefusal::NotAWebAddress)?;
    if url.scheme() == "file" && policy.allow_local {
        return Ok(url);
    }
    if !matches!(url.scheme(), "http" | "https") {
        return Err(NavRefusal::Scheme(url.scheme().to_string()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(NavRefusal::Credentials);
    }
    let host = url.host().ok_or(NavRefusal::NoHost)?;
    let reach = reach_of(&host);
    if reach == Reach::Loopback && policy.allow_local {
        return Ok(url);
    }
    if reach != Reach::Public {
        return Err(NavRefusal::PrivateHost(host.to_string()));
    }
    if url.scheme() == "http" {
        return Err(NavRefusal::Scheme("http".to_string()));
    }
    Ok(url)
}

/// True when the browser may open `raw` under the strict policy.
pub fn is_navigable(raw: &str) -> bool {
    check(raw, NavPolicy::STRICT).is_ok()
}

fn reach_of(host: &Host<&str>) -> Reach {
    match host {
        Host::Ipv4(ip) => reach_v4(*ip),
        Host::Ipv6(ip) => match ip.to_ipv4_mapped() {
            Some(v4) => reach_v4(v4),
            None => reach_v6(*ip),
        },
        Host::Domain(name) => reach_name(name),
    }
}

fn reach_v4(ip: Ipv4Addr) -> Reach {
    let [first, second, ..] = ip.octets();
    if ip.is_loopback() {
        Reach::Loopback
    } else if ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || first == 0
        || first >= 240
        || (first == 100 && (64..=127).contains(&second))
    {
        Reach::Private
    } else {
        Reach::Public
    }
}

fn reach_v6(ip: Ipv6Addr) -> Reach {
    let first = ip.segments()[0];
    if ip.is_loopback() {
        Reach::Loopback
    } else if ip.is_unspecified()
        || ip.is_multicast()
        || (first & 0xfe00) == 0xfc00 // fc00::/7, unique local
        || (first & 0xffc0) == 0xfe80
    // fe80::/10, link local
    {
        Reach::Private
    } else {
        Reach::Public
    }
}

fn reach_name(name: &str) -> Reach {
    let name = name.trim_end_matches('.');
    if name == "localhost" || name.ends_with(".localhost") {
        Reach::Loopback
    } else if !name.contains('.')
        || [".local", ".internal", ".lan", ".home.arpa"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
    {
        Reach::Private
    } else {
        Reach::Public
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_https_addresses_pass() {
        for raw in [
            "https://boards.greenhouse.io/acme/jobs/1",
            "https://jobs.lever.co/acme/abc?lever-source=x",
            "https://acme.ashbyhq.com/xyz/application",
            "HTTPS://Example.COM:8443/a b",
            "https://bücher.example/stellen",
            "https://93.184.216.34/apply",
            "https://172.32.0.1/apply",
            "https://100.128.0.1/apply",
            "https://[2606:2800:220:1:248:1893:25c8:1946]/apply",
            "  https://example.com/apply  ",
        ] {
            assert!(is_navigable(raw), "{raw} should be allowed");
        }
    }

    #[test]
    fn empty_relative_and_scheme_less_links_are_refused() {
        for raw in [
            "",
            "   ",
            "/apply/1",
            "example.com/apply",
            "https://",
            "//example.com/x",
            "?q=1",
        ] {
            assert_eq!(
                check(raw, NavPolicy::STRICT).unwrap_err(),
                NavRefusal::NotAWebAddress,
                "{raw:?}"
            );
        }
    }

    #[test]
    fn other_schemes_and_plain_http_are_refused() {
        for (raw, scheme) in [
            ("javascript:alert(1)", "javascript"),
            ("data:text/html,<script>1</script>", "data"),
            ("file:///C:/Windows/win.ini", "file"),
            ("ftp://example.com/x", "ftp"),
            ("chrome://settings", "chrome"),
            ("about:blank", "about"),
            ("blob:https://example.com/uuid", "blob"),
            ("http://example.com/apply", "http"),
        ] {
            assert_eq!(
                check(raw, NavPolicy::STRICT).unwrap_err(),
                NavRefusal::Scheme(scheme.to_string()),
                "{raw}"
            );
        }
    }

    #[test]
    fn local_and_private_hosts_are_refused() {
        for raw in [
            "https://localhost/x",
            "https://LOCALHOST./x",
            "https://app.localhost/x",
            "https://127.0.0.1/x",
            "https://127.1/x",
            "https://2130706433/x",
            "https://0x7f.0.0.1/x",
            "https://0177.0.0.1/x",
            "https://[::1]/x",
            "https://[::ffff:127.0.0.1]/x",
            "https://10.1.2.3/x",
            "https://172.16.0.1/x",
            "https://172.31.255.255/x",
            "https://192.168.1.1/x",
            "https://169.254.169.254/latest/meta-data",
            "https://100.64.0.1/x",
            "https://0.0.0.0/x",
            "https://224.0.0.1/x",
            "https://[fe80::1]/x",
            "https://[fd00::1]/x",
            "https://intranet/x",
            "https://printer.local/x",
            "https://db.internal/x",
            "https://router.lan/x",
            "https://nas.home.arpa/x",
            "https://./x",
        ] {
            assert!(
                matches!(
                    check(raw, NavPolicy::STRICT),
                    Err(NavRefusal::PrivateHost(_))
                ),
                "{raw} should be refused as local or private"
            );
        }
    }

    #[test]
    fn credentials_in_the_address_are_refused() {
        for raw in [
            "https://user:pass@example.com/x",
            "https://user@example.com/x",
        ] {
            assert_eq!(
                check(raw, NavPolicy::STRICT).unwrap_err(),
                NavRefusal::Credentials,
                "{raw}"
            );
        }
    }

    #[test]
    fn the_test_policy_lets_the_local_machine_through_and_nothing_else() {
        let local = NavPolicy::LOCAL_TESTS;
        assert!(check("http://127.0.0.1:8080/form", local).is_ok());
        assert!(check("https://localhost/form", local).is_ok());
        assert!(check("file:///C:/fixtures/form.html", local).is_ok());
        assert!(check("http://127.0.0.1:8080/form", NavPolicy::STRICT).is_err());
        assert!(check("file:///C:/fixtures/form.html", NavPolicy::STRICT).is_err());
        for raw in [
            "https://10.0.0.1/x",
            "https://169.254.169.254/x",
            "javascript:alert(1)",
            "ftp://example.com/x",
        ] {
            assert!(
                check(raw, local).is_err(),
                "{raw} stays refused even in tests"
            );
        }
    }

    #[test]
    fn a_refusal_says_which_rule_it_broke() {
        assert_eq!(
            NavRefusal::PrivateHost("169.254.169.254".into()).to_string(),
            "`169.254.169.254` is a local or private address"
        );
        assert_eq!(
            NavRefusal::Scheme("ftp".into()).to_string(),
            "the scheme `ftp` is not allowed (https only)"
        );
    }
}
