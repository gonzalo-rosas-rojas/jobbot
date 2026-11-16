//! The page and the files embedded with it: the page with a fresh nonce and the content policy that
//! goes with it, and the fonts served under `/a/<launch-id>/`.

use super::{Guard, Reply, random_hex};
use anyhow::Result;

const PAGE: &str = include_str!("../page.html");

/// What the binary serves besides the page: (name, MIME type, bytes), each embedded at build time.
/// The route compares a name against this table and nothing else, so the table is the whole of what
/// can be fetched; the MIME type is written here, never guessed from the name. Every row has a
/// line in `THIRD_PARTY_NOTICES.md` (rule in `assets/README.md`).
pub const ASSETS: &[(&str, &str, &[u8])] = &[
    // The four families of the page, subset and sanitised (`panel-src/assets/fonts/VENDORED.md`);
    // `panel-src/tools/assets-sync.mjs` copies them here after checking each sha256.
    (
        "barlow-condensed-700.woff2",
        "font/woff2",
        include_bytes!("../assets/barlow-condensed-700.woff2"),
    ),
    (
        "geist-mono-400.woff2",
        "font/woff2",
        include_bytes!("../assets/geist-mono-400.woff2"),
    ),
    (
        "instrument-serif-400italic.woff2",
        "font/woff2",
        include_bytes!("../assets/instrument-serif-400italic.woff2"),
    ),
    (
        "inter-400.woff2",
        "font/woff2",
        include_bytes!("../assets/inter-400.woff2"),
    ),
    (
        "inter-600.woff2",
        "font/woff2",
        include_bytes!("../assets/inter-600.woff2"),
    ),
];

/// Where the page finds its assets this launch: the `__ASSETS__` placeholder becomes this.
fn asset_prefix(launch_id: &str) -> String {
    format!("/a/{launch_id}")
}

/// The page, with a fresh nonce in its `<script>` and `<style>` tags and in the policy that allows
/// only those two. No `unsafe-inline`: text injected into the page cannot run. Fonts and images
/// may only come from this server (`'self'`: the assets under `/a/`), screenshots still arrive as
/// `blob:`; `require-trusted-types-for 'script'` makes a string handed to `innerHTML`, `eval` or a
/// script's `src` throw instead of run, should a library ever try. `X-Frame-Options` repeats
/// `frame-ancestors 'none'` for browsers that only know the old header.
pub(super) fn page_reply(guard: &Guard) -> Result<Reply> {
    let nonce = random_hex(16)?;
    let policy = format!(
        "default-src 'none'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}'; \
         connect-src 'self'; img-src 'self' blob:; font-src 'self'; base-uri 'none'; \
         form-action 'none'; frame-ancestors 'none'; require-trusted-types-for 'script'"
    );
    Ok(Reply {
        status: 200,
        content_type: "text/html; charset=utf-8",
        headers: vec![
            ("Content-Security-Policy", policy),
            ("X-Frame-Options", "DENY".to_string()),
        ],
        body: fill_page(PAGE, &nonce, &guard.launch_id).into_bytes(),
    })
}

/// The page's two placeholders filled: `__NONCE__` with this load's nonce, `__ASSETS__` with this
/// launch's asset prefix (so a stylesheet's `url("__ASSETS__/inter-400.woff2")` points at `/a/<id>/inter-400.woff2`).
fn fill_page(template: &str, nonce: &str, launch_id: &str) -> String {
    template
        .replace("__NONCE__", nonce)
        .replace("__ASSETS__", &asset_prefix(launch_id))
}

/// `GET /a/<launch-id>/<name>`: one row of `ASSETS`, or a 404. `rest` is what follows `/a/`. The
/// launch id must be this launch's and the name must equal a row's, so neither `..` nor a sub-path
/// nor a query can reach anything else; the browser may keep the reply for a year because the next
/// launch serves the same file under another path.
pub(super) fn asset(guard: &Guard, method: &str, rest: &str, query: Option<&str>) -> Reply {
    let found = rest
        .split_once('/')
        .filter(|(launch, _)| *launch == guard.launch_id && query.is_none())
        .and_then(|(_, name)| ASSETS.iter().find(|(known, _, _)| *known == name));
    let Some((_, mime, bytes)) = found else {
        return Reply::message(404, "No existe");
    };
    if method != "GET" {
        return Reply::message(405, "Método no permitido");
    }
    Reply {
        status: 200,
        content_type: mime,
        headers: vec![(
            "Cache-Control",
            "public, max-age=31536000, immutable".to_string(),
        )],
        body: bytes.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::panel;
    use crate::http::support_tests::{LAUNCH, TOKEN, guard, request, text, with, without};
    use crate::http::{TOKEN_HEADER, handle};

    #[test]
    fn the_page_needs_the_right_host_but_no_token_and_gets_a_fresh_nonce() {
        let (p, _d) = panel();
        let req = without(request("GET", "/", ""), "x-panel-token");
        let page = handle(&p, &guard(), &req);
        assert_eq!(page.status, 200);
        assert!(page.content_type.starts_with("text/html"));
        let html = text(&page);
        assert!(!html.contains("__NONCE__"));
        let csp = page.header("content-security-policy").unwrap().to_string();
        let nonce = csp
            .split("'nonce-")
            .nth(1)
            .unwrap()
            .split('\'')
            .next()
            .unwrap()
            .to_string();
        assert!(
            html.contains(&format!("<script nonce=\"{nonce}\"")),
            "{html}"
        );
        assert!(csp.contains("default-src 'none'") && csp.contains("connect-src 'self'"));
        assert!(
            !csp.contains("unsafe-inline") && !csp.contains("unsafe-eval"),
            "{csp}"
        );
        assert!(!html.contains(TOKEN), "the token is never inside the page");
        let again = handle(&p, &guard(), &req);
        assert_ne!(
            again.header("content-security-policy"),
            page.header("content-security-policy"),
            "every load gets its own nonce"
        );
    }

    #[test]
    fn the_page_policy_allows_own_fonts_and_images_and_refuses_every_frame() {
        let (p, _d) = panel();
        let page = handle(&p, &guard(), &request("GET", "/", ""));
        let csp = page.header("content-security-policy").unwrap();
        let nonce = csp
            .split("'nonce-")
            .nth(1)
            .unwrap()
            .split('\'')
            .next()
            .unwrap();
        assert_eq!(
            csp,
            format!(
                "default-src 'none'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}'; connect-src 'self'; img-src 'self' blob:; font-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; require-trusted-types-for 'script'"
            )
        );
        assert_eq!(page.header("x-frame-options"), Some("DENY"));
        let api = handle(&p, &guard(), &request("GET", "/api/snapshot", ""));
        assert!(
            api.header("content-security-policy").is_none(),
            "the policy belongs to the page"
        );
    }

    #[test]
    fn the_page_gets_the_asset_prefix_the_way_it_gets_the_nonce() {
        let filled = fill_page(
            "<style nonce=\"__NONCE__\">.a{background:url(\"__ASSETS__/inter-400.woff2\")}</style><script nonce=\"__NONCE__\">var a=\"__ASSETS__\"+\"/inter-400.woff2\";</script>",
            "n0nce",
            LAUNCH,
        );
        assert_eq!(
            filled,
            "<style nonce=\"n0nce\">.a{background:url(\"/a/f00ba4f00ba4f00b/inter-400.woff2\")}</style><script nonce=\"n0nce\">var a=\"/a/f00ba4f00ba4f00b\"+\"/inter-400.woff2\";</script>"
        );
        assert_eq!(asset_prefix(LAUNCH), "/a/f00ba4f00ba4f00b");

        let (p, _d) = panel();
        let html = text(&handle(&p, &guard(), &request("GET", "/", "")));
        assert!(
            !html.contains("__ASSETS__") && !html.contains("__NONCE__"),
            "no placeholder survives"
        );
    }

    #[test]
    fn an_asset_is_served_under_this_launch_with_its_type_and_a_long_cache() {
        let (p, _d) = panel();
        let req = without(
            request("GET", &format!("/a/{LAUNCH}/inter-400.woff2"), ""),
            TOKEN_HEADER,
        );
        let reply = handle(&p, &guard(), &req);
        assert_eq!(reply.status, 200);
        assert_eq!(reply.content_type, "font/woff2");
        assert_eq!(reply.body, include_bytes!("../assets/inter-400.woff2"));
        assert_eq!(
            reply.header("cache-control"),
            Some("public, max-age=31536000, immutable"),
            "a new start is a new path, so the browser may keep it for good"
        );
        assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
        assert_eq!(
            reply.header("cross-origin-resource-policy"),
            Some("same-origin")
        );
        assert_eq!(reply.header("referrer-policy"), Some("no-referrer"));
        assert!(
            reply.header("content-security-policy").is_none(),
            "an image is not a document"
        );

        // Every row of the table is reachable, and only with its own name.
        for (name, mime, bytes) in ASSETS {
            let got = handle(
                &p,
                &guard(),
                &request("GET", &format!("/a/{LAUNCH}/{name}"), ""),
            );
            assert_eq!((got.status, got.content_type), (200, *mime), "{name}");
            assert_eq!(got.body, *bytes, "{name}");
            assert!(
                !name.contains('/') && !name.contains("..") && !mime.is_empty(),
                "{name}: a plain file name with an explicit type"
            );
        }
    }

    /// The four families of the page (criterion A4), as `panel-src/tools/assets-sync.mjs` copies them.
    const FONTS: [&str; 5] = [
        "barlow-condensed-700.woff2",
        "geist-mono-400.woff2",
        "instrument-serif-400italic.woff2",
        "inter-400.woff2",
        "inter-600.woff2",
    ];

    #[test]
    fn each_font_is_served_as_woff2_under_this_launch_with_the_long_cache() {
        let (p, _d) = panel();
        let get = |target: &str| {
            handle(
                &p,
                &guard(),
                &without(request("GET", target, ""), TOKEN_HEADER),
            )
        };
        let mut total = 0;
        for name in FONTS {
            let reply = get(&format!("/a/{LAUNCH}/{name}"));
            assert_eq!(reply.status, 200, "{name}");
            assert_eq!(reply.content_type, "font/woff2", "{name}");
            assert_eq!(
                reply.header("cache-control"),
                Some("public, max-age=31536000, immutable"),
                "{name}"
            );
            assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
            assert!(
                reply.body.starts_with(b"wOF2"),
                "{name} is a WOFF2 file, not a page the server made up"
            );
            total += reply.body.len();
            // Only under this launch and with exactly this name.
            assert_eq!(get(&format!("/a/0000000000000000/{name}")).status, 404);
            assert_eq!(
                get(&format!("/a/{LAUNCH}/{}", name.to_uppercase())).status,
                404
            );
            assert_eq!(
                get(&format!("/a/{LAUNCH}/{}", name.replace(".woff2", ".woff"))).status,
                404
            );
            assert_eq!(get(&format!("/a/{LAUNCH}/{name}?v=1")).status, 404);
            assert_eq!(get(&format!("/a/{LAUNCH}/sub/{name}")).status, 404);
        }
        assert!(
            total <= 120 * 1024,
            "the five fonts weigh {total} bytes; the ceiling is 120 KB (R8 rule 10)"
        );
        // And nothing else is a font: the table holds exactly these five.
        let fonts_in_table = ASSETS.iter().filter(|(_, mime, _)| *mime == "font/woff2");
        assert_eq!(fonts_in_table.count(), FONTS.len());
    }

    #[test]
    fn every_asset_has_a_row_with_its_hash_in_the_notices_and_no_row_lacks_its_file() {
        // The rule of `assets/README.md`: no row, no asset. The notices file is read at build time,
        // so a new `ASSETS` entry without its line fails here before it reaches a release.
        let notices = include_str!("../../../../THIRD_PARTY_NOTICES.md");
        let rows: Vec<&str> = notices
            .lines()
            .filter(|line| line.starts_with("| `"))
            .collect();
        for (name, mime, bytes) in ASSETS {
            let row = rows
                .iter()
                .find(|row| row.starts_with(&format!("| `{name}` |")))
                .unwrap_or_else(|| panic!("{name} has no row in THIRD_PARTY_NOTICES.md"));
            assert!(row.contains(mime), "{name}: the row names its type {mime}");
            // The sha256 is the last cell of the row, between backticks.
            let hash_cell = row
                .trim_end()
                .trim_end_matches('|')
                .rsplit('|')
                .next()
                .unwrap_or_else(|| panic!("{name}: the row has no cells"));
            let hash = hash_cell.trim().trim_matches('`');
            assert!(
                hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
                "{name}: {hash:?} is not a sha256"
            );
            assert!(!bytes.is_empty(), "{name} is empty");
        }
        let in_table: Vec<&str> = ASSETS.iter().map(|(name, _, _)| *name).collect();
        for row in rows
            .iter()
            .filter(|row| row.contains("image/") || row.contains("font/"))
        {
            let name = row.trim_start_matches("| `").split('`').next().unwrap();
            assert!(
                in_table.contains(&name),
                "{name} has a row in THIRD_PARTY_NOTICES.md but no entry in ASSETS"
            );
        }
        // Names are unique, or the first would shadow the second.
        let mut sorted = in_table.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), in_table.len(), "duplicate asset names");
    }

    #[test]
    fn anything_but_a_listed_asset_under_this_launch_is_404() {
        let (p, _d) = panel();
        let status = |target: &str| {
            handle(
                &p,
                &guard(),
                &without(request("GET", target, ""), TOKEN_HEADER),
            )
            .status
        };
        for target in [
            &format!("/a/{LAUNCH}/nope.woff2"),
            &format!("/a/{LAUNCH}/INTER-400.WOFF2"),
            &format!("/a/{LAUNCH}/inter-400.woff2/"),
            &format!("/a/{LAUNCH}/../inter-400.woff2"),
            &format!("/a/{LAUNCH}/%2e%2e/inter-400.woff2"),
            &format!("/a/{LAUNCH}/sub/inter-400.woff2"),
            &format!("/a/{LAUNCH}/..%2finter-400.woff2"),
            &format!("/a/{LAUNCH}/inter-400.woff2?x=1"),
            &format!("/a/{LAUNCH}/"),
            &format!("/a/{LAUNCH}"),
            "/a/0000000000000000/inter-400.woff2",
            "/a/inter-400.woff2",
            "/a/",
            "/a",
            "/assets/inter-400.woff2",
        ] {
            assert_eq!(status(target), 404, "{target}");
        }
        // `sub/../inter-400.woff2` is not a way out: the address is normalised before it is routed, so it IS
        // `inter-400.woff2` (a browser does the same before it ever sends it), and `/a/<id>/../x` lands on `/a/x`.
        assert_eq!(status(&format!("/a/{LAUNCH}/sub/../inter-400.woff2")), 200);
        let host = with(
            request("GET", &format!("/a/{LAUNCH}/inter-400.woff2"), ""),
            "host",
            "evil.example:4321",
        );
        assert_eq!(
            handle(&p, &guard(), &host).status,
            403,
            "the Host guard comes first"
        );
        let post = request("POST", &format!("/a/{LAUNCH}/inter-400.woff2"), "{}");
        assert_eq!(handle(&p, &guard(), &post).status, 405);
    }
}
