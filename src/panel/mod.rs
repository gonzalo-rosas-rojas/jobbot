//! The local panel: a small web page served on this PC only, drawn from the same
//! SQLite files the commands use. `snapshot` builds the data, `http` holds every guard and route as
//! plain functions, `server` moves bytes. Nothing here starts discovery, a browser or Claude. The
//! one question the panel puts to Windows (is the scheduled task there, and when does it fire?) is
//! `scheduler`'s, read-only, with a deadline and off the request path; `privacy` keeps a whole
//! e-mail address and the user's folder out of what the page receives.

pub mod http;
pub mod jobs;
pub mod privacy;
pub mod scheduler;
pub mod server;
pub mod snapshot;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use crate::paths::Paths;
use crate::prep::PrepStore;
use crate::tracker::Tracker;
use scheduler::{TaskAnswer, TaskQuery, TaskWatch};

/// Where the panel gets the time. `jobbot panel` passes "start time plus time elapsed", like the
/// window watch, starting from `main`'s reading of the real clock; tests pass a stopped one.
pub type Clock = Box<dyn Fn() -> DateTime<Utc> + Send>;

pub struct Panel {
    pub paths: Paths,
    pub tracker: Tracker,
    pub store: PrepStore,
    clock: Clock,
    /// What Windows says about the scheduled task: nothing, until `with_task_query`.
    tasks: TaskWatch,
}

impl Panel {
    pub fn new(paths: Paths, tracker: Tracker, store: PrepStore, clock: Clock) -> Panel {
        Panel {
            paths,
            tracker,
            store,
            clock,
            tasks: TaskWatch::off(),
        }
    }

    /// The panel asks `query` about the scheduled task (once now, in the background, and again
    /// every few minutes); `jobbot panel` hands it the real Task Scheduler. Without it the
    /// snapshot says "no data".
    pub fn with_task_query(mut self, query: impl TaskQuery + 'static) -> Panel {
        self.tasks = TaskWatch::asking(query);
        self
    }

    /// The last answer about the scheduled task, at once; `None` is "no data", not "no task".
    pub fn scheduled_task(&self) -> Option<TaskAnswer> {
        self.tasks.current(self.now())
    }

    /// Opens `data/jobbot.db` and `data/prep.db`, creating `data/` and the files when missing.
    pub fn open(paths: Paths, clock: Clock) -> Result<Panel> {
        std::fs::create_dir_all(&paths.data_dir)
            .with_context(|| format!("creando {}", paths.data_dir.display()))?;
        let tracker = Tracker::open(&paths.data_dir.join("jobbot.db"))?;
        let store = PrepStore::open(&paths.data_dir.join("prep.db"))?;
        Ok(Panel::new(paths, tracker, store, clock))
    }

    pub fn now(&self) -> DateTime<Utc> {
        (self.clock)()
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use std::collections::BTreeMap;

    use super::*;
    use crate::testing::{NOW, make_job_with, real_paths};
    use crate::tracker::State;

    /// A panel over in-memory databases with the clock stopped at `NOW`. Its `data/` and `STOP`
    /// live in the returned temp folder; its `profile/` is the shipped one (`schedule.toml`,
    /// `claude.toml`).
    pub fn panel() -> (Panel, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = Paths::new(dir.path());
        paths.profile_dir = real_paths().profile_dir;
        let panel = Panel::new(
            paths,
            Tracker::in_memory().unwrap(),
            PrepStore::in_memory().unwrap(),
            Box::new(|| *NOW),
        );
        (panel, dir)
    }

    /// Adds job `id` ("Co <id>", "Role <id>") with `score`, creates its application and walks it
    /// through `states` (each with the note "falta email"). Returns the key.
    pub fn add(panel: &Panel, id: &str, score: f64, states: &[State]) -> String {
        let job = make_job_with(|j| {
            j.external_id = id.into();
            j.company = format!("Co {id}");
            j.title = format!("Role {id}");
        });
        panel
            .tracker
            .add_job(&job, score, &BTreeMap::new(), *NOW)
            .unwrap();
        panel.tracker.create_application(&job.key(), *NOW).unwrap();
        for state in states {
            panel
                .tracker
                .transition(&job.key(), *state, Some("falta email"), *NOW)
                .unwrap();
        }
        job.key()
    }
}

#[cfg(test)]
mod page_tests {
    use super::http::ASSETS;
    use regex::Regex;
    use std::collections::{BTreeMap, BTreeSet};

    const PAGE: &str = include_str!("page.html");

    // These tests and `FORBIDDEN` / `THEME_OPENERS` in `panel-src/guard.mjs` say the same thing from
    // two sides: the build refuses a page that breaks them, and Rust refuses a page that got here
    // some other way. Keep the two in step.

    /// Ways to turn data into markup or code. The page draws text with `textContent` and nothing else.
    #[test]
    fn the_page_never_turns_data_into_markup() {
        for forbidden in [
            "innerHTML",
            "outerHTML",
            "insertAdjacentHTML",
            "document.write",
            "eval(",
            "new Function",
            "setAttribute(\"style\"",
            " style=\"",
            "javascript:",
            "splitText",
            "srcdoc",
            "setHTMLUnsafe",
            "parseHTMLUnsafe",
        ] {
            assert!(
                !PAGE.contains(forbidden),
                "the page must not contain {forbidden:?}"
            );
        }
    }

    #[test]
    fn the_page_loads_nothing_from_outside() {
        for needle in [
            "src=\"http",
            "href=\"http",
            "url(http",
            "url(data:",
            "src=\"data:",
            "@import",
            "<link",
            "<iframe",
        ] {
            assert!(
                !PAGE.contains(needle),
                "the page must not contain {needle:?}"
            );
        }
    }

    /// The only network call the page makes is `fetch` to its own origin; nothing else can leave it.
    /// The policy (`connect-src 'self'`) would stop the rest, these words are the second line.
    #[test]
    fn the_page_has_no_way_to_reach_out_but_fetch_to_its_own_origin() {
        for needle in [
            "sendBeacon",
            "WebSocket",
            "EventSource",
            "XMLHttpRequest",
            "importScripts",
            "SharedWorker",
            "new Worker(",
            "new Image(",
            "import(",
        ] {
            assert!(
                !PAGE.contains(needle),
                "the page must not contain {needle:?}"
            );
        }
        let absolute = Regex::new(r#"\bfetch\(\s*["'`](?:https?:)?//"#).unwrap();
        assert!(
            !absolute.is_match(PAGE),
            "a fetch of an absolute address leaves this PC"
        );
        let eval_in_timer = Regex::new(r#"\bset(?:Timeout|Interval)\(\s*["'`]"#).unwrap();
        assert!(
            !eval_in_timer.is_match(PAGE),
            "a timer given a string is eval"
        );
        let loads_code = Regex::new(
            r#"\bcreateElement(?:NS)?\(\s*(?:[^,)]*,\s*)?["'](?:iframe|frame|script|object|embed)["']"#,
        )
        .unwrap();
        assert!(
            !loads_code.is_match(PAGE),
            "an element that loads or runs something"
        );
    }

    /// The only files the page may name are its own, under the `__ASSETS__` prefix the server
    /// fills in (`/a/<launch-id>`), and in-page references (`url(#id)`). Fonts go through an
    /// `@font-face` inside the nonce'd `<style>` (never a `<link>`, which the test above forbids):
    /// `url("__ASSETS__/name.woff2")`. The checks above only catch `url(http`; this one also
    /// catches `url("https://...")`, `url(//host/...)` and a relative path the server never serves.
    #[test]
    fn every_url_in_the_page_is_an_own_asset_or_a_fragment() {
        for (at, _) in PAGE.match_indices("url(") {
            let rest = PAGE[at + 4..].trim_start_matches(['"', '\'', ' ']);
            assert!(
                rest.starts_with("__ASSETS__/") || rest.starts_with('#'),
                "url( must point at __ASSETS__/<name> or #fragment, found {:?}",
                &PAGE[at..(at + 40).min(PAGE.len())]
            );
        }
    }

    /// What the page asks for and what the binary serves are the same list: a name the page asks
    /// for that the table lacks is a 404 and a font that never loads; a row nobody asks for is
    /// weight in every build.
    #[test]
    fn the_page_asks_for_exactly_the_assets_the_binary_serves() {
        let ask = Regex::new(r#"__ASSETS__/([A-Za-z0-9._-]+)"#).unwrap();
        let asked: BTreeSet<&str> = ask
            .captures_iter(PAGE)
            .map(|c| c.get(1).unwrap().as_str())
            .collect();
        let served: BTreeSet<&str> = ASSETS.iter().map(|(name, _, _)| *name).collect();
        assert_eq!(
            asked, served,
            "left: what the page asks for; right: what ASSETS serves"
        );
        // Each one is a font the page declares in an `@font-face`.
        for name in &asked {
            assert!(name.ends_with(".woff2"), "{name} is not a font");
            assert!(
                PAGE.contains(&format!("url(\"__ASSETS__/{name}\") format(\"woff2\")")),
                "{name} is not declared in an @font-face"
            );
        }
        // And the placeholder appears nowhere else: not in a script, not in an attribute.
        for (at, _) in PAGE.match_indices("__ASSETS__") {
            assert!(
                PAGE[..at].ends_with("url(\""),
                "__ASSETS__ outside url(\"...\"): {:?}",
                &PAGE[at.saturating_sub(20)..(at + 40).min(PAGE.len())]
            );
        }
    }

    #[test]
    fn the_tab_title_does_not_name_the_job_search() {
        // Window titles and browser history can be seen by others (a shared screen, a shared
        // browser): the tab is just "Panel" and never says what the panel holds.
        let title = PAGE
            .split("<title>")
            .nth(1)
            .and_then(|rest| rest.split("</title>").next())
            .expect("the page has a title")
            .to_lowercase();
        assert_eq!(title, "panel");
        for word in ["jobbot", "empleo", "postula", "job", "trabajo"] {
            assert!(
                !title.contains(word),
                "the title {title:?} must not say {word:?}"
            );
        }
    }

    /// The page is one HTML file with exactly two scripts (the early theme/language, then the
    /// bundle) and one stylesheet; every tag carries the nonce the server fills in, and the marker
    /// appears nowhere else.
    #[test]
    fn every_script_and_style_tag_carries_the_nonce() {
        assert!(PAGE.contains("<script nonce=\"__NONCE__\">"));
        assert!(PAGE.contains("<style nonce=\"__NONCE__\">"));
        assert_eq!(PAGE.matches("<script").count(), 2, "two scripts");
        assert_eq!(PAGE.matches("<style").count(), 1, "one stylesheet");
        assert_eq!(
            PAGE.matches("<script").count(),
            PAGE.matches("<script nonce=\"__NONCE__\"").count()
        );
        assert_eq!(
            PAGE.matches("<style").count(),
            PAGE.matches("<style nonce=\"__NONCE__\"").count()
        );
        assert_eq!(
            PAGE.matches("__NONCE__").count(),
            3,
            "the marker is on the three tags and nowhere else"
        );
        assert_eq!(
            PAGE.matches("</script").count(),
            2,
            "no script text closes a script tag early"
        );
    }

    /// Without comments, so a `;` or a `:` inside one cannot glue two declarations together.
    fn without_comments(css: &str) -> String {
        let mut out = String::new();
        let mut rest = css;
        while let Some(at) = rest.find("/*") {
            out.push_str(&rest[..at]);
            rest = match rest[at..].find("*/") {
                Some(end) => &rest[at + end + 2..],
                None => "",
            };
        }
        out.push_str(rest);
        out
    }

    /// The `--name: #rrggbb` tokens inside the `{ ... }` that starts right after `opener`.
    fn theme_tokens(opener: &str) -> BTreeMap<String, [f64; 3]> {
        // A Windows checkout turns the line ends into CRLF; the opener is written with LF.
        let page = PAGE.replace("\r\n", "\n");
        let at = page
            .find(opener)
            .unwrap_or_else(|| panic!("the page has no {opener:?}"));
        let body = &page[at + opener.len()..];
        let body = without_comments(&body[..body.find('}').expect("the block closes")]);
        let mut out = BTreeMap::new();
        for decl in body.split(';') {
            let Some((name, value)) = decl.split_once(':') else {
                continue;
            };
            let (name, value) = (name.trim(), value.trim());
            if !name.starts_with("--") || value.len() != 7 || !value.starts_with('#') {
                continue;
            }
            let channel = |i: usize| {
                f64::from(u8::from_str_radix(&value[1 + 2 * i..3 + 2 * i], 16).unwrap()) / 255.0
            };
            out.insert(name.to_string(), [channel(0), channel(1), channel(2)]);
        }
        out
    }

    fn luminance(rgb: [f64; 3]) -> f64 {
        let lin = |c: f64| {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(rgb[0]) + 0.7152 * lin(rgb[1]) + 0.0722 * lin(rgb[2])
    }

    fn contrast(a: [f64; 3], b: [f64; 3]) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    const TINTA: &str = ":root {\n  color-scheme: dark;";
    const PAPEL: &str = ":root[data-theme=\"light\"] {";

    /// Tinta (night glass) is the default and the only theme the sky follows; Papel changes the
    /// sheet and nothing else. `panel-src/test/contrast.test.mjs` reads the same blocks from the
    /// source; this reads them from the page that ships, so a build that dropped or changed one
    /// is caught on this side too. Body text is AAA (7:1), everything else somebody has to read AA (4.5:1).
    #[test]
    fn both_themes_keep_every_pair_of_colours_legible() {
        let tinta = theme_tokens(TINTA);
        let tinta_pairs: [(&str, &str, f64); 14] = [
            ("--text", "--night-1", 7.0),
            ("--text", "--night-2", 7.0),
            ("--text-dim", "--night-1", 4.5),
            ("--text-dim", "--night-2", 4.5),
            ("--text-faint", "--night-0", 4.5),
            ("--text-faint", "--night-1", 4.5),
            ("--text-faint", "--night-2", 4.5),
            ("--gold", "--night-1", 7.0),
            ("--gold", "--night-2", 7.0),
            ("--mint", "--night-1", 4.5),
            ("--alert", "--night-1", 4.5),
            ("--paper-ink", "--paper", 7.0),
            ("--paper-dim", "--paper", 4.5),
            ("--gold-ink", "--paper", 7.0),
        ];
        for (text, ground, min) in tinta_pairs {
            let (t, g) = (tinta[text], tinta[ground]);
            let ratio = contrast(t, g);
            assert!(
                ratio >= min,
                "Tinta: {text} on {ground} is {ratio:.2}:1, below {min}:1"
            );
        }
        let papel = theme_tokens(PAPEL);
        let papel_pairs: [(&str, f64); 4] = [
            ("--ink", 7.0),
            ("--ink-dim", 4.5),
            ("--ink-accent", 7.0),
            ("--ink-alert", 4.5),
        ];
        for (text, min) in papel_pairs {
            let ratio = contrast(papel[text], papel["--sheet-bg"]);
            assert!(
                ratio >= min,
                "Papel: {text} on the sheet is {ratio:.2}:1, below {min}:1"
            );
        }
    }

    #[test]
    fn papel_changes_only_the_sheet_and_no_hex_is_pure_black_or_white() {
        for name in theme_tokens(PAPEL).keys() {
            assert!(
                name.starts_with("--sheet")
                    || name.starts_with("--ink")
                    || name.starts_with("--line"),
                "{name} escapes the sheet in Papel"
            );
        }
        let style = PAGE
            .split("<style nonce=\"__NONCE__\">")
            .nth(1)
            .and_then(|rest| rest.split("</style>").next())
            .expect("the page has its stylesheet");
        let pure = Regex::new(r"(?i)#(?:000|fff)(?:000|fff)?\b").unwrap();
        assert!(
            !pure.is_match(&without_comments(style)),
            "the palette has no #000 and no #fff"
        );
    }
}
