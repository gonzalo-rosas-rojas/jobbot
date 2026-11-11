//! The questions of an application form as data: which block each comes from, what control
//! answers it, its options, and the label normalisation that lets the same question asked by two
//! companies be recognised. Reading them from a job board is the sources adapter's job.

/// Which block of the form a question comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// The standard fields and the company's own questions (`questions`).
    Application,
    /// Latitude, longitude and place of the candidate (`location_questions`).
    Location,
    /// The US equal-opportunity block (`compliance`, type `eeoc`).
    Eeoc,
    /// The voluntary self-identification questions (`demographic_questions`).
    Demographic,
}

/// What kind of control answers a question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Textarea,
    File,
    SingleSelect,
    MultiSelect,
    /// Filled by the page itself (coordinates); a person never answers it.
    Hidden,
    /// A type this module does not know; the original name is kept.
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionOption {
    pub label: String,
    /// The value the form sends: the option `value` or, for demographic questions, its `id`.
    pub value: String,
    /// The option asks for text ("Prefer to self describe as").
    pub free_form: bool,
    /// The option means "I decline to answer".
    pub decline: bool,
}

/// One field of the form. A question with several fields (the resume has a file and a text box)
/// gives one `Question` per field, all with the same label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub section: Section,
    pub label: String,
    /// The explanation under the label, as plain text (may be empty).
    pub description: String,
    /// The form's field name (`first_name`, `question_123`, `question_123[]`). Demographic
    /// questions have no name in the API, so theirs is `demographic_{id}`.
    pub field: String,
    pub kind: FieldKind,
    pub required: bool,
    pub options: Vec<QuestionOption>,
}

impl Question {
    /// False for fields a person never answers (the hidden coordinates).
    pub fn is_visible(&self) -> bool {
        self.kind != FieldKind::Hidden
    }

    /// The label in a form that is the same across companies: see [`normalize_label`].
    pub fn normalized_label(&self) -> String {
        normalize_label(&self.label)
    }

    /// The option that declines to answer, when the question has one.
    pub fn decline_option(&self) -> Option<&QuestionOption> {
        self.options.iter().find(|option| option.decline)
    }
}

/// Lower case, curly quotes made straight, white space collapsed, and no trailing `:` or `*`.
/// Two companies that word a question the same way get the same key.
pub fn normalize_label(label: &str) -> String {
    let straight = label
        .replace(['\u{2018}', '\u{2019}'], "'")
        .replace(['\u{201C}', '\u{201D}'], "\"");
    let collapsed = straight.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
        .to_lowercase()
        .trim_end_matches([':', '*', ' '])
        .to_string()
}

/// True for an option that declines to answer: "Decline To Self Identify", "I don't wish to
/// answer", "Prefer not to say". "Prefer to self describe as" is not one.
pub fn is_decline_label(label: &str) -> bool {
    const MARKERS: [&str; 9] = [
        "decline to self",
        "decline to answer",
        "decline to identify",
        "don't wish to answer",
        "do not wish to answer",
        "don't want to answer",
        "do not want to answer",
        "prefer not to",
        "choose not to",
    ];
    let label = normalize_label(label);
    MARKERS.iter().any(|marker| label.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_label_makes_equal_questions_equal() {
        assert_eq!(
            normalize_label("  Voluntary Self-Identification of Gender:\t"),
            "voluntary self-identification of gender"
        );
        assert_eq!(normalize_label("LinkedIn  Profile *"), "linkedin profile");
        assert_eq!(
            normalize_label("What\u{2019}s your name?\n"),
            "what's your name?"
        );
        assert_eq!(normalize_label(""), "");
    }

    #[test]
    fn decline_labels_are_recognised_and_self_description_is_not() {
        for label in [
            "Decline To Self Identify",
            "I don't wish to answer",
            "I don\u{2019}t wish to answer",
            "I do not wish to answer",
            "Prefer not to say",
            "I prefer not to answer",
            "Decline to answer",
        ] {
            assert!(is_decline_label(label), "{label}");
        }
        for label in [
            "Prefer to self describe as",
            "Two or More Races",
            "Yes",
            "No",
            "I am not a protected veteran",
            "",
        ] {
            assert!(!is_decline_label(label), "{label}");
        }
    }
}
