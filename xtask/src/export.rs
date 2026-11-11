//! The public copy of one commit: built from the commit, never from the working
//! tree, so files nobody committed cannot leak into it; only whitelisted paths; the example
//! profile and CV in place of the real ones, the public README in place of the owner's manual, and
//! no audit of the private profile; scanned before anything is committed; one commit in a fresh
//! repository with no remote.

use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::scan::{Finding, scan_tree};
use crate::terms::{Terms, TermsError};

/// Paths that never leave the repository, whatever the whitelist says: the real profile and CV,
/// the run's data, research notes and plans, the panel's archived drafts.
const NEVER: [&str; 6] = [
    "profile/",
    "cv/",
    "data/",
    "docs/research/",
    "docs/superpowers/",
    "panel-src/archive/",
];

/// Files that never leave the repository wherever they are: the tests that pin the private
/// profile's own values (its answers, figures and exclusions), named `profile_audit*`.
const NEVER_NAMED: &str = "profile_audit";

/// Files the copy takes under another name, and only there: the root `README.md` is the owner's
/// manual and stays private; the public one is kept in `docs/public/`.
const RENAMED: [(&str, &str); 2] = [
    ("docs/public/README.md", "README.md"),
    ("docs/public/README.es.md", "README.es.md"),
];

/// The whitelist: a name ending in `/` takes the folder, any other name one file.
const INCLUDE: [&str; 28] = [
    ".cargo/",
    ".gitattributes",
    ".github/",
    ".gitignore",
    "AI-USE.md",
    "ARCHITECTURE.md",
    "CONTRIBUTING.md",
    "Cargo.lock",
    "Cargo.toml",
    "LICENSE",
    "SECURITY.md",
    "THIRD_PARTY_NOTICES.md",
    "benches/",
    "crates/",
    "deny.toml",
    "docs/COLOR.md",
    "docs/adr/",
    "docs/engineering/",
    "docs/img/",
    "examples/",
    "fuzz/",
    "licenses/",
    "panel-src/",
    "rust-toolchain.toml",
    "scripts/",
    "src/",
    "tests/",
    "xtask/",
];

/// The example files that also take the place of the real ones in the copy.
const FROM_EXAMPLES: [(&str, &str); 2] =
    [("examples/profile/", "profile/"), ("examples/cv/", "cv/")];

/// Where a committed path goes in the public copy: nowhere, its own place, its public name, or
/// (for the example profile and CV) its own place and the real file's.
pub fn destinations(path: &str) -> Vec<String> {
    let audit = path.split('/').any(|part| part.starts_with(NEVER_NAMED));
    if audit || NEVER.iter().any(|never| path.starts_with(never)) {
        return Vec::new();
    }
    if let Some((_, public)) = RENAMED.iter().find(|(private, _)| *private == path) {
        return vec![public.to_string()];
    }
    let included = INCLUDE.iter().any(|name| match name.strip_suffix('/') {
        Some(_) => path.starts_with(name),
        None => path == *name,
    });
    if !included {
        return Vec::new();
    }
    let mut places = vec![path.to_string()];
    for (example, real) in FROM_EXAMPLES {
        if let Some(rest) = path.strip_prefix(example) {
            places.push(format!("{real}{rest}"));
        }
    }
    places
}

/// One export to run.
#[derive(Debug, Clone)]
pub struct Export {
    /// Any folder inside the repository.
    pub repo: PathBuf,
    /// The commit to copy (a hash, a branch, `HEAD`).
    pub commit: String,
    /// Where the copy is made; it must not exist yet.
    pub out: PathBuf,
    /// The private terms file; it must live outside the repository.
    pub terms: PathBuf,
    pub author_name: String,
    pub author_email: String,
    pub message: String,
    /// The account that runs the export: its home folder (`C:\Users\<account>`,
    /// `/home/<account>`) is a finding in any file, whatever the terms file says.
    pub account: Option<String>,
    /// The addresses the copy is published at (the repository's page, its site): a term of the
    /// private list inside one of them, exactly as given, is not a finding. Named on the command
    /// line by whoever runs the export, never read from a file.
    pub public_homes: Vec<String>,
    /// A command run in the copy after the scan and before the commit (the CLI runs the copy's
    /// tests): the replacements rewrite code, so the copy must build and pass on its own. `None`
    /// skips it.
    pub verify: Option<Verify>,
}

/// The command that checks a copy, run in its folder.
#[derive(Debug, Clone)]
pub struct Verify {
    pub program: String,
    pub args: Vec<String>,
    /// Variables set for the command only (the CLI points `CARGO_TARGET_DIR` outside the copy).
    pub env: Vec<(String, String)>,
}

/// A finished export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    /// The commit of the new repository.
    pub commit: String,
    pub files: usize,
    /// Committed paths left out by the whitelist.
    pub left_out: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("the terms file {0} is inside the repository; it must live outside it")]
    TermsInsideRepo(PathBuf),
    #[error("cannot read the terms file {path}: {source}")]
    TermsUnreadable { path: PathBuf, source: io::Error },
    #[error("terms file: {0}")]
    Terms(#[from] TermsError),
    #[error("{0} already exists: an export goes into a new folder")]
    OutExists(PathBuf),
    #[error("git {args}: {detail}")]
    Git { args: String, detail: String },
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("{} finding(s); nothing was committed and the copy was removed", .0.len())]
    Findings(Vec<Finding>),
    #[error(
        "the copy failed `{command}` ({status}); nothing was committed and the copy was removed"
    )]
    Verify { command: String, status: String },
}

/// Runs an export: see the module comment. On any finding the copy is removed.
pub fn export_public(export: &Export) -> Result<Exported, ExportError> {
    let root = fs::canonicalize(git(&export.repo, &["rev-parse", "--show-toplevel"])?.trim())?;
    let unreadable = |source| ExportError::TermsUnreadable {
        path: export.terms.clone(),
        source,
    };
    let terms_path = fs::canonicalize(&export.terms).map_err(unreadable)?;
    if terms_path.starts_with(&root) {
        return Err(ExportError::TermsInsideRepo(export.terms.clone()));
    }
    let mut terms = Terms::parse(&fs::read_to_string(&terms_path).map_err(unreadable)?)?;
    if let Some(account) = export.account.as_deref().filter(|name| !name.is_empty()) {
        terms.forbid_home_of(account);
    }
    for home in &export.public_homes {
        terms.allow_public_home(home);
    }
    if export.out.exists() {
        return Err(ExportError::OutExists(export.out.clone()));
    }
    let commit = format!("{}^{{commit}}", export.commit);
    let commit = git(&root, &["rev-parse", "--verify", "--quiet", &commit])?;
    let listing = git(
        &root,
        &["ls-tree", "-r", "-z", "--full-tree", commit.trim()],
    )?;
    let mut copies = Vec::new();
    let mut left_out = 0;
    for entry in listing.split('\0').filter(|entry| !entry.is_empty()) {
        let (meta, path) = entry.split_once('\t').ok_or_else(|| ExportError::Git {
            args: "ls-tree".into(),
            detail: format!("unexpected line: {entry}"),
        })?;
        let fields: Vec<&str> = meta.split(' ').collect();
        let places = match fields.as_slice() {
            // A blob that is not a symbolic link; a submodule or a link is never copied.
            [mode, "blob", sha] if *mode != "120000" => destinations(path)
                .into_iter()
                .map(|place| (sha.to_string(), place))
                .collect(),
            _ => Vec::new(),
        };
        if places.is_empty() {
            left_out += 1;
        }
        copies.extend(places);
    }
    fs::create_dir_all(&export.out)?;
    let made = copy_check_commit(export, &root, &terms, &copies);
    if made.is_err() {
        // Nothing half-made stays behind: the folder did not exist before this export.
        let _ = fs::remove_dir_all(&export.out);
    }
    let commit = made?;
    Ok(Exported {
        commit,
        files: copies.len(),
        left_out,
    })
}

/// Writes every copy into the new folder, scans it, and commits it when nothing is found. Returns
/// the new commit.
fn copy_check_commit(
    export: &Export,
    root: &Path,
    terms: &Terms,
    copies: &[(String, String)],
) -> Result<String, ExportError> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut requests = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("no stdin"))?;
    let mut replies = BufReader::new(
        child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("no stdout"))?,
    );
    // One request at a time: writing them all first could fill both pipes and stall.
    for (sha, place) in copies {
        writeln!(requests, "{sha}")?;
        requests.flush()?;
        let mut header = String::new();
        replies.read_line(&mut header)?;
        let size = header
            .trim_end()
            .rsplit(' ')
            .next()
            .and_then(|size| size.parse::<usize>().ok())
            .filter(|_| !header.trim_end().ends_with("missing"))
            .ok_or_else(|| ExportError::Git {
                args: "cat-file --batch".into(),
                detail: format!("{place}: {}", header.trim_end()),
            })?;
        let mut content = vec![0; size];
        replies.read_exact(&mut content)?;
        let mut newline = [0; 1];
        replies.read_exact(&mut newline)?;
        let target = export.out.join(place);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        // A text file gets the private replacements; a binary one (a NUL byte, or not UTF-8) is
        // copied as it is and only its path is scanned.
        match String::from_utf8(content) {
            Ok(text) if !text.contains('\0') => fs::write(target, terms.scrub(&text))?,
            Ok(text) => fs::write(target, text)?,
            Err(error) => fs::write(target, error.into_bytes())?,
        }
    }
    drop(requests);
    child.wait()?;
    let findings = scan_tree(&export.out, terms)?;
    if !findings.is_empty() {
        return Err(ExportError::Findings(findings));
    }
    if let Some(verify) = &export.verify {
        let status = Command::new(&verify.program)
            .args(&verify.args)
            .envs(verify.env.iter().map(|(name, value)| (name, value)))
            .current_dir(&export.out)
            .status()?;
        if !status.success() {
            let command = std::iter::once(&verify.program)
                .chain(&verify.args)
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(" ");
            return Err(ExportError::Verify {
                command,
                status: status.to_string(),
            });
        }
    }
    git(&export.out, &["init", "-q", "-b", "main"])?;
    git(&export.out, &["add", "-A"])?;
    let name = format!("user.name={}", export.author_name);
    let email = format!("user.email={}", export.author_email);
    git(
        &export.out,
        &[
            "-c",
            &name,
            "-c",
            &email,
            "commit",
            "-q",
            "-m",
            &export.message,
        ],
    )?;
    Ok(git(&export.out, &["rev-parse", "HEAD"])?.trim().to_string())
}

fn git(dir: &Path, args: &[&str]) -> Result<String, ExportError> {
    let output = Command::new("git").arg("-C").arg(dir).args(args).output()?;
    if !output.status.success() {
        return Err(ExportError::Git {
            args: args.join(" "),
            detail: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_whitelist_keeps_code_and_public_docs_and_never_the_private_folders() {
        for kept in [
            "Cargo.toml",
            "crates/jobbot-domain/src/lib.rs",
            "docs/adr/0001-layered-workspace.md",
            "docs/img/panel-today.jpg",
            "panel-src/src/main.ts",
            "xtask/src/lib.rs",
        ] {
            assert_eq!(destinations(kept), vec![kept.to_string()], "{kept}");
        }
        for dropped in [
            "profile/answers.toml",
            "cv/cv-master.md",
            "docs/research/notes.md",
            "docs/superpowers/plans/plan.md",
            "panel-src/archive/old.html",
            "docs/demo/img/hoy-dark.png",
            "NOTES.md",
            "Cargo.toml.bak",
        ] {
            assert!(destinations(dropped).is_empty(), "{dropped}");
        }
    }

    #[test]
    fn the_example_profile_and_cv_also_take_the_real_ones_place() {
        assert_eq!(
            destinations("examples/profile/answers.toml"),
            vec!["examples/profile/answers.toml", "profile/answers.toml"]
        );
        assert_eq!(
            destinations("examples/cv/cv-master.md"),
            vec!["examples/cv/cv-master.md", "cv/cv-master.md"]
        );
        assert_eq!(
            destinations("examples/README.md"),
            vec!["examples/README.md"]
        );
    }

    #[test]
    fn the_audits_of_the_private_profile_never_leave_the_repository() {
        for dropped in [
            "tests/profile_audit_greenhouse.rs",
            "crates/jobbot-app/tests/profile_audit.rs",
            "crates/jobbot-app/tests/profile_audit/main.rs",
        ] {
            assert!(destinations(dropped).is_empty(), "{dropped}");
        }
        assert_eq!(
            destinations("crates/jobbot-app/tests/use_cases/answers.rs"),
            vec!["crates/jobbot-app/tests/use_cases/answers.rs"]
        );
    }

    #[test]
    fn the_public_readme_takes_the_place_of_the_owners_manual() {
        assert!(destinations("README.md").is_empty());
        assert_eq!(destinations("docs/public/README.md"), vec!["README.md"]);
        assert_eq!(
            destinations("docs/public/README.es.md"),
            vec!["README.es.md"]
        );
        assert!(destinations("docs/public/notes.md").is_empty());
    }

    /// A throwaway repository with one commit: private files, examples, code.
    fn repository(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]).unwrap();
        for (path, text) in files {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        git(dir.path(), &["add", "-A"]).unwrap();
        git(
            dir.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-q",
                "-m",
                "start",
            ],
        )
        .unwrap();
        dir
    }

    const FILES: [(&str, &str); 6] = [
        ("Cargo.toml", "[package]\nname = \"demo\"\n"),
        ("src/main.rs", "fn main() {}\n"),
        ("profile/answers.toml", "name = \"Secret Name\"\n"),
        ("examples/profile/answers.toml", "name = \"Jane Doe\"\n"),
        ("docs/research/notes.md", "about Secret Name\n"),
        ("NOTES.md", "Secret Name again\n"),
    ];

    fn export_of(repo: &Path, scratch: &Path, terms: &str) -> Export {
        let terms_path = scratch.join("terms.txt");
        fs::write(&terms_path, terms).unwrap();
        Export {
            repo: repo.to_path_buf(),
            commit: "HEAD".into(),
            out: scratch.join("public"),
            terms: terms_path,
            author_name: "Jane Doe".into(),
            author_email: "jane@example.com".into(),
            message: "Initial public release".into(),
            account: None,
            public_homes: Vec::new(),
            verify: None,
        }
    }

    #[test]
    fn a_clean_export_is_one_commit_of_the_whitelisted_files_with_the_example_profile() {
        let repo = repository(&FILES);
        let scratch = tempfile::tempdir().unwrap();
        let export = export_of(repo.path(), scratch.path(), "secret name\n");
        let done = export_public(&export).unwrap();
        let out = &export.out;
        assert_eq!(
            done.files, 4,
            "Cargo.toml, main.rs and the example in two places"
        );
        assert_eq!(done.left_out, 3);
        assert_eq!(
            fs::read_to_string(out.join("profile/answers.toml")).unwrap(),
            "name = \"Jane Doe\"\n"
        );
        assert!(out.join("src/main.rs").exists());
        assert!(!out.join("docs/research").exists());
        assert!(!out.join("NOTES.md").exists());
        assert_eq!(
            git(out, &["rev-list", "--count", "HEAD"]).unwrap().trim(),
            "1"
        );
        assert_eq!(
            git(out, &["rev-parse", "HEAD"]).unwrap().trim(),
            done.commit
        );
        assert_eq!(
            git(out, &["log", "-1", "--format=%an <%ae>|%s"])
                .unwrap()
                .trim(),
            "Jane Doe <jane@example.com>|Initial public release"
        );
        assert!(git(out, &["remote"]).unwrap().trim().is_empty());
        assert!(
            git(out, &["status", "--porcelain"])
                .unwrap()
                .trim()
                .is_empty()
        );
    }

    #[test]
    fn a_term_in_an_exported_file_aborts_the_export_and_leaves_no_copy() {
        let mut files = FILES.to_vec();
        files.push(("src/lib.rs", "// thanks, Secret Name\n"));
        let repo = repository(&files);
        let scratch = tempfile::tempdir().unwrap();
        let export = export_of(repo.path(), scratch.path(), "secret name\n");
        match export_public(&export) {
            Err(ExportError::Findings(found)) => {
                assert_eq!(found.len(), 1, "{found:?}");
                assert_eq!((found[0].path.as_str(), found[0].line), ("src/lib.rs", 1));
            }
            other => panic!("expected findings, got {other:?}"),
        }
        assert!(!export.out.exists());
    }

    #[test]
    fn a_published_address_passes_and_the_bare_term_next_to_it_still_stops_the_export() {
        let scratch = tempfile::tempdir().unwrap();
        let repo = repository(&[
            (
                "src/main.rs",
                "fn main() {}
",
            ),
            (
                "AI-USE.md",
                "Source: https://github.com/secret-corp/tool
",
            ),
        ]);
        let mut export = export_of(
            repo.path(),
            scratch.path(),
            "secret-corp
",
        );
        assert!(matches!(
            export_public(&export),
            Err(ExportError::Findings(_))
        ));
        export.public_homes = vec!["https://github.com/secret-corp/".into()];
        export_public(&export).expect("the published address is not a finding");

        let scratch = tempfile::tempdir().unwrap();
        let repo = repository(&[
            (
                "src/main.rs",
                "fn main() {}
",
            ),
            (
                "AI-USE.md",
                "Source: https://github.com/secret-corp/tool, by Secret-Corp
",
            ),
        ]);
        let mut export = export_of(
            repo.path(),
            scratch.path(),
            "secret-corp
",
        );
        export.public_homes = vec!["https://github.com/secret-corp/".into()];
        let Err(ExportError::Findings(findings)) = export_public(&export) else {
            panic!("the bare term is still a finding");
        };
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn a_replacement_scrubs_a_term_before_the_scan() {
        let mut files = FILES.to_vec();
        files.push(("src/lib.rs", "// thanks, Secret Name\n"));
        let repo = repository(&files);
        let scratch = tempfile::tempdir().unwrap();
        let export = export_of(
            repo.path(),
            scratch.path(),
            "secret name\nreplace: Secret Name => Jane Doe\n",
        );
        export_public(&export).unwrap();
        assert_eq!(
            fs::read_to_string(export.out.join("src/lib.rs")).unwrap(),
            "// thanks, Jane Doe\n"
        );
    }

    fn git_check(args: &[&str]) -> Option<Verify> {
        Some(Verify {
            program: "git".into(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            env: vec![("JOBBOT_EXPORT_CHECK".into(), "1".into())],
        })
    }

    #[test]
    fn a_copy_that_fails_its_check_is_not_committed_and_leaves_no_folder() {
        let repo = repository(&FILES);
        let scratch = tempfile::tempdir().unwrap();
        let mut export = export_of(repo.path(), scratch.path(), "secret name\n");
        export.verify = git_check(&["rev-parse", "--verify", "--quiet", "no-such-ref"]);
        assert!(matches!(
            export_public(&export),
            Err(ExportError::Verify { .. })
        ));
        assert!(!export.out.exists());
    }

    #[test]
    fn the_check_runs_in_the_copy_before_the_commit() {
        let repo = repository(&FILES);
        let scratch = tempfile::tempdir().unwrap();
        let mut export = export_of(repo.path(), scratch.path(), "secret name\n");
        // A path relative to the copy, and no repository yet: it runs there, before `git init`.
        export.verify = git_check(&["hash-object", "src/main.rs", "profile/answers.toml"]);
        export_public(&export).unwrap();
        export.out = scratch.path().join("public-2");
        export.verify = git_check(&["rev-parse", "--verify", "--quiet", "HEAD"]);
        assert!(matches!(
            export_public(&export),
            Err(ExportError::Verify { .. })
        ));
    }

    #[test]
    fn the_exporting_accounts_home_folder_is_a_finding_in_any_spelling() {
        for text in [
            "// built in C:\\Users\\SecretUser\\pega\n",
            "// see c:/users/secretuser/x\n",
            "// cache at /home/secretuser/.cargo\n",
        ] {
            let mut files = FILES.to_vec();
            files.push(("src/lib.rs", text));
            let repo = repository(&files);
            let scratch = tempfile::tempdir().unwrap();
            let mut export = export_of(repo.path(), scratch.path(), "secret name\n");
            export.account = Some("secretuser".into());
            assert!(
                matches!(export_public(&export), Err(ExportError::Findings(_))),
                "{text}"
            );
        }
        let mut files = FILES.to_vec();
        files.push(("src/lib.rs", "// C:\\Users\\ana\\x and secretuserx\n"));
        let repo = repository(&files);
        let scratch = tempfile::tempdir().unwrap();
        let mut export = export_of(repo.path(), scratch.path(), "secret name\n");
        export.account = Some("secretuser".into());
        export_public(&export).unwrap();
    }

    #[test]
    fn the_export_refuses_a_terms_file_inside_the_repository_and_an_existing_folder() {
        let repo = repository(&FILES);
        let scratch = tempfile::tempdir().unwrap();
        let mut inside = export_of(repo.path(), scratch.path(), "secret name\n");
        inside.terms = repo.path().join("terms.txt");
        fs::write(&inside.terms, "secret name\n").unwrap();
        assert!(matches!(
            export_public(&inside),
            Err(ExportError::TermsInsideRepo(_))
        ));
        let existing = export_of(repo.path(), scratch.path(), "secret name\n");
        fs::create_dir_all(&existing.out).unwrap();
        assert!(matches!(
            export_public(&existing),
            Err(ExportError::OutExists(_))
        ));
        let empty = export_of(repo.path(), scratch.path(), "# nothing\n");
        assert!(matches!(
            export_public(&empty),
            Err(ExportError::Terms(TermsError::Empty))
        ));
    }

    #[test]
    fn the_export_copies_the_commit_not_the_working_tree() {
        let repo = repository(&FILES);
        fs::write(
            repo.path().join("src/main.rs"),
            "// Secret Name, never committed\n",
        )
        .unwrap();
        fs::write(repo.path().join("src/extra.rs"), "// Secret Name\n").unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let export = export_of(repo.path(), scratch.path(), "secret name\n");
        export_public(&export).unwrap();
        assert_eq!(
            fs::read_to_string(export.out.join("src/main.rs")).unwrap(),
            "fn main() {}\n"
        );
        assert!(!export.out.join("src/extra.rs").exists());
    }
}
