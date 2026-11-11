//! `cargo xtask export-public --author "NAME <EMAIL>" [--commit REV] [--out DIR] [--terms FILE]
//! [--message TEXT] [--public-home URL]...`: see `xtask::export`. The terms file defaults to
//! `export-forbidden.txt` in `JOBBOT_HOME`, or else in `%LOCALAPPDATA%\jobbot`
//! (`$XDG_DATA_HOME/jobbot`, `~/.local/share/jobbot` elsewhere); the copy defaults to a new folder
//! in the temp directory. Before its commit the copy is formatted again (`cargo fmt --all`: a
//! replacement changes the length of a line) and runs `cargo test --workspace --locked
//! --offline` (built in `CARGO_TARGET_DIR`, or `jobbot-public-target` in the temp directory);
//! `--no-verify` skips the tests. `--public-home` names an address the copy is published at (the
//! repository's page, its site): the private list usually forbids the owner's name, and that
//! address carries it; a term inside the address, exactly as given, is then not a finding, and the
//! same term anywhere else still is.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use xtask::export::{Export, ExportError, Verify, export_public};

const USAGE: &str = "usage: cargo xtask export-public --author \"NAME <EMAIL>\" [--commit REV] \
[--out DIR] [--terms FILE] [--message TEXT] [--public-home URL]... [--no-verify]";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let export = match parse(&args) {
        Ok(export) => export,
        Err(message) => {
            eprintln!("{message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match export_public(&export) {
        Ok(done) => {
            println!(
                "{} exported to {}: {} files, {} committed paths left out, commit {}. Nothing was pushed.",
                export.commit,
                export.out.display(),
                done.files,
                done.left_out,
                done.commit
            );
            ExitCode::SUCCESS
        }
        Err(ExportError::Findings(findings)) => {
            for finding in &findings {
                println!(
                    "{}:{}: [{:?}] {}",
                    finding.path, finding.line, finding.kind, finding.matched
                );
            }
            eprintln!(
                "{} finding(s): nothing was committed and the copy was removed",
                findings.len()
            );
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Reads the command line into an export; the repository is the current folder.
fn parse(args: &[String]) -> Result<Export, String> {
    let mut args = args.iter();
    if args.next().map(String::as_str) != Some("export-public") {
        return Err("the only task is export-public".into());
    }
    let mut commit = "HEAD".to_string();
    let mut out = None;
    let mut terms = None;
    let mut author = None;
    let mut message = "Initial public release".to_string();
    let mut verify = true;
    let mut public_homes = Vec::new();
    while let Some(flag) = args.next() {
        if flag == "--no-verify" {
            verify = false;
            continue;
        }
        let value = args
            .next()
            .ok_or_else(|| format!("{flag} needs a value"))?
            .clone();
        match flag.as_str() {
            "--commit" => commit = value,
            "--out" => out = Some(PathBuf::from(value)),
            "--terms" => terms = Some(PathBuf::from(value)),
            "--author" => author = Some(value),
            "--message" => message = value,
            "--public-home" => public_homes.push(public_home(&value)?),
            other => return Err(format!("unknown option {other}")),
        }
    }
    let (author_name, author_email) =
        split_author(&author.ok_or("--author is required: the public commit's author")?)?;
    let terms = match terms {
        Some(terms) => terms,
        None => {
            default_terms().ok_or("no --terms, and no JOBBOT_HOME or data folder to look in")?
        }
    };
    let out = out.unwrap_or_else(|| {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        env::temp_dir().join(format!("jobbot-public-{now}"))
    });
    Ok(Export {
        repo: env::current_dir().map_err(|error| error.to_string())?,
        commit,
        out,
        terms,
        author_name,
        author_email,
        message,
        account: account_from(|key| env::var(key).ok()),
        public_homes,
        format: Some(copy_format()),
        verify: verify.then(copy_tests),
    })
}

/// A `--public-home` value: an address the copy is published at. It has to be a whole https
/// address down to a folder (`https://github.com/NAME/`, `https://NAME.github.io/`), so that it
/// cannot be shortened into something that lets a bare word through.
fn public_home(value: &str) -> Result<String, String> {
    let rest = value.strip_prefix("https://").unwrap_or("");
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let whole = host.contains('.')
        && !host.contains(['@', ':', ' '])
        && value.ends_with('/')
        && (host.split('.').count() >= 3 || !path.is_empty());
    if whole {
        Ok(value.to_string())
    } else {
        Err(format!(
            "--public-home must be the https address the copy is published at, ending in `/` \
             (https://github.com/NAME/ or https://NAME.github.io/), got {value:?}"
        ))
    }
}

/// The copy is formatted after the replacements, so that it passes the formatting check of its
/// own CI whatever the replaced names measure.
fn copy_format() -> Verify {
    Verify {
        program: "cargo".into(),
        args: ["fmt", "--all"].map(String::from).to_vec(),
        env: Vec::new(),
    }
}

/// The check run in the copy before its commit: the copy's own test suite, built outside the
/// copy (`CARGO_TARGET_DIR`, kept between exports so the next one builds only what changed).
fn copy_tests() -> Verify {
    let target = env::var("CARGO_TARGET_DIR")
        .ok()
        .filter(|dir| !dir.is_empty())
        .map_or_else(
            || env::temp_dir().join("jobbot-public-target"),
            PathBuf::from,
        );
    Verify {
        program: "cargo".into(),
        args: ["test", "--workspace", "--locked", "--offline"]
            .map(String::from)
            .to_vec(),
        env: vec![(
            "CARGO_TARGET_DIR".into(),
            target.to_string_lossy().into_owned(),
        )],
    }
}

/// `"Jane Doe <jane@example.com>"` as its name and address.
fn split_author(author: &str) -> Result<(String, String), String> {
    let invalid = || format!("--author must look like \"NAME <EMAIL>\", got {author:?}");
    let (name, rest) = author.split_once('<').ok_or_else(invalid)?;
    let email = rest.strip_suffix('>').ok_or_else(invalid)?.trim();
    let name = name.trim();
    if name.is_empty() || !email.contains('@') {
        return Err(invalid());
    }
    Ok((name.to_string(), email.to_string()))
}

/// The account running the export, read through `var`: its home folder becomes a forbidden term.
fn account_from(var: impl Fn(&str) -> Option<String>) -> Option<String> {
    let named = var("USERNAME").or_else(|| var("USER"));
    let from_home = || {
        let home = var("USERPROFILE").or_else(|| var("HOME"))?;
        let last = home
            .trim_end_matches(['\\', '/'])
            .rsplit(['\\', '/'])
            .next()?;
        Some(last.to_string())
    };
    named
        .or_else(from_home)
        .filter(|name| !name.trim().is_empty())
}

/// Where the private terms list lives when `--terms` is not given.
fn default_terms() -> Option<PathBuf> {
    let folder = env::var_os("JOBBOT_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("jobbot")))
        .or_else(|| env::var_os("XDG_DATA_HOME").map(|dir| PathBuf::from(dir).join("jobbot")))
        .or_else(|| {
            env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share/jobbot"))
        })?;
    Some(folder.join("export-forbidden.txt"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn reads_the_options_and_the_author() {
        let export = parse(&args(&[
            "export-public",
            "--author",
            "Jane Doe <jane@example.com>",
            "--commit",
            "abc123",
            "--out",
            "out",
            "--terms",
            "terms.txt",
        ]))
        .unwrap();
        assert_eq!(export.commit, "abc123");
        assert_eq!(export.out, PathBuf::from("out"));
        assert_eq!(export.terms, PathBuf::from("terms.txt"));
        assert_eq!(
            (export.author_name.as_str(), export.author_email.as_str()),
            ("Jane Doe", "jane@example.com")
        );
        assert_eq!(export.message, "Initial public release");
    }

    #[test]
    fn a_public_home_is_a_whole_https_address_down_to_a_folder() {
        let author = ["export-public", "--author", "Jane Doe <jane@example.com>"];
        let with = |home: &'static str| {
            let mut list = author.to_vec();
            list.extend(["--public-home", home]);
            parse(&args(&list))
        };
        assert!(parse(&args(&author)).unwrap().public_homes.is_empty());
        let mut two = author.to_vec();
        two.extend(["--public-home", "https://github.com/jane-doe/"]);
        two.extend(["--public-home", "https://jane-doe.github.io/"]);
        assert_eq!(
            parse(&args(&two)).unwrap().public_homes,
            [
                "https://github.com/jane-doe/",
                "https://jane-doe.github.io/"
            ]
        );
        for bad in [
            "jane",
            "jane-doe",
            "https://",
            "https://github.com/",
            "https://github.com/jane-doe",
            "http://github.com/jane-doe/",
            "https://jane@example.com/x/",
            "https://localhost/jane/",
        ] {
            assert!(with(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_copy_runs_its_tests_before_the_commit_unless_told_not_to() {
        let author = ["export-public", "--author", "Jane Doe <jane@example.com>"];
        let checked = parse(&args(&author))
            .unwrap()
            .verify
            .expect("checked by default");
        assert_eq!(checked.program, "cargo");
        assert_eq!(
            checked.args,
            ["test", "--workspace", "--locked", "--offline"]
        );
        assert!(
            checked
                .env
                .iter()
                .any(|(name, _)| name == "CARGO_TARGET_DIR")
        );
        let mut skipped = author.to_vec();
        skipped.push("--no-verify");
        let skipped = parse(&args(&skipped)).unwrap();
        assert!(skipped.verify.is_none());
        // Formatting is not a check that can be skipped: without it the copy fails its own CI.
        let format = skipped.format.expect("formatted always");
        assert_eq!(
            (format.program.as_str(), format.args),
            ("cargo", vec!["fmt".to_string(), "--all".to_string()])
        );
    }

    #[test]
    fn the_account_is_the_user_name_or_else_the_home_folders_name() {
        fn vars(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
            move |key| {
                pairs
                    .iter()
                    .find(|(name, _)| *name == key)
                    .map(|(_, value)| value.to_string())
            }
        }
        assert_eq!(
            account_from(vars(&[("USERNAME", "ana")])),
            Some("ana".into())
        );
        assert_eq!(
            account_from(vars(&[("USERPROFILE", r"C:\Users\bo")])),
            Some("bo".into())
        );
        assert_eq!(
            account_from(vars(&[("HOME", "/home/cy")])),
            Some("cy".into())
        );
        assert_eq!(account_from(vars(&[("USERNAME", "")])), None);
    }

    #[test]
    fn refuses_another_task_a_missing_author_and_a_malformed_one() {
        assert!(parse(&args(&["build"])).is_err());
        assert!(parse(&args(&["export-public", "--terms", "t.txt"])).is_err());
        for bad in [
            "Jane Doe",
            "<jane@example.com>",
            "Jane <nobody>",
            "Jane <a@b",
        ] {
            assert!(split_author(bad).is_err(), "{bad}");
        }
        assert!(parse(&args(&["export-public", "--author"])).is_err());
    }
}
