//! The file a form gets for the CV. The tailored PDF keeps its key-based name in `data/cv`, which
//! nobody reads; the copy a form attaches is named the way Gonza tracks his applications and a
//! recruiter reads the attachment: `cv_<first name>_<surname>_<job title>.pdf`.

use std::path::{Path, PathBuf};

use jobbot_domain::answers::AnswerBank;
use jobbot_domain::models::Job;
use jobbot_domain::text::safe_key;

/// Under the data directory: one folder per application, so two jobs with the same title never
/// share a copy.
const ATTACHMENTS_DIR: &str = "cv/adjuntos";
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

/// Copies `cv_pdf` to `<data_dir>/cv/adjuntos/<job key>/<attachment_name>` and returns the copy.
/// The name is a courtesy, never a reason to stop a run: if the copy fails the form gets the
/// tailored PDF as it is.
pub fn attachment_for(cv_pdf: &Path, job: &Job, bank: &AnswerBank, data_dir: &Path) -> PathBuf {
    let dir = data_dir.join(ATTACHMENTS_DIR).join(safe_key(&job.key()));
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
