//! The file a form gets for the CV. The tailored PDF keeps its key-based name in `data/cv`, which
//! nobody reads; the copy a form attaches is named the way Gonza tracks his applications and a
//! recruiter reads the attachment: `cv_<first name>_<surname>_<job title>.pdf`.

use std::path::{Path, PathBuf};

use jobbot_domain::answers::AnswerBank;
use jobbot_domain::models::Job;
use jobbot_domain::text::safe_key;

/// A title longer than this is cut, so the name stays well inside Windows' path limit.
const MAX_TITLE_CHARS: usize = 100;

/// The name the attached CV carries: `cv_alex_rivera_Software Engineer, Developer Tools.pdf`.
/// The person part is the first two words of the bank's answer to "Name", in lower case; without
/// one the name is `cv_<title>.pdf`. Characters a Windows file name cannot hold become `-`.
pub fn attachment_name(title: &str, bank: &AnswerBank) -> String {
    let person = bank
        .lookup("Name")
        .and_then(|entry| entry.value.as_deref())
        .map(|name| {
            name.split_whitespace()
                .take(2)
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
                .join("_")
        })
        .filter(|person| !person.is_empty());
    let title = clean_title(title);
    match person {
        Some(person) => format!("cv_{person}_{title}.pdf"),
        None => format!("cv_{title}.pdf"),
    }
}

/// `title` as a file name: forbidden characters become `-`, runs of spaces become one, and the
/// end loses the spaces and dots Windows would drop.
fn clean_title(title: &str) -> String {
    let replaced: String = title
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let joined = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = joined.chars().take(MAX_TITLE_CHARS).collect();
    let trimmed = cut.trim_end_matches([' ', '.']);
    if trimmed.is_empty() {
        "CV".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Name of the folder of the tailored CVs inside the data folder: `<data_dir>/cv`.
pub const CV_DIR_NAME: &str = "cv";

/// Name, inside the folder of the tailored CVs, of the folder of readable copies.
pub const ATTACHMENTS_DIR_NAME: &str = "adjuntos";

/// The folder that holds the readable copies of one application's CV: `adjuntos/<job key>/` inside
/// the folder of the tailored CVs (`<data_dir>/cv`). The one place that says where they live, for
/// `attachment_for` (which makes them) and `refresh_attachment_copies` (which replaces them).
pub fn attachments_folder(cv_dir: &Path, job_key: &str) -> PathBuf {
    cv_dir.join(ATTACHMENTS_DIR_NAME).join(safe_key(job_key))
}

/// A readable CV copy that could not be refreshed: it still holds the CV that was replaced, and
/// whoever attaches it must be told.
#[derive(Debug, Clone, PartialEq)]
pub struct CopyFailure {
    pub job_key: String,
    /// The copy, or the folder that could not be read.
    pub file: PathBuf,
    pub reason: String,
}

/// Keeps a copy of `file` in `keep_in` (created when needed) under `kept_name`, so that what is
/// about to be replaced can still be found.
pub async fn keep_before_replacing(
    file: &Path,
    keep_in: &Path,
    kept_name: &str,
) -> std::io::Result<()> {
    tokio::fs::create_dir_all(keep_in).await?;
    tokio::fs::copy(file, keep_in.join(kept_name)).await?;
    Ok(())
}

/// Overwrites the readable copies `attachment_for` made of an application's CV with `cv_pdf`, its
/// freshly rendered tailored PDF. The copies sit in `attachments_folder`, and the handoff sheet
/// names them, so they must not keep the CV that was replaced. Each copy is kept first in
/// `keep_in` (created when needed) as `<job key>.<name>.<stamp>.pdf`: unlike the main PDF's kept
/// copy its name carries the job key, because two jobs can share a copy's name. Only copies
/// already there are rewritten; the folder `cv_dir` is the one `attachments_folder` takes. A
/// folder that does not exist (`NotFound`) means the application has no copy; any other failure
/// is returned, never swallowed, and never stops the run (the same courtesy rule as
/// `attachment_for`).
pub async fn refresh_attachment_copies(
    cv_dir: &Path,
    cv_pdf: &Path,
    job_key: &str,
    keep_in: &Path,
    stamp: &str,
) -> Vec<CopyFailure> {
    let folder = attachments_folder(cv_dir, job_key);
    let failure = |file: &Path, err: &std::io::Error| CopyFailure {
        job_key: job_key.to_string(),
        file: file.to_path_buf(),
        reason: err.to_string(),
    };
    let mut entries = match tokio::fs::read_dir(&folder).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(err) => return vec![failure(&folder, &err)],
    };
    let mut failures = Vec::new();
    loop {
        let path = match entries.next_entry().await {
            Ok(Some(entry)) => entry.path(),
            Ok(None) => break,
            Err(err) => {
                failures.push(failure(&folder, &err));
                break;
            }
        };
        let is_pdf = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"));
        if !is_pdf {
            continue;
        }
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let kept_name = format!("{}.{stem}.{stamp}.pdf", safe_key(job_key));
        let replaced = async {
            keep_before_replacing(&path, keep_in, &kept_name).await?;
            tokio::fs::copy(cv_pdf, &path).await
        }
        .await;
        if let Err(err) = replaced {
            tracing::warn!(error = %err, file = %path.display(), "the readable CV copy could not be refreshed");
            failures.push(failure(&path, &err));
        }
    }
    failures
}

/// Copies `cv_pdf` to `<data_dir>/cv/adjuntos/<job key>/<attachment_name>` and returns the copy.
/// The name is a courtesy, never a reason to stop a run: if the copy fails the form gets the
/// tailored PDF as it is.
pub fn attachment_for(cv_pdf: &Path, job: &Job, bank: &AnswerBank, data_dir: &Path) -> PathBuf {
    let dir = attachments_folder(&data_dir.join(CV_DIR_NAME), &job.key());
    let target = dir.join(attachment_name(&job.title, bank));
    let copied = std::fs::create_dir_all(&dir).and_then(|()| std::fs::copy(cv_pdf, &target));
    match copied {
        Ok(_) => target,
        Err(err) => {
            tracing::warn!(error = %err, "the CV copy with a readable name failed; attaching the tailored PDF");
            cv_pdf.to_path_buf()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jobbot_domain::answers::parse_answers;

    fn bank(name: Option<&str>) -> AnswerBank {
        let entry = match name {
            Some(name) => format!("[[entry]]\npatterns = [\"=name\"]\nvalue = \"{name}\"\n"),
            None => String::new(),
        };
        parse_answers(&format!(
            "{entry}[[entry]]\npatterns = [\"=email\"]\nvalue = \"a@b.c\"\n"
        ))
        .unwrap()
    }

    #[test]
    fn the_name_carries_the_first_name_the_surname_and_the_title_as_posted() {
        assert_eq!(
            attachment_name(
                "Software Engineer, Developer Tools",
                &bank(Some("Alex Rivera Soto"))
            ),
            "cv_alex_rivera_Software Engineer, Developer Tools.pdf"
        );
    }

    #[test]
    fn characters_windows_refuses_become_dashes_and_the_end_loses_dots() {
        assert_eq!(
            attachment_name(
                "Senior Python/Flask+React  Engineer: LATAM?...",
                &bank(Some("Ana Pérez"))
            ),
            "cv_ana_pérez_Senior Python-Flask+React Engineer- LATAM-.pdf"
        );
    }

    #[test]
    fn without_a_name_in_the_bank_the_name_is_cv_and_the_title() {
        assert_eq!(
            attachment_name("AI Engineer", &bank(None)),
            "cv_AI Engineer.pdf"
        );
        assert_eq!(attachment_name("  ...  ", &bank(None)), "cv_CV.pdf");
    }

    #[test]
    fn a_long_title_is_cut() {
        let name = attachment_name(&"x".repeat(300), &bank(None));
        assert_eq!(
            name.chars().count(),
            "cv_".len() + MAX_TITLE_CHARS + ".pdf".len()
        );
    }

    #[test]
    fn the_copy_lands_in_a_folder_per_job_and_the_tailored_pdf_stays() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("greenhouse_123.pdf");
        std::fs::write(&pdf, b"%PDF-1.4 tailored").unwrap();
        let job = Job {
            source: "greenhouse".to_string(),
            external_id: "123".to_string(),
            title: "Staff Engineer".to_string(),
            ..Job::default()
        };
        let copy = attachment_for(&pdf, &job, &bank(Some("Alex Rivera Soto")), dir.path());
        assert_eq!(
            copy,
            dir.path()
                .join("cv/adjuntos/greenhouse_123/cv_alex_rivera_Staff Engineer.pdf")
        );
        assert_eq!(std::fs::read(&copy).unwrap(), b"%PDF-1.4 tailored");
        assert!(pdf.is_file());
    }

    #[test]
    fn a_copy_that_fails_attaches_the_tailored_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("not-there.pdf");
        let job = Job {
            source: "lever".to_string(),
            external_id: "1".to_string(),
            title: "Engineer".to_string(),
            ..Job::default()
        };
        assert_eq!(
            attachment_for(&missing, &job, &bank(None), dir.path()),
            missing
        );
    }
}
