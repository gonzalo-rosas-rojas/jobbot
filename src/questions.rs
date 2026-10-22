//! The application questions of a Greenhouse job, read without a browser.
//!
//! `GET {base}/v1/boards/{board}/jobs/{id}?questions=true` is public and needs no key. It lists
//! every field of the application form: the standard ones (name, email, resume), the company's
//! own questions, the location fields, the EEOC block (`compliance`) and the voluntary
//! demographic questions. Reading it lets the unanswered questions of many jobs be grouped and
//! answered once, before any form is opened. Nothing here applies, fills or sends anything.

use anyhow::Context;
use reqwest::Client;
use serde_json::Value;

use crate::sources::base::{get_json, id_string, str_field};
use crate::text::html_to_text;

pub const DEFAULT_BASE_URL: &str = "https://boards-api.greenhouse.io";

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

/// Every question of a `?questions=true` response, in form order: application, location, EEOC,
/// demographic. Lenient: an entry without a label or a field name is skipped and a body that is
/// not an object gives an empty list.
pub fn parse_questions(body: &Value) -> Vec<Question> {
    let mut questions = Vec::new();
    push_questions(&mut questions, &body["questions"], Section::Application);
    push_questions(
        &mut questions,
        &body["location_questions"],
        Section::Location,
    );
    for block in body["compliance"].as_array().into_iter().flatten() {
        push_questions(&mut questions, &block["questions"], Section::Eeoc);
    }
    push_demographic(&mut questions, &body["demographic_questions"]["questions"]);
    questions
}

fn kind_of(api_type: &str) -> FieldKind {
    match api_type {
        "input_text" => FieldKind::Text,
        "textarea" => FieldKind::Textarea,
        "input_file" => FieldKind::File,
        "multi_value_single_select" => FieldKind::SingleSelect,
        "multi_value_multi_select" => FieldKind::MultiSelect,
        "input_hidden" => FieldKind::Hidden,
        other => FieldKind::Other(other.to_string()),
    }
}

/// `{label, required, description, fields: [{name, type, values: [{label, value}]}]}` entries:
/// one `Question` per field that has a name.
fn push_questions(out: &mut Vec<Question>, items: &Value, section: Section) {
    for item in items.as_array().into_iter().flatten() {
        let label = str_field(item, "label").trim();
        if label.is_empty() {
            continue;
        }
        let description = html_to_text(str_field(item, "description"));
        let required = item["required"].as_bool().unwrap_or(false);
        for field in item["fields"].as_array().into_iter().flatten() {
            let name = str_field(field, "name");
            if name.is_empty() {
                continue;
            }
            let options = field["values"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|value| {
                    let label = str_field(value, "label").trim();
                    (!label.is_empty()).then(|| QuestionOption {
                        label: label.to_string(),
                        value: id_string(&value["value"]).unwrap_or_default(),
                        free_form: false,
                        decline: is_decline_label(label),
                    })
                })
                .collect();
            out.push(Question {
                section,
                label: label.to_string(),
                description: description.clone(),
                field: name.to_string(),
                kind: kind_of(str_field(field, "type")),
                required,
                options,
            });
        }
    }
}

/// `{id, label, required, type, answer_options: [{id, label, free_form, decline_to_answer}]}`.
fn push_demographic(out: &mut Vec<Question>, items: &Value) {
    for item in items.as_array().into_iter().flatten() {
        let label = str_field(item, "label").trim();
        let Some(id) = id_string(&item["id"]) else {
            continue;
        };
        if label.is_empty() {
            continue;
        }
        let options = item["answer_options"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|option| {
                let label = str_field(option, "label").trim();
                (!label.is_empty()).then(|| QuestionOption {
                    label: label.to_string(),
                    value: id_string(&option["id"]).unwrap_or_default(),
                    free_form: option["free_form"].as_bool().unwrap_or(false),
                    decline: option["decline_to_answer"].as_bool().unwrap_or(false)
                        || is_decline_label(label),
                })
            })
            .collect();
        out.push(Question {
            section: Section::Demographic,
            label: label.to_string(),
            description: String::new(),
            field: format!("demographic_{id}"),
            kind: kind_of(str_field(item, "type")),
            required: item["required"].as_bool().unwrap_or(false),
            options,
        });
    }
}

/// The questions of one job on the public Greenhouse API.
pub async fn fetch_questions(
    client: &Client,
    board: &str,
    job_id: &str,
) -> anyhow::Result<Vec<Question>> {
    fetch_questions_from(client, DEFAULT_BASE_URL, board, job_id).await
}

/// Like [`fetch_questions`] against another server (tests use a wiremock server).
pub async fn fetch_questions_from(
    client: &Client,
    base_url: &str,
    board: &str,
    job_id: &str,
) -> anyhow::Result<Vec<Question>> {
    let url = format!(
        "{}/v1/boards/{board}/jobs/{job_id}",
        base_url.trim_end_matches('/')
    );
    let body = get_json(client, &url, &[("questions", "true")])
        .await
        .with_context(|| format!("questions of greenhouse:{board}/{job_id}"))?;
    Ok(parse_questions(&body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Real `?questions=true` responses (descriptions cut to 300 characters), captured on
    /// 2026-10-04: a Databricks job with an EEOC block and a Datadog job with demographic questions.
    fn databricks() -> Value {
        serde_json::from_str(include_str!("../tests/fixtures/questions/databricks.json")).unwrap()
    }

    fn datadog() -> Value {
        serde_json::from_str(include_str!("../tests/fixtures/questions/datadog.json")).unwrap()
    }

    fn in_section(questions: &[Question], section: Section) -> Vec<&Question> {
        questions
            .iter()
            .filter(|question| question.section == section)
            .collect()
    }

    #[test]
    fn reads_every_block_of_a_real_job_in_form_order() {
        let questions = parse_questions(&databricks());

        assert_eq!(in_section(&questions, Section::Application).len(), 18);
        assert_eq!(in_section(&questions, Section::Location).len(), 3);
        assert_eq!(in_section(&questions, Section::Eeoc).len(), 3);
        assert_eq!(questions.len(), 24);
        let order: Vec<Section> = questions.iter().map(|question| question.section).collect();
        let mut sorted = order.clone();
        sorted.sort_by_key(|section| *section as u8);
        assert_eq!(order, sorted, "application, location, EEOC, demographic");
    }

    #[test]
    fn standard_fields_carry_their_names_kinds_and_required_flags() {
        let questions = parse_questions(&databricks());

        let first = &questions[0];
        assert_eq!(first.label, "First Name");
        assert_eq!(first.field, "first_name");
        assert_eq!(first.kind, FieldKind::Text);
        assert!(first.required);
        assert!(first.options.is_empty());
        // The resume has two fields under one label: a file and a text box.
        let resume: Vec<(&str, &FieldKind)> = questions
            .iter()
            .filter(|question| question.label == "Resume/CV")
            .map(|question| (question.field.as_str(), &question.kind))
            .collect();
        assert_eq!(
            resume,
            [
                ("resume", &FieldKind::File),
                ("resume_text", &FieldKind::Textarea)
            ]
        );
        let cover = questions
            .iter()
            .find(|question| question.field == "cover_letter")
            .unwrap();
        assert!(!cover.required);
    }

    #[test]
    fn selects_carry_their_options_and_the_multi_select_keeps_its_bracket() {
        let questions = parse_questions(&databricks());

        let authorized = questions
            .iter()
            .find(|question| {
                question
                    .label
                    .starts_with("Are you legally authorized to work")
            })
            .unwrap();
        assert_eq!(authorized.kind, FieldKind::SingleSelect);
        assert_eq!(authorized.options.len(), 2);
        assert!(authorized.required);
        assert!(
            authorized
                .options
                .iter()
                .all(|option| !option.label.is_empty() && !option.value.is_empty())
        );
        let multi = questions
            .iter()
            .find(|question| question.field == "question_35111049002[]")
            .unwrap();
        assert_eq!(multi.kind, FieldKind::MultiSelect);
        assert_eq!(multi.options.len(), 4);
    }

    #[test]
    fn the_location_block_has_hidden_coordinates_and_a_visible_place() {
        let questions = parse_questions(&databricks());
        let location = in_section(&questions, Section::Location);

        let longitude = location.iter().find(|q| q.field == "longitude").unwrap();
        assert_eq!(longitude.kind, FieldKind::Hidden);
        assert!(!longitude.is_visible());
        let place = location.iter().find(|q| q.field == "location").unwrap();
        assert_eq!(place.kind, FieldKind::Text);
        assert!(place.is_visible());
    }

    #[test]
    fn the_eeoc_block_marks_its_decline_options() {
        let questions = parse_questions(&databricks());
        let eeoc = in_section(&questions, Section::Eeoc);

        let veteran = eeoc.iter().find(|q| q.field == "veteran_status").unwrap();
        assert_eq!(
            veteran.decline_option().map(|o| o.label.as_str()),
            Some("I don't wish to answer")
        );
        let race = eeoc.iter().find(|q| q.field == "race").unwrap();
        assert_eq!(
            race.decline_option().map(|o| o.label.as_str()),
            Some("Decline To Self Identify")
        );
        assert!(race.options.iter().filter(|option| option.decline).count() == 1);
        assert!(!eeoc.iter().any(|q| q.required));
    }

    #[test]
    fn demographic_questions_use_the_decline_flag_and_free_form_marks() {
        let questions = parse_questions(&datadog());
        let demographic = in_section(&questions, Section::Demographic);

        assert_eq!(demographic.len(), 1);
        let gender = demographic[0];
        assert_eq!(gender.label, "Voluntary Self-Identification of Gender:");
        assert_eq!(gender.field, "demographic_1757");
        assert_eq!(gender.kind, FieldKind::SingleSelect);
        assert!(gender.required);
        assert_eq!(gender.options.len(), 5);
        let describe = gender
            .options
            .iter()
            .find(|option| option.label.starts_with("Prefer to self describe"))
            .unwrap();
        assert!(describe.free_form && !describe.decline);
        assert_eq!(describe.value, "12582");
        assert_eq!(
            gender.decline_option().map(|o| o.label.as_str()),
            Some("I don't wish to answer")
        );
    }

    #[test]
    fn long_selects_and_how_did_you_hear_are_read_whole() {
        let questions = parse_questions(&datadog());

        let heard = questions
            .iter()
            .find(|q| q.label.starts_with("How did you hear about this opport"))
            .unwrap();
        assert_eq!(heard.options.len(), 35);
        let cities = questions
            .iter()
            .find(|q| q.field == "question_59674549[]")
            .unwrap();
        assert_eq!(cities.kind, FieldKind::MultiSelect);
        assert_eq!(cities.options.len(), 45);
    }

    #[test]
    fn a_malformed_response_never_panics() {
        assert!(parse_questions(&json!(null)).is_empty());
        assert!(parse_questions(&json!([])).is_empty());
        assert!(parse_questions(&json!({"questions": null, "compliance": "x"})).is_empty());
        let body = json!({"questions": [
            {"required": true, "fields": [{"name": "a", "type": "input_text"}]},
            {"label": "No fields"},
            {"label": "No name", "fields": [{"type": "input_text"}]},
            {"label": "Odd", "required": true, "fields": [{"name": "x", "type": "weird_type"}]}
        ]});

        let questions = parse_questions(&body);

        assert_eq!(questions.len(), 1);
        assert_eq!(
            questions[0].kind,
            FieldKind::Other("weird_type".to_string())
        );
        assert!(questions[0].required);
    }

    #[test]
    fn a_description_becomes_plain_text() {
        let body = json!({"questions": [{
            "label": "Why us?", "required": false,
            "description": "<p>Tell us <b>why</b>.</p>",
            "fields": [{"name": "question_1", "type": "textarea", "values": []}]
        }]});

        let questions = parse_questions(&body);

        assert_eq!(questions[0].description, "Tell us why.");
        assert_eq!(questions[0].kind, FieldKind::Textarea);
    }

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
        let questions = parse_questions(&datadog());
        let linkedin = questions
            .iter()
            .find(|q| q.label == "LinkedIn Profile")
            .unwrap();
        assert_eq!(linkedin.normalized_label(), "linkedin profile");
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

    #[tokio::test]
    async fn fetch_asks_for_the_questions_of_one_job() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/boards/databricks/jobs/7882009002"))
            .and(query_param("questions", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(databricks()))
            .expect(1)
            .mount(&server)
            .await;

        let questions =
            fetch_questions_from(&Client::new(), &server.uri(), "databricks", "7882009002")
                .await
                .unwrap();

        assert_eq!(questions.len(), 24);
    }

    #[tokio::test]
    async fn fetch_names_the_job_when_it_is_gone_or_the_body_is_not_json() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/boards/acme/jobs/1"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/boards/acme/jobs/2"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>blocked</html>"))
            .mount(&server)
            .await;

        let gone = fetch_questions_from(&Client::new(), &server.uri(), "acme", "1")
            .await
            .unwrap_err();
        assert!(
            format!("{gone:#}").contains("greenhouse:acme/1"),
            "{gone:#}"
        );
        assert!(format!("{gone:#}").contains("404"), "{gone:#}");
        let html = fetch_questions_from(&Client::new(), &server.uri(), "acme", "2")
            .await
            .unwrap_err();
        assert!(format!("{html:#}").contains("not JSON"), "{html:#}");
    }
}
