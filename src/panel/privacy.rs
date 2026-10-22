//! What the snapshot must never carry (criterion A9): a whole e-mail address, or the folder of the
//! Windows user. `claude auth status` reports both, and the messages the preflight writes quote
//! them; this is the one place that takes them out before anything reaches the page.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::answers::load_answers;
use crate::claude::{address_pattern, load_claude_config, mask_email};

const LOCAL_APP_DATA: &str = "%LOCALAPPDATA%";
const APP_DATA: &str = "%APPDATA%";
const USER_PROFILE: &str = "%USERPROFILE%";

/// `C:\Users\<name>` (any drive, either slash, or the `/c/Users/<name>` of a Unix-style shell),
/// and the `AppData\Local` or `AppData\Roaming` that may follow it. `name` is the class of the
/// characters a user's folder name may hold; with `then_separator` the name must be followed by a
/// separator, which is kept (a name that may hold spaces needs something to say where it ends).
fn users_folder(name: &str, then_separator: bool) -> Regex {
    let end = if then_separator {
        r"(?P<sep>[\\/])"
    } else {
        ""
    };
    Regex::new(&format!(
        r"(?i)(?:[A-Za-z]:|/[A-Za-z])[\\/]+Users[\\/]+(?P<name>{name}+)(?:[\\/]+AppData[\\/]+(?P<which>Local|Roaming)\b)?{end}"
    ))
    .expect("the pattern is fixed")
}

/// In a value that is one folder, the name runs to the next separator and may hold spaces.
static FOLDER_OF_A_PATH: LazyLock<Regex> = LazyLock::new(|| users_folder(r"[^\\/]", false));
/// In a sentence a name followed by a separator may hold spaces (`C:\Users\Zoe Q\x`) ...
static SPACED_FOLDER_IN_TEXT: LazyLock<Regex> =
    LazyLock::new(|| users_folder(r#"[^\\/\r\n"'<>|:;,()]"#, true));
/// ... and one that is not runs to the next space or punctuation (`C:\Users\ana falló`).
static FOLDER_IN_TEXT: LazyLock<Regex> =
    LazyLock::new(|| users_folder(r#"[^\\/\s"'<>|:;,()]"#, false));

/// The folders of the Windows user, as this process sees them. They are injected so a test names
/// its own and never depends on who runs it.
#[derive(Debug, Clone, Default)]
pub struct UserPaths {
    /// His own addresses, in `address_key` form: the ones `redact` masks.
    own_addresses: Vec<String>,
    /// His folders (`USERPROFILE`, `LOCALAPPDATA`, `APPDATA`) as patterns with the variable that
    /// replaces each, compiled once here (a filter that compiles a pattern for every string it
    /// reads is the slowest thing in the panel) and in the order they apply: the longest first.
    known: Vec<(Regex, &'static str)>,
}

impl UserPaths {
    /// `local_app_data` and `app_data` default to the usual place under `profile`.
    pub fn new(
        profile: Option<&str>,
        local_app_data: Option<&str>,
        app_data: Option<&str>,
    ) -> UserPaths {
        let own = |value: Option<&str>| {
            value
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        let profile = own(profile);
        let under = |tail: &str| {
            profile
                .as_ref()
                .map(|p| format!("{}\\{tail}", p.trim_end_matches(['\\', '/'])))
        };
        let local_app_data = own(local_app_data).or_else(|| under("AppData\\Local"));
        let app_data = own(app_data).or_else(|| under("AppData\\Roaming"));
        let known = [
            (&local_app_data, LOCAL_APP_DATA),
            (&app_data, APP_DATA),
            (&profile, USER_PROFILE),
        ]
        .into_iter()
        .filter_map(|(folder, marker)| {
            known_folder(folder.as_deref()?).map(|pattern| (pattern, marker))
        })
        .collect();
        UserPaths {
            own_addresses: Vec::new(),
            known,
        }
    }

    /// Adds addresses that are his, so `redact` masks them and leaves everybody else's.
    pub fn with_own_addresses(mut self, addresses: impl IntoIterator<Item = String>) -> UserPaths {
        for address in addresses {
            // Only a value that is an address from end to end: an answer that merely mentions one
            // is a sentence, not a mailbox.
            let address = address.trim();
            if address_pattern()
                .find(address)
                .is_none_or(|m| m.as_str() != address)
            {
                continue;
            }
            let key = address_key(address);
            if !self.own_addresses.contains(&key) {
                self.own_addresses.push(key);
            }
        }
        self
    }

    /// This process's folders, plus the addresses the profile says are his: `expected_email` of
    /// `claude.toml` (with `claude.local.toml` laid over it) and every address the answer bank
    /// fills into forms. A file that cannot be read adds nothing.
    pub fn for_profile(profile_dir: &Path) -> UserPaths {
        let mut own = Vec::new();
        if let Ok(config) = load_claude_config(&profile_dir.join("claude.toml")) {
            own.push(config.expected_email);
        }
        if let Ok(bank) = load_answers(&profile_dir.join("answers.toml")) {
            for entry in bank.entries {
                own.extend(entry.value);
                own.extend(entry.alternatives);
            }
        }
        UserPaths::from_env().with_own_addresses(own)
    }

    /// The filter for everything the page receives: his folders and his own addresses are taken
    /// out, an address of a company or a recruiter is left, because the page needs it.
    pub fn redact(&self, text: &str) -> String {
        self.hide_with(
            text,
            &[&SPACED_FOLDER_IN_TEXT, &FOLDER_IN_TEXT],
            Addresses::Own,
        )
    }

    /// `redact` on every string of a JSON value, except the ones under a key that holds a URL
    /// (`url`, `apply_url`, `link`): a link must still open.
    pub fn redact_json(&self, value: serde_json::Value) -> serde_json::Value {
        use serde_json::Value;
        match value {
            // Most strings have nothing to take out: they keep their own allocation.
            Value::String(text) => match self.hide_changed(
                &text,
                &[&SPACED_FOLDER_IN_TEXT, &FOLDER_IN_TEXT],
                Addresses::Own,
            ) {
                Some(changed) => Value::String(changed),
                None => Value::String(text),
            },
            Value::Array(items) => Value::Array(
                items
                    .into_iter()
                    .map(|item| self.redact_json(item))
                    .collect(),
            ),
            Value::Object(map) => Value::Object(
                map.into_iter()
                    .map(|(key, item)| {
                        let is_link = URL_FIELDS.contains(&key.as_str()) && item.is_string();
                        (
                            key,
                            if is_link {
                                item
                            } else {
                                self.redact_json(item)
                            },
                        )
                    })
                    .collect(),
            ),
            other => other,
        }
    }

    /// `USERPROFILE`, `LOCALAPPDATA` and `APPDATA` of this process.
    pub fn from_env() -> UserPaths {
        let var = |name: &str| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .map(|p| p.to_string_lossy().into_owned())
        };
        UserPaths::new(
            var("USERPROFILE").as_deref(),
            var("LOCALAPPDATA").as_deref(),
            var("APPDATA").as_deref(),
        )
    }

    /// `text` with the user's folders replaced by `%LOCALAPPDATA%`, `%APPDATA%` and
    /// `%USERPROFILE%`, e-mail addresses masked, and nothing else touched. For a message, where a
    /// folder name ends at the next space.
    pub fn hide_in_text(&self, text: &str) -> String {
        self.hide_with(
            text,
            &[&SPACED_FOLDER_IN_TEXT, &FOLDER_IN_TEXT],
            Addresses::All,
        )
    }

    /// The same for a value that is one folder: a name with spaces is one name, and a Windows path
    /// comes out with `\` for every separator.
    pub fn hide_path(&self, path: &str) -> String {
        let hidden = self.hide_with(path, &[&FOLDER_OF_A_PATH], Addresses::All);
        let windows = hidden.starts_with('%') || {
            let mut chars = hidden.chars();
            matches!((chars.next(), chars.next()), (Some(c), Some(':')) if c.is_ascii_alphabetic())
        };
        if windows {
            hidden.replace('/', "\\")
        } else {
            hidden
        }
    }

    fn hide_with(&self, text: &str, folders: &[&Regex], addresses: Addresses) -> String {
        self.hide_changed(text, folders, addresses)
            .unwrap_or_else(|| text.to_string())
    }

    /// `hide_with`, or `None` when nothing was touched (then the answer is `text` as it is).
    fn hide_changed(&self, text: &str, folders: &[&Regex], addresses: Addresses) -> Option<String> {
        let mut out: Cow<'_, str> = Cow::Borrowed(text);
        // The folders this process knows by name first, the longest first: a profile outside
        // `C:\Users` is found by what the environment says and not by its shape.
        for (pattern, marker) in &self.known {
            if let Some(replaced) = replace_known(pattern, &out, marker) {
                out = Cow::Owned(replaced);
            }
        }
        // Every pattern below needs a separator (a folder) or an `@` (an address) to match at all,
        // so a string with neither, which is nearly all of them, is not scanned.
        if out.contains(['\\', '/']) {
            for folder in folders {
                let replaced = match folder.replace_all(&out, |caps: &Captures| {
                    let whole = caps.get(0).expect("group 0 always matches");
                    if starts_a_longer_word(&out, whole.start()) {
                        return whole.as_str().to_string();
                    }
                    let marker = match caps.name("which").map(|m| m.as_str().to_ascii_lowercase()) {
                        Some(which) if which == "local" => LOCAL_APP_DATA,
                        Some(_) => APP_DATA,
                        None => USER_PROFILE,
                    };
                    let sep = caps.name("sep").map_or("", |m| m.as_str());
                    format!("{marker}{sep}")
                }) {
                    Cow::Owned(replaced) => Some(replaced),
                    Cow::Borrowed(_) => None,
                };
                if let Some(replaced) = replaced {
                    out = Cow::Owned(replaced);
                }
            }
        }
        if out.contains('@') {
            let replaced =
                match address_pattern().replace_all(&out, |caps: &Captures| match addresses {
                    Addresses::Own if !self.own_addresses.contains(&address_key(&caps[0])) => {
                        caps[0].to_string()
                    }
                    _ => mask_email(&caps[0]),
                }) {
                    Cow::Owned(replaced) => Some(replaced),
                    Cow::Borrowed(_) => None,
                };
            if let Some(replaced) = replaced {
                out = Cow::Owned(replaced);
            }
        }
        match out {
            Cow::Owned(changed) => Some(changed),
            Cow::Borrowed(_) => None,
        }
    }
}

/// Which addresses a pass masks: every one (the fields about his Claude account, where no third
/// party can appear) or only his own (everything else, where a recruiter's address is content).
#[derive(Clone, Copy)]
enum Addresses {
    All,
    Own,
}

/// The JSON keys whose strings are links: a link must open as it was, so the filter skips them.
const URL_FIELDS: [&str; 3] = ["url", "apply_url", "link"];

/// What makes two spellings one mailbox: case, a `+tag`, and the dots of a Gmail address.
fn address_key(address: &str) -> String {
    let address = address.trim().to_lowercase();
    let Some((local, domain)) = address.rsplit_once('@') else {
        return address;
    };
    let local = local.split('+').next().unwrap_or(local);
    let local = if matches!(domain, "gmail.com" | "googlemail.com") {
        local.replace('.', "")
    } else {
        local.to_string()
    };
    format!("{local}@{domain}")
}

/// True when the character before `at` belongs to a word: `xC:\Users` is not a drive.
fn starts_a_longer_word(text: &str, at: usize) -> bool {
    text[..at]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric())
}

/// The pattern for `folder` (either slash, any case), or `None` when it names nothing.
fn known_folder(folder: &str) -> Option<Regex> {
    let parts: Vec<String> = folder
        .split(['\\', '/'])
        .filter(|part| !part.is_empty())
        .map(regex::escape)
        .collect();
    if parts.is_empty() {
        return None;
    }
    Regex::new(&format!(r"(?i){}", parts.join(r"[\\/]+"))).ok()
}

/// `text` with every `pattern` (a `known_folder`) that is a whole folder name, not the start of a
/// longer one, replaced by `marker`; `None` when there was nothing to replace.
fn replace_known(pattern: &Regex, text: &str, marker: &str) -> Option<String> {
    let mut out = String::new();
    let mut last = 0;
    let mut replaced = false;
    for found in pattern.find_iter(text) {
        let next = text[found.end()..].chars().next();
        let continues = next.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-');
        if continues || starts_a_longer_word(text, found.start()) {
            continue;
        }
        out.push_str(&text[last..found.start()]);
        out.push_str(marker);
        last = found.end();
        replaced = true;
    }
    if !replaced {
        return None;
    }
    out.push_str(&text[last..]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ana() -> UserPaths {
        UserPaths::new(
            Some(r"C:\Users\ana"),
            Some(r"C:\Users\ana\AppData\Local"),
            Some(r"C:\Users\ana\AppData\Roaming"),
        )
    }

    #[test]
    fn the_folders_of_the_user_become_their_variables() {
        let user = ana();
        assert_eq!(
            user.hide_path(r"C:\Users\ana\AppData\Local\jobbot\claude-config"),
            r"%LOCALAPPDATA%\jobbot\claude-config"
        );
        assert_eq!(
            user.hide_path(r"C:\Users\ana\AppData\Roaming\npm"),
            r"%APPDATA%\npm"
        );
        assert_eq!(
            user.hide_path(r"C:\Users\ana\Documents"),
            r"%USERPROFILE%\Documents"
        );
        assert_eq!(user.hide_path(r"C:\Users\ana"), "%USERPROFILE%");
    }

    #[test]
    fn a_path_written_with_slashes_or_other_case_is_hidden_and_comes_out_with_backslashes() {
        let user = ana();
        assert_eq!(
            user.hide_path("C:/Users/ana/AppData/Local/jobbot/claude-config"),
            r"%LOCALAPPDATA%\jobbot\claude-config"
        );
        assert_eq!(
            user.hide_path(r"c:\USERS\Ana\appdata\local\x"),
            r"%LOCALAPPDATA%\x"
        );
    }

    #[test]
    fn a_folder_of_another_user_or_drive_is_hidden_too() {
        // Not the user this process runs as (a scheduled task under another account), another drive,
        // and a name with a space in it: the folder is one name.
        let user = ana();
        assert_eq!(
            user.hide_path(r"D:\Users\Zoe Quint\AppData\Local\jobbot"),
            r"%LOCALAPPDATA%\jobbot"
        );
        assert_eq!(
            user.hide_path(r"E:\Users\Zoe Quint\Desktop\cv.pdf"),
            r"%USERPROFILE%\Desktop\cv.pdf"
        );
    }

    #[test]
    fn a_profile_outside_users_is_hidden_by_what_the_environment_says() {
        let user = UserPaths::new(Some(r"D:\ana"), Some(r"D:\ana\local"), None);
        assert_eq!(
            user.hide_path(r"D:\ana\local\jobbot"),
            r"%LOCALAPPDATA%\jobbot"
        );
        assert_eq!(user.hide_path(r"D:\ana\notes"), r"%USERPROFILE%\notes");
        assert_eq!(
            user.hide_path(r"D:\anabel\notes"),
            r"D:\anabel\notes",
            "another folder that starts with the same letters is not the profile"
        );
    }

    #[test]
    fn a_message_keeps_its_words_and_loses_the_folder() {
        let user = ana();
        assert_eq!(
            user.hide_in_text(
                r"claude usa el directorio C:/Users/ana/.claude en vez del de jobbot (C:\Users\ana\AppData\Local\jobbot\claude-config): no llegó"
            ),
            r"claude usa el directorio %USERPROFILE%/.claude en vez del de jobbot (%LOCALAPPDATA%\jobbot\claude-config): no llegó"
        );
        // A name with a space cannot be told from the words after it; the profile of this process is
        // still found whole, by what the environment says.
        let spaced = UserPaths::new(Some(r"C:\Users\Zoe Quint"), None, None);
        assert_eq!(
            spaced.hide_in_text(r"falta C:\Users\Zoe Quint\x.json"),
            r"falta %USERPROFILE%\x.json"
        );
    }

    #[test]
    fn what_is_not_a_user_folder_is_left_alone() {
        let user = ana();
        for text in [
            "data/handoff/test_1.md",
            r"C:\Program Files\ClaudeCode",
            "https://example.com/Users/ana/profile",
            "sin sesión",
        ] {
            assert_eq!(user.hide_in_text(text), text);
        }
        assert_eq!(user.hide_path("data/handoff/x.md"), "data/handoff/x.md");
        assert_eq!(
            user.hide_path("C:/Program Files/ClaudeCode"),
            r"C:\Program Files\ClaudeCode",
            "a Windows path that is not the user's only changes its separators"
        );
    }

    fn mine() -> UserPaths {
        ana().with_own_addresses([
            "Mio.Nombre@Mail.example".to_string(),
            "otro@gmail.com".to_string(),
        ])
    }

    #[test]
    fn redact_masks_his_own_addresses_in_any_spelling_and_leaves_everybody_elses() {
        let user = mine();
        assert_eq!(
            user.redact("De: Yo <mio.nombre@mail.example>"),
            "De: Yo <m***@mail.example>"
        );
        assert_eq!(
            user.redact("MIO.NOMBRE+empleo@mail.example"),
            "M***@mail.example",
            "case and a +tag are the same mailbox"
        );
        assert_eq!(
            user.redact("o.tro@gmail.com y o.tro@gmail.com.ar y o.tro@mail.example"),
            "o***@gmail.com y o.tro@gmail.com.ar y o.tro@mail.example",
            "Gmail ignores dots; another domain does not, and a longer domain is another address"
        );
        for third_party in [
            "Ana <ana@acme.example>",
            "recruiter@empresa.example",
            "g***@example.com",
        ] {
            assert_eq!(user.redact(third_party), third_party);
        }
        assert_eq!(
            UserPaths::default().redact("mio.nombre@mail.example"),
            "mio.nombre@mail.example",
            "no own address known, nothing to mask"
        );
    }

    #[test]
    fn redact_takes_out_the_folder_and_the_address_together() {
        let user = mine();
        assert_eq!(
            user.redact(
                r"falló C:\Users\ana\AppData\Local\jobbot\x.json de mio.nombre@mail.example y de ana@acme.example"
            ),
            r"falló %LOCALAPPDATA%\jobbot\x.json de m***@mail.example y de ana@acme.example"
        );
    }

    #[test]
    fn redact_json_walks_every_string_but_not_the_urls_or_the_keys() {
        let user = mine();
        let url = r"https://empresa.example/apply?e=mio.nombre@mail.example&p=C:\Users\ana\x";
        let out = user.redact_json(serde_json::json!({
            "text": r"C:\Users\ana\x y mio.nombre@mail.example",
            "mio.nombre@mail.example": "la clave no se toca",
            "n": 3, "ok": true, "none": null,
            "url": url, "apply_url": url, "link": url,
            "cards": [{"note": "mio.nombre@mail.example", "url": url}, "otro@gmail.com"],
            "nested": {"deeper": {"reason": r"D:\Users\Zoe Q\y"}},
        }));
        assert_eq!(
            out,
            serde_json::json!({
                "text": r"%USERPROFILE%\x y m***@mail.example",
                "mio.nombre@mail.example": "la clave no se toca",
                "n": 3, "ok": true, "none": null,
                "url": url, "apply_url": url, "link": url,
                "cards": [{"note": "m***@mail.example", "url": url}, "o***@gmail.com"],
                "nested": {"deeper": {"reason": r"%USERPROFILE%\y"}},
            })
        );
    }

    #[test]
    fn a_whole_address_is_masked_and_a_masked_one_stays() {
        let user = UserPaths::default();
        assert_eq!(
            user.hide_in_text("la sesión es de jane.doe@example.com y no de g***@example.com"),
            "la sesión es de j***@example.com y no de g***@example.com"
        );
    }

    /// The filter as it was before it compiled its patterns once: every known folder compiled
    /// again for every string, six allocating passes. It is the reference the fast one must equal.
    mod reference {
        use super::*;

        /// The pattern the old filter built for a folder; `None` is what made it return the text.
        pub fn pattern(folder: &str) -> Option<Regex> {
            let parts: Vec<String> = folder
                .split(['\\', '/'])
                .filter(|part| !part.is_empty())
                .map(regex::escape)
                .collect();
            if parts.is_empty() {
                return None;
            }
            Regex::new(&format!(r"(?i){}", parts.join(r"[\\/]+"))).ok()
        }

        fn replace_known(text: &str, pattern: &Regex, marker: &str) -> String {
            let mut out = String::with_capacity(text.len());
            let mut last = 0;
            for found in pattern.find_iter(text) {
                let next = text[found.end()..].chars().next();
                let continues = next.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-');
                if continues || starts_a_longer_word(text, found.start()) {
                    continue;
                }
                out.push_str(&text[last..found.start()]);
                out.push_str(marker);
                last = found.end();
            }
            out.push_str(&text[last..]);
            out
        }

        /// `known` are the patterns of `(local_app_data, app_data, profile)`, in that order, as
        /// `UserPaths::new` resolved the folders. Compiling them once is the only difference the
        /// test lets the old filter have, so that it runs over a whole corpus in a blink.
        pub fn hide_with(
            known: &[Option<Regex>; 3],
            own_addresses: &[String],
            text: &str,
            folders: &[&Regex],
            addresses: Addresses,
        ) -> String {
            let mut out = text.to_string();
            for (pattern, marker) in known.iter().zip([LOCAL_APP_DATA, APP_DATA, USER_PROFILE]) {
                if let Some(pattern) = pattern {
                    out = replace_known(&out, pattern, marker);
                }
            }
            for folder in folders {
                let haystack = out;
                out = folder
                    .replace_all(&haystack, |caps: &Captures| {
                        let whole = caps.get(0).expect("group 0 always matches");
                        if starts_a_longer_word(&haystack, whole.start()) {
                            return whole.as_str().to_string();
                        }
                        let marker =
                            match caps.name("which").map(|m| m.as_str().to_ascii_lowercase()) {
                                Some(which) if which == "local" => LOCAL_APP_DATA,
                                Some(_) => APP_DATA,
                                None => USER_PROFILE,
                            };
                        let sep = caps.name("sep").map_or("", |m| m.as_str());
                        format!("{marker}{sep}")
                    })
                    .into_owned();
            }
            address_pattern()
                .replace_all(&out, |caps: &Captures| match addresses {
                    Addresses::Own if !own_addresses.contains(&address_key(&caps[0])) => {
                        caps[0].to_string()
                    }
                    _ => mask_email(&caps[0]),
                })
                .into_owned()
        }
    }

    /// Every spelling the filter has to treat the same as it always did: folders in both slashes
    /// and any case, spaced and unspaced names, folders glued to a word, the three known folders
    /// next to one another, addresses of his and of others, and text with neither.
    fn corpus() -> Vec<String> {
        let base = [
            "",
            "plain text with nothing to hide",
            "Acme Corp - Senior Rust Engineer (Remote)",
            "2026-10-09T22:00:00Z",
            r"C:\Users\ana",
            r"C:\Users\ana\",
            r"c:/users/ANA/AppData/Local/jobbot/x.json",
            r"C:\Users\ana\AppData\Roaming\Claude",
            r"C:\Users\ana\AppData\Local",
            r"C:\Users\ana\AppData\LocalLow\x",
            r"C:\Users\ana\Documents\cv.pdf y C:\Users\ana\Desktop\z",
            r"C:\Users\ana falló",
            r"C:\Users\anabel\x",
            r"C:\Users\ana-b\x",
            r"C:\Users\ana_b\x",
            r"xC:\Users\ana\x",
            r"fooC:\Users\Zoe Q\y",
            r"D:\Users\Zoe Q\y and E:\Users\Max\z",
            r"/c/Users/ana/AppData/Local/x and /d/Users/Zoe Q/y",
            r"C:\\Users\\ana\\x",
            r"C:\Users\\ana//AppData\\Local",
            r"error: no se pudo abrir C:\Users\ana\AppData\Local\jobbot\profile\claude.toml: acceso denegado",
            r#"{"path":"C:\\Users\\ana\\x","other":"D:\\Users\\Zoe\\y"}"#,
            r#"(C:\Users\ana) "C:\Users\Zoe Q\x" 'D:\Users\Max\y' <E:\Users\Li\z>"#,
            "mio.nombre@mail.example",
            "Mio.Nombre@Mail.example escribe a recruiter@company.example",
            "otro+tag@gmail.com y o.t.r.o@gmail.com y OTRO@GMAIL.COM",
            "o***@gmail.com ya esta enmascarado",
            "a@b",
            "@",
            "x@",
            "sin arroba pero con barra / y \\ sueltas",
            "ñandú café ¿qué? @ñ \\ día",
            "Maria Jose <maria@empresa.example> C:\\Users\\ana\\Local y mio.nombre@mail.example",
            "%USERPROFILE%\\x y %APPDATA%\\y",
            r"C:\Users\ana\AppData\Local\jobbot et C:\Users\ana\AppData\Roaming\x et C:\Users\ana",
            "line one\nC:\\Users\\Zoe Q\\y\r\nline three recruiter@company.example",
            "https://example.com/a/b?c=d&e=f",
            "10/10/2026 and 3\\4",
        ];
        let mut out: Vec<String> = base.iter().map(|s| s.to_string()).collect();
        // Each one with a few others after it (a different few each, so the pairs cover the mix),
        // and with a word glued to each side.
        for (i, a) in base.iter().enumerate() {
            for k in 0..4 {
                let b = base[(i * 7 + k * 11 + 1) % base.len()];
                out.push(format!("{a} {b}"));
            }
            out.push(format!("x{a}"));
            out.push(format!("{a}x"));
            out.push(format!("{a}-"));
        }
        out
    }

    /// One user per way the folders may be given: all three, only the profile, a profile outside
    /// `C:\Users`, a folder with no separator at all, another drive, a trailing slash, none.
    fn users() -> Vec<(UserPaths, [Option<Regex>; 3])> {
        let own = [
            "Mio.Nombre@Mail.example".to_string(),
            "otro@gmail.com".to_string(),
        ];
        let cases: [(Option<&str>, Option<&str>, Option<&str>); 7] = [
            (
                Some(r"C:\Users\ana"),
                Some(r"C:\Users\ana\AppData\Local"),
                Some(r"C:\Users\ana\AppData\Roaming"),
            ),
            (Some(r"C:\Users\ana"), None, None),
            (Some(r"D:\Perfiles\Ana Q\"), None, None),
            (Some("ana"), None, None),
            (Some(r"c:/users/ana/"), Some("  "), Some("")),
            (None, Some(r"C:\Users\ana\AppData\Local"), None),
            (None, None, None),
        ];
        cases
            .into_iter()
            .map(|(profile, local, app)| {
                let user = UserPaths::new(profile, local, app).with_own_addresses(own.clone());
                // What `new` resolved, the way the old struct stored it.
                let trimmed = |v: Option<&str>| {
                    v.map(str::trim)
                        .filter(|v| !v.is_empty())
                        .map(str::to_string)
                };
                let profile = trimmed(profile);
                let under = |tail: &str| {
                    profile
                        .as_ref()
                        .map(|p| format!("{}\\{tail}", p.trim_end_matches(['\\', '/'])))
                };
                let local = trimmed(local).or_else(|| under("AppData\\Local"));
                let app = trimmed(app).or_else(|| under("AppData\\Roaming"));
                let patterns = [&local, &app, &profile]
                    .map(|folder| folder.as_deref().and_then(reference::pattern));
                (user, patterns)
            })
            .collect()
    }

    #[test]
    fn the_filter_that_compiles_once_says_what_the_one_that_compiled_every_time_said() {
        let own = vec![
            address_key("Mio.Nombre@Mail.example"),
            address_key("otro@gmail.com"),
        ];
        let texts = corpus();
        for (user, patterns) in users() {
            for text in &texts {
                for (folders, label) in [
                    (vec![&*SPACED_FOLDER_IN_TEXT, &*FOLDER_IN_TEXT], "in text"),
                    (vec![&*FOLDER_OF_A_PATH], "of a path"),
                ] {
                    for (addresses, which) in [(Addresses::All, "all"), (Addresses::Own, "own")] {
                        let expected =
                            reference::hide_with(&patterns, &own, text, &folders, addresses);
                        assert_eq!(
                            user.hide_with(text, &folders, addresses),
                            expected,
                            "{label}/{which} on {text:?} for {user:?}"
                        );
                        // `None` promises the text came through as it was; `Some` may still be equal
                        // (a match that turned out to start inside a word is put back).
                        let changed = user.hide_changed(text, &folders, addresses);
                        assert!(
                            changed.is_some() || expected == *text,
                            "None but the text changes: {label}/{which} on {text:?}"
                        );
                    }
                }
                // The two entry points the panel uses.
                let expected = reference::hide_with(
                    &patterns,
                    &own,
                    text,
                    &[&SPACED_FOLDER_IN_TEXT, &FOLDER_IN_TEXT],
                    Addresses::Own,
                );
                assert_eq!(user.redact(text), expected, "redact on {text:?}");
                assert_eq!(
                    user.redact_json(serde_json::Value::String(text.clone())),
                    serde_json::Value::String(expected),
                    "redact_json on {text:?}"
                );
            }
        }
    }
}
